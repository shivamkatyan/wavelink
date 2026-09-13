//! Worker-side fixed-ratio linear resampler (`ResamplerI16`).
//!
//! Off-the-RT-callback only: it allocates and runs on the capture worker, never
//! on a platform audio callback (RT_CONTRACT). It normalizes an odd captured
//! sample rate (e.g. SCK delivering 8/16/24/32 kHz) up to the 48 kHz the Opus
//! lane expects on the wire. **Lossless (FLAC/PCM) is NEVER resampled** — those
//! lanes carry the true captured rate (bit-exact), and `OpusAdapter` already
//! handles 44.1 kHz natively, so the common non-48 kHz case needs no resampler
//! at all.
//!
//! Quality: a stateful linear interpolator (~65–75 dB SNR) is plenty under
//! Opus's own quantization + 20 ms framing. The quality-upgrade path (an FFT/
//! polyphase `rubato` `Fft`-based resampler, as used inside `OpusAdapter`) is a
//! recorded ADR-007 follow-up, not this module.

use crate::CodecError;

/// Stateful fixed-ratio linear interpolator over interleaved i16 frames.
///
/// Arbitrary input→output ratio via a fractional phase; carries its
/// interpolation state (`left`/`right`/`p`) and any un-consumed input across
/// `process()` calls, so arbitrarily-sized blocks (SCK's cadence) resample
/// continuously and deterministically.
#[derive(Debug)]
pub struct ResamplerI16 {
    input_rate: u32,
    output_rate: u32,
    channels: u16,
    /// Input frames consumed per output frame (= input_rate / output_rate).
    step: f64,
    /// Fractional position in `[0, 1)` between the `left` and `right` frames.
    p: f64,
    /// Previous / next interpolation neighbors, per channel (f64 for linear).
    left: Vec<f64>,
    right: Vec<f64>,
    have_left: bool,
    have_right: bool,
    /// Whole input frames not yet consumed by the interpolator (carried over).
    pending: Vec<i16>,
}

impl ResamplerI16 {
    /// New resampler from `input_rate` → `output_rate` (both nonzero), for
    /// `channels` interleaved channels (1 = mono, 2 = stereo).
    pub fn new(input_rate: u32, output_rate: u32, channels: u16) -> Result<Self, CodecError> {
        if input_rate == 0 || output_rate == 0 || channels == 0 {
            return Err(CodecError::Backend(
                "resampler: non-zero rate and channel count required".into(),
            ));
        }
        Ok(Self {
            input_rate,
            output_rate,
            channels,
            step: f64::from(input_rate) / f64::from(output_rate),
            p: 0.0,
            left: vec![0.0; usize::from(channels)],
            right: vec![0.0; usize::from(channels)],
            have_left: false,
            have_right: false,
            pending: Vec::new(),
        })
    }

