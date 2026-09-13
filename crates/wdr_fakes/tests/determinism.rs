//! Determinism property: the same seed produces identical chunk-stream hashes
//! across two runs (and across different chunk sizes, which must not change
//! the resulting hash of the total byte stream).

use wdr_fakes::source::{ChannelKind, Fixture, FixtureKind, PcmSource, SampleFormat, Stereo};

mod common;

#[test]
fn same_seed_implies_same_stream_hash() {
    let a = common::golden_hash(
        FixtureKind::PseudoRandomPcm,
        SampleFormat::I24,
        48_000,
        ChannelKind::Stereo,
    );
    let b = common::golden_hash(
        FixtureKind::PseudoRandomPcm,
        SampleFormat::I24,
        48_000,
        ChannelKind::Stereo,
    );
    assert_eq!(a, b);

    let c = common::golden_hash(
        FixtureKind::ChannelId {
            lane: Stereo::RightPattern,
        },
        SampleFormat::I16,
        44_100,
        ChannelKind::Stereo,
    );
    let d = common::golden_hash(
        FixtureKind::ChannelId {
            lane: Stereo::RightPattern,
        },
        SampleFormat::I16,
        44_100,
        ChannelKind::Stereo,
    );
    assert_eq!(c, d);
}

#[test]
fn chunking_does_not_change_the_hash() {
    // Total-byte hash must be independent of chunk boundaries.
    let mut a = Fixture::new(
        FixtureKind::PseudoRandomPcm,
        SampleFormat::I16,
        48_000,
        ChannelKind::Stereo,
        4096,
    );
    let mut b = Fixture::new(
        FixtureKind::PseudoRandomPcm,
        SampleFormat::I16,
        48_000,
        ChannelKind::Stereo,
        4096,
    );
    assert_eq!(a.drain_hash(64).hex(), b.drain_hash(1024).hex());
}
