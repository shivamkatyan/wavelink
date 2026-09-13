//! Discovery e2e (FR-003, ADR-006): a `QuicRenderReceiver` advertises
//! `_wdr._tcp`; an emitter *discovers* it via the same `wdr_discovery::browse`
//! route `ref_emitter --discover` uses, then dials and loses nothing — the
//! canonical golden hash is preserved. This is the loopback analogue of
//! receiver-announces → emitter-browses → dial.

use std::time::Duration;

use wdr_discovery::{advertise_loopback, browse, RECEIVER_INSTANCE};
use wdr_entitlement::provider::Tier;
use wdr_fakes::source::{ChannelKind, Fixture, FixtureKind, PcmSource, SampleFormat};
use wdr_proto::Codec;
use wdr_refsim::receiver::BufferProfile;
use wdr_refsim::receiver_server;
use wdr_refsim::sink::{AudioFrameSink, QuicAudioSink, QuicRenderReceiver, SinkFormat};

const RATE_HZ: u32 = 48_000;
const GOLDEN_PRNG_I16_48K_STEREO: &str =
    "b7a3c25c8ccaa05643f14dfb5bc0e223f4ace3154c11ecd25bed5975d839c223";

#[test]
fn discover_receiver_via_mdns_then_lossless_roundtrip() {
    let addr: std::net::SocketAddr = "127.0.0.1:0".parse().expect("loopback addr");
    let mut receiver = QuicRenderReceiver::listen(
        addr,
        BufferProfile::Balanced,
        receiver_server::null_render_sink(),
    )
    .expect("receiver listens");
    let port = receiver.local_addr().port();

    // The receiver announces itself — exactly `ref_receiver --advertise`.
    let _advertiser =
        advertise_loopback(RECEIVER_INSTANCE, port).expect("receiver advertises _wdr._tcp");

    // The emitter browses — exactly `ref_emitter --discover`.
    let peers = browse(Duration::from_secs(10), 1).expect("emitter discovers a peer");
    assert_eq!(peers[0].port, port, "resolved port is the advertised one");
    let dial_target = peers[0].socket_addr().to_string();

    let mut sink = QuicAudioSink::connect(&dial_target, Tier::Pro, Codec::Flac)
        .expect("sink dials the discovered peer");
    sink.on_format(SinkFormat::canonical())
        .expect("sink format");
    let mut fixture = Fixture::new(
        FixtureKind::PseudoRandomPcm,
        SampleFormat::I16,
        RATE_HZ,
        ChannelKind::Stereo,
        4096,
    );
    let chunk = fixture.next_chunk(4096);
    sink.on_block(chunk.bytes).expect("sink block");
    sink.finish().expect("sink finish");

    let outcome = receiver
        .wait(Duration::from_secs(15))
        .expect("receiver outcome");
    assert_eq!(
        outcome.hash_hex(),
        GOLDEN_PRNG_I16_48K_STEREO,
        "discovered path must be lossless hash-perfect"
    );
    assert_eq!(outcome.metrics.packets_recv, 4);
    assert_eq!(outcome.metrics.loss, 0);
}