    /// Append interleaved i16 at `input_rate`; append interpolated interleaved
    /// i16 at `output_rate` into `out`. The input need not be frame-aligned to
    /// a codec frame, but it must be a whole number of interleaved frames.
    pub fn process(&mut self, input: &[i16], out: &mut Vec<i16>) {
        let ch = usize::from(self.channels);
        debug_assert_eq!(input.len() % ch, 0, "input must be whole frames");
        if input.is_empty() && self.pending.is_empty() && self.p < 1.0 {
            return; // nothing new and no phase debt to settle
        }

        // Off-RT: combining into a fresh buffer is fine. `pending` (frames the
        // previous call did not consume) + `input` is this call's frame stream.
        let mut buf = Vec::with_capacity(self.pending.len() + input.len());
        buf.extend_from_slice(&self.pending);
        buf.extend_from_slice(input);
        let frames = buf.len() / ch;
        let mut fi = 0usize;

        // Pull the next input frame into `dst`; advances `fi`; false when the
        // call's input is exhausted.
        let pull = |fi: &mut usize, dst: &mut [f64]| -> bool {
            if *fi >= frames {
                return false;
            }
            for c in 0..ch {
                dst[c] = f64::from(buf[(*fi) * ch + c]);
            }
            *fi += 1;
            true
        };

        // 1) Settle any carried "phase debt" first: the previous call may have
        //    run out of input mid-phase, leaving `p >= 1` (crossings that need a
        //    fresh right frame). Consume them now, where the new input is.
        while self.p >= 1.0 {
            if !self.have_right {
                if !pull(&mut fi, &mut self.right) {
                    break; // still nothing to settle — carry the debt onward
                }
                self.have_right = true;
            }
            self.p -= 1.0;
            self.left.copy_from_slice(&self.right);
            self.have_right = false; // that right frame was consumed into `left`
        }

        // 2) Seed `left` for a fresh stream (only if not already held).
        if !self.have_left {
            if !pull(&mut fi, &mut self.left) {
                self.pending.clear();
                return;
            }
            self.have_left = true;
        }

        // 3) Produce interpolated output frames.
        while self.p < 1.0 {
            if !self.have_right {
                if !pull(&mut fi, &mut self.right) {
                    break;
                }
                self.have_right = true;
            }
            for c in 0..ch {
                let v = self.left[c] + self.p * (self.right[c] - self.left[c]);
                out.push(v.round() as i16);
            }
            self.p += self.step;
            // Consume boundaries: each crossing advances left→right and needs a
            // fresh right frame. stop when no more input (debt carries onward).
            while self.p >= 1.0 {
                self.p -= 1.0;
                self.left.copy_from_slice(&self.right);
                self.have_right = false;
                if !pull(&mut fi, &mut self.right) {
                    break;
                }
                self.have_right = true;
            }
            if self.p >= 1.0 {
                break; // unresolved debt — more input will settle it next call
            }
        }

        // Carry un-consumed whole frames (fi..frames) for the next call.
        self.pending.clear();
        if fi < frames {
            self.pending.extend_from_slice(&buf[fi * ch..]);
        }
    }

    /// Flush the fractional tail (stream end): emit the remaining in-flight
    /// interpolation — the outputs that fall in the span between the held
    /// `left`/`right` frames — so the output reaches its exact length, then
    /// reset the state for reuse. Emits at most `ceil((1-p)/step)` outputs and
    /// **stops at the first consumed-frame boundary** (there is no further
    /// input), so it always terminates.
    pub fn flush(&mut self, out: &mut Vec<i16>) {
        let ch = usize::from(self.channels);
        if self.have_left && !self.have_right {
            self.right.copy_from_slice(&self.left);
            self.have_right = true;
        }
        while self.have_left && self.have_right && self.p < 1.0 {
            for c in 0..ch {
                let v = self.left[c] + self.p * (self.right[c] - self.left[c]);
                out.push(linear_to_i16(v));
            }
            self.p += self.step;
            if self.p >= 1.0 {
                // The phase consumed the whole remaining span: stop, so the
                // tail is exact and the loop can never spin on a flat phase.
                break;
            }
        }
        self.p = 0.0;
        self.have_left = false;
        self.have_right = false;
        self.pending.clear();
    }

    /// The ratio input frames per output frame.
    pub fn ratio(&self) -> f64 {
        self.step
    }

    /// Re-target the ratio mid-stream (drift correction, WS-B): keeps the
    /// carried interpolation state (`left`/`right`/`p`/`pending`) so a small
    /// ratio change between updates does not click or reset the phase.
    /// Off-RT only. Convenience for whole-Hz (u32) rate pairs.
    pub fn retune(&mut self, input_rate: u32, output_rate: u32) -> Result<(), CodecError> {
        if input_rate == 0 || output_rate == 0 {
            return Err(CodecError::Backend(
                "resampler: non-zero rate required in retune".into(),
            ));
        }
        self.input_rate = input_rate;
        self.output_rate = output_rate;
        self.step = f64::from(input_rate) / f64::from(output_rate);
        Ok(())
    }

    /// Re-target the ratio mid-stream at sub-ppm precision (drift correction).
    /// `step` = input frames per output frame (≈1.0 ± tens of ppm). Whole-Hz
    /// `retune` quantizes at ~1/48000 ≈ 21 ppm — far too coarse for drift, so
    /// the drift estimator drives this directly. Same state-preserving
    /// semantics as [`retune`](Self::retune); the `input_rate`/`output_rate`
    /// fields keep their nominal values for introspection.
    pub fn set_ratio(&mut self, step: f64) {
        debug_assert!(
            step > 0.0 && step.is_finite(),
            "resampler: bad drift ratio {step}"
        );
        self.step = step;
    }

