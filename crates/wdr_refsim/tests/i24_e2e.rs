//! 24-bit emitter lane end-to-end (WS-A): the `QuicAudioSink`/`QuicRenderReceiver`
//! live pair driven with `SinkFormat::canonical_24()` (I24Packed), proving the
//! i24 codec surface is wired through the live transport seam — not just at the
//! adapter level — and that the receiver's decoded canonical (low-3-byte LE)
//! stream hashes exactly to the recorded i24 golden.

use std::time::Duration;

use wdr_entitlement::provider::Tier;
use wdr_fakes::source::{ChannelKind, Fixture, FixtureKind, PcmSource, SampleFormat};
use wdr_proto::Codec;
use wdr_refsim::receiver::BufferProfile;
use wdr_refsim::receiver_server;
use wdr_refsim::sink::{AudioFrameSink, QuicAudioSink, QuicRenderReceiver, SinkFormat};

const RATE_HZ: u32 = 48_000;
/// Canonical pseudo-random i24 / 48k / stereo golden (chunk 512, total 4096 —
/// docs/orchestration/reports/t-B0-fakes.md). Shared by the FLAC and PCM cells:
/// both 24-bit codecs are lossless, so `hash(decoded) == hash(source)`.
const GOLDEN_PRNG_I24_48K_STEREO: &str =
    "edf3016e9c7dd72253b5781fbd0443ed278482261d92c960ee573ebc491ef3ca";

#[test]
fn i24_flac_loopback_golden_hash_preserved() {
    assert_i24_lossless(Codec::Flac);
}

#[test]
fn i24_pcm_loopback_golden_hash_preserved() {
    assert_i24_lossless(Codec::Pcm);
}

fn assert_i24_lossless(codec: Codec) {
    let addr: std::net::SocketAddr = "127.0.0.1:0".parse().expect("loopback addr");
    let mut receiver = QuicRenderReceiver::listen(
        addr,
        BufferProfile::Balanced,
        receiver_server::null_render_sink(),
    )
    .expect("listen");

    let mut sink = QuicAudioSink::connect_with_format(
        &receiver.local_addr().to_string(),
        Tier::Pro,
        codec,
        SinkFormat::canonical_24(),
    )
    .expect("sink connect");
    sink.on_format(SinkFormat::canonical_24())
        .expect("sink format");

    // The canonical 4096-value i24 fixture (low-3-byte packed) in one block;
    // the sink accumulates to whole 512-sample frames (4 frames here).
    let mut fixture = Fixture::new(
        FixtureKind::PseudoRandomPcm,
        SampleFormat::I24,
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
        GOLDEN_PRNG_I24_48K_STEREO,
        "24-bit live lane must hash exactly to the i24 golden"
    );
    assert_eq!(outcome.metrics.packets_recv, 4, "4 whole 24-bit frames");
    assert_eq!(outcome.metrics.loss, 0);
    assert_eq!(outcome.metrics.underruns, 0);
    assert_eq!(outcome.metrics.malformed, 0);
}
