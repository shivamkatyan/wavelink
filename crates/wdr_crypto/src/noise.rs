//! Noise XX handshake for WDR pairing (SECURITY_SPEC §2.1, §2.2, §3.2; ADR-006).
//!
//! * Pattern/suite locked: `Noise_XX_25519_ChaChaPoly_BLAKE2s`, psk=0, no
//!   fallback pattern.
//! * Identity: ed25519 long-term keypair; the static X25519 key is derived
//!   deterministically from the ed25519 seed ([`crate::identity`]).
//! * SAS: the caller compares the *final* handshake hash out-of-band, using
//!   [`crate::session::sas_digits`] over [`NoiseHandshake::handshake_hash`].
//! * When a role is given an expected peer fingerprint, a mismatched presented
//!   static key is rejected with [`NoiseHandshakeError::FingerprintMismatch`].
//!
//! Message exchange is an explicit two-phase state machine so the *final*
//! transcript hash is captured at exactly the point it is defined
//! (immediately before the `Split`), and so message order is enforced by the
//! type system / method availability.

use std::convert::TryInto;

use snow::params::NoiseParams;
use snow::resolvers::DefaultResolver;
use snow::{Builder, HandshakeState, TransportState};

use crate::identity::IdentityKeyPair;

/// Locked Noise pattern + suite (SECURITY_SPEC §2.1).
pub const NOISE_PATTERN: &str = "Noise_XX_25519_ChaChaPoly_BLAKE2s";

/// Errors arising from the Noise handshake glue.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NoiseHandshakeError {
    /// snow-level failure.
    Snow(String),
    /// The peer's presented static X25519 key != the expected fingerprint.
    FingerprintMismatch,
    /// A buffer was too small for a handshake message.
    BufferTooSmall,
    /// The peer's static key was not available.
    MissingPeerStatic,
}

impl From<snow::Error> for NoiseHandshakeError {
    fn from(e: snow::Error) -> Self {
        Self::Snow(e.to_string())
    }
}

/// Directional (split) keys produced by the Noise `Split`
/// (SECURITY_SPEC §3.2: per-direction transport keys).
///
/// `initiator_key` is what the *initiator* encrypts with (both directions of
/// transport in Noise are keyed per-direction; the two split keys are labeled
/// initiator→responder and responder→initiator).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NoiseSessionKeys {
    /// Key used by the initiator for sending (responder decrypts).
    pub initiator_key: [u8; 32],
    /// Key used by the responder for sending (initiator decrypts).
    pub responder_key: [u8; 32],
}

/// A completed Noise XX session (transport mode) with the final transcript hash
/// captured for SAS and the peer static pubkey retained for QR binding.
pub struct NoiseHandshake {
    transport: TransportState,
    transcript_hash: [u8; 32],
    peer_static: Option<[u8; 32]>,
    keys: NoiseSessionKeys,
    initiator: bool,
}

impl NoiseHandshake {
    /// The final Noise handshake hash `h` (32 bytes, BLAKE2s) — the SAS source.
    /// Both roles compute the same value iff the handshake completed with
    /// matching inputs (SECURITY_SPEC §2.2).
    #[must_use]
    pub fn handshake_hash(&self) -> [u8; 32] {
        self.transcript_hash
    }

    /// The remote static X25519 public key established during the handshake
    /// (the "actual handshake pubkey" for QR binding).
    #[must_use]
    pub fn peer_static_x25519(&self) -> Option<[u8; 32]> {
        self.peer_static
    }

    /// Whether this peer ran the initiator role.
    #[must_use]
    pub fn is_initiator(&self) -> bool {
        self.initiator
    }

    /// The Noise split keys (per-direction). These feed
    /// [`crate::session::derive_session_keys`] for plane (control/media)
    /// separation.
    #[must_use]
    pub fn split_keys(&self) -> NoiseSessionKeys {
        self.keys
    }

