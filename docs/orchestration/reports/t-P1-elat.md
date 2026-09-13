# t-P1-elat — Latency Measurement Specification + Executable Probe

**task_id:** t-P1-elat · **root_task_id:** R-P1-ELAT · **owner_role:** Reliability · **date:** 2026-09-07 · **status:** complete

## Summary

Authored `docs/planning/LATENCY_MEASUREMENT.md` — the governing specification for every latency
number the project reports (PRODUCT_SPEC §8 / PROTOCOL_SPEC §Timing & sequence rules / TEST_PLAN
Integration), plus the contract for the executable probe the B1/P2 harness implements. Appended
risk row R15 (latency acceptance flaky / device-dependent) to `docs/planning/RISK_REGISTER.md`.

### Key content

- **Timestamp points T0–T3** precisely defined: T0 = source-sample/first-sample-of-frame stamp
  (emitter synthetic source stamp; the `Frame.media_ts` wall-clock basis), T1 = send-complete
  (post QUIC handoff), T2 = receiver-decode-ready (before decode, codec-comparable), T3 = render
  presentation (sim: `NullRenderSink` accept; physical: DAC presentation time).
- **Clock method** tri-parted: §3.1 single-clock (in-process/in-sim loopback), §3.2 cross-device
  in-band offset estimation (emitter sample counter + receiver arrival monotonic clock + agreed
  WLS/robust offset calibration — measurement-only, not the streaming estimator), §3.3 shared
  NTP/PTP **measurement-only**, never correctness. Sim containers are cross-device → §3.2.
- **Capture-to-render** in each context + **warm-up exclusions** (prefill + first 100 frames/2 s +
  500 ms recovery transients).
- **Run discipline:** ≥300 s clean measurement per profile, ≥5 runs per cell, ≥1000 clean samples
  per run, per-run spread reported.
- **Percentile method:** linear interpolation between adjacent order statistics (R type 7 /
  `PERCENTILE.INC`) on the sorted clean samples; p50/p95/p99/max, `n` and method tag stated.
- **Uncertainty:** `U = max(frame_period/2, (p99−p50)/2) + cross_clock_residual`; acceptance is
  `p95 + U ≤ budget`.
- **Separation:** simulated vs physical never merged; run-elapsed/transport numbers explicitly
  recategorised (`latency_us` → `run_elapsed_us`) so RTT ≠ e2e is never violated in a report.
- **Device-gated Low-Latency probe (§10):** on-connect timestamped probe frame, ≥3 exchanges,
  one-way p95, per-device e2e budget estimate, qualify iff `budget + margin ≤ 80 ms − 5 ms`,
  per-device capability-hash cache with bounded TTL (no stable device ID — FR-055).
- **Executable probe (§11):** `WDR_LAT_*` env contract, recording hooks (T0/T1/T2/T3 per frame in
  `wdr_refsim`), JSON `wdr.latency.v1` output schema, and the compose/metrics/assert plug-in
  interface for the B1/P2 worker.
- **Budget model (§12):** Balanced ≤150 ms p95 and Low-Latency ≤80 ms p95 head breakdown per
  codec (Opus 20/10 ms, FLAC 5 ms) using t-B0-transport loopback (149 µs/857 µs) + t-B1 e2e
  (FLAC clean ≈71 ms, Opus ≈21 ms, 1% loss ≈90 ms) as starting references; device-dependent
  render/DAC head flagged (Android USB DAC 20–80 ms, R07). Clean-LAN expect: Balanced
  Opus20 97.5 ms / Opus10 87.5 ms / FLAC 81.0 ms (all ≤150); Low-Latency Opus20 82.5 ✗
  (forces 10 ms), Opus10 72.5 ✓, FLAC 66.0 ✓ — all device-gated. **No TBD anywhere in budgets.**

## Files changed

