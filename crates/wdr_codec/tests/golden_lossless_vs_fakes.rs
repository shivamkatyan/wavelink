//! Lossless-integrity tests wired to the **shared canonical fixtures** from
//! `wdr_fakes` (task t-B0-wire: the codec worker consumes the fakes goldens).
//!
//! Contract (ADR-005, TEST_PLAN §Golden audio, `wdr_fakes` crate docs):
//! lossless means **sample-identical** — `hash(decoded) == hash(source)` with
//! no tolerance. The canonical byte stream is `i16` little-endian for 16-bit
//! and 24-bit packed (3 bytes/sample, right-aligned i32 / low-3-bytes LE) for
//! 24-bit, stereo interleaved `L–R–L–R` at 48 kHz. The recorded golden hashes
//! (blake3, chunk=512 samples, total=4096 samples) live in `t-B0-fakes.md` and
//! are asserted here as `hash(decoded bytes) == canonical golden` for every
//! fixture row the `wdr_codec` adapters can drive.
//!
//! # Coverage matrix
//!
//! | Adapter | Format | Rate / channels | Status |
//! |---------|--------|-----------------|--------|
//! | `PcmAdapter`  | i16 | 48k / stereo | ✅ hash(decoded)==golden for all 9 fixtures + channel order |
//! | `FlacAdapter` | i16 | 48k / stereo | ✅ hash(decoded)==golden for all 9 fixtures + channel order |
//! | `PcmAdapter::new24` + `CodecAdapter24` | 24-bit | 48k / stereo | ✅ hash(decoded)==golden for the 5 recorded i24 fixtures |
//! | `FlacAdapter::new(.., 24)` + `CodecAdapter24` | 24-bit | 48k / stereo | ✅ hash(decoded)==golden for the 5 recorded i24 fixtures |
//!
//! Existing golden tests (`tests/golden.rs`) are left unchanged; they may
//! overlap — that is intended. Malformed-input coverage stays in the existing
//! golden/roundtrip/properties suites.

use wdr_codec::{CodecAdapter, CodecAdapter24, FlacAdapter, PcmAdapter, SampleRepr};
use wdr_fakes::hash::HashSink;
use wdr_fakes::source::{
    unpack_i24, ChannelKind, Fixture, FixtureKind, PcmSource, SampleFormat, Stereo,
};

/// Canonical golden instrumentation parameters (must match `wdr_fakes`
/// `tests/golden.rs` / `examples/golden_hashes.rs` exactly).
const CHUNK: u32 = 512;
const SAMPLES: u64 = 4096;
/// i16 lossless adapter test cell (the declared supported matrix): 48 kHz
/// stereo, the same rate/channels every recorded i16 golden row uses.
const RATE: u32 = 48_000;
const CHANNELS: u16 = 2;

/// One recorded canonical golden row (blake3 hex) for an i16/48k/stereo
/// fixture. The constants come from `wdr_fakes/tests/golden.rs` and
/// `docs/orchestration/reports/t-B0-fakes.md`.
struct GoldenSpec {
    name: &'static str,
    kind: FixtureKind,
    golden: &'static str,
}

