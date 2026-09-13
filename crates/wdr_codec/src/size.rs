//! Frame-size calculator (ADR-005 duty) + the ADR-005 spot-check matrix.
//!
//! The calculator computes, for a given `budget` in bytes (default
//! `MAX_FRAME_PAYLOAD` = 4 KiB), the maximum number of frames (samples per
//! channel) whose **raw PCM payload** fits in the budget:
//!
//! ```text
//! max_frame_samples = budget / (channels * bytes_per_sample)
//! ```
//!
//! This raw-byte quantity is the conservative worst-case upper bound for both
//! the FLAC profile (a FLAC frame of `n` inter-channel samples can never
//! exceed the raw PCM byte size) and the raw-PCM profile (exact). It is
//! therefore the single unifying ceiling consistent with
//! `MAX_FRAME_PAYLOAD` (PROTOCOL_SPEC §State machine numeric bounds: "audio
//! frame payload cap ≤4 KB, enforced before allocation"). For Opus the bound
//! still applies as the raw ceiling, but any legal Opus packet is additionally
//! bounded to ≤1275 bytes by libopus, so Opus frames always fit the 4 KiB cap
//! regardless of the raw ceiling.

use core::cmp;
use core::fmt::Write as _;

pub use crate::adapters::CodecKind;

/// The protocol frame-payload cap (PROTOCOL_SPEC §State machine, numeric
/// bounds): 4 KiB. Mirrors `wdr_proto::MAX_FRAME_PAYLOAD`; independently
/// defined here so `wdr_codec` has no hard dependency on `wdr_proto` at the
/// B0 crate boundary (the frame-container worker owns the wire type).
pub const MAX_FRAME_PAYLOAD: usize = 4096;

/// Claimed default budget when the caller omits one (ADR-005 "default 4096").
pub const DEFAULT_BUDGET: usize = MAX_FRAME_PAYLOAD;

/// Fallback cap so the calculator never returns an unbounded value even for an
/// absurdly large budget (purely defensive; the raw ceiling is already exact).
pub const MAX_FRAME_SAMPLES_FALLBACK: usize = 8192;

/// Compute the maximum number of frames (samples per channel) that fit `budget`
/// bytes of frame payload.
///
/// `bytes_per_sample` is the packed byte width (2 for 16-bit, 3 for 24-bit
/// packed). The result is `budget / (channels * bytes_per_sample)`, capped at
/// [`MAX_FRAME_SAMPLES_FALLBACK`]; returns `0` if fewer than one full frame
/// fits (`budget`, `channels` or `bytes_per_sample` too small / zero).
#[must_use]
pub fn max_frame_samples(
    sample_rate: u32,
    channels: u16,
    bytes_per_sample: u16,
    budget: usize,
) -> usize {
    let _ = sample_rate; // reserved: the raw bound is rate-independent; Opus
                         // frame-duration granularity is selected by the caller
                         // at profile-selection time.
    let channels = usize::from(channels);
    let bytes_per_sample = usize::from(bytes_per_sample);
    if channels == 0 || bytes_per_sample == 0 {
        return 0;
    }
    let Some(raw) = channels.checked_mul(bytes_per_sample) else {
        return 0;
    };
    let Some(frames) = budget.checked_div(raw) else {
        return 0;
    };
    cmp::min(frames, MAX_FRAME_SAMPLES_FALLBACK)
}

/// A deterministic "incompressible noise" fixture (seeded, no RNG dependency).
///
/// Uses a splitmix32 LCG; the high bits pass as near-perfect uniform i16 noise
/// for incompressible-FLAC sizing checks (ADR-005 worst-case fixtures).
#[must_use]
pub fn noise_fixture(samples: usize, channels: usize, seed: u32) -> Vec<i16> {
    let mut state = seed;
    let mut next = move || {
        state = state.wrapping_add(0x9E37_79B9);
        let mut z = state;
        z = (z ^ (z >> 16)).wrapping_mul(0x21F0_AAAD);
        z = (z ^ (z >> 15)).wrapping_mul(0x735A_2D97);
        z ^ (z >> 15)
    };
    (0..samples * channels)
        .map(|_| (next() >> 16) as i16)
        .collect()
}