| Path | Change |
|---|---|
| `docs/planning/LATENCY_MEASUREMENT.md` | New — latency measurement specification + executable probe contract (this task's deliverable). |
| `docs/planning/RISK_REGISTER.md` | New row **R15** — latency acceptance flaky / device-dependent; owner Reliability, phase P1/B1–B7, mitigation = precision method + device-gated probe. |
| `docs/orchestration/reports/t-P1-elat.md` | This report. |

## Decisions

1. **True capture-to-render is T3−T0, not run elapsed.** The existing rev-5 `latency_us`
   (`started.elapsed()`, whole-run incl. connect+handshake+prefill) is re-categorised as
   `run_elapsed_us` in the probe schema; no report may quote it as an e2e p95.
2. **Sim containers are treated as cross-device** (separate monotonic clocks) → §3.2 offset-
   estimator method; §3.1 single-clock is reserved for in-process loopback tests.
3. **Acceptance is on the upper bound** `p95 + U` — keeps pass/fail honest under scheduler noise.
4. **Low Latency gate is explicit:** 20 ms Opus does not fit the 80 ms budget even in the best
   case (headroom forces 10 ms frames); probe qualification as in §10, per-device cache.
5. **Budget heads have concrete numbers** (no TBD), with the render/DAC head flagged
   device-dependent and gated by the probe — matching ADR-007/R07 mitigation-by-design.

## Commands run

No build/lint commands were required (document-only task, write scope: 3 allowed paths). The
following `git` checks were performed:

```bash
git -C /home/shivam/ps status --short        # only the 3 allowed files changed
git -C /home/shivam/ps diff --stat           # 3 files, 1 new + 1 appended row + 1 report
```

## Validation results

- Manual traceability check: every PROTOCOL_SPEC §Timing rule (capture=first-sample, render=DAC
  presentation, warm-up excluded, ≥5 min/profile, p50/p95/p99/max+spread, simulated vs physical
  separated) and PRODUCT_SPEC §8 metric (Balanced ≤150 ms, device-gated Low-Latency ≤80 ms,
  full context block) is explicitly addressed in the spec sections 2/3/4/5/6/7/8/9/13.
- TEST_PLAN "never substitute network RTT for e2e latency" honoured via the §9 recategorisation
  rule.
- Budget tables contain only quantified entries (no TBD); Low-Latency 20 ms Opus row is an
  explicit ✓/✗ verdict, not a placeholder.
- Percentile method stated precisely (§7: R-type-7 linear interpolation) with a required method
  tag in the output schema.
- Device-gate probe fully specified (§10: procedure, budget formula, cache policy, examples).

## Acceptance criteria

| Criterion | Result |
|---|---|
| Timestamp points + clock method defined (single-clock vs cross-device offset vs measurement-only NTP) | ✅ §2/§3 |
| Capture-to-render definition per context; warm-up exclusion; ≥5-min/≥5-run run discipline | ✅ §4/§5/§6 |
| Percentile calculation precise (order-statistic interpolation, n, method tag) | ✅ §7 |
| Measurement uncertainty defined and reported | ✅ §8 |
| Simulated vs physical separated; RTT ≠ e2e | ✅ §9 |
| Device-gated Low-Latency probe spec'd (connect probe, budget, per-device cache) | ✅ §10 |
| Executable probe contract: fields, sample size, output schema, compose/metrics plug-in | ✅ §11 |
| Budget model tables per codec with measured references and device-dependency flags | ✅ §12 (no TBD) |
| Risk register row appended (owner + gate) | ✅ R15 |
| Report written | ✅ this file |

## Risks / limitations

- **Spec-only deliverable:** the probe code itself is implemented by the B1/P2 harness worker
  (t-B1-SOAK / P2) using §11 as the contract — no crate changes were made here (write scope).
- The loopback transport numbers and t-B1 e2e numbers referenced in the budget are starting
  references; the first §11 recording run will firm the frame-level p95s (follow-up).
- `ClockHandle::now_ms` at 1 ms resolution bounds the earliest in-process hook to frame-period/2
  granularity; the spec's `U` formula compensates until finer clocks land.

## Follow-up tasks

- Implement the §11 recording hooks + `latency-*.json` output in `wdr_refsim` (with t-B1-SOAK).
- Wire `WDR_LAT_*` through `docker/sim-start.sh` + `docker/assert.sh` envelope assertions
  (DevEx-integrates).
- Physical-lab runbook applies §4.3/§10 (real DAC presentation time, probe on real devices).
- Ensure a cached probe result picture appears in the on-connect Low-Latency UI decision per FR-053.

## Blockers

None. Workspace remained green; only the 3 allowed paths were written.

---

# Host reference-loopback latency — p50/p95/p99 re-measure (2026-09-14, simulated)

Re-run driven by `scripts/verify/latency-host-loopback.sh`:
`wdr_refsim --example latprobe` times `QuicAudioSink → QuicRenderReceiver`
per frame over quinn loopback (FLAC i16/48k stereo, 512 spc), 5 runs × 256
frames per profile. **Simulated / upper-bound** — no physical DAC; device-SLO
probes (Balanced ≤150 ms, Low ≤80 ms, start ≤3 s) stay device-gated. The
first-frame row is the host 'publish → first render' upper bound; recovery ≤5 s
is proven by the wdr_session reconnect/backoff FSM (FR-025), not this loopback.

### `balanced`
```
  first_frame_to_render_ms: p50=53.044 p95=53.795 p99=53.795 (5 runs)
  per_frame_latency_ms:    p50=30.786 p95=58.318 p99=60.610 (n=1280)
```

### `low`
```
  first_frame_to_render_ms: p50=51.430 p95=51.548 p99=51.548 (5 runs)
  per_frame_latency_ms:    p50=24.917 p95=55.425 p99=58.452 (n=1280)
```

### `resilient`
```
  first_frame_to_render_ms: p50=51.970 p95=53.598 p99=53.598 (5 runs)
  per_frame_latency_ms:    p50=25.757 p95=57.535 p99=60.180 (n=1280)
```

