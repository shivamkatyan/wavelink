//! Manual-IP + QR fallback pairing token (FR-003 / ADR-006).
//!
//! mDNS discovery is the convenience path; when it is unavailable (or the user
//! deliberately pairs across networks), the peer's dialable address, a pairing
//! nonce and the handshake fingerprint travel as a compact `wdr://` URI — the
//! bytes a QR code carries (`≤ 512` bytes, PROTOCOL_SPEC's token bound). The QR
//! render/scan itself stays a device/shell gate; this module is the pure
//! host-side encode/decode, fully unit-testable here.

/// One manual/QR pairing token: everything needed to dial + pin a peer without
/// mDNS.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManualPairingPayload {
    /// Dialable host of the peer ("192.168.1.5", "hostname", or raw IPv6 like
    /// "::1" — encoded with brackets).
    pub host: String,
    /// QUIC port the peer listens on.
    pub port: u16,
    /// Pairing nonce (16 random bytes).
    pub nonce: [u8; 16],
    /// Peer X25519 static fingerprint (the WS-D handshake pin).
    pub fingerprint: [u8; 32],
    /// Optional human-readable label (≤ 32 chars, URL-safe).
    pub label: Option<String>,
}

/// Typed errors from manual/QR token parsing. No panics on data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ManualError {
    Malformed,
    /// Not a `wdr://` URI.
    Scheme,
    /// Missing/invalid host.
    Host,
    /// Missing/invalid port (must be 1..=65535).
    Port,
    /// Nonce must be exactly 16 bytes (32 hex chars).
    NonceLen,
    /// Fingerprint must be exactly 32 bytes (64 hex chars).
    FingerprintLen,
    LabelTooLong,
    /// The encoded token exceeds the 512-byte transport bound.
    Bounds,
}

impl core::fmt::Display for ManualError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            ManualError::Malformed => write!(f, "manual/QR token: malformed"),
            ManualError::Scheme => write!(f, "manual/QR token: not a wdr:// URI"),
            ManualError::Host => write!(f, "manual/QR token: missing host"),
            ManualError::Port => write!(f, "manual/QR token: missing/invalid port"),
            ManualError::NonceLen => write!(f, "manual/QR token: nonce must be 16 bytes (32 hex)"),
            ManualError::FingerprintLen => {
                write!(f, "manual/QR token: fingerprint must be 32 bytes (64 hex)")
            }
            ManualError::LabelTooLong => write!(f, "manual/QR token: label > 32 chars"),
            ManualError::Bounds => write!(f, "manual/QR token: URI exceeds 512 bytes"),
        }
    }
}
impl std::error::Error for ManualError {}

/// Max encoded token size (PROTOCOL_SPEC's QR/manual bound).
pub const MAX_TOKEN_BYTES: usize = 512;
/// Max human-readable label length (chars).
pub const MAX_LABEL_CHARS: usize = 32;

impl ManualPairingPayload {
    /// Encode as a `wdr://` URI: `wdr://<host>:<port>?nonce=<hex32>&fp=<hex64>`
    /// with an optional `#<label>` suffix. IPv6 hosts are bracketed.
    pub fn encode_uri(&self) -> String {
        let host = if self.host.contains(':') && !self.host.starts_with('[') {
            format!("[{}]", self.host)
        } else {
            self.host.clone()
        };
        let mut uri = format!(
            "wdr://{host}:{}?nonce={}&fp={}",
            self.port,
            hex(&self.nonce),
            hex(&self.fingerprint)
        );
        if let Some(label) = &self.label {
            uri.push('#');
            uri.push_str(label);
        }
        uri
    }

    /// The token's bytes (the QR payload) — capped at [`MAX_TOKEN_BYTES`].
    pub fn to_bytes(&self) -> Vec<u8> {
        self.encode_uri().into_bytes()
    }