/// A single row of the ADR-005 spot-check matrix (rate × bit-depth × codec).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MatrixRow {
    /// Sample rate in Hz.
    pub sample_rate: u32,
    /// Bits per sample (16 or 24; 24 shown as its packed byte width).
    pub bits_per_sample: u16,
    /// Codec profile.
    pub codec: CodecKind,
    /// Computed `max_frame_samples` for `MAX_FRAME_PAYLOAD`.
    pub max_samples: usize,
    /// Actual measured byte size of the encoded frame used for the spot-check
    /// (incompressible fixture), if measured.
    pub measured_bytes: Option<usize>,
    /// Whether the measured/raw frame fits the 4 KiB cap.
    pub fits: Option<bool>,
}

impl MatrixRow {
    /// A compact TSV line for the matrix table.
    pub fn to_tsv(&self) -> String {
        let mut s = String::new();
        let _ = write!(
            s,
            "{rate}\t{bits}\t{codec}\t{max}\t{meas}\t{fit}",
            rate = self.sample_rate,
            bits = self.bits_per_sample,
            codec = self.codec.as_str(),
            max = self.max_samples,
            meas = self
                .measured_bytes
                .map_or_else(|| "-".into(), |v| v.to_string()),
            fit = self.fits.map_or_else(|| "-".into(), |v| v.to_string()),
        );
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn max_frame_samples_pcm_16bit_48k_stereo() {
        // 4096 bytes / (2 ch * 2 B) = 1024 frames.
        assert_eq!(max_frame_samples(48_000, 2, 2, MAX_FRAME_PAYLOAD), 1024);
    }

    #[test]
    fn max_frame_samples_pcm_24bit_48k_stereo() {
        // 4096 / (2 * 3) = 682 frames.
        assert_eq!(max_frame_samples(48_000, 2, 3, MAX_FRAME_PAYLOAD), 682);
    }

    #[test]
    fn max_frame_samples_mono_16bit() {
        assert_eq!(max_frame_samples(44_100, 1, 2, MAX_FRAME_PAYLOAD), 2048);
    }

    #[test]
    fn max_frame_samples_44k_same_as_48k_bits_agree() {
        // Rate-independent raw ceiling.
        assert_eq!(
            max_frame_samples(44_100, 2, 3, MAX_FRAME_PAYLOAD),
            max_frame_samples(48_000, 2, 3, MAX_FRAME_PAYLOAD)
        );
    }

    #[test]
    fn max_frame_samples_budget_too_small_returns_zero() {
        assert_eq!(max_frame_samples(48_000, 2, 2, 3), 0);
        assert_eq!(max_frame_samples(48_000, 2, 2, 0), 0);
        assert_eq!(max_frame_samples(48_000, 0, 2, 4096), 0);
        assert_eq!(max_frame_samples(48_000, 2, 0, 4096), 0);
    }

    #[test]
    fn max_frame_samples_respects_fallback_cap() {
        assert_eq!(
            max_frame_samples(48_000, 1, 1, usize::MAX),
            MAX_FRAME_SAMPLES_FALLBACK
        );
    }

    #[test]
    fn noise_fixture_is_deterministic() {
        let a = noise_fixture(100, 2, 1234);
        let b = noise_fixture(100, 2, 1234);
        assert_eq!(a, b);
        assert_ne!(a, noise_fixture(100, 2, 9999));
    }

    #[test]
    fn tsv_rows_render() {
        let row = MatrixRow {
            sample_rate: 48_000,
            bits_per_sample: 16,
            codec: CodecKind::Flac,
            max_samples: 1024,
            measured_bytes: Some(990),
            fits: Some(true),
        };
        let line = row.to_tsv();
        assert!(
            line.starts_with("48000\t16\tFlac\t1024\t990\ttrue"),
            "{line}"
        );
    }
}
