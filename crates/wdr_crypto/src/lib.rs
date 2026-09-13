//! `wdr_crypto` — Wavelink security foundation (task t-B0-crypto).
//!
//! Implements the locked crypto contract from [`SECURITY_SPEC`]:
//!
//! * **Noise XX pairing** — `Noise_XX_25519_ChaChaPoly_BLAKE2s` (psk=0, no
//!   fallback), via `snow`. [`noise`] gives an explicit state machine
//!   ([`noise::InitiatorHandshake`] / [`noise::ResponderWaitingMsg3`]) that
//!   binds a caller-supplied trusted peer fingerprint, captures the final
//!   transcript hash for SAS, and rejects a responder whose identity does not
//!   match the expected fingerprint.
//! * **Identity / keygen** — [`identity`]: ed25519 long-term identity keys
//!   (sign/verify), and the deterministic ed25519→x25519 static mapping used
//!   for the Noise static key.
//! * **AEAD session ciphers** — [`aead`]: XChaCha20-Poly1305 with a 24-byte
//!   nonce (`random_salt16 ‖ counter8(BE)`) by default, plus a 12-byte-nonce
//!   ChaCha20-Poly1305 path driven by a persisted monotonic counter, both with
//!   authenticated failure.
//! * **Replay protection** — [`replay::ReplayWindow`]: DTLS-style 256-entry bit
//!   window over u64 packet indices, `Mutex`-protected.
//! * **Key metadata / separation** — [`session`]: separate control vs media
//!   session keys derived from the Noise split keys via HKDF, fresh per session,
//!   with rekey triggers (24 h / 2^56 records).
//! * **0-RTT guard** — [`session::SessionStatus`]: no audio may be sent before
//!   the handshake completes ([`session::SessionStatus::can_transport_media`]).
//!
//! [`SECURITY_SPEC`]: ../../docs/planning/SECURITY_SPEC.md

pub mod aead;
pub mod data_cipher;
pub mod identity;
pub mod noise;
pub mod replay;
pub mod session;

pub use aead::{
    ChaCha12SessionCipher, DecryptError12, MonotonicCounter, SaltedNonce, SessionKey,
    XChaChaSessionCipher,
};
pub use data_cipher::{SessionDataCipher, SessionDecryptOutcome};
pub use identity::{
    ed25519_to_x25519_static, generate_ephemeral, x25519_shared_secret, IdentityKeyPair,
};
pub use noise::{NoiseHandshake, NoiseHandshakeError, NoiseSessionKeys, NOISE_PATTERN};
pub use replay::ReplayWindow;
pub use session::{Direction, Plane, SessionKeys, SessionStatus};

/// Security-relevant constants from SECURITY_SPEC (values locked by the spec).
pub mod constants {
    /// Replay window width (entries), DTLS-style, per (plane, direction).
    pub const REPLAY_WINDOW_SIZE: u64 = 256;

    /// Key age rekey trigger: 24 h per SECURITY_SPEC §3.5.
    pub const REKEY_AFTER_AGE_SECS: u64 = 24 * 60 * 60;

    /// Record-count rekey trigger per key scope (spec §3.5).
    pub const REKEY_AFTER_RECORDS: u64 = 1 << 56;

    /// Count of AEAD auth failures within the window that force a rekey attempt.
    pub const AUTH_FAILURE_REKEY_THRESHOLD: u32 = 3;

    /// The window (in seconds) the auth-failure count is evaluated over.
    pub const AUTH_FAILURE_WINDOW_SECS: u32 = 60;
}
