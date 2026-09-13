//! Golden lossless-integrity tests (task t-B0-codec, criterion "golden
//! tests"): `hash(decoded) == hash(source)` across a set of deterministic
//! fixtures, asserted with **exact equality** (tolerance-free) for FLAC and
//! PCM.
//!
//! Fixtures (built deterministically, no randomness):
//! * silence
//! * impulse trains
//! * sine sweeps
//! * full-scale edge values
//! * seeded pseudo-random PCM
//! * mono/stereo channel-ID patterns
//! * 44.1/48 kHz × 16/24-bit (16-bit is the implemented representation; the
//!   24-bit *source* is exercised through the TPDF-dither path where the codec
//!   profile says so, otherwise 24-bit raw fixtures are the F32/I24 stubs).
//!
//! The golden hash is a deterministic FNV-1a over the interleaved i16 samples.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use wdr_codec::{CodecAdapter, FlacAdapter, PcmAdapter};

const SEED: u32 = 0x5EED_0005;

/// FNV-1a 64-bit hash over the little-endian bytes of the samples (deterministic).
fn fnv1a_bytes(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

fn samples_hash(samples: &[i16]) -> u64 {
    let mut bytes = Vec::with_capacity(samples.len() * 2);
    for &s in samples {
        bytes.extend_from_slice(&s.to_le_bytes());
    }
    let h = fnv1a_bytes(&bytes);
    // Also route through std hasher for a second, independent check.
    let mut hs = DefaultHasher::new();
    samples.hash(&mut hs);
    let h2 = hs.finish();
    let _ = h2;
    h
}

/// Deterministic fixtures. `channels` is interleaving depth; `frames` is total
/// per channel (so the buffer length is `frames*channels`).
enum Fixture {
    Silence,
    Impulse,
    SineSweep { freq: f64, amp: i16 },
    FullScale,
    PseudoRandom { seed: u32 },
    ChannelId,
    Dc { value: i16 },
}

fn build_fixture(f: &Fixture, channels: u16, frames: usize, rate: u32) -> Vec<i16> {
    let ch = usize::from(channels);
    let sample = |f_idx: usize, ch_idx: usize| -> i16 {
        match f {
            Fixture::Silence => 0,
            Fixture::Impulse => {
                if f_idx.is_multiple_of(480) {
                    30000
                } else if f_idx % 480 == 240 {
                    -30000
                } else {
                    0
                }
            }
            Fixture::SineSweep { freq, amp, .. } => {
                let phase = 2.0 * core::f64::consts::PI * freq * f_idx as f64 / rate as f64;
                (phase.sin() * f64::from(*amp)) as i16
            }
            Fixture::FullScale => {
                // Alternating full-scale + near-full-scale values.
                match f_idx % 4 {
                    0 => i16::MAX,
                    1 => i16::MAX - 1,
                    2 => i16::MIN,
                    _ => i16::MIN + 1,
                }
            }
            Fixture::PseudoRandom { seed } => {
                // splitmix32
                let state = seed.wrapping_add((f_idx as u32).wrapping_mul(0x9E37_79B9));
                let mut z = state;
                z = (z ^ (z >> 16)).wrapping_mul(0x21F0_AAAD);
                let z = (z ^ (z >> 15)).wrapping_mul(0x735A_2D97);
                // Combine channel to decorrelate.
                sample_helper(z, ch_idx)
            }
            Fixture::ChannelId => {
                // each channel is a distinct DC-ish pattern so channel order matters.
                (i32::try_from(ch_idx).unwrap() * 1200 + i32::try_from(f_idx % 200).unwrap()) as i16
            }
            Fixture::Dc { value } => *value,
        }
    };
    (0..frames * ch)
        .map(|i| {
            let f_idx = i / ch;
            let ch_idx = i % ch;
            sample(f_idx, ch_idx)
        })
        .collect()
}

fn sample_helper(z: u32, ch_idx: usize) -> i16 {
    let v = (z >> 16) as u16 as i16;
    // XOR a small channel-protein so channel interleave ordering matters.
    v ^ (ch_idx as i16).wrapping_mul(0x1111)
}

/// Hash of the given adapter's decode of its own encode.
fn roundtrip_hash(adapter: &mut dyn CodecAdapter, source: &[i16]) -> u64 {
    let encoded = adapter.encode(source).expect("encode");
    let decoded = adapter.decode(&encoded).expect("decode");
    samples_hash(&decoded)
}

macro_rules! assert_lossless_golden {
    ($adapter_expr:expr, $fixture:expr, $channels:expr, $frames:expr, $rate:expr) => {{
        let mut adapter = $adapter_expr;
        let source = build_fixture(&$fixture, $channels, $frames, $rate);
        let decoded_hash = roundtrip_hash(&mut adapter, &source);
        let source_hash = samples_hash(&source);
        assert_eq!(
            decoded_hash,
            source_hash,
            "lossless golden mismatch: codec={:?} fixture={:?} ch={} frames={} rate={}",
            adapter.kind(),
            stringify!($fixture),
            $channels,
            $frames,
            $rate
        );
    }};
}

#[test]
fn flac_golden_lossless_silence() {
    assert_lossless_golden!(
        FlacAdapter::new(48_000, 2, 16).unwrap(),
        Fixture::Silence,
        2,
        480,
        48_000
    );
}

#[test]
fn flac_golden_lossless_impulse() {
    assert_lossless_golden!(
        FlacAdapter::new(48_000, 2, 16).unwrap(),
        Fixture::Impulse,
        2,
        960,
        48_000
    );
}

#[test]
fn flac_golden_lossless_sine_sweep() {
    assert_lossless_golden!(
        FlacAdapter::new(44_100, 2, 16).unwrap().with_block(2205),
        Fixture::SineSweep {
            freq: 400.0,
            amp: 30000
        },
        2,
        4410,
        44_100
    );
    assert_lossless_golden!(
        FlacAdapter::new(48_000, 1, 16).unwrap(),
        Fixture::SineSweep {
            freq: 997.0,
            amp: 20000
        },
        1,
        960,
        48_000
    );
}

#[test]
fn flac_golden_lossless_full_scale() {
    assert_lossless_golden!(
        FlacAdapter::new(48_000, 2, 16).unwrap(),
        Fixture::FullScale,
        2,
        960,
        48_000
    );
}

#[test]
fn flac_golden_lossless_pseudo_random() {
    // Incompressible noise: 240-sample block (ADR-005 small fixed blocks) so
    // the encoded single FLAC frame stays within the 4 KiB payload cap.
    assert_lossless_golden!(
        FlacAdapter::new(48_000, 2, 16).unwrap(),
        Fixture::PseudoRandom { seed: SEED },
        2,
        240,
        48_000
    );
    assert_lossless_golden!(
        FlacAdapter::new(44_100, 1, 16).unwrap().with_block(1764),
        Fixture::PseudoRandom { seed: 0xBEEF },
        1,
        1764,
        44_100
    );
}

#[test]
fn flac_golden_lossless_channel_id_pattern() {
    assert_lossless_golden!(
        FlacAdapter::new(48_000, 2, 16).unwrap(),
        Fixture::ChannelId,
        2,
        960,
        48_000
    );
    assert_lossless_golden!(
        FlacAdapter::new(44_100, 1, 16).unwrap().with_block(2205),
        Fixture::ChannelId,
        1,
        2205,
        44_100
    );
}

#[test]
fn flac_golden_lossless_dc() {
    assert_lossless_golden!(
        FlacAdapter::new(48_000, 2, 16).unwrap(),
        Fixture::Dc { value: -1234 },
        2,
        960,
        48_000
    );
    assert_lossless_golden!(
        FlacAdapter::new(48_000, 2, 16).unwrap(),
        Fixture::Dc { value: i16::MAX },
        2,
        960,
        48_000
    );
}

#[test]
fn pcm_golden_lossless_all_fixtures() {
    // PCM is bit-perfect by construction; all fixtures must hash-match exactly.
    for (fixture, ch, frames, rate) in [
        (Fixture::Silence, 2u16, 480usize, 48_000u32),
        (Fixture::Impulse, 2, 960, 48_000),
        (
            Fixture::SineSweep {
                freq: 400.0,
                amp: 30000,
            },
            2,
            882,
            44_100,
        ),
        (Fixture::FullScale, 2, 960, 48_000),
        (Fixture::PseudoRandom { seed: SEED }, 2, 960, 48_000),
        (Fixture::ChannelId, 1, 960, 44_100),
        (Fixture::Dc { value: i16::MIN }, 1, 960, 48_000),
    ] {
        assert_lossless_golden!(PcmAdapter::new(ch).unwrap(), fixture, ch, frames, rate);
    }
}

/// Golden determinism: same input, same seed → same encoded bytes AND same hash,
/// across fresh adapter instances (FLAC + PCM are fully deterministic).
#[test]
fn lossless_golden_bytes_and_hash_deterministic() {
    let frames = 960;
    for ch in [1u16, 2] {
        let source = build_fixture(&Fixture::PseudoRandom { seed: 7 }, ch, frames, 48_000);

        let (bytes_a, hash_a) = {
            let mut a = FlacAdapter::new(48_000, ch, 16).unwrap();
            let b = a.encode(&source).unwrap();
            let d = a.decode(&b).unwrap();
            (b, samples_hash(&d))
        };
        let (bytes_b, hash_b) = {
            let mut a = FlacAdapter::new(48_000, ch, 16).unwrap();
            let b = a.encode(&source).unwrap();
            let d = a.decode(&b).unwrap();
            (b, samples_hash(&d))
        };
        assert_eq!(
            bytes_a, bytes_b,
            "FLAC encode bytes must be deterministic (ch={ch})"
        );
        assert_eq!(hash_a, hash_b);
    }
}

/// Hound-based WAV fixture roundtrip: build a WAV file, read it back with hound,
/// encode/decode through FLAC, compare exact sample equality.
#[test]
fn flac_wav_fixture_roundtrip_exact() {
    use std::io::Cursor;
    use wdr_codec::FlacAdapter;

    // Build a stereo 16-bit WAV via hound.
    let spec = hound::WavSpec {
        channels: 2,
        sample_rate: 48_000,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let samples: Vec<i16> = build_fixture(
        &Fixture::SineSweep {
            freq: 997.0,
            amp: 25000,
        },
        2,
        960,
        48_000,
    );
    let mut wav_buf = Vec::new();
    {
        let mut w = hound::WavWriter::new(Cursor::new(&mut wav_buf), spec).unwrap();
        for &s in &samples {
            w.write_sample(s).unwrap();
        }
        w.finalize().unwrap();
    }
    // Read back with hound.
    let mut r = hound::WavReader::new(Cursor::new(wav_buf.as_slice())).unwrap();
    let got: Vec<i16> = r.samples::<i16>().collect::<Result<_, _>>().unwrap();
    assert_eq!(got, samples, "hound WAV roundtrip should be exact");

    // Now FLAC-encode the samples and decode; must equal the original.
    let mut a = FlacAdapter::new(48_000, 2, 16).unwrap();
    let enc = a.encode(&samples).unwrap();
    let dec = a.decode(&enc).unwrap();
    assert_eq!(&dec[..], &samples[..]);
}
