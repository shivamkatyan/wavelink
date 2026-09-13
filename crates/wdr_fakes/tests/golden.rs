//! Golden integrity: every fixture type at 44.1/48 kHz and 16/24-bit produces
//! the **recorded canonical hash** through `PcmSource → HashSink` (blake3 of
//! the canonical byte stream, chunk=512, total=4096 samples).
//!
//! These values are reproduced by `examples/golden_hashes.rs` and recorded in
//! `docs/orchestration/reports/t-B0-fakes.md`. The codec worker's
//! lossless-equality test asserts `hash(source) == golden` for each of these.

use wdr_fakes::source::{ChannelKind, Fixture, FixtureKind, PcmSource, SampleFormat, Stereo};

mod common;

const CHUNK: u32 = 512;
const SAMPLES: u64 = 4096;

#[track_caller]
fn run(kind: FixtureKind, format: SampleFormat, rate_hz: u32, channels: ChannelKind, exp: &str) {
    let mut fx = Fixture::new(kind.clone(), format, rate_hz, channels, SAMPLES);
    let got = fx.drain_hash(CHUNK).hex();
    assert_eq!(
        got, exp,
        "golden mismatch kind={kind:?} fmt={format:?} rate={rate_hz} ch={channels:?}"
    );
}

#[test]
fn source_and_hash_sink_agree_for_lossless() {
    // ---- silence ----------------------------------------------------------
    run(
        FixtureKind::Silence,
        SampleFormat::I16,
        44_100,
        ChannelKind::Mono,
        "128daa44a4f7badaed2244bb6fe009d5e7803177414e01d7d9df80c190e14906",
    );
    run(
        FixtureKind::Silence,
        SampleFormat::I16,
        48_000,
        ChannelKind::Stereo,
        "128daa44a4f7badaed2244bb6fe009d5e7803177414e01d7d9df80c190e14906",
    );
    run(
        FixtureKind::Silence,
        SampleFormat::I24,
        44_100,
        ChannelKind::Mono,
        "819ad8f20ee2578f84eeb28b4aa852458c066911cce810767021030961e43e60",
    );
    run(
        FixtureKind::Silence,
        SampleFormat::I24,
        48_000,
        ChannelKind::Stereo,
        "819ad8f20ee2578f84eeb28b4aa852458c066911cce810767021030961e43e60",
    );

    // ---- impulse train -----------------------------------------------------
    run(
        FixtureKind::ImpulseTrain { period: 64 },
        SampleFormat::I16,
        48_000,
        ChannelKind::Stereo,
        "0fd38c7cea5e94e00ea2e4f075aaa2e3d2776cecba091f31a1bbc8fa8718504c",
    );
    run(
        FixtureKind::ImpulseTrain { period: 256 },
        SampleFormat::I16,
        48_000,
        ChannelKind::Stereo,
        "a9c9d032850a93dd5a88307de178731f147a0cbd6c30b31305b1795f259957a1",
    );

    // ---- sine sweep ---------------------------------------------------------
    run(
        FixtureKind::SineSweep {
            f0: 20.0,
            f1: 20_000.0,
        },
        SampleFormat::I16,
        48_000,
        ChannelKind::Stereo,
        "0736b76584c1087dac709cede7be36549c920c5caad4aab7d7df62b58056df08",
    );
    run(
        FixtureKind::SineSweep {
            f0: 200.0,
            f1: 20_000.0,
        },
        SampleFormat::I16,
        48_000,
        ChannelKind::Stereo,
        "3865f9f5789cbe1d08577ba88604817821fc8ef771c56ed4efbfe1fe409e9f4d",
    );

    // ---- full-scale edge (values ±32767 alternating) -------------------------
    run(
        FixtureKind::FullScaleEdge,
        SampleFormat::I16,
        44_100,
        ChannelKind::Stereo,
        "931b80605fa8cf6a647c3166894c3631f752b0bcedbbed084676d3fe25d9627d",
    );
    run(
        FixtureKind::FullScaleEdge,
        SampleFormat::I16,
        48_000,
        ChannelKind::Stereo,
        "931b80605fa8cf6a647c3166894c3631f752b0bcedbbed084676d3fe25d9627d",
    );
    run(
        FixtureKind::FullScaleEdge,
        SampleFormat::I24,
        44_100,
        ChannelKind::Stereo,
        "0a212e00175469d8156a45786d8efd226d2447842d8033b4ce5035fa85272978",
    );
    run(
        FixtureKind::FullScaleEdge,
        SampleFormat::I24,
        48_000,
        ChannelKind::Stereo,
        "0a212e00175469d8156a45786d8efd226d2447842d8033b4ce5035fa85272978",
    );

    // ---- seeded pseudo-random PCM ---------------------------------------------
    run(
        FixtureKind::PseudoRandomPcm,
        SampleFormat::I16,
        44_100,
        ChannelKind::Mono,
        "b7a3c25c8ccaa05643f14dfb5bc0e223f4ace3154c11ecd25bed5975d839c223",
    );
    run(
        FixtureKind::PseudoRandomPcm,
        SampleFormat::I16,
        44_100,
        ChannelKind::Stereo,
        "b7a3c25c8ccaa05643f14dfb5bc0e223f4ace3154c11ecd25bed5975d839c223",
    );
    run(
        FixtureKind::PseudoRandomPcm,
        SampleFormat::I16,
        48_000,
        ChannelKind::Stereo,
        "b7a3c25c8ccaa05643f14dfb5bc0e223f4ace3154c11ecd25bed5975d839c223",
    );
    run(
        FixtureKind::PseudoRandomPcm,
        SampleFormat::I24,
        48_000,
        ChannelKind::Stereo,
        "edf3016e9c7dd72253b5781fbd0443ed278482261d92c960ee573ebc491ef3ca",
    );

    // ---- mono / stereo channel-ID (left = pattern A, right = pattern B) -------
    run(
        FixtureKind::ChannelId {
            lane: Stereo::LeftPattern,
        },
        SampleFormat::I16,
        44_100,
        ChannelKind::Stereo,
        "5c0f5d79e7784169c1b635d884316bcadf2a485a6aabf9bcf551a3e64798311f",
    );
    run(
        FixtureKind::ChannelId {
            lane: Stereo::LeftPattern,
        },
        SampleFormat::I16,
        48_000,
        ChannelKind::Stereo,
        "5c0f5d79e7784169c1b635d884316bcadf2a485a6aabf9bcf551a3e64798311f",
    );
    run(
        FixtureKind::ChannelId {
            lane: Stereo::LeftPattern,
        },
        SampleFormat::I24,
        48_000,
        ChannelKind::Stereo,
        "89569544da2e919779c563282867930ecbf7d4c529ea29f747d359c5de394515",
    );
    run(
        FixtureKind::ChannelId {
            lane: Stereo::RightPattern,
        },
        SampleFormat::I16,
        44_100,
        ChannelKind::Stereo,
        "a958a425f58d8d3e0bb4a3f1b2f980cef257231f712c0524c714a92d671c25c6",
    );
    run(
        FixtureKind::ChannelId {
            lane: Stereo::RightPattern,
        },
        SampleFormat::I16,
        48_000,
        ChannelKind::Stereo,
        "a958a425f58d8d3e0bb4a3f1b2f980cef257231f712c0524c714a92d671c25c6",
    );
    run(
        FixtureKind::ChannelId {
            lane: Stereo::RightPattern,
        },
        SampleFormat::I24,
        48_000,
        ChannelKind::Stereo,
        "e5c162450ac2eb8d47505c9133bc48bae8a1ac025147910316748d6d6fad6555",
    );
}
