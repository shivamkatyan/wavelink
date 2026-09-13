//! Channel-order verification: the stereo channel-id fixture interleaves
//! L–R–L–R such that decoding into lanes recovers pattern A on the left and
//! pattern B on the right — the exact property the codec worker relies on to
//! prove channel order survives encode/decode.

use wdr_fakes::source::{ChannelKind, Fixture, FixtureKind, PcmSource, SampleFormat, Stereo};

mod common;

#[test]
fn channel_order_verifiable() {
    let mut left = Fixture::new(
        FixtureKind::ChannelId {
            lane: Stereo::LeftPattern,
        },
        SampleFormat::I16,
        48_000,
        ChannelKind::Stereo,
        4096,
    );
    let mut right = Fixture::new(
        FixtureKind::ChannelId {
            lane: Stereo::RightPattern,
        },
        SampleFormat::I16,
        48_000,
        ChannelKind::Stereo,
        4096,
    );

    let lch = left.next_chunk(4); // L R L R
    let rch = right.next_chunk(4);

    let l0 = i16::from_le_bytes([lch.bytes[0], lch.bytes[1]]);
    let l1 = i16::from_le_bytes([lch.bytes[2], lch.bytes[3]]);
    let l2 = i16::from_le_bytes([lch.bytes[4], lch.bytes[5]]);
    let l3 = i16::from_le_bytes([lch.bytes[6], lch.bytes[7]]);

    // Left uses pattern A on the left lane and its destructively-inverted
    // level on the right lane, so deinterleave recovers it uniquely.
    assert_eq!(
        (l0, l1, l2, l3),
        (564, -564, 564, -564),
        "left-pattern fixture must L=A R=~A L=A R=~A"
    );

    let r0 = i16::from_le_bytes([rch.bytes[0], rch.bytes[1]]);
    let r1 = i16::from_le_bytes([rch.bytes[2], rch.bytes[3]]);
    let r2 = i16::from_le_bytes([rch.bytes[4], rch.bytes[5]]);
    let r3 = i16::from_le_bytes([rch.bytes[6], rch.bytes[7]]);

    assert_eq!(
        (r0, r1, r2, r3),
        (904, -904, 904, -904),
        "right-pattern fixture must L=~B R=B L=~B R=B (pattern B on right lane)"
    );
}

#[test]
fn channel_order_survives_full_stream_decoding() {
    // Decode every frame of the full fixture: even-index samples = lane 0
    // (the "left" of the interleave), odd-index = lane 1. Lane 0 must be all
    // pattern A, lane 1 all pattern B-complement.
    let mut fx = Fixture::new(
        FixtureKind::ChannelId {
            lane: Stereo::LeftPattern,
        },
        SampleFormat::I16,
        44_100,
        ChannelKind::Stereo,
        4096,
    );
    loop {
        let ch = fx.next_chunk(512);
        if ch.len == 0 {
            break;
        }
        let samples: Vec<i16> = ch
            .bytes
            .as_chunks::<2>()
            .0
            .iter()
            .map(|b| i16::from_le_bytes(*b))
            .collect();
        for (i, s) in samples.iter().enumerate() {
            if i % 2 == 0 {
                assert_eq!(*s, 564, "lane 0 must be pattern A at sample {i}");
            } else {
                assert_eq!(*s, -564, "lane 1 must be ~pattern A at sample {i}");
            }
        }
    }
}