    /// Write the next Noise transport message (app record stage).
    pub fn write_transport(
        &mut self,
        plaintext: &[u8],
        out: &mut [u8],
    ) -> Result<usize, NoiseHandshakeError> {
        self.transport
            .write_message(plaintext, out)
            .map_err(Into::into)
    }

    /// Read the next Noise transport message.
    pub fn read_transport(
        &mut self,
        packet: &[u8],
        out: &mut [u8],
    ) -> Result<usize, NoiseHandshakeError> {
        self.transport.read_message(packet, out).map_err(Into::into)
    }
}

/// A pairing handshake in progress, initiator role (has sent msg1, awaits msg2).
pub struct InitiatorHandshake {
    state: HandshakeState,
    expected_peer_static: Option<[u8; 32]>,
}

/// A pairing handshake in progress, responder role (has sent msg2, awaits msg3).
pub struct ResponderWaitingMsg3 {
    state: HandshakeState,
    expected_peer_static: Option<[u8; 32]>,
}

impl InitiatorHandshake {
    /// Create a pairing initiator. `expected_peer_static` (optional) pins the
    /// responder's fingerprint; a mismatch aborts at msg2 time.
    pub fn initiator(
        identity: IdentityKeyPair,
        expected_peer_static: Option<[u8; 32]>,
    ) -> Result<Self, NoiseHandshakeError> {
        let state = build(&identity, true)?;
        Ok(Self {
            state,
            expected_peer_static,
        })
    }

    /// Convenience alias for [`Self::initiator`].
    pub fn pair_initiator(
        identity: IdentityKeyPair,
        expected_peer_static: Option<[u8; 32]>,
    ) -> Result<Self, NoiseHandshakeError> {
        Self::initiator(identity, expected_peer_static)
    }

    /// Write message 1 (`e`) into `out`. Returns the written length.
    pub fn write_hello(&mut self, out: &mut [u8]) -> Result<usize, NoiseHandshakeError> {
        self.state
            .write_message(&[], out)
            .map_err(NoiseHandshakeError::from)
    }

    /// Consume message 2, write message 3 into `out`, and finish.
    /// Returns (msg3_len, [`NoiseHandshake`]).
    pub fn complete_after_msg2(
        mut self,
        msg2: &[u8],
        out: &mut [u8],
    ) -> Result<(usize, NoiseHandshake), NoiseHandshakeError> {
        self.state.read_message(msg2, &mut [])?;
        let n = self.state.write_message(&[], out)?;

        verify_peer_static(&self.state, self.expected_peer_static)?;
        let peer_static = peer_static_bytes(&self.state);
        let transcript = transcript_from(&self.state);

        // Capture the directional split keys, then convert to transport mode.
        let (raw1, raw2) = self.state.dangerously_get_raw_split();
        let keys = NoiseSessionKeys {
            initiator_key: raw1,
            responder_key: raw2,
        };
        let transport = self.state.into_transport_mode()?;

        Ok((
            n,
            NoiseHandshake {
                transport,
                transcript_hash: transcript,
                peer_static,
                keys,
                initiator: true,
            },
        ))
    }
}

impl ResponderWaitingMsg3 {
    /// Create a pairing responder, optionally pinned to an expected peer fingerprint.
    pub fn responder(
        identity: IdentityKeyPair,
        expected_peer_static: Option<[u8; 32]>,
    ) -> Result<Self, NoiseHandshakeError> {
        let state = build(&identity, false)?;
        Ok(Self {
            state,
            expected_peer_static,
        })
    }

    /// Convenience alias for [`Self::responder`].
    pub fn pair_responder(
        identity: IdentityKeyPair,
        expected_peer_static: Option<[u8; 32]>,
    ) -> Result<Self, NoiseHandshakeError> {
        Self::responder(identity, expected_peer_static)
    }

    /// Consume message 1 and write message 2 into `out`; returns written length.
    pub fn read_hello(
        &mut self,
        msg1: &[u8],
        out: &mut [u8],
    ) -> Result<usize, NoiseHandshakeError> {
        self.state.read_message(msg1, &mut [])?;
        self.state
            .write_message(&[], out)
            .map_err(NoiseHandshakeError::from)
    }

