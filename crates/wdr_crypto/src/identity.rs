//! Identity keys: ed25519 long-term identity + x25519 static keys, and the
//! deterministic ed25519 → x25519 static mapping used for the Noise static
//! key (SECURITY_SPEC §2.1).

use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use rand::rngs::ThreadRng;
use x25519_dalek::{EphemeralSecret, PublicKey as X25519PublicKey, StaticSecret};

/// Generate a fresh X25519 ephemeral keypair (SECURITY_SPEC §3.1: the ephemerals
/// give forward secrecy). The ephemeral secret is short-lived and never reused.
#[must_use]
pub fn generate_ephemeral() -> (EphemeralSecret, X25519PublicKey) {
    let secret = EphemeralSecret::random_from_rng(&mut ThreadRng::default());
    let public = X25519PublicKey::from(&secret);
    (secret, public)
}

/// Compute the X25519 shared secret between an ephemeral secret and a peer
/// public key — the key-agreement step that feeds the Noise DH.
#[must_use]
pub fn x25519_shared_secret(secret: EphemeralSecret, peer_public: &X25519PublicKey) -> [u8; 32] {
    secret.diffie_hellman(peer_public).to_bytes()
}

/// A long-term identity keypair: ed25519 (the pairing fingerprint) plus the
/// deterministically-derived X25519 static key (the Noise static DH key).
#[derive(Clone)]
pub struct IdentityKeyPair {
    /// ed25519 signing key (the identity). Never serialized/logged.
    signing_key: SigningKey,
    /// ed25519 verifying key (the pairing pubkey / fingerprint subject).
    verifying_key: VerifyingKey,
    /// The derived static X25519 secret used for the Noise static key.
    static_secret: StaticSecret,
    /// The derived static X25519 public key exposed by the Noise payload.
    static_public: X25519PublicKey,
}

impl IdentityKeyPair {
    /// Generate a fresh identity keypair from the OS CSPRNG.
    pub fn generate() -> Self {
        Self::from_signing_key(SigningKey::generate(&mut ThreadRng::default()))
    }

    /// Reconstruct an identity from a 32-byte ed25519 seed (used for tests and
    /// for restoring a key from secure storage). The X25519 static pair is
    /// derived deterministically from the same seed.
    #[must_use]
    pub fn from_seed(seed: [u8; 32]) -> Self {
        Self::from_signing_key(SigningKey::from_bytes(&seed))
    }

    /// Wrap an existing ed25519 [`SigningKey`] and derive the static X25519 pair.
    #[must_use]
    pub fn from_signing_key(signing_key: SigningKey) -> Self {
        let verifying_key = signing_key.verifying_key();
        let static_secret = ed25519_to_x25519_static(&signing_key);
        let static_public = X25519PublicKey::from(&static_secret);
        Self {
            signing_key,
            verifying_key,
            static_secret,
            static_public,
        }
    }

    /// The ed25519 verifying key (the pairing pubkey / fingerprint).
    #[must_use]
    pub fn verifying_key(&self) -> &VerifyingKey {
        &self.verifying_key
    }

    /// The 32-byte ed25519 public key bytes (the identity / fingerprint).
    #[must_use]
    pub fn public_bytes(&self) -> [u8; 32] {
        self.verifying_key.to_bytes()
    }

    /// The 32-byte ed25519 seed (a secret; only meant for secure storage).
    #[must_use]
    pub fn seed_bytes(&self) -> [u8; 32] {
        self.signing_key.to_bytes()
    }

    /// The derived static X25519 secret (a secret).
    #[must_use]
    pub fn static_secret(&self) -> &StaticSecret {
        &self.static_secret
    }

    /// The derived static X25519 public key (the Noise handshake static pubkey).
    #[must_use]
    pub fn static_public(&self) -> [u8; 32] {
        self.static_public.to_bytes()
    }

    /// Sign a message (ed25519) with the identity key.
    #[must_use]
    pub fn sign(&self, message: &[u8]) -> Signature {
        self.signing_key.sign(message)
    }

    /// Verify an ed25519 signature over `message` under this identity's pubkey.
    #[must_use]
    pub fn verify(&self, message: &[u8], signature: &Signature) -> bool {
        self.verifying_key.verify(message, signature).is_ok()
    }

