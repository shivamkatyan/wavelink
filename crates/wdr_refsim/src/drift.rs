//! Receiver-driven WLS clock-drift estimator + bounded resample (ADR-007;
//! FR-024, closes RISK_REGISTER R11).
//!
//! A lossless stream is delivered at the emitter's *nominal* rate, but the
//! receiver's render clock can drift relative to it — without correction the
//! render would build up a persistent lag (underrun/overrun at the boundary),
//! i.e. a silent clock-drift oscillation. This module estimates the *apparent*
//! source rate from the relationship between the emitter's media sample counter
//! ([`Frame::media_ts`]) and the receiver's arrival wall clock
//! ([`crate::receiver::ClockHandle`] — injectable in tests), then applies a
//! **bounded** resample so the render stays aligned without ever auto-switching
//! codecs.
//!
//! Dynamics (ADR-007): WLS fit over sample-counter-vs-arrival deltas with
//! MAD outlier rejection; a deadband suppresses micro-corrections; a per-update
//! slew bound (≤ ~1 ppm / Hz) prevents oscillation; correction engages only
//! after a prefetch-seeded warm-up; an estimate beyond `max_est_ppm` is
//! surfaced as [`DriftSignal::ExcessiveDrift`] (the session layer's hook to
//! renegotiate-with-confirm — never a silent auto-change).
//!
//! Honesty contract (FR-024): a drift-corrected stream is **never bit-perfect**.
//! The estimator reports `corrected()` and the receiver surfaces
//! [`crate::receiver::DriftReport::bit_exact`] so "resampled/converted" is never
//! conflated with "bit-exact" (ADR-007: resampled path never labeled bit-perfect).

use wdr_codec::resample::ResamplerI16;

/// Tuning for the drift estimator (ADR-007 "PENDING spike before final
/// constants" — these are the spike's nominal defaults, host-measurable).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DriftConfig {
    /// Nominal (render) sample rate the correction targets, Hz.
    pub nominal_rate: u32,
    /// Recompute the estimate this often on the arrival clock, ms (~1 Hz).
    pub update_interval_ms: u64,
    /// Max per-update change to the applied correction, ppm (slew bound).
    pub max_ppm_step: f64,
    /// Ignore |drift| below this, ppm (deadband).
    pub deadband_ppm: f64,
    /// Minimum frame observations before the first correction (warm-up).
    pub prefill_frames: usize,
    /// |drift| above this → [`DriftSignal::ExcessiveDrift`] (ppm).
    pub max_est_ppm: f64,
    /// Max (media_ts, arrival_ms) observations retained for the WLS fit
    /// (bounded memory: a sliding window, never unbounded).
    pub window: usize,
}

impl Default for DriftConfig {
    fn default() -> Self {
        Self {
            nominal_rate: 48_000,
            update_interval_ms: 1_000,
            max_ppm_step: 0.75,
            deadband_ppm: 0.5,
            prefill_frames: 96,
            max_est_ppm: 50.0,
            window: 4096,
        }
    }
}

/// What the estimator is doing / has decided.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DriftSignal {
    /// Clock aligned / correction inside the deadband — nothing applied.
    Idle,
    /// A bounded correction is engaged at `ppm` (nonzero, slew-limited).
    Correcting { ppm: f64 },
    /// The apparent drift exceeded `max_est_ppm` — correction is frozen and the
    /// caller must act (renegotiate-with-confirm per ADR-007); `reset()` to
    /// resume. Never a silent auto-change.
    ExcessiveDrift { ppm: f64 },
}

/// WLS drift estimator + bounded resampler over one lossless i16 lane.
///
/// The estimator is clock-agnostic in its API: the receiver feeds each frame's
/// `(media_ts, arrival_now_ms)` pair via [`Self::sample_frame`], so the arrival
/// clock is whichever the receiver owns (injectable in tests, wall-ms in
/// production). It owns only the bounded-resample core.
pub struct DriftEstimator {
    cfg: DriftConfig,
    resampler: ResamplerI16,
    /// Sliding window of (media_ts, arrival_ms) observations.
    xs: Vec<f64>,
    ys: Vec<f64>,
    total_frames: usize,
    last_update_ms: Option<u64>,
    /// Last applied correction, ppm (0 = identity).
    applied_ppm: f64,
    signal: DriftSignal,
    corrected: bool,
}

