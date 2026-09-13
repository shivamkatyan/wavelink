//! Session key metadata, plane (control/media) separation, SAS binding and the
//! 0-RTT guard (SECURITY_SPEC §3.2, §3.5, §3.6).

use hkdf::Hkdf;
use sha2::Sha256;

use crate::noise::NoiseSessionKeys;

/// A transport plane.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Plane {
    Control,
    Media,
}

/// A direction within a plane.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Tx,
    Rx,
}

/// Session lifecycle status — used to enforce the 0-RTT guard
/// (SECURITY_SPEC §3.6 / SEC-13): media may only be sent after the Noise
/// handshake has confirmed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionStatus {
    /// Pairing/connecting; handshake not yet confirmed. `can_transport_media() == false`.
    Handshaking,
    /// Session keys established; media transport allowed.
    Confirmed,
    /// Rejected (fingerprint mismatch, downgrade, revoke, auth-failure exhaustion…).
    Rejected,
    /// Session ended normally.
    Terminated,
}

impl SessionStatus {
    /// The 0-RTT guard: audio may only be transported after the handshake
    /// completes and the peer is confirmed. Returns `false` for every state
    /// except `Confirmed`.
    #[must_use]
    pub const fn can_transport_media(&self) -> bool {
        matches!(self, Self::Confirmed)
    }

    /// Shorthand for the guard on the control plane (same rule).
    #[must_use]
    pub const fn can_send_control(&self) -> bool {
        matches!(self, Self::Confirmed)
    }
}

/// Per-session traffic keys, separated per plane (control/media) and direction
/// (tx/rx) — 4 keys/session (SECURITY_SPEC §3.2). All memory-only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionKeys {
    pub control_tx: [u8; 32],
    pub control_rx: [u8; 32],
    pub media_tx: [u8; 32],
    pub media_rx: [u8; 32],
    /// Session-unique random identifier (for correlation, non-secret).
    pub session_id: u64,
}

impl SessionKeys {
    /// Look up the key for a (plane, direction).
    #[must_use]
    pub fn key(&self, plane: Plane, dir: Direction) -> [u8; 32] {
        match (plane, dir) {
            (Plane::Control, Direction::Tx) => self.control_tx,
            (Plane::Control, Direction::Rx) => self.control_rx,
            (Plane::Media, Direction::Tx) => self.media_tx,
            (Plane::Media, Direction::Rx) => self.media_rx,
        }
    }

    /// Whether control and media are cryptographically separated (distinct keys).
    #[must_use]
    pub fn planes_separated(&self) -> bool {
        self.control_tx != self.media_tx && self.control_rx != self.media_rx
    }
}

/// Derive the per-plane/per-direction session keys from the Noise split keys
/// via HKDF-SHA-256 with distinct `info` labels (domain separation) and a
/// per-session salt.
///
/// * `extract` over the *concatenated* split keys (both directions) with the
///   per-session `session_id` as salt,
/// * `expand` with `info = b"wdr-plane-v1"` producing 128 output bytes split
///   as `control_tx ‖ control_rx ‖ media_tx ‖ media_rx`.
///
/// Note: the Noise split already separates the *direction* (initiator-egress vs
/// responder-egress); here we additionally separate *planes* so a compromise of
/// one plane's key does not decrypt the other (T1/T8 in SECURITY_SPEC §1). The
/// `session_id` (session-unique) is mixed into the extract salt so that even a
/// pathological reuse of split material still yields per-session keys
/// (SEC-02: fresh key establishment, no reuse across sessions).
#[must_use]
pub fn derive_session_keys(split: &NoiseSessionKeys, session_id: u64) -> SessionKeys {
    // Concatenate both directional split keys as the IKM.
    let mut ikm = [0u8; 64];
    ikm[..32].copy_from_slice(&split.initiator_key);
    ikm[32..].copy_from_slice(&split.responder_key);

    // Session-unique salt (never identical across sessions).
    let salt = session_id.to_le_bytes();

    let hk = Hkdf::<Sha256>::new(Some(&salt), &ikm);
    let mut out = [0u8; 128];
    hk.expand(b"wdr-plane-v1", &mut out)
        .expect("128 bytes of HKDF output is inside the 255*32 limit");

    SessionKeys {
        control_tx: first32(&out[0..32]),
        control_rx: first32(&out[32..64]),
        media_tx: first32(&out[64..96]),
        media_rx: first32(&out[96..128]),
        session_id,
    }
}

