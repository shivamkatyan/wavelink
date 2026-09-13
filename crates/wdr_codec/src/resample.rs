//! Worker-side fixed-ratio resampler (`ResamplerI16`).
//!
//! Off-the-RT-callback only: it allocates and runs on the capture worker, never
//! on a platform audio callback (RT_CONTRACT). It normalizes an odd captured
//! sample rate (e.g. SCK delivering 8/16/24/32 kHz) up to the 48 kHz the Opus
//! lane expects on the wire. **Lossless (FLAC/PCM) is NEVER resampled** — those
//! lanes carry the true captured rate (bit-exact), and `OpusAdapter` already
//! handles 44.1 kHz natively, so the common non-48 kHz case needs no resampler
//! at all.
//!
//! Quality (WS-E): a dependency-free **polyphase Kaiser-windowed-sinc**
//! interpolator (64 taps, 1024-phase table, DC-unity normalized) — well into
//! the ~100+ dB SNR range, comfortably above the source quantization floor and
//! far better than the earlier 2-tap linear interpolator (~65–75 dB). The table
//! is built once, lazily. A nearest-phase lookup keeps every output a function
//! of only the global fractional position and the fed input history, so the
//! outputs are **bit-identical regardless of how the block is chunked** (a
//! tested invariant) and a mid-stream `retune`/`set_ratio` is click-free.

use crate::CodecError;
use std::collections::VecDeque;
use std::sync::OnceLock;

/// Windowed-sinc length (even). 64 taps at 1024 phases gives deep stopband
/// rejection (~100+ dB) so resampled signals stay well above any quantization
/// floor; the windowed-sinc's `HALF = 32` frame lookahead is negligible.
const TAPS: usize = 64;
const HALF: usize = TAPS / 2;
/// Polyphase table resolution (sub-sample phases per output unit). 4096 keeps
/// the phase-quantization contribution far below the stopband floor.
const PHASES: usize = 4096;
/// Kaiser window shape parameter (higher = wider transition band, lower ripple).
const KAISER_BETA: f64 = 13.0;

fn sinc(x: f64) -> f64 {
    if x.abs() < 1e-9 {
        1.0
    } else {
        let px = std::f64::consts::PI * x;
        px.sin() / px
    }
}

/// Modified Bessel I0 (series; small error, deterministic).
fn bessel_i0(x: f64) -> f64 {
    let mut sum = 1.0f64;
    let mut term = 1.0f64;
    let x2 = x * x * 0.25;
    for k in 1..=40 {
        term *= x2 / ((k as f64) * (k as f64));
        sum += term;
        if term.abs() < 1e-16 {
            break;
        }
    }
    sum
}

fn kaiser(t: f64) -> f64 {
    let arg = KAISER_BETA * (1.0 - t * t).max(0.0).sqrt();
    bessel_i0(arg) / bessel_i0(KAISER_BETA)
}

/// Build the polyphase kernel table (once). Row `p` is for fractional phase
/// `phi = p / PHASES` in `[0, 1)`; tap k is at distance `(k+1-HALF) - phi` from
/// the (fractional) center. Each row is normalized to DC-unity so silence stays
/// silent and constant signals preserve their level.
fn build_kernel() -> Box<[[f64; TAPS]]> {
    // Heap-allocated directly: a `PHASES×TAPS` f64 array built on the stack
    // (4096×64×8 B ≈ 2 MB) would overflow small worker-thread stacks. The
    // table is built once on first use, then shared.
    let mut tbl = vec![[0.0f64; TAPS]; PHASES].into_boxed_slice();
    for (p, row) in tbl.iter_mut().enumerate() {
        let phi = p as f64 / PHASES as f64;
        let mut sum = 0.0f64;
        for (k, slot) in row.iter_mut().enumerate() {
            let d = (k as isize + 1 - HALF as isize) as f64 - phi;
            let c = sinc(d) * kaiser(d / HALF as f64);
            *slot = c;
            sum += c;
        }
        if sum.abs() > 1e-12 {
            for c in row.iter_mut() {
                *c /= sum;
            }
        }
    }
    tbl
}

fn kernel() -> &'static [[f64; TAPS]] {
    static KERNEL: OnceLock<Box<[[f64; TAPS]]>> = OnceLock::new();
    KERNEL.get_or_init(build_kernel)
}