impl DriftEstimator {
    /// New estimator targeting `cfg.nominal_rate`, i16 lane, `channels`
    /// interleaved. Arrival clock is supplied per-sample by the receiver.
    pub fn new(cfg: DriftConfig, channels: u16) -> Self {
        let resampler = ResamplerI16::new(cfg.nominal_rate, cfg.nominal_rate, channels)
            .expect("drift resampler: nonzero nominal rate + channels required by config");
        Self {
            cfg,
            resampler,
            xs: Vec::new(),
            ys: Vec::new(),
            total_frames: 0,
            last_update_ms: None,
            applied_ppm: 0.0,
            signal: DriftSignal::Idle,
            corrected: false,
        }
    }

    /// Record one (emitter media `frame_ts`, arrival `now_ms`) pair and, when
    /// the update cadence + warm-up are met, recompute the correction.
    pub fn sample_frame(&mut self, frame_ts: u64, now_ms: u64) {
        self.push(frame_ts as f64, now_ms as f64);
        self.total_frames += 1;
        let due = match self.last_update_ms {
            None => true,
            Some(t) => now_ms.saturating_sub(t) >= self.cfg.update_interval_ms,
        };
        if due && self.total_frames >= self.cfg.prefill_frames {
            self.last_update_ms = Some(now_ms);
            self.run_update();
        }
    }

    /// Feed one decoded i16 block through the (possibly corrected) resampler.
    /// Output lands in `out` (i16 interleaved, ~nominal cadence).
    pub fn process_frame(&mut self, decoded: &[i16], out: &mut Vec<i16>) {
        self.resampler.process(decoded, out);
    }

    /// Whether a finite correction has ever been applied (→ render is
    /// resampled/converted, hence never bit-exact).
    pub fn corrected(&self) -> bool {
        self.corrected
    }

    /// The currently applied correction, ppm (0 = identity).
    pub fn applied_ppm(&self) -> f64 {
        self.applied_ppm
    }

    /// The estimator's current signal.
    pub fn signal(&self) -> &DriftSignal {
        &self.signal
    }

    /// Clear history + correction (e.g. after an explicit session renegotiation
    /// in response to [`DriftSignal::ExcessiveDrift`]).
    pub fn reset(&mut self) {
        self.xs.clear();
        self.ys.clear();
        self.total_frames = 0;
        self.applied_ppm = 0.0;
        self.corrected = false;
        self.signal = DriftSignal::Idle;
        self.resampler.set_ratio(1.0);
    }

    fn push(&mut self, x: f64, y: f64) {
        if self.xs.len() >= self.cfg.window {
            self.xs.remove(0);
            self.ys.remove(0);
        }
        self.xs.push(x);
        self.ys.push(y);
    }

    /// One WLS fit over the window; MAD-gated outlier drop + refit.
    fn fit_slope(&self) -> Option<f64> {
        let n = self.xs.len();
        if n < 2 {
            return None;
        }
        let (mut sx, mut sy, mut sxx, mut sxy) = (0.0f64, 0.0f64, 0.0f64, 0.0f64);
        for i in 0..n {
            let (x, y) = (self.xs[i], self.ys[i]);
            sx += x;
            sy += y;
            sxx += x * x;
            sxy += x * y;
        }
        let denom = (n as f64) * sxx - sx * sx;
        if denom.abs() < 1e-9 {
            return None;
        }
        let b = ((n as f64) * sxy - sx * sy) / denom;
        // MAD outlier rejection on residuals; refit once on the survivors.
        let a = (sy - b * sx) / (n as f64);
        let mut resid: Vec<f64> = (0..n).map(|i| self.ys[i] - (a + b * self.xs[i])).collect();
        let med = median(&mut resid);
        let mut abs_dev: Vec<f64> = resid.iter().map(|r| (r - med).abs()).collect();
        let mad = median(&mut abs_dev);
        let mad = if mad <= 0.0 { 1e-9 } else { mad };
        let mut xs2 = Vec::new();
        let mut ys2 = Vec::new();
        for ((&x, &y), &r) in self.xs.iter().zip(self.ys.iter()).zip(resid.iter()) {
            if (r - med).abs() <= 4.0 * mad {
                xs2.push(x);
                ys2.push(y);
            }
        }
        let m = xs2.len();
        if m < 2 {
            return None;
        }
        let (mut sx2, mut sy2, mut sxx2, mut sxy2) = (0.0f64, 0.0f64, 0.0f64, 0.0f64);
        for (&x, &y) in xs2.iter().zip(ys2.iter()) {
            sx2 += x;
            sy2 += y;
            sxx2 += x * x;
            sxy2 += x * y;
        }
        let denom2 = (m as f64) * sxx2 - sx2 * sx2;
        if denom2.abs() < 1e-9 {
            return None;
        }
        Some(((m as f64) * sxy2 - sx2 * sy2) / denom2)
    }