    /// Consume the final message 3 and finish, yielding [`NoiseHandshake`].
    pub fn complete_after_msg3(
        mut self,
        msg3: &[u8],
    ) -> Result<NoiseHandshake, NoiseHandshakeError> {
        self.state.read_message(msg3, &mut [])?;

        verify_peer_static(&self.state, self.expected_peer_static)?;
        let peer_static = peer_static_bytes(&self.state);
        let transcript = transcript_from(&self.state);

        let (raw1, raw2) = self.state.dangerously_get_raw_split();
        let keys = NoiseSessionKeys {
            initiator_key: raw1,
            responder_key: raw2,
        };
        let transport = self.state.into_transport_mode()?;

        Ok(NoiseHandshake {
            transport,
            transcript_hash: transcript,
            peer_static,
            keys,
            initiator: false,
        })
    }
}

// ---------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------

fn build(
    identity: &IdentityKeyPair,
    initiator: bool,
) -> Result<HandshakeState, NoiseHandshakeError> {
    let params: NoiseParams = NOISE_PATTERN.parse().map_err(NoiseHandshakeError::from)?;
    let mut b = Builder::with_resolver(params, Box::new(DefaultResolver));
    let static_bytes = identity.static_secret().to_bytes();
    b = b
        .local_private_key(&static_bytes)
        .map_err(NoiseHandshakeError::from)?;
    if initiator {
        b.build_initiator().map_err(Into::into)
    } else {
        b.build_responder().map_err(Into::into)
    }
}

fn transcript_from(hs: &HandshakeState) -> [u8; 32] {
    hs.get_handshake_hash()
        .try_into()
        .expect("BLAKE2s handshake transcript is 32 bytes")
}

fn peer_static_bytes(state: &HandshakeState) -> Option<[u8; 32]> {
    state
        .get_remote_static()
        .map(|s| s.try_into().expect("X25519 static is 32 bytes"))
}

fn verify_peer_static(
    state: &HandshakeState,
    expected: Option<[u8; 32]>,
) -> Result<(), NoiseHandshakeError> {
    let Some(expect) = expected else {
        return Ok(());
    };
    let Some(peer) = state.get_remote_static() else {
        return Err(NoiseHandshakeError::MissingPeerStatic);
    };
    let peer: [u8; 32] = peer
        .try_into()
        .map_err(|_| NoiseHandshakeError::MissingPeerStatic)?;
    if peer != expect {
        return Err(NoiseHandshakeError::FingerprintMismatch);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Convenience: one-shot helpers and the high-level pair API
// ---------------------------------------------------------------------------

/// Run a full XX pairing exchange in-memory between an initiator and responder,
/// returning the initiator's and responder's completed [`NoiseHandshake`]s.
///
/// This is the primary high-level entry point for tests and the pairing UI
/// (the transport (de)serialization of each message is left to the caller, which
/// for the parity of tests here just uses local buffers).
#[allow(clippy::many_single_char_names)]
pub fn pair_initiator<const N: usize, const M: usize, const O: usize>(
    init_identity: IdentityKeyPair,
    init_expected: Option<[u8; 32]>,
    resp_identity: IdentityKeyPair,
    resp_expected: Option<[u8; 32]>,
) -> Result<(NoiseHandshake, NoiseHandshake), NoiseHandshakeError> {
    let mut i = InitiatorHandshake::initiator(init_identity, init_expected)?;
    let mut r = ResponderWaitingMsg3::responder(resp_identity, resp_expected)?;

    let mut m1 = [0u8; N];
    let mut m2 = [0u8; M];
    let mut m3 = [0u8; O];

    let n1 = i.write_hello(&mut m1)?;
    let n2 = r.read_hello(&m1[..n1], &mut m2)?;
    let (n3, init_done) = i.complete_after_msg2(&m2[..n2], &mut m3)?;
    let resp_done = r.complete_after_msg3(&m3[..n3])?;

    Ok((init_done, resp_done))
}
