//! Shared test helpers for the `wdr_fakes` integration test suite.
//!
//! Exposed only to integration tests (each `tests/*.rs` is its own crate that
//! includes this file via `mod common;`); never compiled into the library or
//! shipped.

#![allow(dead_code)]

use wdr_fakes::hash::HashSinkState;
use wdr_fakes::source::{ChannelKind, Fixture, FixtureKind, PcmSource, SampleFormat};
/// Stereo 16-bit fixtures at 48 kHz.
pub fn fmt_s16_stereo_48k() -> wdr_fakes::SourceFormat {
    wdr_fakes::SourceFormat::stereo_i16_48k()
}

/// Mono 24-bit fixtures at 44.1 kHz.
pub fn fmt_s24_mono_44_1k() -> wdr_fakes::SourceFormat {
    wdr_fakes::SourceFormat::mono_i24_44_1k()
}

/// Fixture length (samples) used by the canonical golden vectors.
pub const GOLDEN_SAMPLES: u64 = 4096;

/// Build the canonical golden fixture for `kind` at `format`/`rate`/`channels`,
/// seeded via [`source_seed`] with the fixture tag, then drain it and return
/// the running blake3 hash (canonical bytes). This is the function the codec
/// worker's lossless-equality tests reuse.
pub fn golden_hash(
    kind: FixtureKind,
    format: SampleFormat,
    rate_hz: u32,
    channels: ChannelKind,
) -> String {
    let mut fx = Fixture::new(kind, format, rate_hz, channels, GOLDEN_SAMPLES);
    fx.drain_hash(512).hex()
}

/// Hash a fixture with an explicit seed (for the determinism property).
pub fn golden_hash_seeded(seed: [u8; 32], samples: u64) -> String {
    let mut fx = Fixture::new(
        FixtureKind::PseudoRandomPcm,
        SampleFormat::I16,
        48_000,
        ChannelKind::Stereo,
        samples,
    );
    fx.with_seed(seed);
    fx.drain_hash(512).hex()
}

/// A full streaming pipeline helper: source → hash sink, comparing the two
/// independent hashers used by `HashSinkState`.
pub fn pipeline_hash(
    kind: FixtureKind,
    format: SampleFormat,
    rate_hz: u32,
    channels: ChannelKind,
    total: u64,
) -> String {
    let mut fx = Fixture::new(kind, format, rate_hz, channels, total);
    let mut sink = HashSinkState::default();
    loop {
        let got = fx.next_chunk(512);
        if got.len == 0 {
            break;
        }
        sink.update_bytes(got.bytes);
    }
    sink.hex()
}
