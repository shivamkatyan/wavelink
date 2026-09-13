//! AEAD session ciphers for the WDR data plane (SECURITY_SPEC §3.1–§3.3).
//!
//! Two constructors:
//! * [`XChaChaSessionCipher`] — XChaCha20-Poly1305, 24-byte nonce =
//!   `random_salt16 ‖ counter8(BE)` (the locked default).
//! * [`ChaCha12SessionCipher`] — ChaCha20-Poly1305, 12-byte nonce driven by a
//!   caller-supplied monotonic counter (the documented non-default path; the
//!   counter is expected to be a persisted monotonic counter per §3.3).

use chacha20poly1305::aead::{Aead, KeyInit};
use chacha20poly1305::XNonce;

/// A 32-byte AEAD key.
pub type SessionKey = [u8; 32];

/// Outcome of an authenticated decrypt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecryptResult {
    /// Decryption succeeded.
    Ok { plaintext: Vec<u8>, index: u64 },
    /// Authentication failed (tamper or wrong key/nonce). No distinguishing detail leaked.
    AuthFailure,
}

/// Authenticated-failure error enum for the 12-byte nonce path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecryptError12 {
    /// Ciphertext length is invalid (too short even to carry the tag).
    Length,
    /// The AEAD tag did not verify.
    Auth,
}

// ---------------------------------------------------------------------------
// 24-byte nonce: random_salt16 ‖ counter8(BE) — the locked default.
// ---------------------------------------------------------------------------

/// A 24-byte XChaCha20-Poly1305 nonce: `random_salt16 ‖ counter8(BE)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SaltedNonce {
    salt: [u8; 16],
    counter: u64,
}

impl SaltedNonce {
    /// Build a nonce from a fresh 16-byte random salt and a counter.
    #[must_use]
    pub fn new(salt: [u8; 16], counter: u64) -> Self {
        Self { salt, counter }
    }

    /// Split into the (salt, counter) parts.
    #[must_use]
    pub fn parts(&self) -> ([u8; 16], u64) {
        (self.salt, self.counter)
    }

    /// Encode as the canonical 24 bytes `salt ‖ counter8(BE)`.
    #[must_use]
    pub fn to_bytes(&self) -> [u8; 24] {
        let mut out = [0u8; 24];
        out[..16].copy_from_slice(&self.salt);
        out[16..].copy_from_slice(&self.counter.to_be_bytes());
        out
    }

    /// Construct from the canonical 24-byte encoding.
    #[must_use]
    pub fn from_bytes(b: &[u8; 24]) -> Self {
        let mut salt = [0u8; 16];
        salt.copy_from_slice(&b[..16]);
        let counter = u64::from_be_bytes(b[16..].try_into().expect("8 bytes"));
        Self { salt, counter }
    }

    /// The counter increment needed before use in a nonce.
    #[must_use]
    pub fn next(&self) -> Option<Self> {
        self.counter.checked_add(1).map(|c| Self {
            salt: self.salt,
            counter: c,
        })
    }
}

/// The locked-default data-plane cipher: XChaCha20-Poly1305.
pub struct XChaChaSessionCipher {
    key: chacha20poly1305::XChaCha20Poly1305,
    salt: [u8; 16],
}

impl XChaChaSessionCipher {
    /// Construct from a 32-byte key and a fresh random 16-byte salt.
    #[must_use]
    pub fn new(key: SessionKey, salt: [u8; 16]) -> Self {
        Self {
            key: chacha20poly1305::XChaCha20Poly1305::new((&key).into()),
            salt,
        }
    }

    /// The salt (transiently stored with the session keys — memory only).
    #[must_use]
    pub fn salt(&self) -> [u8; 16] {
        self.salt
    }

    /// Canonical nonce for a packet index.
    #[must_use]
    pub fn nonce_for(&self, index: u64) -> XNonce {
        SaltedNonce::new(self.salt, index).to_bytes().into()
    }

    /// Encrypt `plaintext` under `index` (a distinct nonce per index, which must
    /// never repeat within this key scope). Returns ciphertext+tag.
    #[must_use]
    pub fn encrypt(&self, index: u64, plaintext: &[u8]) -> Vec<u8> {
        self.key
            .encrypt(&self.nonce_for(index), plaintext)
            .expect("XChaCha20-Poly1305 encrypt is infallible given a 24-byte nonce")
    }

    /// Decrypt `ciphertext` under `index` with authenticated failure.
    pub fn decrypt(&self, index: u64, ciphertext: &[u8]) -> Result<Vec<u8>, DecryptError12> {
        self.key
            .decrypt(&self.nonce_for(index), ciphertext)
            .map_err(|_| DecryptError12::Auth)
    }
}

// ---------------------------------------------------------------------------
// 12-byte nonce path (ChaCha20-Poly1305) driven by a monotonic counter.
// ---------------------------------------------------------------------------

/// A ChaCha20-Poly1305 data-plane cipher whose 12-byte nonce is
/// `salt4 ‖ counter8(BE)` where `counter8` is a caller-supplied *persisted
/// monotonic* counter (SECURITY_SPEC §3.1 / §3.3; never reset across restarts).
/// The counter is owned by the caller (e.g. the session core / a durable store)
/// and fed in per record; this type holds the fixed 4-byte salt for the scope.
pub struct ChaCha12SessionCipher {
    key: chacha20poly1305::ChaCha20Poly1305,
    salt4: [u8; 4],
}

impl ChaCha12SessionCipher {
    /// Construct from a 32-byte key and a 4-byte salt for this key scope.
    #[must_use]
    pub fn new(key: SessionKey, salt4: [u8; 4]) -> Self {
        Self {
            key: chacha20poly1305::ChaCha20Poly1305::new((&key).into()),
            salt4,
        }
    }

