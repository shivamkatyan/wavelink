//! `wdr_crypto` wiring (WS-D): Noise XX pairing + per-frame AEAD + fingerprint
//! pinning over the QUIC reliable lane, **opt-in**.
//!
//! Loopback security for the reference sim is quinn TLS 1.3 with an
//! accept-any server identity. When a caller wants real pairing semantics, it
//! pins a peer fingerprint and this module runs a `Noise_XX` handshake over a
//! magic-prefixed bidirectional stream, then each media frame payload is
//! wrapped in `AEAD(seq, payload)` (XChaCha20-Poly1305 via
//! `wdr_crypto`) before packing — the per-frame CRC then covers ciphertext, so
//! a tampered frame fails authentication on the receiver.
//!
//! The lane is **reliable/lossless-only** this workstream (`connect_secure`
//! refuses Opus): the lossy datagram lane can reorder/drop, which would
//! desynchronize the AEAD index from `Frame.seq` (recorded follow-up).
//!
//! Cross-peer key discipline (a real trap): the media plane's two `SessionDataCipher`s
//! on *both* peers must use the **same key bytes** (`session_keys.media_tx`);
//! `Direction` is only a local label and the AEAD salt derives from the key, not
//! the direction — pairing `key(Media,Tx)` with `key(Media,Rx)` would break
//! every frame. `seal_frame` asserts sender index == `Frame.seq` so the
//! reliable lane's 1:1 emit-per-seq keeps encrypt/decrypt indexes aligned.

use wdr_codec::size::MAX_FRAME_PAYLOAD;
use wdr_crypto::data_cipher::{SessionDataCipher, SessionDecryptOutcome};
use wdr_crypto::identity::IdentityKeyPair;
use wdr_crypto::noise::{InitiatorHandshake, NoiseHandshakeError, ResponderWaitingMsg3};
use wdr_crypto::session::{derive_session_keys, Direction, Plane, SessionKeys, SessionStatus};

/// Byte prefix distinguishing the secure lane's first stream item from a plain
/// frame item (a Frame header can never begin with these bytes).
pub const SECURE_MAGIC: &[u8; 3] = b"WDR";

/// A peer's parameters for a secure lane.
pub struct SecureParams {
    /// This peer's long-term identity.
    pub identity: IdentityKeyPair,
    /// Pin the peer's X25519 static fingerprint; `None` accepts what is presented.
    pub expected_peer_fingerprint: Option<[u8; 32]>,
}

/// A completed secure session: handshake transcript + per-plane session keys
/// and the media-plane AEAD ciphers wired on the shared flow key.
pub struct SecureSession {
    pub handshake_hash: [u8; 32],
    /// The peer's X25519 static pubkey (what the Noise handshake established;
    /// the value a fingerprint pin is compared against).
    pub peer_static: [u8; 32],
    pub session_keys: SessionKeys,
    /// Emitter encrypt path (media plane, `media_tx`).
    tx_cipher: SessionDataCipher,
    /// Receiver decrypt path (media plane, `media_tx`).
    rx_cipher: SessionDataCipher,
}

/// Typed errors from the secure lane. No panics on data.
#[derive(Debug)]
pub enum SecError {
    Noise(NoiseHandshakeError),
    /// Stream I/O or buffer failure during the handshake.
    Transport(String),
    /// The first stream item did not carry the secure magic.
    NotSecure,
    /// The receiver's decrypt index fell out of the replay window (tag valid
    /// but replayed/out-of-order) — SECURITY_SPEC §3.4.
    Replay,
    /// AEAD authentication failed (tamper / wrong key) — §3.5.
    AuthFailed,
    /// The cipher refused (session not confirmed) — 0-RTT guard.
    NotReady,
    /// Sender index diverged from `Frame.seq` (would desync decrypt).
    IndexDivergence {
        expected: u64,
        got: u64,
    },
    /// `payload + tag` would exceed the bounded frame payload.
    PayloadTooLarge,
}

impl From<NoiseHandshakeError> for SecError {
    fn from(e: NoiseHandshakeError) -> Self {
        SecError::Noise(e)
    }
}

impl core::fmt::Display for SecError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            SecError::Noise(e) => write!(f, "secure: noise: {e:?}"),
            SecError::Transport(s) => write!(f, "secure: transport: {s}"),
            SecError::NotSecure => write!(f, "secure: first item lacks the WDR magic"),
            SecError::Replay => write!(f, "secure: replay/out-of-window AEAD index"),
            SecError::AuthFailed => write!(f, "secure: AEAD authentication failed"),
            SecError::NotReady => write!(f, "secure: session not confirmed (0-RTT guard)"),
            SecError::IndexDivergence { expected, got } => {
                write!(f, "secure: encrypt index {got} != Frame.seq {expected}")
            }
            SecError::PayloadTooLarge => {
                write!(f, "secure: frame + AEAD tag exceeds the payload cap")
            }
        }
    }
}
impl std::error::Error for SecError {}