    /// Decode a `wdr://` URI back into a token.
    pub fn parse_uri(s: &str) -> Result<Self, ManualError> {
        let rest = s.strip_prefix("wdr://").ok_or(ManualError::Scheme)?;
        // Split the optional `#label` suffix first.
        let (authority, label) = match rest.find('#') {
            Some(i) => (&rest[..i], Some(&rest[i + 1..])),
            None => (rest, None),
        };
        if authority.is_empty() {
            return Err(ManualError::Malformed);
        }
        // authority = [host] : port ? query
        let (hostport, query) = match authority.find('?') {
            Some(i) => (&authority[..i], &authority[i + 1..]),
            None => (authority, ""),
        };
        let hostport = hostport.trim_matches('/');
        let hostport = match hostport.strip_prefix('[') {
            Some(inner) => {
                // IPv6 literal: `[::1]:port`
                let end = inner.find(']').ok_or(ManualError::Malformed)?;
                let host = &inner[..end];
                let after = &inner[end + 1..];
                let port = after.strip_prefix(':').ok_or(ManualError::Malformed)?;
                (host, port)
            }
            None => {
                // host:port (host must not contain ':' — that would be IPv6)
                let idx = hostport.rfind(':').ok_or(ManualError::Port)?;
                (&hostport[..idx], &hostport[idx + 1..])
            }
        };
        let (host, port_str) = hostport;
        if host.is_empty() {
            return Err(ManualError::Host);
        }
        let port: u16 = port_str.parse().map_err(|_| ManualError::Port)?;
        if port == 0 {
            return Err(ManualError::Port);
        }
        // Parse the query params (nonce + fp).
        let mut nonce: Option<[u8; 16]> = None;
        let mut fingerprint: Option<[u8; 32]> = None;
        for kv in query.split('&') {
            if kv.is_empty() {
                continue;
            }
            let (k, v) = kv.split_once('=').ok_or(ManualError::Malformed)?;
            match k {
                "nonce" => nonce = Some(dehex::<16>(v).ok_or(ManualError::NonceLen)?),
                "fp" => fingerprint = Some(dehex::<32>(v).ok_or(ManualError::FingerprintLen)?),
                _ => {} // unknown params ignored (forward-compat)
            }
        }
        let nonce = nonce.ok_or(ManualError::Malformed)?;
        let fingerprint = fingerprint.ok_or(ManualError::Malformed)?;
        let label = match label {
            Some(l) => {
                if l.is_empty() {
                    None
                } else if l.chars().count() > MAX_LABEL_CHARS {
                    return Err(ManualError::LabelTooLong);
                } else if !l
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
                {
                    return Err(ManualError::Malformed); // URL-safe only
                } else {
                    Some(l.to_string())
                }
            }
            None => None,
        };
        Ok(Self {
            host: host.to_string(),
            port,
            nonce,
            fingerprint,
            label,
        })
    }

    /// Decode token bytes (the QR payload) back into a token.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, ManualError> {
        if bytes.len() > MAX_TOKEN_BYTES {
            return Err(ManualError::Bounds);
        }
        let s = std::str::from_utf8(bytes).map_err(|_| ManualError::Malformed)?;
        Self::parse_uri(s)
    }
}

fn hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

/// Parse `n` bytes of lowercase hex.
fn dehex<const N: usize>(s: &str) -> Option<[u8; N]> {
    if s.len() != N * 2 {
        return None;
    }
    let mut out = [0u8; N];
    for (i, chunk) in s.as_bytes().as_chunks::<2>().0.iter().enumerate() {
        out[i] = u8::from_str_radix(std::str::from_utf8(chunk).ok()?, 16).ok()?;
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> ManualPairingPayload {
        ManualPairingPayload {
            host: "192.168.1.5".to_string(),
            port: 9_000,
            nonce: [0x11; 16],
            fingerprint: [0x22; 32],
            label: Some("Living-Room-DAC".to_string()),
        }
    }

    #[test]
    fn ipv4_roundtrip_with_label() {
        let p = sample();
        let uri = p.encode_uri();
        assert!(uri.starts_with("wdr://192.168.1.5:9000?nonce="));
        assert!(uri.ends_with(
            "&fp=2222222222222222222222222222222222222222222222222222222222222222#Living-Room-DAC"
        ));
        assert!(uri.len() < MAX_TOKEN_BYTES, "fits the 512-byte bound");
        let parsed = ManualPairingPayload::parse_uri(&uri).expect("parse");
        assert_eq!(parsed, p);
        // Bytes roundtrip too (the QR payload form).
        assert_eq!(ManualPairingPayload::from_bytes(&p.to_bytes()).unwrap(), p);
    }

    #[test]
    fn ipv6_and_no_label_roundtrip() {
        let p = ManualPairingPayload {
            host: "::1".to_string(),
            port: 1234,
            nonce: [7; 16],
            fingerprint: [9; 32],
            label: None,
        };
        let uri = p.encode_uri();
        assert!(uri.starts_with("wdr://[::1]:1234?nonce="));
        let parsed = ManualPairingPayload::parse_uri(&uri).expect("parse");
        assert_eq!(parsed, p);
    }

    #[test]
    fn malformed_tokens_are_rejected() {
        assert_eq!(
            ManualPairingPayload::parse_uri("http://1.2.3.4:9000?nonce=x&fp=y"),
            Err(ManualError::Scheme)
        );
        // Missing port.
        assert!(ManualPairingPayload::parse_uri("wdr://1.2.3.4").is_err());
        // Port 0.
        assert!(matches!(
            ManualPairingPayload::parse_uri("wdr://1.2.3.4:0?nonce=0&fp=0"),
            Err(ManualError::Port)
        ));
        // Short hex → wrong nonce length.
        assert!(ManualPairingPayload::parse_uri("wdr://1.2.3.4:9000?nonce=abcd&fp=0").is_err());
        // Non-hex chars.
        assert!(ManualPairingPayload::parse_uri(
            "wdr://1.2.3.4:9000?nonce=zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz&fp=0"
        )
        .is_err());
        // Label too long.
        let long = "x".repeat(40);
        assert!(ManualPairingPayload::parse_uri(&format!(
            "wdr://1.2.3.4:9000?nonce=00000000000000000000000000000000&fp=0000000000000000000000000000000000000000000000000000000000000000#{long}"
        ))
        .is_err());
        // Bytes over the 512-byte bound.
        let mut big = [0u8; MAX_TOKEN_BYTES + 1];
        big.fill(b'x');
        assert_eq!(
            ManualPairingPayload::from_bytes(&big),
            Err(ManualError::Bounds)
        );
    }
}