    /// Encode the 12-byte nonce for a given persisted counter value.
    #[must_use]
    pub fn nonce_for(&self, counter: u64) -> [u8; 12] {
        let mut out = [0u8; 12];
        out[..4].copy_from_slice(&self.salt4);
        out[4..].copy_from_slice(&counter.to_be_bytes());
        out
    }

    /// Encrypt under the given persisted counter (never reused within the key scope).
    #[must_use]
    pub fn encrypt(&self, counter: u64, plaintext: &[u8]) -> Vec<u8> {
        self.key
            .encrypt(&self.nonce_for(counter).into(), plaintext)
            .expect("ChaCha20-Poly1305 encrypt is infallible given a 12-byte nonce")
    }

    /// Decrypt with authenticated failure.
    pub fn decrypt(&self, counter: u64, ciphertext: &[u8]) -> Result<Vec<u8>, DecryptError12> {
        self.key
            .decrypt(&self.nonce_for(counter).into(), ciphertext)
            .map_err(|_| DecryptError12::Auth)
    }
}

// ---------------------------------------------------------------------------
// A monotonic, persist-friendly counter for the 12-byte path.
// ---------------------------------------------------------------------------

/// A 64-bit little-endian, only-incrementing counter suitable as the source for a
/// persisted monotonic counter (SECURITY_SPEC §3.3). The *caller* is responsible
/// for fsync/atomic persistence of the value returned by [`MonotonicCounter::get`]
/// so a crash cannot wrap the nonce counter within a key lifetime.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MonotonicCounter(u64);

impl MonotonicCounter {
    /// Create at a known value (e.g. restored from persistent storage).
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// The current value.
    #[must_use]
    pub const fn get(&self) -> u64 {
        self.0
    }

    /// Bump the counter by one. Returns `None` on `u64::MAX` so the caller can
    /// request a key renewal before the nonce counter wraps.
    #[must_use]
    pub fn tick(&mut self) -> Option<u64> {
        let next = self.0.checked_add(1)?;
        self.0 = next;
        Some(next)
    }

    /// Bump by `n` in one step; rejects overflow (→ rekey).
    #[must_use]
    pub fn advance_by(&mut self, n: u64) -> Option<u64> {
        let next = self.0.checked_add(n)?;
        self.0 = next;
        Some(next)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn random_key() -> SessionKey {
        rand::random()
    }

    #[test]
    fn xchacha_roundtrip_and_tamper_rejection() {
        let key = random_key();
        let salt = [7u8; 16];
        let c = XChaChaSessionCipher::new(key, salt);
        let pt = b"control message payload";
        let ct = c.encrypt(42, pt);
        assert_eq!(c.decrypt(42, &ct).unwrap(), pt);

        let mut tampered = ct.clone();
        let last = tampered.len() - 1;
        tampered[last] ^= 1;
        assert!(matches!(
            c.decrypt(42, &tampered),
            Err(DecryptError12::Auth)
        ));

        // Wrong nonce (index) must fail auth.
        assert!(matches!(c.decrypt(43, &ct), Err(DecryptError12::Auth)));
    }

    #[test]
    fn salted_nonce_encoding_roundtrip_be_counter() {
        let n = SaltedNonce::new([0xab; 16], 0x0102_0304_0506_0708);
        let bytes = n.to_bytes();
        assert_eq!(&bytes[..16], &[0xab; 16]);
        assert_eq!(
            u64::from_be_bytes(bytes[16..].try_into().unwrap()),
            0x0102_0304_0506_0708
        );
        assert_eq!(SaltedNonce::from_bytes(&bytes), n);
    }

    #[test]
    fn xchacha_nonces_are_unique_across_indices() {
        let salt = [3u8; 16];
        let mut seen = std::collections::HashSet::new();
        for i in 0..1000u64 {
            let n = SaltedNonce::new(salt, i).to_bytes();
            assert!(seen.insert(n), "nonce collision at index {i}");
        }
    }

    #[test]
    fn chacha12_roundtrip_and_counter_derivation() {
        let key = random_key();
        let c = ChaCha12SessionCipher::new(key, [1, 2, 3, 4]);

        let mut counter = MonotonicCounter::new(100);
        let idx = counter.tick().unwrap();
        let n = c.nonce_for(idx);
        assert_eq!(&n[..4], &[1, 2, 3, 4]);
        assert_eq!(u64::from_be_bytes(n[4..].try_into().unwrap()), 101);

        let ct = c.encrypt(idx, b"media frame");
        assert_eq!(c.decrypt(idx, &ct).unwrap(), b"media frame");
        // Reusing the counter (a nonce-wrapping bug) must fail, not silently decrypt
        // with a different plaintext; and a wrong counter must fail auth:
        assert!(matches!(c.decrypt(100, &ct), Err(DecryptError12::Auth)));
    }

    #[test]
    fn monotonic_counter_never_wraps() {
        let mut c = MonotonicCounter::new(u64::MAX - 1);
        assert_eq!(c.tick(), Some(u64::MAX));
        assert_eq!(c.tick(), None); // overflow -> caller must rekey
        assert_eq!(c.get(), u64::MAX);
    }

    #[test]
    fn chacha12_tags_ciphertext_and_rejects_tamper() {
        let c = ChaCha12SessionCipher::new(random_key(), [9; 4]);
        let ct = c.encrypt(5, b"audio");
        let mut bad = ct.clone();
        bad[0] ^= 0xff;
        assert!(matches!(c.decrypt(5, &bad), Err(DecryptError12::Auth)));
    }
}