/// All 9 recorded i16/48k/stereo golden rows. The `FixtureKind` variants are
/// used exactly as exported by `wdr_fakes::source`.
fn i16_48k_stereo_goldens() -> Vec<GoldenSpec> {
    vec![
        GoldenSpec {
            name: "silence",
            kind: FixtureKind::Silence,
            golden: "128daa44a4f7badaed2244bb6fe009d5e7803177414e01d7d9df80c190e14906",
        },
        GoldenSpec {
            name: "impulse-train-64",
            kind: FixtureKind::ImpulseTrain { period: 64 },
            golden: "0fd38c7cea5e94e00ea2e4f075aaa2e3d2776cecba091f31a1bbc8fa8718504c",
        },
        GoldenSpec {
            name: "impulse-train-256",
            kind: FixtureKind::ImpulseTrain { period: 256 },
            golden: "a9c9d032850a93dd5a88307de178731f147a0cbd6c30b31305b1795f259957a1",
        },
        GoldenSpec {
            name: "sine-sweep-20-20000",
            kind: FixtureKind::SineSweep {
                f0: 20.0,
                f1: 20_000.0,
            },
            golden: "0736b76584c1087dac709cede7be36549c920c5caad4aab7d7df62b58056df08",
        },
        GoldenSpec {
            name: "sine-sweep-200-20000",
            kind: FixtureKind::SineSweep {
                f0: 200.0,
                f1: 20_000.0,
            },
            golden: "3865f9f5789cbe1d08577ba88604817821fc8ef771c56ed4efbfe1fe409e9f4d",
        },
        GoldenSpec {
            name: "full-scale-edge",
            kind: FixtureKind::FullScaleEdge,
            golden: "931b80605fa8cf6a647c3166894c3631f752b0bcedbbed084676d3fe25d9627d",
        },
        GoldenSpec {
            name: "pseudo-random-pcm",
            kind: FixtureKind::PseudoRandomPcm,
            golden: "b7a3c25c8ccaa05643f14dfb5bc0e223f4ace3154c11ecd25bed5975d839c223",
        },
        GoldenSpec {
            name: "channel-id-left",
            kind: FixtureKind::ChannelId {
                lane: Stereo::LeftPattern,
            },
            golden: "5c0f5d79e7784169c1b635d884316bcadf2a485a6aabf9bcf551a3e64798311f",
        },
        GoldenSpec {
            name: "channel-id-right",
            kind: FixtureKind::ChannelId {
                lane: Stereo::RightPattern,
            },
            golden: "a958a425f58d8d3e0bb4a3f1b2f980cef257231f712c0524c714a92d671c25c6",
        },
    ]
}

/// Convert the canonical i16-le wire bytes of a chunk back into interleaved
/// `i16` samples (the adapter input slice).
fn i16_samples_from_canonical(bytes: &[u8]) -> Vec<i16> {
    bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|b| i16::from_le_bytes(*b))
        .collect()
}

/// Convert decoded interleaved `i16` samples back into the canonical i16-le
/// wire bytes so the decoded stream can be hashed with the same HashSink the
/// source is hashed with.
fn canonical_bytes_from_i16(samples: &[i16]) -> Vec<u8> {
    let mut out = Vec::with_capacity(samples.len() * 2);
    for &s in samples {
        out.extend_from_slice(&s.to_le_bytes());
    }
    out
}

/// Convert canonical i24 low-3-byte wire bytes back into interleaved
/// right-aligned `i32` samples (the `CodecAdapter24` input slice).
fn i32_samples_from_canonical_i24(bytes: &[u8]) -> Vec<i32> {
    bytes
        .as_chunks::<3>()
        .0
        .iter()
        .map(|c| unpack_i24(c))
        .collect()
}

