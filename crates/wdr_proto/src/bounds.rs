//! Numeric bounds mirrored from the security & protocol specifications.
//!
//! Every value here is locked (`SECURITY_SPEC.md` §4, `PROTOCOL_SPEC.md`
//! §Numeric bounds) and is enforced at parse time **before any allocation**
//! sized by untrusted input (SEC-05).

/// Maximum size of a single encoded control message (≤ 16 KB).
pub const MAX_CONTROL_MSG: usize = 16 * 1024;

/// Maximum size of an audio frame payload after AEAD (≤ 4 KB),
/// enforced before allocation for decode/decrypt buffers.
pub const MAX_FRAME_PAYLOAD: usize = 4096;

/// Control request rate cap (requests/second per peer).
pub const MAX_REQUEST_RATE: u32 = 20;

/// Alias per the protocol-facing name (`MAX_RATE`).
pub const MAX_RATE: u32 = MAX_REQUEST_RATE;

/// Maximum concurrent in-progress pairing handshakes (listener, global).
pub const MAX_PENDING_HANDSHAKES: usize = 8;

/// Pairing window: connecting → confirmed/rejected (seconds).
pub const PAIRING_WINDOW_SECS: u64 = 60;

/// Session idle expiry with no payload (seconds).
pub const SESSION_IDLE_EXPIRY_SECS: u64 = 60;

/// Control response timeout (milliseconds).
pub const CONTROL_RESPONSE_TIMEOUT_SECS: u64 = 5;

/// Default replay window width (entries, per plane/direction).
pub const REPLAY_WINDOW_WIDTH: u64 = 256;

/// Maximum encoded size of a single mDNS TXT record (bytes).
pub const MAX_TXT_RECORD_BYTES: usize = 512;