/// Derive the per-direction Noise transport keys (used by the noise module when
/// the split keys are not directly requested). Kept for API symmetry.
#[must_use]
pub fn derive_directional_keys(transcript: &[u8; 32]) -> NoiseSessionKeys {
    let hk = Hkdf::<Sha256>::new(None, transcript);
    let mut out = [0u8; 64];
    hk.expand(b"wdr-direction-v1", &mut out)
        .expect("64 bytes of HKDF output is inside the 255*32 limit");
    NoiseSessionKeys {
        initiator_key: first32(&out[0..32]),
        responder_key: first32(&out[32..64]),
    }
}

fn first32(s: &[u8]) -> [u8; 32] {
    s.try_into().expect("32-byte slice")
}

/// The 6-digit SAS shown to the user, derived from the final handshake hash
/// (SECURITY_SPEC §2.2). Deterministic and stable for a given transcript.
#[must_use]
pub fn sas_digits(h: &[u8; 32]) -> String {
    // Take 20 bits from the first 3 bytes to get 000000..999999 range-free.
    let value = u32::from_le_bytes([h[0], h[1], h[2], 0]) & 0x000F_FFFF;
    let value = value % 1_000_000;
    format!("{value:06}")
}

/// Rekey triggers per SECURITY_SPEC §3.5 (24 h or 2^56 records, whichever first).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RekeyPolicy {
    /// Seconds the current key scope has been in use.
    pub age_secs: u64,
    /// Records encrypted in this key scope.
    pub records: u64,
}

impl RekeyPolicy {
    /// Whether the policy requires an in-session rekey.
    #[must_use]
    pub fn should_rekey(&self) -> bool {
        self.age_secs >= crate::constants::REKEY_AFTER_AGE_SECS
            || self.records >= crate::constants::REKEY_AFTER_RECORDS
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_split() -> NoiseSessionKeys {
        NoiseSessionKeys {
            initiator_key: [0x11; 32],
            responder_key: [0x22; 32],
        }
    }

    #[test]
    fn key_separation_control_vs_media() {
        let keys = derive_session_keys(&sample_split(), 7);
        assert_ne!(keys.control_tx, keys.media_tx);
        assert_ne!(keys.control_rx, keys.media_rx);
        assert_ne!(keys.control_tx, keys.control_rx);
        assert!(keys.planes_separated());
    }

    #[test]
    fn fresh_keys_differ_per_session() {
        let a = derive_session_keys(&sample_split(), 1);
        let b = derive_session_keys(&sample_split(), 2);
        assert_ne!(a.media_tx, b.media_tx);
        assert_ne!(a.session_id, b.session_id);
    }

    #[test]
    fn planewise_lookup_matches() {
        let keys = derive_session_keys(&sample_split(), 3);
        assert_eq!(keys.key(Plane::Control, Direction::Tx), keys.control_tx);
        assert_eq!(keys.key(Plane::Media, Direction::Rx), keys.media_rx);
    }

    #[test]
    fn sas_is_6_digits_and_stable() {
        let h = [0u8; 32];
        let s1 = sas_digits(&h);
        let s2 = sas_digits(&h);
        assert_eq!(s1.len(), 6);
        assert_eq!(s1, s2);
        // Different transcript -> very likely different SAS.
        let mut h2 = h;
        h2[0] = 1;
        assert_ne!(sas_digits(&h), sas_digits(&h2));
        for ch in s1.chars() {
            assert!(ch.is_ascii_digit());
        }
    }

    #[test]
    fn rekey_triggers_on_age_and_record_count() {
        assert!(!RekeyPolicy {
            age_secs: 60,
            records: 1_000
        }
        .should_rekey());
        assert!(RekeyPolicy {
            age_secs: crate::constants::REKEY_AFTER_AGE_SECS,
            records: 0
        }
        .should_rekey());
        assert!(RekeyPolicy {
            age_secs: 0,
            records: crate::constants::REKEY_AFTER_RECORDS
        }
        .should_rekey());
        // A hair under both thresholds does not rekey.
        assert!(!RekeyPolicy {
            age_secs: crate::constants::REKEY_AFTER_AGE_SECS - 1,
            records: crate::constants::REKEY_AFTER_RECORDS - 1,
        }
        .should_rekey());
    }

    #[test]
    fn zero_rtt_guard_blocks_media_before_confirmation() {
        let s = SessionStatus::Handshaking;
        assert!(!s.can_transport_media());
        assert!(!s.can_send_control());
        assert!(SessionStatus::Confirmed.can_transport_media());
        assert!(!SessionStatus::Rejected.can_transport_media());
        assert!(!SessionStatus::Terminated.can_transport_media());
    }
}