const HS_BUF: usize = 256;

/// Run the initiator half of the Noise XX handshake over the first
/// bidirectional stream (magic-prefixed length-prefixed items).
pub async fn initiator_handshake(
    conn: &quinn::Connection,
    p: SecureParams,
) -> Result<SecureSession, SecError> {
    let mut i = InitiatorHandshake::initiator(p.identity, p.expected_peer_fingerprint)?;
    let (mut send, mut recv) = conn
        .open_bi()
        .await
        .map_err(|e| SecError::Transport(format!("open_bi: {e}")))?;

    let mut m1 = [0u8; HS_BUF];
    let n1 = i.write_hello(&mut m1)?;
    let mut first = Vec::with_capacity(SECURE_MAGIC.len() + n1);
    first.extend_from_slice(SECURE_MAGIC);
    first.extend_from_slice(&m1[..n1]);
    write_item(&mut send, &first).await?;

    let m2 = read_item(&mut recv).await?;
    let mut m3 = [0u8; HS_BUF];
    let (n3, done) = i.complete_after_msg2(&m2, &mut m3)?;
    write_item(&mut send, &m3[..n3]).await?;
    send.finish()
        .map_err(|e| SecError::Transport(format!("send.finish: {e}")))?;

    Ok(finish_session(&done))
}

/// Run the responder half of the Noise XX handshake over the first
/// bidirectional stream the emitter opens.
pub async fn responder_handshake(
    conn: &quinn::Connection,
    p: SecureParams,
) -> Result<SecureSession, SecError> {
    let mut r = ResponderWaitingMsg3::responder(p.identity, p.expected_peer_fingerprint)?;
    let (mut send, mut recv) = conn
        .accept_bi()
        .await
        .map_err(|e| SecError::Transport(format!("accept_bi: {e}")))?;

    let first = read_item(&mut recv).await?;
    if first.len() < SECURE_MAGIC.len() || &first[..SECURE_MAGIC.len()] != SECURE_MAGIC {
        return Err(SecError::NotSecure);
    }
    let msg1 = &first[SECURE_MAGIC.len()..];
    let mut m2 = [0u8; HS_BUF];
    let n2 = r.read_hello(msg1, &mut m2)?;
    write_item(&mut send, &m2[..n2]).await?;

    let m3 = read_item(&mut recv).await?;
    let done = r.complete_after_msg3(&m3)?;
    send.finish()
        .map_err(|e| SecError::Transport(format!("send.finish: {e}")))?;

    Ok(finish_session(&done))
}

/// Build the [`SecureSession`] from a completed Noise handshake: derive the
/// per-plane session keys (session-unique id from the transcript) and wire both
/// media ciphers on the SAME flow key (`media_tx` — see the module note).
fn finish_session(done: &wdr_crypto::noise::NoiseHandshake) -> SecureSession {
    let session_id = u64::from_le_bytes(done.handshake_hash()[..8].try_into().expect("8 bytes"));
    let keys = derive_session_keys(&done.split_keys(), session_id);
    let tx_cipher = SessionDataCipher::new(
        keys.media_tx,
        Plane::Media,
        Direction::Tx,
        SessionStatus::Confirmed,
    );
    let rx_cipher = SessionDataCipher::new(
        keys.media_tx,
        Plane::Media,
        Direction::Rx,
        SessionStatus::Confirmed,
    );
    SecureSession {
        handshake_hash: done.handshake_hash(),
        peer_static: done
            .peer_static_x25519()
            .expect("XX handshake always establishes the peer static"),
        session_keys: keys,
        tx_cipher,
        rx_cipher,
    }
}

/// Encrypt one media frame payload for `seq` (emitter side), asserting the
/// sender's AEAD index stays aligned with `Frame.seq` (reliable lane 1:1).
pub fn seal_frame(sess: &mut SecureSession, seq: u64, payload: &[u8]) -> Result<Vec<u8>, SecError> {
    if payload.len() + XCHACHA_TAG_BYTES > MAX_FRAME_PAYLOAD {
        return Err(SecError::PayloadTooLarge);
    }
    let expected = sess.tx_cipher.next_out_index();
    if expected != seq {
        return Err(SecError::IndexDivergence { expected, got: seq });
    }
    sess.tx_cipher
        .encrypt_session_data(payload)
        .ok_or(SecError::NotReady)
}