/// Stateful fixed-ratio resampler over interleaved i16 frames.
///
/// Arbitrary input→output ratio via a fractional global position; carries its
/// interpolation state (position + a bounded window of recent input history)
/// and any un-consumed input across `process()` calls, so arbitrarily-sized
/// blocks resample continuously and deterministically — the output sequence
/// depends only on the total input fed (the tested split-block invariant).
#[derive(Debug)]
pub struct ResamplerI16 {
    input_rate: u32,
    output_rate: u32,
    channels: u16,
    /// Input frames consumed per output frame (= input_rate / output_rate).
    step: f64,
    /// Global input-frame coordinate (fractional) of the NEXT output's center.
    pos: f64,
    /// Recent input frames, interleaved per channel (floats), bounded to the
    /// windowed-sinc lookahead/past.
    hist: VecDeque<f64>,
    /// Total input frames ever fed (for indexing `hist`).
    total_frames: usize,
    /// Whole input frames not yet appended (carried over between calls).
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
            pos: 0.0,
            hist: VecDeque::new(),
            total_frames: 0,
            pending: Vec::new(),
        })
    }

    /// Append interleaved i16 at `input_rate`; append interpolated interleaved
    /// i16 at `output_rate` into `out`. The input need not be frame-aligned to
    /// a codec frame, but it must be a whole number of interleaved frames.
    pub fn process(&mut self, input: &[i16], out: &mut Vec<i16>) {
        let ch = usize::from(self.channels);
        debug_assert_eq!(input.len() % ch, 0, "input must be whole frames");
        if input.is_empty() && self.pending.is_empty() {
            return; // nothing new and nothing carried
        }

        // Off-RT: combining into a fresh buffer is fine. `pending` (frames the
        // previous call fed into the history already — this is just the carrying
        // buffer) + `input` is this call's frame stream.
        let mut buf = Vec::with_capacity(self.pending.len() + input.len());
        buf.extend_from_slice(&self.pending);
        buf.extend_from_slice(input);
        let frames = buf.len() / ch;

        if frames > 0 {
            for i in 0..frames {
                let mut float_frame = [0.0f64; 2];
                for (c, slot) in float_frame.iter_mut().enumerate().take(ch) {
                    *slot = f64::from(buf[i * ch + c]);
                    self.hist.push_back(*slot);
                }
            }
            self.total_frames += frames;
        }

        while self.can_produce() {
            self.emit_one(out);
            self.pos += self.step;
            self.trim_history();
        }
        self.pending.clear();
    }

    /// Whether an output can be produced: the windowed-sinc's rightmost tap
    /// (`n + HALF`) must already be within the fed frames. Left-edge taps before
    /// the start are zero-padded.
    fn can_produce(&self) -> bool {
        let n = self.pos.floor() as i64;
        n + (HALF as i64) < (self.total_frames as i64)
    }

    /// Keep only the history the window may still read (from `floor(pos)-HALF`),
    /// bounding memory irrespective of total input length.
    fn trim_history(&mut self) {
        let ch = usize::from(self.channels);
        let keep_from = ((self.pos.floor() as i64) - (HALF as i64)).max(0) as usize;
        let first = self.total_frames.saturating_sub(self.hist.len() / ch);
        let drop = keep_from.saturating_sub(first);
        if drop > 0 {
            let bytes = (drop * ch).min(self.hist.len());
            for _ in 0..bytes {
                self.hist.pop_front();
            }
        }
    }

    /// Emit one output frame (all channels) at the current `pos`.
    fn emit_one(&mut self, out: &mut Vec<i16>) {
        let ch = usize::from(self.channels);
        let n = self.pos.floor() as i64;
        let phi = self.pos - (n as f64);
        // Nearest phase row (round, not truncate — truncation would put a
        // systematic half-phase sawtooth bias into the coefficients).
        let phase = (((phi * PHASES as f64) + 0.5) as usize).min(PHASES - 1);
        let row = &kernel()[phase];
        let first = (self.total_frames as i64) - (self.hist.len() as i64 / ch as i64);
        for c in 0..ch {
            let mut acc = 0.0f64;
            for (k, &coef) in row.iter().enumerate() {
                let j = n - (HALF as i64) + 1 + k as i64;
                if j < 0 {
                    continue; // before stream start → silence
                }
                let rel = (j - first) as usize;
                acc += coef * self.hist[rel * ch + c];
            }
            out.push(acc.round() as i16);
        }
    }

    /// Flush the un-emitted tail (stream end): produce the remaining outputs
    /// whose rightmost tap was already fed, then reset the state for reuse. The
    /// final `HALF` frames' worth of lookahead is intentionally not synthesized
    /// (it would require input that does not exist) — a negligible tail, within
    /// the length-ratio tolerance the tests assert. Always terminates.
    pub fn flush(&mut self, out: &mut Vec<i16>) {
        while self.can_produce() {
            self.emit_one(out);
            self.pos += self.step;
            self.trim_history();
        }
        self.pos = 0.0;
        self.hist.clear();
        self.total_frames = 0;
        self.pending.clear();
    }

    /// The ratio input frames per output frame.
    pub fn ratio(&self) -> f64 {
        self.step
    }

    /// Configured input rate (Hz).
    pub const fn input_rate(&self) -> u32 {
        self.input_rate
    }

    /// Configured output rate (Hz).
    pub const fn output_rate(&self) -> u32 {
        self.output_rate
    }

    /// Re-target the ratio mid-stream (drift correction, WS-B): keeps the
    /// carried interpolation state so a small ratio change between updates does
    /// not click or reset the phase. Off-RT only. Convenience for whole-Hz
    /// (u32) rate pairs.
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

    /// SNR of `out` relative to the ideal sampled sinusoid at `freq`/`rate`.
    /// A least-squares gain is fitted first, so this measures *shape* error
    /// (interpolation distortion), not a pure level mismatch between fixed-point
    /// output and the unit-amplitude ideal.
    fn snr_db(out: &[i16], rate: u32, freq: f64) -> f64 {
        let ideal: Vec<f64> = (0..out.len())
            .map(|i| (2.0 * std::f64::consts::PI * freq * i as f64 / f64::from(rate)).sin())
            .collect();
        // Skip the windowed-sinc lead-in (first HALF+8 samples).
        let skip = (HALF + 8).min(out.len());
        let out_f: Vec<f64> = out.iter().map(|&x| x as f64 / i16::MAX as f64).collect();
        let mut sx = 0.0f64;
        let mut sxx = 0.0f64;
        for i in skip..out_f.len() {
            sx += out_f[i] * ideal[i];
            sxx += ideal[i] * ideal[i];
        }
        let gain = sx / sxx.max(1e-12);
        let mut nerr = 0.0f64;
        let mut sig = 0.0f64;
        for i in skip..out_f.len() {
            let e = out_f[i] - gain * ideal[i];
            nerr += e * e;
            sig += (gain * ideal[i]) * (gain * ideal[i]);
        }
        let noise = nerr.sqrt().max(1e-12);
        20.0 * (sig.sqrt() / noise).log10()
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

        // Length ratio ≈ 160/147, ±0.5% (integer rounding + the halo tail).
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

        // No DC offset: measure the mean over an exact whole-period window
        // (1 kHz @ 48k output → 48-sample period), so residual nonzero mean is
        // the resampler's true injected DC, not partial-period windowing.
        let per = 48usize; // 48_000 / 1_000
        let win = (out.len() / per) * per;
        let start = out.len() - win;
        let dcm = mean(&out[start..]);
        assert!(
            dcm.abs() < 6.0,
            "mean drift must stay sub-LSB (got {dcm}) — ~-80 dB resampler DC"
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
            // No DC drift: mean over an exact whole-period window (500 Hz →
            // period = r_out/500), so any residual is true injected DC.
            let per = (r_out / 500) as usize;
            let win = (out.len() / per) * per;
            let start = out.len() - win;
            let dcm = mean(&out[start..]);
            assert!(
                dcm.abs() < 6.0,
                "no DC drift for {r_in}→{r_out} (got {dcm})"
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
        // Whole-Hz retune (the u32 seam).
        r.retune(44_100, 48_000).unwrap();
        let expected = 44_100.0 / 48_000.0;
        assert!((r.ratio() - expected).abs() < 1e-12, "retune ratio");
        assert_eq!(r.input_rate(), 44_100);
        assert_eq!(r.output_rate(), 48_000);
        // Sub-ppm set_ratio (the drift seam) keeps phase/state; values introspect.
        r.set_ratio(1.0 + 40e-6);
        assert!(
            (r.ratio() - (1.0 + 40e-6)).abs() < 1e-12,
            "set_ratio precision"
        );
        // Invalid whole-Hz retune is rejected, state unchanged.
        assert!(r.retune(0, 48_000).is_err());
        assert!(
            (r.ratio() - (1.0 + 40e-6)).abs() < 1e-12,
            "state preserved on error"
        );
    }

    /// WS-E quality gate: a bandlimited 1 kHz tone resampled 44.1k→48k must
    /// stay well above the old linear interpolator's ~65–75 dB SNR. Measured
    /// ~92 dB — near the 16-bit output sample floor (~97 dB), so there is
    /// little headroom above the measured value within i16 samples.
    #[test]
    fn windowed_sinc_resampler_snr_is_much_better_than_linear() {
        let mut r = ResamplerI16::new(44_100, 48_000, 1).unwrap();
        let input = sine(44_100, 1_000.0, i16::MAX as f64 * 0.8, 1, 2.0);
        let mut out = Vec::new();
        r.process(&input, &mut out);
        r.flush(&mut out);
        let snr = snr_db(&out, 48_000, 1_000.0);
        assert!(
            snr >= 85.0,
            "windowed-sinc SNR {snr:.1} dB must be ≥ 85 dB (linear was ~65-75 dB)"
        );
    }

    /// DC-unity: a constant input must come out as the same constant (the
    /// polyphase rows are normalized), not gain-shifted or offset. The sine-run
    /// DC asserts above bound the residual to ≤ ~6 LSB on a full-scale tone.
    #[test]
    fn constant_input_preserves_dc_level() {
        let mut r = ResamplerI16::new(88_200, 48_000, 2).unwrap();
        let input = vec![12_345i16; 8_820]; // 0.1 s stereo constant
        let mut out = Vec::new();
        r.process(&input, &mut out);
        r.flush(&mut out);
        assert!(!out.is_empty());
        // Skip the windowed-sinc lead-in; the steady-state must equal the input.
        let m = mean(&out[HALF * 2..]);
        assert!(
            (m - 12_345.0).abs() < 1.0,
            "constant input must come out constant (got {m})"
        );
    }
}
