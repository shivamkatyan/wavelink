//! TPDF dither for the documented 24→16 bit lossy down-conversion
//! (ADR-004 / PROTOCOL_SPEC §Codec profiles).
//!
//! TPDF (triangular probability density function, 2 LSB peak-to-peak) adds two
//! uniform [-0.5, 0.5) LSB draws per sample — the sum is a symmetric triangle
//! around zero. It is the classical dithering choice for PCM quantisation:
//! the mean of the signal is preserved (the dither is zero-mean), giving a
//! clean, non-correlated error spectrum rather than harmonic distortion.

use crate::error::CodecError;
use rand::rngs::StdRng;

/// Trait for the injectable, deterministic RNG used by [`tpdf_dither_24_to_16`].
///
/// Implemented automatically for `&mut rand::rngs::StdRng` (and any type that
/// exposes [`rand::RngCore`]), so callers can pass a seeded [`StdRng::from_seed`]
/// to get fully deterministic dither.
pub trait DitherRng: rand::Rng {
    /// Draw a sample in the half-open interval `[-0.5, 0.5)`.
    fn uniform_half(&mut self) -> f64;
}

impl DitherRng for StdRng {
    fn uniform_half(&mut self) -> f64 {
        use rand::RngExt as _;
        self.random_range(-0.5f64..0.5)
    }
}