/// Decrypt one media frame payload for `seq` (receiver side) after the wire CRC
/// already passed. Replay/auth failures surface here, never as a panic.
pub fn open_frame(
    sess: &mut SecureSession,
    seq: u64,
    ciphertext: &[u8],
) -> Result<Vec<u8>, SecError> {
    match sess.rx_cipher.decrypt_session_data(seq, ciphertext) {
        SessionDecryptOutcome::Ok(pt) => Ok(pt),
        SessionDecryptOutcome::Rejected => Err(SecError::Replay),
        SessionDecryptOutcome::AuthFailed => Err(SecError::AuthFailed),
        SessionDecryptOutcome::NotReady => Err(SecError::NotReady),
    }
}

const XCHACHA_TAG_BYTES: usize = 16;

async fn write_item(send: &mut quinn::SendStream, body: &[u8]) -> Result<(), SecError> {
    let len = u16::try_from(body.len()).map_err(|_| SecError::PayloadTooLarge)?;
    let mut buf = Vec::with_capacity(2 + body.len());
    buf.extend_from_slice(&len.to_le_bytes());
    buf.extend_from_slice(body);
    send.write_all(&buf)
        .await
        .map_err(|e| SecError::Transport(format!("write: {e}")))
}

async fn read_item(recv: &mut quinn::RecvStream) -> Result<Vec<u8>, SecError> {
    let mut len_buf = [0u8; 2];
    read_exact(recv, &mut len_buf).await?;
    let len = u16::from_le_bytes(len_buf) as usize;
    if len > MAX_FRAME_PAYLOAD + 13 {
        return Err(SecError::Transport("item exceeds the bounded size".into()));
    }
    let mut body = vec![0u8; len];
    read_exact(recv, &mut body).await?;
    Ok(body)
}

async fn read_exact(recv: &mut quinn::RecvStream, buf: &mut [u8]) -> Result<(), SecError> {
    let mut off = 0;
    while off < buf.len() {
        let n = tokio::time::timeout(
            std::time::Duration::from_millis(5_000),
            recv.read(&mut buf[off..]),
        )
        .await
        .map_err(|_| SecError::Transport("read timeout".into()))?
        .map_err(|_| SecError::Transport("read error".into()))?;
        match n {
            Some(0) | None => return Err(SecError::Transport("stream ended early".into())),
            Some(n) => off += n,
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn alice_bob() -> (SecureParams, SecureParams) {
        let a = IdentityKeyPair::from_seed([1u8; 32]);
        let b = IdentityKeyPair::from_seed([2u8; 32]);
        let a_fp = a.static_public();
        let b_fp = b.static_public();
        (
            SecureParams {
                identity: a,
                expected_peer_fingerprint: Some(b_fp),
            },
            SecureParams {
                identity: b,
                expected_peer_fingerprint: Some(a_fp),
            },
        )
    }

    /// One in-memory Noise XX exchange → (emitter session, receiver session).
    /// Both `finish_session` results wire their media ciphers on the SAME
    /// `media_tx` flow key, so the emitter's tx cipher and the receiver's rx
    /// cipher interoperate (the cross-peer key discipline, tested here).
    fn session_tx_rx_pair() -> (SecureSession, SecureSession) {
        let (pa, pb) = alice_bob();
        let (i_done, r_done) = wdr_crypto::noise::pair_initiator::<256, 256, 256>(
            pa.identity,
            pa.expected_peer_fingerprint,
            pb.identity,
            pb.expected_peer_fingerprint,
        )
        .expect("in-memory Noise XX pair");
        (finish_session(&i_done), finish_session(&r_done))
    }

    #[test]
    fn seal_open_roundtrip_and_replay_rejection() {
        let (mut tx, mut rx) = session_tx_rx_pair();
        let pt = b"a lossless frame payload";
        let ct0 = seal_frame(&mut tx, 0, pt).expect("seal");
        let ct1 = seal_frame(&mut tx, 1, pt).expect("seal");

        assert_eq!(open_frame(&mut rx, 0, &ct0).unwrap(), pt);
        assert_eq!(open_frame(&mut rx, 1, &ct1).unwrap(), pt);
        // Reopening an old index is a replay.
        assert!(matches!(
            open_frame(&mut rx, 0, &ct0),
            Err(SecError::Replay)
        ));
        // Encryption index must match Frame.seq.
        assert!(matches!(
            seal_frame(&mut tx, 99, pt),
            Err(SecError::IndexDivergence { .. })
        ));
    }

    #[test]
    fn tampered_ciphertext_fails_authentication() {
        let (mut tx, mut rx) = session_tx_rx_pair();
        let pt = b"frame";
        let mut ct = seal_frame(&mut tx, 0, pt).unwrap();
        ct[0] ^= 0x80;
        assert!(matches!(
            open_frame(&mut rx, 0, &ct),
            Err(SecError::AuthFailed)
        ));
    }
}
