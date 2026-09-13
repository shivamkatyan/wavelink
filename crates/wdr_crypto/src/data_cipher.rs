//! `SessionDataCipher` — the per-(plane, direction) authenticated encrypt/decrypt
//! wrapper the session core uses for `encrypt_session_data` / `decrypt`, tying
//! together the AEAD cipher (XChaCha20-Poly1305 default) with monotonic index
//! enforcement, the replay window, and the 0-RTT/`SessionStatus` gate
//! (SECURITY_SPEC §3.1–§3.6).

use crate::aead::{SessionKey, XChaChaSessionCipher};
use crate::replay::{ReplayDecision, ReplayWindow};
use crate::session::{Direction, Plane, SessionStatus};
/// Outcome of a `decrypt_session_data` call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionDecryptOutcome {
    /// Authenticated plaintext.
    Ok(Vec<u8>),
    /// The packet index was a replay or out-of-window (SECURITY_SPEC §3.4).
    Rejected,
    /// The AEAD authentication tag failed (tamper / wrong key) — counts toward
    /// the rekey threshold (SECURITY_SPEC §3.5).
    AuthFailed,
    /// The session is not yet confirmed (0-RTT off) or terminated.
    NotReady,
}

/// Per-(plane, direction) session data cipher: XChaCha20-Poly1305 AEAD +
/// monotonic index accounting + a 256-entry replay window.
pub struct SessionDataCipher {
    cipher: XChaChaSessionCipher,
    /// Next outbound packet index for this key scope.
    out_index: u64,
    /// Inbound replay window (covers this direction's receive path).
    inbound: ReplayWindow,
    plane: Plane,
    direction: Direction,
    /// Signaled by the session core; `encrypt_session_data` refuses when not Confirmed.
    status: SessionStatus,
}

impl SessionDataCipher {
    /// Create a data cipher for one (plane, direction). The per-key-scope salt is
    /// derived deterministically from the session key via HKDF, so the two peers
    /// that share a flow key derive the *same* salt without an explicit handshake
    /// (the key already uniquely identifies the (plane, direction) scope, since
    /// session keys are separated per plane/direction).
    #[must_use]
    pub fn new(key: SessionKey, plane: Plane, direction: Direction, status: SessionStatus) -> Self {
        let salt = derive_salt(&key);
        Self {
            cipher: XChaChaSessionCipher::new(key, salt),
            out_index: 0,
            inbound: ReplayWindow::new(),
            plane,
            direction,
            status,
        }
    }

    /// The (plane, direction) this cipher serves.
    #[must_use]
    pub fn target(&self) -> (Plane, Direction) {
        (self.plane, self.direction)
    }

    /// Update the session status gate (e.g. from `Handshaking` to `Confirmed`).
    pub fn set_status(&mut self, status: SessionStatus) {
        self.status = status;
    }

    /// Encrypt session data with the next outbound index.
    ///
    /// * Returns `None` if the 0-RTT guard refuses (session not `Confirmed`) —
    ///   this is the SEC-13 assertion that no audio is sent pre-handshake.
    /// * Liveness invariant (debug builds): a live cipher is never `Rejected` or
    ///   `Terminated`; those states are transitioned out of before use.
    pub fn encrypt_session_data(&mut self, plaintext: &[u8]) -> Option<Vec<u8>> {
        debug_assert!(
            matches!(
                self.status,
                SessionStatus::Confirmed | SessionStatus::Handshaking
            ),
            "0-RTT guard: a live session is never Rejected/Terminated when encrypting (SEC-13)"
        );
        if !self.status.can_transport_media() {
            return None;
        }
        // Reserve a fresh index (monotonic; the cipher enforces the 2^56 rekey
        // ceiling at the constants layer — here we just never wrap).
        let index = self.out_index;
        if index == u64::MAX {
            return None; // key scope exhausted -> caller must rekey
        }
        self.out_index = index.wrapping_add(1);
        Some(self.cipher.encrypt(index, plaintext))
    }

    /// The index the next outbound record will use.
    #[must_use]
    pub fn next_out_index(&self) -> u64 {
        self.out_index
    }

    /// Decrypt session data: check the replay window, then authenticate.
    /// `index` comes from the wire (the serialized sequence number).
    pub fn decrypt_session_data(&mut self, index: u64, ciphertext: &[u8]) -> SessionDecryptOutcome {
        if !self.status.can_transport_media() {
            return SessionDecryptOutcome::NotReady;
        }
        match self.inbound.check_and_record(index) {
            ReplayDecision::Reject => SessionDecryptOutcome::Rejected,
            ReplayDecision::Accept => match self.cipher.decrypt(index, ciphertext) {
                Ok(pt) => SessionDecryptOutcome::Ok(pt),
                Err(_) => SessionDecryptOutcome::AuthFailed,
            },
        }
    }
}

