//! `QuicRenderReceiver` roundtrip (WS3): the receiver-side seam mirror of
//! `QuicAudioSink` — it owns the quinn server, runs the shared
//! `receiver_server` lane, and delivers decoded canonical bytes into a
//! `RenderSink`. The null device (hash sink) proves the lossless golden path
//! end-to-end through the live server, exactly like the emitter-side
//! `ref_live_sink` tests do.

use std::time::Duration;

use wdr_entitlement::provider::Tier;
use wdr_fakes::source::{ChannelKind, Fixture, FixtureKind, PcmSource, SampleFormat};
use wdr_proto::Codec;
use wdr_refsim::receiver::BufferProfile;
use wdr_refsim::receiver_server;
use wdr_refsim::sink::{AudioFrameSink, QuicAudioSink, QuicRenderReceiver, SinkFormat};

const RATE_HZ: u32 = 48_000;
/// Canonical pseudo-random i16 / 48k / stereo golden (chunk 512, total 4096).
const GOLDEN_PRNG_I16_48K_STEREO: &str =
    "b7a3c25c8ccaa05643f14dfb5bc0e223f4ace3154c11ecd25bed5975d839c223";

#[test]
fn quic_render_receiver_lossless_flac_roundtrip_hash_preserved() {
    let addr: std::net::SocketAddr = "127.0.0.1:0".parse().expect("loopback addr");
    let mut receiver = QuicRenderReceiver::listen(
        addr,
        BufferProfile::Balanced,
        receiver_server::null_render_sink(),
    )
    .expect("listen");

    let mut sink =
        QuicAudioSink::connect(&receiver.local_addr().to_string(), Tier::Pro, Codec::Flac)
            .expect("sink connect");
    sink.on_format(SinkFormat::canonical())
        .expect("sink format");

    // The canonical 4096-sample fixture in one block; the sink accumulates to
    // whole 512-sample frames (4 frames here).
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
        "live render receiver must hash virtually to the canonical golden"
    );
    assert_eq!(outcome.metrics.packets_recv, 4, "4 whole FLAC frames");
    assert_eq!(outcome.metrics.loss, 0);
    assert_eq!(outcome.metrics.underruns, 0);
    assert_eq!(outcome.metrics.malformed, 0);
}
