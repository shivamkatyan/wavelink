//! Integration tests for the Noise XX handshake and session key derivation
//! (task t-B0-crypto). These exercise the real, end-to-end pairing flow the way
//! the session core will drive it.

use wdr_crypto::identity::IdentityKeyPair;
use wdr_crypto::noise::{
    pair_initiator, InitiatorHandshake, NoiseHandshakeError, ResponderWaitingMsg3,
};
use wdr_crypto::session::{derive_session_keys, sas_digits, Direction, Plane, SessionStatus};

/// Wire buffer sizes used by the handshake tests (generous fixed sizes; the
/// actual messages are 80 / 96 / 48 bytes).
const MSG1: usize = 128;
const MSG2: usize = 160;
const MSG3: usize = 128;

fn two_identities() -> (IdentityKeyPair, IdentityKeyPair) {
    (IdentityKeyPair::generate(), IdentityKeyPair::generate())
}

#[test]
fn x25519_static_pub_is_distinct_from_ed25519_fingerprint() {
    // An X25519 static public key must never reveal or equal the ed25519 fingerprint.
    let a = IdentityKeyPair::generate();
    assert_ne!(a.public_bytes(), a.static_public());
}

#[test]
fn handshake_success_matching_hash_and_sas() {
    let (init_id, resp_id) = two_identities();
    // Capture static pubs before moving the identities into the handshake.
    let init_static = init_id.static_public();
    let resp_static = resp_id.static_public();

    let (init, resp) = pair_initiator::<MSG1, MSG2, MSG3>(init_id, None, resp_id, None)
        .expect("XX handshake should succeed");

    // Both roles must agree on the final transcript hash (the SAS source).
    assert_eq!(init.handshake_hash(), resp.handshake_hash());
    assert_eq!(
        sas_digits(&init.handshake_hash()),
        sas_digits(&resp.handshake_hash())
    );

    // Peer static pubkeys exchanged must match the other's X25519 static.
    assert_eq!(init.peer_static_x25519(), Some(resp_static));
    assert_eq!(resp.peer_static_x25519(), Some(init_static));

    assert!(init.is_initiator());
    assert!(!resp.is_initiator());

    // Directional split keys agree between the roles and differ from each other.
    assert_eq!(
        init.split_keys().initiator_key,
        resp.split_keys().initiator_key
    );
    assert_eq!(
        init.split_keys().responder_key,
        resp.split_keys().responder_key
    );
    assert_ne!(
        init.split_keys().initiator_key,
        init.split_keys().responder_key
    );
}

#[test]
fn handshake_transport_encryption_is_mutual_and_authenticated() {
    let (init_id, resp_id) = two_identities();
    let (mut init, mut resp) =
        pair_initiator::<MSG1, MSG2, MSG3>(init_id, None, resp_id, None).expect("handshake ok");

    let mut ch1 = [0u8; 256];
    let n = init
        .write_transport(b"hello from initiator", &mut ch1)
        .unwrap();
    let mut back = [0u8; 256];
    let m = resp.read_transport(&ch1[..n], &mut back).unwrap();
    assert_eq!(&back[..m], b"hello from initiator");

    let mut ch2 = [0u8; 256];
    let n = resp
        .write_transport(b"hello from responder", &mut ch2)
        .unwrap();
    let m = init.read_transport(&ch2[..n], &mut back).unwrap();
    assert_eq!(&back[..m], b"hello from responder");
}

#[test]
fn responder_rejects_unexpected_fingerprint() {
    let (init_id, resp_id) = two_identities();
    let other = IdentityKeyPair::generate();
    // The responder is pinned to the WRONG peer fingerprint.
    let mut i = InitiatorHandshake::initiator(init_id, None).unwrap();
    let mut r = ResponderWaitingMsg3::responder(resp_id, Some(other.static_public())).unwrap();

    let mut m1 = [0u8; MSG1];
    let mut m2 = [0u8; MSG2];
    let mut m3 = [0u8; MSG3];

    let n1 = i.write_hello(&mut m1).unwrap();
    let n2 = r.read_hello(&m1[..n1], &mut m2).unwrap();
    let (n3, _) = i.complete_after_msg2(&m2[..n2], &mut m3).unwrap();
    let err = r.complete_after_msg3(&m3[..n3]).err().unwrap();
    assert_eq!(err, NoiseHandshakeError::FingerprintMismatch);
}