    /// Configured input rate (Hz).
    pub const fn input_rate(&self) -> u32 {
        self.input_rate
    }

    /// Configured output rate (Hz).
    pub const fn output_rate(&self) -> u32 {
        self.output_rate
    }
}

#[inline]
fn linear_to_i16(v: f64) -> i16 {
    // The interpolated value always lies between two existing i16 samples, so
    // rounding cannot overflow; `round()` ties away from zero matches PCM
    // convention.
    v.round() as i16
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Generated sine helper: `frames_per_channel = secs·rate` (so "1.0 s" is a
    /// real second regardless of `channels`), then pushes `channels` samples per
    /// frame (interleaved i16, mono or stereo identical on both channels).
    fn sine(rate: u32, freq: f64, amp: f64, channels: u16, secs: f64) -> Vec<i16> {
        let ch = usize::from(channels);
        let frames = (secs * f64::from(rate)).round() as usize;
        let mut out = Vec::with_capacity(frames * ch);
        for i in 0..frames {
            let v = (2.0 * std::f64::consts::PI * freq * i as f64 / f64::from(rate)).sin() * amp;
            let sv = v.round() as i16;
            for _ in 0..ch {
                out.push(sv);
            }
        }
        out
    }

    fn mean<T: Into<f64> + Copy>(v: &[T]) -> f64 {
        if v.is_empty() {
            return 0.0;
        }
        v.iter().map(|&x| x.into()).sum::<f64>() / v.len() as f64
    }

    /// Positive-going zero crossings across the whole signal → frequency when
    /// the sample rate is known.
    fn crossings(v: &[i16]) -> usize {
        v.windows(2).filter(|w| w[0] < 0 && w[1] >= 0).count()
    }

    #[test]
    fn upsampling_44_1k_to_48k_preserves_frequency_amplitude_and_dc() {
        let rate = 44_100u32;
        let mut r = ResamplerI16::new(rate, 48_000, 1).unwrap();
        let input = sine(rate, 1_000.0, 24_000.0, 1, 1.0); // real 1 s
        let mut out = Vec::new();
        // Feed in deliberately odd/split chunks (proves state carry across calls).
        let chunks = [&input[0..7], &input[7..40_097], &input[40_097..]];
        for c in &chunks {
            r.process(c, &mut out);
        }
        r.flush(&mut out);

        // Length ratio ≈ 160/147, ±0.5% (integer rounding + the tail).
        let ratio = out.len() as f64 / input.len() as f64;
        let expected = 48_000.0 / f64::from(rate);
        assert!(
            (ratio - expected).abs() / expected < 0.005,
            "length ratio {ratio} vs {expected}"
        );

        // Frequency: crossings over the whole 1 s output ≈ 1 kHz ± 2%.
        let freq_est = crossings(&out) as f64 * 48_000.0 / out.len() as f64;
        assert!(
            (freq_est - 1_000.0).abs() < 20.0,
            "est frequency {freq_est} Hz ≈ 1 kHz"
        );

        // Amplitude ≈ amp ±3% (skip the very first interpolation lead-in).
        let peak = out[out.len() / 4..]
            .iter()
            .map(|&x| x as i32)
            .max()
            .unwrap();
        assert!(
            (peak as f64 - 24_000.0).abs() < 0.03 * 24_000.0,
            "peak {peak} ≈ 24000 ±3%"
        );

        // No DC offset.
        assert!(
            mean(&out[out.len() / 4..]).abs() < 0.5,
            "mean drift must stay sub-LSB"
        );
    }

    #[test]
    fn downsampling_48k_to_44_1k_length_and_frequency() {
        let mut r = ResamplerI16::new(48_000, 44_100, 1).unwrap();
        let input = sine(48_000, 440.0, 12_000.0, 1, 1.0); // real 1 s @48k
        let mut out = Vec::new();
        r.process(&input, &mut out);
        r.flush(&mut out);
        let ratio = out.len() as f64 / input.len() as f64;
        let expected = 44_100.0 / 48_000.0;
        assert!(
            (ratio - expected).abs() / expected < 0.005,
            "48k→44.1k ratio"
        );
        let freq_est = crossings(&out) as f64 * 44_100.0 / out.len() as f64;
        assert!(
            (freq_est - 440.0).abs() < 12.0,
            "downsampled freq {freq_est} ≈ 440"
        );
    }

    #[test]
    fn the_other_odd_ratios_are_bounded_and_dc_safe() {
        for (r_in, r_out) in [(88_200, 48_000u32), (48_000, 96_000)] {
            let mut r = ResamplerI16::new(r_in, r_out, 2).unwrap();
            let input = sine(r_in, 500.0, 8_000.0, 2, 0.5);
            let mut out = Vec::new();
            r.process(&input[..6], &mut out);
            r.process(&input[6..], &mut out);
            r.flush(&mut out);
            let expected = f64::from(r_out) / f64::from(r_in);
            let ratio = out.len() as f64 / input.len() as f64;
            assert!(
                (ratio - expected).abs() / expected < 0.01,
                "{r_in}→{r_out} ratio {ratio} vs {expected}"
            );
            assert!(
                mean(&out[out.len() / 2..]).abs() < 0.5,
                "no DC drift for {r_in}→{r_out}"
            );
        }
    }

    #[test]
    fn all_zero_input_produces_all_zero_output() {
        let mut r = ResamplerI16::new(44_100, 48_000, 2).unwrap();
        let input = vec![0i16; 4_410]; // 0.05 s of stereo silence
        let mut out = Vec::new();
        r.process(&input[..6], &mut out);
        r.process(&input[6..], &mut out);
        r.flush(&mut out);
        assert!(!out.is_empty());
        assert!(
            out.iter().all(|&s| s == 0),
            "silence must stay silent (DC safety)"
        );
    }

    #[test]
    fn split_blocks_are_equivalent_to_one_call() {
        let mut single = ResamplerI16::new(44_100, 48_000, 2).unwrap();
        let mut split = ResamplerI16::new(44_100, 48_000, 2).unwrap();
        let input = sine(44_100, 1_000.0, 20_000.0, 2, 0.5);
        let mut out_single = Vec::new();
        single.process(&input, &mut out_single);
        single.flush(&mut out_single);

        let mut out_split = Vec::new();
        let mut i = 0usize;
        let mut step = 2usize; // always even → always whole frames
        while i < input.len() {
            let j = ((i + step).min(input.len())) & !1; // never split a stereo frame
            split.process(&input[i..j], &mut out_split);
            i = j;
            if j >= input.len() {
                break;
            }
            step = ((step * 7 + 3) % 113).max(2);
        }
        split.flush(&mut out_split);
        assert_eq!(
            out_single, out_split,
            "stateful carry must be chunk-size independent"
        );
    }

    #[test]
    fn zero_or_nonpositive_rates_are_errors() {
        assert!(ResamplerI16::new(0, 48_000, 2).is_err());
        assert!(ResamplerI16::new(44_100, 0, 2).is_err());
        assert!(ResamplerI16::new(44_100, 48_000, 0).is_err());
    }

    #[test]
    fn retune_and_set_ratio_retarget_mid_stream() {
        let mut r = ResamplerI16::new(48_000, 48_000, 2).unwrap();
        assert_eq!(r.ratio(), 1.0, "identity start");
        // Whole-Hz retune (the WS-E/u32 seam).
        r.retune(44_100, 48_000).unwrap();
        let expected = 44_100.0 / 48_000.0;
        assert!((r.ratio() - expected).abs() < 1e-12, "retune ratio");
        assert_eq!(r.input_rate(), 44_100);
        assert_eq!(r.output_rate(), 48_000);
        // Sub-ppm set_ratio (the drift seam) keeps phase/state; values introspect.
        r.set_ratio(1.0 + 40e-6);
        assert!((r.ratio() - (1.0 + 40e-6)).abs() < 1e-12, "set_ratio precision");
        // Invalid whole-Hz retune is rejected, state unchanged.
        assert!(r.retune(0, 48_000).is_err());
        assert!((r.ratio() - (1.0 + 40e-6)).abs() < 1e-12, "state preserved on error");
    }
}