fn derive_salt(key: &SessionKey) -> [u8; 16] {
    use hkdf::Hkdf;
    use sha2::Sha256;
    let hk = Hkdf::<Sha256>::new(None, key);
    let mut salt = [0u8; 16];
    hk.expand(b"wdr-salt-v1", &mut salt)
        .expect("16 bytes of HKDF output is always valid");
    salt
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encrypt_refused_before_confirmation() {
        let mut c = SessionDataCipher::new(
            [0x5a; 32],
            Plane::Media,
            Direction::Tx,
            SessionStatus::Handshaking,
        );
        assert!(c.encrypt_session_data(b"audio").is_none());
        assert!(c.decrypt_session_data(0, b"x").is_err_or_none_not_ok());

        // Once confirmed, the same instance works.
        c.set_status(SessionStatus::Confirmed);
        let ct = c
            .encrypt_session_data(b"audio")
            .expect("confirmed can encrypt");
        assert!(!ct.is_empty());
    }

    #[test]
    fn roundtrip_and_auth_rejects_tamper_and_replay() {
        // Sender and receiver share the same session key for a given direction;
        // the salt is derived identically on both sides.
        let mut tx = SessionDataCipher::new(
            [0x5a; 32],
            Plane::Media,
            Direction::Tx,
            SessionStatus::Confirmed,
        );
        let mut rx = SessionDataCipher::new(
            [0x5a; 32],
            Plane::Media,
            Direction::Rx,
            SessionStatus::Confirmed,
        );

        let pt = b"media frame";
        let ct0 = tx.encrypt_session_data(pt).unwrap();
        let ct1 = tx.encrypt_session_data(pt).unwrap();

        assert_eq!(
            rx.decrypt_session_data(0, &ct0),
            SessionDecryptOutcome::Ok(pt.to_vec())
        );
        assert_eq!(
            rx.decrypt_session_data(1, &ct1),
            SessionDecryptOutcome::Ok(pt.to_vec())
        );

        // Replay of index 0 is dropped by the receive window.
        assert_eq!(
            rx.decrypt_session_data(0, &ct0),
            SessionDecryptOutcome::Rejected
        );

        // Tampering fails authentication.
        let mut bad = ct1.clone();
        bad[0] ^= 0x80;
        assert_eq!(
            rx.decrypt_session_data(2, &bad),
            SessionDecryptOutcome::AuthFailed
        );
    }

    #[test]
    fn out_of_window_rejected() {
        let mut tx = SessionDataCipher::new(
            [0x5a; 32],
            Plane::Control,
            Direction::Tx,
            SessionStatus::Confirmed,
        );
        let mut rx = SessionDataCipher::new(
            [0x5a; 32],
            Plane::Control,
            Direction::Rx,
            SessionStatus::Confirmed,
        );
        // Deliver 0..=1024 in order; each must authenticate.
        let mut last = Vec::new();
        for i in 0..=1024u64 {
            last = tx.encrypt_session_data(b"x").unwrap();
            assert_eq!(
                rx.decrypt_session_data(i, &last),
                SessionDecryptOutcome::Ok(b"x".to_vec())
            );
        }
        let _ = &last;

        // A valid ciphertext from index 400 (well outside the 256-entry window of
        // newest=1024) replayed now must be dropped, even though its tag is valid.
        let mut tx2 = SessionDataCipher::new(
            [0x5a; 32],
            Plane::Control,
            Direction::Tx,
            SessionStatus::Confirmed,
        );
        let mut ct_400 = Vec::new();
        for _ in 0..=400u64 {
            ct_400 = tx2.encrypt_session_data(b"x").unwrap();
        }
        assert_eq!(
            rx.decrypt_session_data(400, &ct_400),
            SessionDecryptOutcome::Rejected
        );
    }

    trait IsErrOrNoneNotOk {
        fn is_err_or_none_not_ok(&self) -> bool;
    }
    impl IsErrOrNoneNotOk for SessionDecryptOutcome {
        fn is_err_or_none_not_ok(&self) -> bool {
            !matches!(self, SessionDecryptOutcome::Ok(_))
        }
    }
}