    fn run_update(&mut self) {
        let Some(b) = self.fit_slope() else {
            return;
        };
        // b = ms per *sample*; effective source rate = 1000/b Hz.
        let effective_rate = 1000.0 / b;
        let raw_ppm = (f64::from(self.cfg.nominal_rate) / effective_rate - 1.0) * 1e6;

        if raw_ppm.abs() > self.cfg.max_est_ppm {
            self.signal = DriftSignal::ExcessiveDrift { ppm: raw_ppm };
            return; // frozen; never a silent auto-change (caller renegotiates).
        }

        // Slew-limit the *applied* correction toward the raw estimate.
        let step = self.cfg.max_ppm_step;
        let target = raw_ppm
            .max(self.applied_ppm - step)
            .min(self.applied_ppm + step);

        if target.abs() < self.cfg.deadband_ppm {
            // Inside the deadband: if we never corrected, stay bit-exact and
            // idle; if we had corrected, hold the last applied value (no
            // oscillation), still reporting Idle.
            self.signal = DriftSignal::Idle;
            if !self.corrected {
                self.applied_ppm = 0.0;
                self.resampler.set_ratio(1.0);
            }
            return;
        }

        // Apply the bounded correction at sub-ppm precision. Resampler step =
        // input frames per output frame = effective/nominal = 1/(1+ppm·1e-6):
        // a slow apparent source (ppm > 0) resamples *up* to keep the render at
        // nominal wall cadence.
        self.applied_ppm = target;
        self.signal = DriftSignal::Correcting { ppm: target };
        self.corrected = true;
        self.resampler.set_ratio(1.0 / (1.0 + target * 1e-6));
    }
}

