//! Protocol error taxonomy (retryable vs terminal) and decode errors.
//!
//! See `PROTOCOL_SPEC.md` §Error taxonomy and `SECURITY_SPEC.md` §5.1:
//! every error carries a retryable flag, a stable code and a recovery action.

use serde::{Deserialize, Serialize};

/// Stable error codes used on the wire (control `Error` message).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ErrorCode {
    /// Unparseable or protocol-incompatible message (bounded parsers; SEC-05).
    Malformed,
    /// Encoded message exceeded a configured size bound.
    TooLarge,
    /// Peer offered an older protocol version (terminal; SEC-08)
    VersionTooOld,
    /// Peer offered a different/weaker handshake pattern or cipher suite (SEC-08).
    PatternMismatch,
    /// Request rate cap exceeded (`MAX_REQUEST_RATE`/s). Not queued, dropped.
    RateLimited,
    /// Exceeded pending-handshake cap (`MAX_PENDING_HANDSHAKES`).
    PendingHandshakeFull,
    /// Pairing window expired (`PAIRING_WINDOW_SECS`); fresh restart required.
    PairingTimeout,
    /// Pairing attempt rejected (e.g. fingerprint mismatch, revoked, caps).
    PairAttemptRejected,
    /// SAS/QR mismatch confirmed off-channel; re-confirm with fresh attempt.
    SasMismatch,
    /// Authentication/tamper failure on a control or media record.
    AuthenticationFailure,
    /// Policy/capability incompatibility (terminal for session; negotiate).
    PolicyViolation,
    /// Trust-store / capability rejection: this identity is revoked (SEC-09).
    Revoked,
    /// A requested resource or operation is gone / no longer valid.
    MissingResource,
    /// Local-network (mDNS/discovery) permission is missing or denied; the
    /// user must grant it in OS settings before discovery/pairing works
    /// (iOS local-network, Android nearby, Windows network capabilities).
    LocalNetworkPermission,
    /// Transient internal error (retryable).
    Internal,
    /// Reserved extension slot (future codes; never treated as a real error).
    Reserved,
}

impl ErrorCode {
    /// Whether this error class is retryable (transient) or terminal
    /// (policy/capability/permission; PROTOCOL_SPEC §Error taxonomy).
    #[must_use]
    pub const fn is_retryable(&self) -> bool {
        use ErrorCode::*;
        match self {
            Malformed
            | TooLarge
            | RateLimited
            | PendingHandshakeFull
            | PairingTimeout
            | AuthenticationFailure
            | Internal => true,
            VersionTooOld
            | PatternMismatch
            | PairAttemptRejected
            | SasMismatch
            | PolicyViolation
            | Revoked
            | MissingResource
            | LocalNetworkPermission
            | Reserved => false,
        }
    }
}

/// Recovery actions surfaced to the user by the UI/diagnostics layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum UserAction {
    /// Grant the missing permission (e.g. LAN/mDNS/local-network).
    GrantPermission,
    /// Reconnect/verify the DAC (power, cable, wake, re-pair).
    ReconnectDac,
    /// Adjust the jitter/transport buffer configuration.
    ChangeBuffer,
    /// Move to Wi-Fi (drop the unreliable path).
    ReturnToWifi,
    /// No user action; retry transparently or informational only.
    None,
}

/// Control-plane protocol error: code + retryable flag + user action + codec
/// version. The retryable flag is the source of truth; `code` is for logging:
impl Error {
    /// Build a `ProtocolError` from a code, which derives the retryable flag
    /// and a best-effort user action.
    pub const fn protocol(code: ErrorCode) -> Self {
        let action = user_action_for(code);
        Self {
            code,
            retryable: code.is_retryable(),
            user_action: action,
            version: 1,
        }
    }

    /// Whether this error is retryable (transient network/route etc.).
    #[must_use]
    pub const fn is_retryable(&self) -> bool {
        self.retryable
    }

    /// Human recovery action to surface to the user.
    #[must_use]
    pub const fn user_action(&self) -> UserAction {
        self.user_action
    }
}