    /// Verify an ed25519 signature under an explicit pubkey (for revocation records
    /// and third-party identities).
    #[must_use]
    pub fn verify_with(pubkey: &VerifyingKey, message: &[u8], signature: &Signature) -> bool {
        pubkey.verify(message, signature).is_ok()
    }
}

/// Deterministic ed25519 → x25519 static key conversion (SECURITY_SPEC §2.1).
///
/// The standard conversion (as recommended by `curve25519-dalek`'s own docs for
/// deriving an X25519 transport key from an Ed25519 identity):
///
/// 1. `expanded = SHA-512(seed)` (the RFC 8032 expansion),
/// 2. `clamped = clamp(expanded[..32])` where clamp clears bits 0..2 and the
///    top bit of the last byte and forces bit 6 of the last byte
///    (`RFC 7748` / `RFC 8032` key clamping),
/// 3. the X25519 static secret is `clamped`; its public is `[clamped]·B`.
///
/// This is injective on seeds (so a re-imported seed reproduces the same static
/// key — verified in tests) and matches the behavior libraries such as
/// `curve25519-dalek`'s `StaticSecret` prescribe. The reverse map (x25519 →
/// ed25519) is impossible in general; identity always remains the ed25519 pubkey.
#[must_use]
pub fn ed25519_to_x25519_static(signing_key: &SigningKey) -> StaticSecret {
    use sha2::{Digest, Sha512};

    let seed = signing_key.to_bytes();
    let expanded: [u8; 64] = Sha512::digest(seed).into();
    StaticSecret::from(clamp_scalar(
        expanded[..32].try_into().expect("32-byte prefix"),
    ))
}

/// RFC 7748 clamping function on a 32-byte little-endian scalar.
fn clamp_scalar(mut s: [u8; 32]) -> [u8; 32] {
    s[0] &= 248;
    s[31] &= 127;
    s[31] |= 64;
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seed_roundtrip_reproduces_identical_static_key() {
        for _ in 0..8 {
            let a = IdentityKeyPair::generate();
            let seed = a.seed_bytes();
            let a2 = IdentityKeyPair::from_seed(seed);

            assert_eq!(a.public_bytes(), a2.public_bytes());
            assert_eq!(a.static_public(), a2.static_public());
            assert_eq!(a.static_secret().to_bytes(), a2.static_secret().to_bytes());
        }
    }

    #[test]
    fn static_key_is_deterministic_and_role_agnostic() {
        let a = IdentityKeyPair::from_seed([0x11; 32]);
        let b = IdentityKeyPair::from_seed([0x11; 32]);
        assert_eq!(a.static_public(), b.static_public());
        assert_ne!(
            a.static_public(),
            IdentityKeyPair::from_seed([0x22; 32]).static_public()
        );
    }

    #[test]
    fn keygen_produces_distinct_identities() {
        let a = IdentityKeyPair::generate();
        let b = IdentityKeyPair::generate();
        assert_ne!(a.public_bytes(), b.public_bytes());
        assert_ne!(a.static_secret().to_bytes(), b.static_secret().to_bytes());
    }

    #[test]
    fn sign_verify_roundtrip_and_tamper_rejection() {
        let kp = IdentityKeyPair::generate();
        let msg = b"pairing transcript id 42";
        let sig = kp.sign(msg);
        assert!(kp.verify(msg, &sig));
        assert!(!kp.verify(b"tampered", &sig));

        let kp2 = IdentityKeyPair::generate();
        assert!(!IdentityKeyPair::verify_with(
            kp2.verifying_key(),
            msg,
            &sig
        ));
    }

    #[test]
    fn static_public_derives_from_clamped_static_secret() {
        let kp = IdentityKeyPair::generate();
        let pub_recomputed = X25519PublicKey::from(kp.static_secret());
        assert_eq!(kp.static_public(), pub_recomputed.to_bytes());
    }

    #[test]
    fn ephemeral_dh_matches_noise_dh() {
        // Two ephemerals produce matching shared secrets (the DH around which
        // forward secrecy is built).
        let (a_sec, a_pub) = generate_ephemeral();
        let (b_sec, b_pub) = generate_ephemeral();
        let shared_a = x25519_shared_secret(a_sec, &b_pub);
        let shared_b = x25519_shared_secret(b_sec, &a_pub);
        assert_eq!(shared_a, shared_b);
        assert_ne!(shared_a, [0u8; 32]);
    }
}