/// Input forms accepted by [`tpdf_dither_24_to_16`]: either raw 24-bit-pack
/// bytes (3 per sample) or already-deinterleaved i32 samples.
#[derive(Clone, Copy)]
pub enum TwentyFourBitSamples<'a> {
    /// 24-bit packed little-endian bytes (3 bytes/sample), interleaved.
    Packed24(&'a [u8]),
    /// 24-bit samples left-aligned or right-aligned in i32.
    I32(&'a [i32]),
}

/// Down-convert 24-bit PCM to 16-bit with (deterministic, injectable) TPDF
/// dither.
///
/// * `samples`: the 24-bit source (see [`TwentyFourBitSamples`]).
/// * `rng`: the seeded RNG; pass a `StdRng::from_seed` for determinism.
///
/// The 24-bit value is taken as a right-aligned i32 in `[-2^23, 2^23)`. The
/// conversion: `out = (v >> 8) + round(tpdf)` with the fractional 8 bits of
/// the source contributing to a deterministic sub-LSB that biases toward the
/// true 16-bit value, then clamped to i16.
pub fn tpdf_dither_24_to_16(
    samples: TwentyFourBitSamples<'_>,
    rng: &mut dyn DitherRng,
) -> Result<Vec<i16>, CodecError> {
    let values: Vec<i32> = match samples {
        TwentyFourBitSamples::Packed24(bytes) => {
            if bytes.len() % 3 != 0 {
                return Err(CodecError::OddSampleBuffer {
                    len: bytes.len(),
                    bytes_per_sample: 3,
                });
            }
            bytes
                .as_chunks::<3>()
                .0
                .iter()
                .map(|b| {
                    let raw = (i32::from(b[0])) | (i32::from(b[1]) << 8) | (i32::from(b[2]) << 16);
                    // sign-extend the 24-bit two's complement.
                    if raw & 0x0080_0000 != 0 {
                        raw | !0x00FF_FFFF
                    } else {
                        raw
                    }
                })
                .collect()
        }
        TwentyFourBitSamples::I32(v) => v.to_vec(),
    };

    let mut out = Vec::with_capacity(values.len());
    for v in values {
        // The 16-bit rounded value (round-half-away handling not needed: the
        // dither adds the triangle, then we truncate the low 8 bits).
        let base = v >> 8;
        // TPDF: two uniform draws sum to a symmetric triangle in [-1, 1).
        let d = rng.uniform_half() + rng.uniform_half();
        let combined = base as f64 + d;
        let r = i32::try_from(combined.round() as i64).unwrap_or(if combined > 0.0 {
            i32::MAX
        } else {
            i32::MIN
        });
        out.push(r.clamp(i16::MIN as i32, i16::MAX as i32) as i16);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::SeedableRng;

    /// A deterministic RNG wrapper for tests (shared seed across calls).
    fn rng(seed: u64) -> StdRng {
        let mut s = [0u8; 32];
        let bytes = seed.to_le_bytes();
        s[..8].copy_from_slice(&bytes);
        StdRng::from_seed(s)
    }

    #[test]
    fn dither_24_to_16_mean_preserved() {
        // A block of 24-bit values that share the same top 16 bits: dithering
        // must keep the mean extremely close to the true 16-bit value.
        // The true value: take v with 16-bit part = 1000 and fractional 8 bits
        // uniformly distributed → dither should reproduce ~1000 on average.
        let base_16: i32 = 1000;
        let mut inputs = Vec::new();
        for frac in 0..256 {
            let v = (base_16 << 8) | frac;
            inputs.push(v);
        }
        let mut r = rng(99);
        let out = tpdf_dither_24_to_16(TwentyFourBitSamples::I32(&inputs), &mut r).unwrap();
        let mean: f64 = out.iter().map(|&s| f64::from(s)).sum::<f64>() / out.len() as f64;
        // 256 samples of i16 output; mean should be ~1000 (within 0.5).
        assert!(
            (mean - f64::from(base_16)).abs() < 0.5,
            "mean {mean} vs true {base_16}"
        );
    }

    #[test]
    fn dither_is_extremely_close_to_quantised_baseline() {
        // For a DC input whose 8 fractional bits are 0, dithering must stay
        // within ±1 of the exact rounded value (no bias greater than one LSB).
        let inputs: Vec<i32> = (0..4096).map(|i| (1234i32 << 8) + (i & 0x7f)).collect();
        let mut r = rng(7);
        let out = tpdf_dither_24_to_16(TwentyFourBitSamples::I32(&inputs), &mut r).unwrap();
        for (i, &s) in out.iter().enumerate() {
            let expect = inputs[i] >> 8;
            let diff = f64::from(s) - f64::from(expect);
            assert!(
                diff.abs() <= 1.0,
                "sample {i}: {s} vs {expect} (diff {diff})"
            );
        }
    }

    #[test]
    fn dither_is_deterministic_given_seed() {
        let inputs: Vec<i32> = (0..512).map(|i| i * 1000).collect();
        let mut r1 = rng(42);
        let mut r2 = rng(42);
        let a = tpdf_dither_24_to_16(TwentyFourBitSamples::I32(&inputs), &mut r1).unwrap();
        let b = tpdf_dither_24_to_16(TwentyFourBitSamples::I32(&inputs), &mut r2).unwrap();
        assert_eq!(a, b);
        // Different seed -> (almost certainly) different result.
        let mut r3 = rng(43);
        let c = tpdf_dither_24_to_16(TwentyFourBitSamples::I32(&inputs), &mut r3).unwrap();
        assert_ne!(a, c);
    }

    #[test]
    fn dither_packed24_matches_i32_form() {
        // Build packed24 bytes for values that fit in 24 bits and compare
        // against the I32 form with the same seeded rng.
        let values: Vec<i32> = vec![0, 255, 256, -1, -256, (1 << 23) - 1, -(1 << 23)];
        let mut packed = Vec::new();
        for v in &values {
            let raw = *v as u32 & 0x00FF_FFFF;
            packed.push((raw & 0xff) as u8);
            packed.push(((raw >> 8) & 0xff) as u8);
            packed.push(((raw >> 16) & 0xff) as u8);
        }
        let mut r1 = rng(5);
        let mut r2 = rng(5);
        let from_packed =
            tpdf_dither_24_to_16(TwentyFourBitSamples::Packed24(&packed), &mut r1).unwrap();
        let from_i32 = tpdf_dither_24_to_16(TwentyFourBitSamples::I32(&values), &mut r2).unwrap();
        assert_eq!(from_packed, from_i32);
    }

    #[test]
    fn dither_rejects_odd_byte_length() {
        let mut r = rng(1);
        assert!(matches!(
            tpdf_dither_24_to_16(TwentyFourBitSamples::Packed24(&[1, 2, 3, 4]), &mut r),
            Err(CodecError::OddSampleBuffer { .. })
        ));
    }

    #[test]
    fn dither_edge_values_clamp_to_i16() {
        let inputs: Vec<i32> = vec![(1 << 23) - 1, -(1 << 23)];
        let mut r = rng(3);
        let out = tpdf_dither_24_to_16(TwentyFourBitSamples::I32(&inputs), &mut r).unwrap();
        // The true 16-bit value of +8388607>>8 = 32767 (clamped), -8388608>>8 = -32768.
        assert_eq!(out[0], 32767);
        assert_eq!(out[1], -32768);
    }
}