const fn user_action_for(code: ErrorCode) -> UserAction {
    use ErrorCode::*;
    match code {
        Malformed | Internal | TooLarge | RateLimited | PendingHandshakeFull => UserAction::None,
        PairingTimeout | AuthenticationFailure => UserAction::None,
        LocalNetworkPermission => UserAction::GrantPermission,
        VersionTooOld | PatternMismatch => UserAction::ReconnectDac,
        PairAttemptRejected | SasMismatch | Revoked => UserAction::ReconnectDac,
        PolicyViolation => UserAction::ChangeBuffer,
        MissingResource => UserAction::ReturnToWifi,
        Reserved => UserAction::None,
    }
}

/// Wire-format protocol error (the control `Error` message payload).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Error {
    /// Stable wire code.
    pub code: ErrorCode,
    /// Retryable vs terminal (surfaced directly; source of truth).
    pub retryable: bool,
    /// Suggested recovery action (may be `None`).
    pub user_action: UserAction,
    /// Error scheme version for forward compatibility.
    pub version: u32,
}

/// Typed decode failure for `postcard` (roundtrip/wire decode helpers).
///
/// `LimitExceeded` is produced when an oversized or truncated message trips a
/// decode-size cap *before* any allocation beyond the bounded target buffer
/// (SECURITY_SPEC §4 / SEC-05). This mirrors `postcard::Error` without
/// depending on its formatting, so no allocation is needed to report it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecodeError {
    /// Input declared more length/data than the target bound allows, or the
    /// target container would exceed its cap.
    LimitExceeded,
    /// Input ran out of bytes while decoding a complete value.
    Truncated,
    /// Well-formed input that is not a valid member of the wire schema.
    DataInvalid,
    /// Message fell under the protocol version floor (`msg_ver`).
    VersionTooNew,
    /// The frame payload exceeded `MAX_FRAME_PAYLOAD` (checked before buffering).
    PayloadOverflow,
}

impl DecodeError {
    /// Human-readable stable description (no heap allocation).
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::LimitExceeded => "message exceeds the declared size cap",
            Self::Truncated => "message truncated before complete",
            Self::DataInvalid => "data not a valid member of the wire schema",
            Self::VersionTooNew => "message version newer than this decoder",
            Self::PayloadOverflow => "frame payload exceeds MAX_FRAME_PAYLOAD",
        }
    }
}

impl core::fmt::Display for DecodeError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl core::fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(match self {
            Self::Malformed => "malformed",
            Self::TooLarge => "too-large",
            Self::VersionTooOld => "version-too-old",
            Self::PatternMismatch => "pattern-mismatch",
            Self::RateLimited => "rate-limited",
            Self::PendingHandshakeFull => "pending-handshake-full",
            Self::PairingTimeout => "pairing-timeout",
            Self::PairAttemptRejected => "pair-attempt-rejected",
            Self::SasMismatch => "sas-mismatch",
            Self::AuthenticationFailure => "authentication-failure",
            Self::PolicyViolation => "policy-violation",
            Self::Revoked => "revoked",
            Self::MissingResource => "missing-resource",
            Self::LocalNetworkPermission => "local-network-permission",
            Self::Internal => "internal",
            Self::Reserved => "reserved",
        })
    }
}

impl core::fmt::Display for UserAction {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(match self {
            Self::GrantPermission => "grant-permission",
            Self::ReconnectDac => "reconnect-dac",
            Self::ChangeBuffer => "change-buffer",
            Self::ReturnToWifi => "return-to-wifi",
            Self::None => "none",
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ErrorCode::*;

    #[test]
    fn retryable_classification() {
        assert!(TooLarge.is_retryable());
        assert!(RateLimited.is_retryable());
        assert!(!Revoked.is_retryable());
        assert!(!VersionTooOld.is_retryable());
        assert!(!PolicyViolation.is_retryable());
    }

    #[test]
    fn protocol_error_carries_flag_and_action() {
        let e = Error::protocol(Revoked);
        assert_eq!(e.code, Revoked);
        assert!(!e.is_retryable());
        assert_eq!(e.user_action(), UserAction::ReconnectDac);

        let e = Error::protocol(RateLimited);
        assert!(e.is_retryable());
        assert_eq!(e.user_action(), UserAction::None);
    }

    #[test]
    fn decode_error_display() {
        assert_eq!(
            DecodeError::Truncated.to_string(),
            "message truncated before complete"
        );
        assert_eq!(
            DecodeError::PayloadOverflow.to_string(),
            "frame payload exceeds MAX_FRAME_PAYLOAD"
        );
    }
}