#[test]
fn initiator_rejects_unexpected_fingerprint() {
    let (init_id, resp_id) = two_identities();
    let other = IdentityKeyPair::generate();
    // The initiator is pinned to the WRONG responder fingerprint.
    let mut i = InitiatorHandshake::initiator(init_id, Some(other.static_public())).unwrap();
    let mut r = ResponderWaitingMsg3::responder(resp_id, None).unwrap();

    let mut m1 = [0u8; MSG1];
    let mut m2 = [0u8; MSG2];
    let mut m3 = [0u8; MSG3];

    let n1 = i.write_hello(&mut m1).unwrap();
    let n2 = r.read_hello(&m1[..n1], &mut m2).unwrap();
    let err = i.complete_after_msg2(&m2[..n2], &mut m3).err().unwrap();
    assert_eq!(err, NoiseHandshakeError::FingerprintMismatch);
}

#[test]
fn handshake_success_with_correct_pinned_fingerprints() {
    let (init_id, resp_id) = two_identities();
    let init_pin = init_id.static_public();
    let resp_pin = resp_id.static_public();
    let (_, resp) =
        pair_initiator::<MSG1, MSG2, MSG3>(init_id, Some(resp_pin), resp_id, Some(init_pin))
            .expect("pinned, matching handshake succeeds");
    assert_eq!(resp.peer_static_x25519(), Some(init_pin));
}

#[test]
fn handshake_fresh_split_keys_per_session() {
    let (a, b) = two_identities();
    let (split_a, _) = pair_initiator::<MSG1, MSG2, MSG3>(a, None, b, None).unwrap();
    let (c, d) = two_identities();
    let (split_c, _) = pair_initiator::<MSG1, MSG2, MSG3>(c, None, d, None).unwrap();

    let ka = derive_session_keys(&split_a.split_keys(), 1);
    let kb = derive_session_keys(&split_c.split_keys(), 2);
    assert_ne!(ka.media_tx, kb.media_tx);
    assert_ne!(ka.control_tx, kb.control_tx);
    assert!(ka.planes_separated());
}

#[test]
fn derive_session_keys_separate_control_and_media_planes() {
    let (a, b) = two_identities();
    let (init, resp) = pair_initiator::<MSG1, MSG2, MSG3>(a, None, b, None).unwrap();
    assert_eq!(init.handshake_hash(), resp.handshake_hash());

    let keys = derive_session_keys(&init.split_keys(), 99);
    assert_ne!(
        keys.key(Plane::Control, Direction::Tx),
        keys.key(Plane::Media, Direction::Tx)
    );
    assert!(keys.planes_separated());

    // Both roles derive identical keys from the same split material.
    let resp_keys = derive_session_keys(&resp.split_keys(), 99);
    assert_eq!(keys.media_tx, resp_keys.media_tx);
    assert_eq!(keys.control_rx, resp_keys.control_rx);
}

#[test]
fn zero_rtt_guard_blocks_media_until_confirmed() {
    // SEC-13 (0-RTT off): no data may be accepted before the handshake completes.
    let status = SessionStatus::Handshaking;
    assert!(!status.can_transport_media());
    assert!(!status.can_send_control());

    // A completed handshake yields a non-trivial transcript; it is only after
    // SAS/QR confirmation that the session core moves to Confirmed, at which
    // point media transport is permitted.
    let (a, b) = two_identities();
    let (init, _) = pair_initiator::<MSG1, MSG2, MSG3>(a, None, b, None).unwrap();
    assert_ne!(init.handshake_hash(), [0u8; 32]);

    let confirmed = SessionStatus::Confirmed;
    assert!(confirmed.can_transport_media());
    assert!(confirmed.can_send_control());
}

#[test]
fn error_variants_are_distinct() {
    assert_ne!(
        NoiseHandshakeError::FingerprintMismatch,
        NoiseHandshakeError::MissingPeerStatic
    );
    assert_eq!(
        std::mem::discriminant(&NoiseHandshakeError::FingerprintMismatch),
        std::mem::discriminant(&NoiseHandshakeError::FingerprintMismatch)
    );
}