/// Median of an even/odd-length slice of f64s (mutating copy).
fn median(v: &mut [f64]) -> f64 {
    if v.is_empty() {
        return 0.0;
    }
    let n = v.len();
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    if n % 2 == 1 {
        v[n / 2]
    } else {
        0.5 * (v[n / 2 - 1] + v[n / 2])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Deterministic arrival pairs at an EXACT 10 ms cadence (frame_samples 480
    /// @48k) so per-frame wall times are exact integers — a u64-ms clock cannot
    /// wreck the fit with rounding. `skew_ppm > 0` = the source appears *slow*
    /// (each wall arrival stretched by 1 + skew·1e-6).
    fn arrivals(n: usize, skew_ppm: f64) -> Vec<(u64, u64)> {
        const FRAME_SAMPLES: u64 = 480; // 480 @48k = exactly 10 ms/frame
        (0..n)
            .map(|i| {
                let ts = (i as u64) * FRAME_SAMPLES;
                let wall = (10_000.0 + i as f64 * 10.0 * (1.0 + skew_ppm * 1e-6)) as u64;
                (ts, wall)
            })
            .collect()
    }

    /// Fast, deterministic test cadence: an update every 10 frames (100 ms).
    fn test_config(mut cfg: DriftConfig) -> DriftConfig {
        cfg.update_interval_ms = 100;
        cfg.prefill_frames = 20;
        cfg.window = 4096;
        cfg
    }

    fn drive(cfg: DriftConfig, pairs: Vec<(u64, u64)>) -> DriftEstimator {
        let mut e = DriftEstimator::new(cfg, 2);
        for (ts, now) in pairs {
            e.sample_frame(ts, now);
        }
        e
    }

    #[test]
    fn matched_clocks_idle_and_stay_bit_exact() {
        let e = drive(test_config(DriftConfig::default()), arrivals(400, 0.0));
        assert_eq!(e.signal().clone(), DriftSignal::Idle);
        assert!(!e.corrected(), "matched clocks must stay bit-exact");
        assert_eq!(e.applied_ppm(), 0.0);
    }

    #[test]
    fn jump_converges_toward_injected_drift() {
        let mut cfg = test_config(DriftConfig::default());
        cfg.max_ppm_step = 10.0; // fast slew for a finite test
        cfg.max_est_ppm = 2_000.0; // 300 ppm injected, well within range
        let injected = 300.0;
        let e = drive(cfg, arrivals(8_000, injected));
        assert!(e.corrected(), "300 ppm drift must engage a correction");
        match e.signal() {
            DriftSignal::Correcting { ppm } => assert!(
                (ppm - injected).abs() < injected * 0.5,
                "converged toward injected {injected} ppm (got {ppm})"
            ),
            other => panic!("expected Correcting, got {other:?}"),
        }
        assert!(e.applied_ppm() > 0.0, "slow source → positive ppm");
    }

    #[test]
    fn inside_deadband_stays_idle() {
        let mut cfg = test_config(DriftConfig::default());
        cfg.deadband_ppm = 2.0;
        // 0.3 ppm over 4 s of audio is sub-ms — a 1 ms clock cannot even
        // observe it, so it must stay inside the deadband and idle.
        let e = drive(cfg, arrivals(400, 0.3));
        assert_eq!(e.signal().clone(), DriftSignal::Idle);
        assert!(!e.corrected());
    }

    #[test]
    fn excessive_drift_never_auto_corrects() {
        let cfg = test_config(DriftConfig::default()); // max_est_ppm = 50
        let e = drive(cfg, arrivals(2_000, 250.0)); // 250 ppm ≫ 50
        assert!(
            matches!(e.signal(), DriftSignal::ExcessiveDrift { .. }),
            "out-of-range apparent drift must surface ExcessiveDrift"
        );
        assert!(!e.corrected(), "never a silent auto-change");
    }

    #[test]
    fn slew_limits_the_rate_of_correction() {
        let mut cfg = test_config(DriftConfig::default());
        cfg.max_ppm_step = 2.0;
        cfg.max_est_ppm = 20_000.0;
        let mut e = DriftEstimator::new(cfg, 2);
        // Dense staircase: 5000 ppm over 2000 frames makes an arrival bump
        // every ~20 frames, so the WLS sees a genuine slope (not outliers).
        let mut saw_correction = false;
        let mut last = 0.0f64;
        let mut max_jump = 0.0f64;
        for (i, (ts, now)) in arrivals(2_000, 5_000.0).iter().enumerate() {
            e.sample_frame(*ts, *now);
            let cur = e.applied_ppm();
            if e.corrected() {
                saw_correction = true;
            }
            if i > 0 && cur > last + 1e-9 {
                max_jump = max_jump.max(cur - last);
            }
            last = cur;
        }
        assert!(
            saw_correction,
            "a real source-drift slope must engage correction"
        );
        assert!(e.applied_ppm() > 0.0, "slow source → positive ppm");
        assert!(
            max_jump <= 2.0 + 1e-9,
            "applied correction must never jump more than max_ppm_step per \
             update (max single jump {max_jump} ppm)"
        );
    }

    #[test]
    fn ramp_drift_is_tracked_within_bounds() {
        let mut cfg = test_config(DriftConfig::default());
        cfg.max_ppm_step = 5.0;
        cfg.max_est_ppm = 2_000.0;
        // Linear ramp 0 → +100 ppm over 4000 frames.
        let e = drive(
            cfg,
            (0..4000)
                .map(|i| {
                    let skew = 100.0 * i as f64 / 4000.0; // ppm at frame i
                    let ts = (i as u64) * 480;
                    let wall = (10_000.0 + i as f64 * 10.0 * (1.0 + skew * 1e-6)) as u64;
                    (ts, wall)
                })
                .collect(),
        );
        assert!(
            e.applied_ppm() > 0.0 && e.applied_ppm() <= 120.0,
            "ramp tracked within bounds (got {} ppm)",
            e.applied_ppm()
        );
    }

    #[test]
    fn reset_recovers_from_correction() {
        let mut cfg = test_config(DriftConfig::default());
        cfg.max_ppm_step = 5.0;
        cfg.max_est_ppm = 2_000.0;
        let mut e = drive(cfg, arrivals(2_000, 300.0));
        assert!(e.corrected());
        e.reset();
        assert!(!e.corrected());
        assert_eq!(e.applied_ppm(), 0.0);
        assert_eq!(e.signal().clone(), DriftSignal::Idle);
    }
}
