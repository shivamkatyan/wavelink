//! Secure lane e2e (WS-D): Noise XX pairing + per-frame AEAD + fingerprint
//! pinning over the reliable lane. Asserts that a fully-pinned secure loopback
//! preserves the canonical lossless golden hash (AEAD is transparent to the
//! decoded audio), that a wrong fingerprint is rejected on EITHER side, and that
//! the lane is lossless-only (Opus refused).

use std::time::Duration;

use wdr_crypto::identity::IdentityKeyPair;
use wdr_entitlement::provider::Tier;
use wdr_fakes::source::{ChannelKind, Fixture, FixtureKind, PcmSource, SampleFormat};
use wdr_proto::Codec;
use wdr_refsim::receiver::BufferProfile;
use wdr_refsim::receiver_server;
use wdr_refsim::sink::{AudioFrameSink, QuicAudioSink, QuicRenderReceiver, SinkFormat};

const GOLDEN_PRNG_I16_48K_STEREO: &str =
    "b7a3c25c8ccaa05643f14dfb5bc0e223f4ace3154c11ecd25bed5975d839c223";

#[test]
fn secure_pair_loopback_preserves_the_golden_hash() {
    let emitter_id = IdentityKeyPair::from_seed([0x11; 32]);
    let receiver_id = IdentityKeyPair::from_seed([0x22; 32]);
    let receiver_fp = receiver_id.static_public();
    let emitter_fp = emitter_id.static_public();
    let addr: std::net::SocketAddr = "127.0.0.1:0".parse().expect("loopback addr");

    let mut receiver = QuicRenderReceiver::listen_secure(
        addr,
        BufferProfile::Balanced,
        receiver_server::null_render_sink(),
        Some(emitter_fp),
        receiver_id,
    )
    .expect("secure listen");

    let mut sink = QuicAudioSink::connect_secure(
        &receiver.local_addr().to_string(),
        Tier::Pro,
        Codec::Flac,
        Some(receiver_fp),
        emitter_id,
    )
    .expect("secure sink connect");
    sink.on_format(SinkFormat::canonical())
        .expect("sink format");

    let mut fixture = Fixture::new(
        FixtureKind::PseudoRandomPcm,
        SampleFormat::I16,
        48_000,
        ChannelKind::Stereo,
        4096,
    );
    let chunk = fixture.next_chunk(4096);
    sink.on_block(chunk.bytes).expect("sink block");
    sink.finish().expect("sink finish");

    let outcome = receiver
        .wait(Duration::from_secs(15))
        .expect("secure receive outcome");
    assert_eq!(
        outcome.hash_hex(),
        GOLDEN_PRNG_I16_48K_STEREO,
        "AEAD must be transparent — secure lane decodes to the golden hash"
    );
    assert!(
        outcome.metrics.secured,
        "secured flag set on the secure lane"
    );
    assert_eq!(outcome.metrics.sec_rejected, 0);
    assert_eq!(outcome.metrics.packets_recv, 4);
    assert_eq!(outcome.metrics.loss, 0);
    assert_eq!(outcome.metrics.underruns, 0);
}

#[test]
fn wrong_initiator_fingerprint_is_rejected() {
    let emitter_id = IdentityKeyPair::from_seed([0x33; 32]);
    let receiver_id = IdentityKeyPair::from_seed([0x44; 32]);
    let stranger = IdentityKeyPair::from_seed([0x55; 32]).static_public();
    let addr: std::net::SocketAddr = "127.0.0.1:0".parse().expect("loopback addr");

    let mut receiver = QuicRenderReceiver::listen_secure(
        addr,
        BufferProfile::Balanced,
        receiver_server::null_render_sink(),
        None,
        receiver_id,
    )
    .expect("secure listen");

    // The emitter pins a fingerprint the receiver does not present → the XX
    // handshake must fail at msg2 verification, never silently pair.
    let err = QuicAudioSink::connect_secure(
        &receiver.local_addr().to_string(),
        Tier::Pro,
        Codec::Flac,
        Some(stranger),
        emitter_id,
    );
    assert!(
        err.is_err(),
        "initiator must reject a mismatched receiver fingerprint"
    );
    let _ = receiver.wait(Duration::from_secs(6)); // receiver's half also errors
}

#[test]
fn wrong_responder_fingerprint_is_rejected() {
    let emitter_id = IdentityKeyPair::from_seed([0x66; 32]);
    let receiver_id = IdentityKeyPair::from_seed([0x77; 32]);
    let stranger = IdentityKeyPair::from_seed([0x88; 32]).static_public();
    let addr: std::net::SocketAddr = "127.0.0.1:0".parse().expect("loopback addr");

    let mut receiver = QuicRenderReceiver::listen_secure(
        addr,
        BufferProfile::Balanced,
        receiver_server::null_render_sink(),
        Some(stranger),
        receiver_id,
    )
    .expect("secure listen");

    // Receiver pins a fingerprint the emitter's identity does not carry → the
    // responder's msg3 verification fails; nothing renders.
    let res = QuicAudioSink::connect_secure(
        &receiver.local_addr().to_string(),
        Tier::Pro,
        Codec::Flac,
        Some(emitter_id.static_public()),
        emitter_id,
    );
    let res = res.map(|sink| {
        drop(sink);
    });
    let _ = res; // connect_secure's initiator half may succeed; receiver must not
    let outcome = receiver.wait(Duration::from_secs(6));
    assert!(
        outcome.is_err(),
        "responder must reject a mismatched emitter fingerprint (received {outcome:?})"
    );
}

#[test]
fn secure_lane_refuses_lossy_opus() {
    let emitter_id = IdentityKeyPair::from_seed([0x99; 32]);
    let err =
        QuicAudioSink::connect_secure("127.0.0.1:0", Tier::Free, Codec::Opus, None, emitter_id);
    assert!(
        err.is_err(),
        "secure lane is reliable/lossless-only this phase — Opus must be refused"
    );
}