/// Convert decoded interleaved `i32` samples back into the canonical i24
/// low-3-byte wire stream (mirror `wdr_fakes` `SampleFormat::I24` encoding).
fn canonical_i24_from_i32(samples: &[i32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(samples.len() * 3);
    for &s in samples {
        let c = s.clamp(-(1i32 << 23) + 1, (1i32 << 23) - 1);
        let b = c.to_le_bytes();
        out.extend_from_slice(&b[..3]);
    }
    out
}

/// Drive the full 4096-sample fixture through `adapter.encode → adapter.decode`
/// in 512-sample chunks (the same chunking the canonical goldens are recorded
/// with), hashing both the source wire bytes and the decoded wire bytes.
/// Asserts, tolerance-free:
///  1. `hash(decoded) == hash(source)` (sample-identical lossless equality), and
///  2. `hash(source) == recorded canonical golden` (the shared fakes goldens).
///
/// Returns the decoded hex for callers that want to reuse it.
#[track_caller]
fn assert_lossless_against_canonical_golden(
    adapter: &mut dyn CodecAdapter,
    spec: &GoldenSpec,
) -> String {
    let mut fx = Fixture::new(
        spec.kind.clone(),
        SampleFormat::I16,
        RATE,
        ChannelKind::Stereo,
        SAMPLES,
    );
    let mut source_sink = HashSink::start();
    let mut decoded_sink = HashSink::start();
    let mut chunks = 0u32;
    loop {
        let got = fx.next_chunk(CHUNK);
        if got.len == 0 {
            break;
        }
        chunks += 1;
        assert_eq!(got.len, CHUNK as usize, "expected chunk of {CHUNK} samples");
        source_sink.push(got.bytes);

        let samples = i16_samples_from_canonical(got.bytes);
        let encoded = adapter.encode(&samples).expect("encode");
        let decoded = adapter.decode(&encoded).expect("decode");
        assert_eq!(
            decoded.len(),
            samples.len(),
            "decoded sample count must match source chunk"
        );
        let decoded_bytes = canonical_bytes_from_i16(&decoded);
        assert_eq!(decoded_bytes.len(), got.bytes.len());
        decoded_sink.push(&decoded_bytes);
    }
    assert!(chunks > 0, "fixture produced no chunks");
    assert_eq!(
        chunks * CHUNK,
        SAMPLES as u32,
        "chunking must reproduce the canonical stream length"
    );

    let decoded_hex = decoded_sink.hex();
    let source_hex = source_sink.hex();

    // Lossless contract: hash(decoded) == hash(source), tolerance-free.
    assert_eq!(
        decoded_hex,
        source_hex,
        "lossless equality broken for {}: {}: decoded {} != source {}",
        spec.name,
        adapter.name(),
        decoded_hex,
        source_hex
    );
    // The source hashed with the canonical chunking equals the recorded golden.
    assert_eq!(
        source_hex, spec.golden,
        "fixture drift vs canonical golden for {}: {}",
        spec.name, source_hex
    );

    decoded_hex
}

// ---------------------------------------------------------------------------
// FLAC — all 9 recorded i16/48k/stereo canonical goldens
// ---------------------------------------------------------------------------

#[test]
fn flac_i16_48k_stereo_all_canonical_fakes_goldens() {
    for spec in i16_48k_stereo_goldens() {
        let mut a = FlacAdapter::new(RATE, CHANNELS, 16).unwrap();
        let hex = assert_lossless_against_canonical_golden(&mut a, &spec);
        assert_eq!(
            hex, spec.golden,
            "FLAC decoded hash must equal canonical golden for {}",
            spec.name
        );
    }
}

// ---------------------------------------------------------------------------
// PCM — all 9 recorded i16/48k/stereo canonical goldens
// ---------------------------------------------------------------------------

#[test]
fn pcm_i16_48k_stereo_all_canonical_fakes_goldens() {
    for spec in i16_48k_stereo_goldens() {
        let mut a = PcmAdapter::new(CHANNELS).unwrap();
        let hex = assert_lossless_against_canonical_golden(&mut a, &spec);
        assert_eq!(
            hex, spec.golden,
            "PCM decoded hash must equal canonical golden for {}",
            spec.name
        );
    }
}

// ---------------------------------------------------------------------------
// Channel order — decode preserves left = pattern A / right = pattern B
// ---------------------------------------------------------------------------

/// Distinct DC levels the channel-id fixtures use (wdr_fakes `Stereo::level`):
/// LeftPattern = +564, RightPattern = −904. For stereo the interleave is
/// `L–R–L–R`; a fixture with `lane: LeftPattern` emits L=+564 / R=−564 and one
/// with `lane: RightPattern` emits L=+904 / R=−904.
fn expected_lane_levels(kind: &FixtureKind) -> (i16, i16) {
    match kind {
        FixtureKind::ChannelId {
            lane: Stereo::LeftPattern,
        } => (564, -564),
        FixtureKind::ChannelId {
            lane: Stereo::RightPattern,
        } => (904, -904),
        _ => unreachable!("channel-order check is channel-id only"),
    }
}

#[track_caller]
fn assert_channel_order_preserved(adapter: &mut dyn CodecAdapter, kind: FixtureKind) {
    let (left, right) = expected_lane_levels(&kind);
    let mut fx = Fixture::new(kind, SampleFormat::I16, RATE, ChannelKind::Stereo, SAMPLES);
    let mut frames_checked = 0usize;
    loop {
        let got = fx.next_chunk(CHUNK);
        if got.len == 0 {
            break;
        }
        let samples = i16_samples_from_canonical(got.bytes);
        let encoded = adapter.encode(&samples).expect("encode");
        let decoded = adapter.decode(&encoded).expect("decode");
        assert_eq!(
            decoded.len(),
            samples.len(),
            "channel-order decode length mismatch"
        );
        for frame in 0..decoded.len() / usize::from(CHANNELS) {
            assert_eq!(
                decoded[frame * 2],
                left,
                "left lane must stay pattern A after {} decode",
                adapter.name()
            );
            assert_eq!(
                decoded[frame * 2 + 1],
                right,
                "right lane must stay pattern B after {} decode",
                adapter.name()
            );
        }
        frames_checked += decoded.len() / usize::from(CHANNELS);
    }
    assert_eq!(frames_checked, SAMPLES as usize / usize::from(CHANNELS));
}

#[test]
fn pcm_preserves_channel_id_left_equals_pattern_a_right_equals_pattern_b() {
    assert_channel_order_preserved(
        &mut PcmAdapter::new(CHANNELS).unwrap(),
        FixtureKind::ChannelId {
            lane: Stereo::LeftPattern,
        },
    );
    assert_channel_order_preserved(
        &mut PcmAdapter::new(CHANNELS).unwrap(),
        FixtureKind::ChannelId {
            lane: Stereo::RightPattern,
        },
    );
}

#[test]
fn flac_preserves_channel_id_left_equals_pattern_a_right_equals_pattern_b() {
    assert_channel_order_preserved(
        &mut FlacAdapter::new(RATE, CHANNELS, 16).unwrap(),
        FixtureKind::ChannelId {
            lane: Stereo::LeftPattern,
        },
    );
    assert_channel_order_preserved(
        &mut FlacAdapter::new(RATE, CHANNELS, 16).unwrap(),
        FixtureKind::ChannelId {
            lane: Stereo::RightPattern,
        },
    );
}

// ---------------------------------------------------------------------------
// i24 — 24-bit lossless via `CodecAdapter24`, driven against the recorded
// canonical i24 goldens (ADR-005 follow-up)
// ---------------------------------------------------------------------------

/// The 5 recorded i24/48k/stereo golden rows (chunk=512, total=4096), exactly
/// as in `wdr_fakes/tests/golden.rs` / `t-B0-fakes.md`. (Impulse-train and
/// sine-sweep have i16 goldens only today.)
fn i24_48k_stereo_goldens() -> Vec<GoldenSpec> {
    vec![
        GoldenSpec {
            name: "silence",
            kind: FixtureKind::Silence,
            golden: "819ad8f20ee2578f84eeb28b4aa852458c066911cce810767021030961e43e60",
        },
        GoldenSpec {
            name: "full-scale-edge",
            kind: FixtureKind::FullScaleEdge,
            golden: "0a212e00175469d8156a45786d8efd226d2447842d8033b4ce5035fa85272978",
        },
        GoldenSpec {
            name: "pseudo-random-pcm",
            kind: FixtureKind::PseudoRandomPcm,
            golden: "edf3016e9c7dd72253b5781fbd0443ed278482261d92c960ee573ebc491ef3ca",
        },
        GoldenSpec {
            name: "channel-id-left",
            kind: FixtureKind::ChannelId {
                lane: Stereo::LeftPattern,
            },
            golden: "89569544da2e919779c563282867930ecbf7d4c529ea29f747d359c5de394515",
        },
        GoldenSpec {
            name: "channel-id-right",
            kind: FixtureKind::ChannelId {
                lane: Stereo::RightPattern,
            },
            golden: "e5c162450ac2eb8d47505c9133bc48bae8a1ac025147910316748d6d6fad6555",
        },
    ]
}

/// Drive the full 4096-sample fixture through `adapter.encode_24 →
/// adapter.decode_24` in 512-sample chunks, hashing both the source wire
/// bytes and the decoded wire bytes in canonical i24 form. Same lossless
/// contract as the i16 driver: `hash(decoded) == hash(source)` and
/// `hash(source) == recorded canonical golden`.
#[track_caller]
fn assert_i24_lossless_against_canonical_golden(
    adapter: &mut dyn CodecAdapter24,
    spec: &GoldenSpec,
) -> String {
    let mut fx = Fixture::new(
        spec.kind.clone(),
        SampleFormat::I24,
        RATE,
        ChannelKind::Stereo,
        SAMPLES,
    );
    let mut source_sink = HashSink::start();
    let mut decoded_sink = HashSink::start();
    let mut chunks = 0u32;
    loop {
        let got = fx.next_chunk(CHUNK);
        if got.len == 0 {
            break;
        }
        chunks += 1;
        assert_eq!(got.len, CHUNK as usize, "expected chunk of {CHUNK} samples");
        source_sink.push(got.bytes);

        let samples = i32_samples_from_canonical_i24(got.bytes);
        let encoded = adapter.encode_24(&samples).expect("encode_24");
        let decoded = adapter.decode_24(&encoded).expect("decode_24");
        assert_eq!(
            decoded.len(),
            samples.len(),
            "decoded sample count must match source chunk"
        );
        let decoded_bytes = canonical_i24_from_i32(&decoded);
        assert_eq!(decoded_bytes.len(), got.bytes.len());
        decoded_sink.push(&decoded_bytes);
    }
    assert!(chunks > 0, "fixture produced no chunks");
    assert_eq!(
        chunks * CHUNK,
        SAMPLES as u32,
        "chunking must reproduce the canonical stream length"
    );

    let decoded_hex = decoded_sink.hex();
    let source_hex = source_sink.hex();

    // Lossless contract: hash(decoded) == hash(source), tolerance-free.
    assert_eq!(
        decoded_hex,
        source_hex,
        "i24 lossless equality broken for {}: {}: decoded {} != source {}",
        spec.name,
        adapter.name(),
        decoded_hex,
        source_hex
    );
    // The source hashed with the canonical chunking equals the recorded golden.
    assert_eq!(
        source_hex, spec.golden,
        "fixture drift vs canonical i24 golden for {}: {}",
        spec.name, source_hex
    );

    decoded_hex
}

#[test]
fn flac_i24_48k_stereo_all_canonical_fakes_goldens() {
    for spec in i24_48k_stereo_goldens() {
        let mut a = FlacAdapter::new(RATE, CHANNELS, 24).unwrap();
        let hex = assert_i24_lossless_against_canonical_golden(&mut a, &spec);
        assert_eq!(
            hex, spec.golden,
            "FLAC-24 decoded hash must equal canonical golden for {}",
            spec.name
        );
    }
}

#[test]
fn pcm_i24_48k_stereo_all_canonical_fakes_goldens() {
    for spec in i24_48k_stereo_goldens() {
        let mut a = PcmAdapter::new24(CHANNELS).unwrap();
        let hex = assert_i24_lossless_against_canonical_golden(&mut a, &spec);
        assert_eq!(
            hex, spec.golden,
            "PCM-24 decoded hash must equal canonical golden for {}",
            spec.name
        );
    }
}

#[test]
fn i24_channel_order_preserved_by_codec_adapter_24() {
    // Decode the i24 channel-id fixtures through both 24-bit adapters and
    // assert left = pattern A (+564) / right = pattern B (−904), the same
    // levels the i16 channel-order check uses.
    assert_channel_order_preserved_24(
        &mut FlacAdapter::new(RATE, CHANNELS, 24).unwrap(),
        FixtureKind::ChannelId {
            lane: Stereo::LeftPattern,
        },
        FixtureKind::ChannelId {
            lane: Stereo::RightPattern,
        },
    );
    assert_channel_order_preserved_24(
        &mut PcmAdapter::new24(CHANNELS).unwrap(),
        FixtureKind::ChannelId {
            lane: Stereo::LeftPattern,
        },
        FixtureKind::ChannelId {
            lane: Stereo::RightPattern,
        },
    );
}

#[track_caller]
fn assert_channel_order_preserved_24(
    adapter: &mut dyn CodecAdapter24,
    left_kind: FixtureKind,
    right_kind: FixtureKind,
) {
    for (kind, (left, right)) in [
        (left_kind, (564i32, -564i32)),
        (right_kind, (904i32, -904i32)),
    ] {
        let mut fx = Fixture::new(kind, SampleFormat::I24, RATE, ChannelKind::Stereo, SAMPLES);
        let mut frames_checked = 0usize;
        loop {
            let got = fx.next_chunk(CHUNK);
            if got.len == 0 {
                break;
            }
            let samples = i32_samples_from_canonical_i24(got.bytes);
            let encoded = adapter.encode_24(&samples).expect("encode_24");
            let decoded = adapter.decode_24(&encoded).expect("decode_24");
            assert_eq!(
                decoded.len(),
                samples.len(),
                "i24 channel-order decode length mismatch"
            );
            for frame in 0..decoded.len() / usize::from(CHANNELS) {
                assert_eq!(
                    decoded[frame * 2],
                    left,
                    "left lane must stay pattern A after {} decode",
                    adapter.name()
                );
                assert_eq!(
                    decoded[frame * 2 + 1],
                    right,
                    "right lane must stay pattern B after {} decode",
                    adapter.name()
                );
            }
            frames_checked += decoded.len() / usize::from(CHANNELS);
        }
        assert_eq!(frames_checked, SAMPLES as usize / usize::from(CHANNELS));
    }
}

// ---------------------------------------------------------------------------
// i24 representation contract — the remaining stubs stay *typed* errors, and
// 24-bit is genuinely implemented (never a silent i16 fall-through)
// ---------------------------------------------------------------------------

/// The `wdr_codec` adapters now implement 16-bit and 24-bit lossless
/// (ADR-005 follow-up). `F32`/`I32` remain declared-but-unimplemented stubs
/// and must keep returning a typed `Unsupported` — nothing below i16/i24 is
/// claimable as verified.
#[test]
fn i24_implemented_and_f32_i32_declared() {
    assert!(matches!(SampleRepr::I24Packed.validate(), Ok(())));
    // WS-E: F32/I32 are now implemented on the PCM adapter (raw IEEE-f32 /
    // 32-bit-int passthrough); they validate as declared. FLAC stays 16/24 and
    // gates that in its constructor (its decode range can't span ±2^31 in i32).
    assert!(matches!(SampleRepr::F32.validate(), Ok(())));
    assert!(matches!(SampleRepr::I32.validate(), Ok(())));
    // The codec is honest per-constructor: PCM builds f32/i32, FLAC does not.
    assert!(wdr_codec::adapters::PcmAdapter::new_f32(2).is_ok());
    assert!(wdr_codec::adapters::PcmAdapter::new_i32(2).is_ok());
    assert!(wdr_codec::adapters::FlacAdapter::new(48_000, 2, 32).is_err());
}
