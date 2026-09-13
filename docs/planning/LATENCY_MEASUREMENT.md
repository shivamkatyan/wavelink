# Latency Measurement — Specification & Executable Probe

**Root task:** R-P1-ELAT · **Task:** t-P1-elat · **Owner role:** Reliability · **Date:** 2026-09-07
**Status:** Active spec — implementable by the B1/P2 harness; executable probe contract in §11.

This document is the *measurement spec* for every latency number the project reports. It is
the single authority for how "capture-to-render", "simulated vs physical", "p95", "warm-up",
"run length", and "uncertainty" are defined, and it specifies the executable probe the B1/P2
harness implements (interface + output schema), plus the device-gated Low-Latency probe from
ADR-007 / PRODUCT_SPEC §8.

Compliance anchors (must be satisfied together):
- PRODUCT_SPEC §8 — Balanced ≤150 ms p95 capture-to-render on controlled reference LAN; Low
  Latency ≤80 ms p95, opt-in + device-gated (latency probe on connect); Start ≤3 s; Recover ≤5 s;
  every result reports timestamp points, clock method, warm-up, run length, device/load/network
  conditions, percentile basis, uncertainty; simulated and physical reported separately.
- PROTOCOL_SPEC §Timing & sequence rules — capture timestamp = first sample of frame at source;
  render timestamp = DAC presentation time; warm-up excluded; run length ≥5 min/profile;
  p50/p95/p99/max + spread; simulated vs physical separated.
- TEST_PLAN — timestamped synthetic impulses / loopback for e2e latency; **never substitute
  network RTT for e2e latency** (§Integration, gated at Reliability & performance).

---

## 1. Scope & terminology

- **End-to-end (e2e) audio latency** = the time from a source sample existing at the emitter's
  capture point **until the corresponding sample is presented at the render/DAC point** on the
  receiver: `T3 − T0` (defined below). This is the quantity that must meet 150 ms/80 ms.
- **Transport one-way / RTT** = time on the network path only. **RTT is NOT e2e latency**
  (TEST_PLAN): the loopback echo p50 149 µs / p99 857 µs from `t-B0-transport` is a *transport
  round-trip*, and the rev-5 B1 whole-run `latency_us=71838` is *run wall-clock including
  connect, handshake, and prefill* — **neither is a capture-to-render p95**. They are input
  references only (§12) and must always be labelled as such.
- **Start / Recover** (3 s / 5 s) are *time-to-audible* metrics measured by separate monotonic
  probes (see §4.3). They are never merged into the frame-level latency statistics.

## 2. Timestamp points

Four points define the pipeline. All are recorded per frame with the declared clock (§3).

| Point | Name | Definition | Clock domain |
|---|---|---|---|
| **T0** | source-sample / capture time | Emitter stamps the **first sample of the frame at source**. In the sim this is the emitter's synthetic-source stamp; it is the wall-clock value of `Frame.media_ts` at emit (sample counter + wall-clock base, `wdr_proto::frame.rs`). | Emitter clock |
| **T1** | send-complete | After the full frame datagram/stream-write is handed off (QUIC `send_datagram` / `write_all` returns for that frame). | Emitter clock |
| **T2** | receiver-decode-ready | Receiver hands the frame payload to the decoder (frame left the jitter buffer, in-order, CRC-guarded). `T2` is taken *before* decode so it is comparable across codecs; decode time is separately bounded (RT contract, R03). | Receiver clock |
| **T3** | render presentation | **DAC presentation time.** Simulated: when the render sink accepts the decoded frame (the `NullRenderSink::render()` call in `wdr_refsim::receiver.rs`), i.e. the simulated device play-out instant at `ClockHandle::now_ms()`. Physical: the first sample written to the DAC/audio-callback boundary; true electrical presentation time is only measured with the hardware loopback rig (R04, HARDWARE_VALIDATION.md) and reported as such. | Receiver clock |

Derived sub-intervals (diagnostic, always recorded, never the acceptance metric):
- `t_encode = T1 − T0` — capture+accumulate+encode+send path (emitter-local, single clock).
- `t_transit = T2 − T1` — network path observer (clock-domain caveat: cross-device requires §3.2).
- `t_decode_render = T3 − T2` — decode + jitter-buffer hold + render queue (receiver-local, single clock).
- **e2e = T3 − T0** — the acceptance metric.

## 3. Clock method

The clock method must be stated on **every** reported result. Three sanctioned methods:

### 3.1 Single-clock (in-process / in-sim loopback)
Both endpoints execute in one process / one address space sharing one monotonic clock domain.
`T0..T3` are all read from that clock → `T3 − T0` is exact (no offset estimation). Applies to:
`wdr_refsim::tests::ref_e2e` loopback tests, and any future in-crate pipeline bench.

### 3.2 Cross-device, in-band (default for the Docker/compose sim AND physical)
No shared clock. Each side reads its **own** monotonic clock. Method:
1. `T0` is carried in-band by the emitter sample counter (`Frame.media_ts`, sample counter +
   wall-clock base). The receiver converts capture time to its own clock with an **agreed offset
   estimator**: a WLS/robust regression of (emitter sample counter → receiver arrival monotonic
   clock) pairs collected from a probe exchange (the same estimator family as ADR-007, but this
   instance is **measurement-only**, separate from the streaming drift estimator). Outlier rejection
   identical to ADR-007 policy.
2. `T3 − T0 = (T3 − arrival_clock) + (arrival_clock − T0_est)` where `T0_est` is the offset-
   mapped capture time; the estimator residual is folded into uncertainty (§8).
3. Fallback allowed (and the only sanctioned "shared clock"): a **shared NTP/PTP discipline used
   for measurement only** — it **never drives capture or render correctness**; streaming
   robustness stays on ADR-007 (receiver-driven adaptive resampling + drift estimator). State
   discipline, offset, and residual in the report.

### 3.3 Simulated single-clock note
"Single clock per side" is true in the reference sim (each side is one logical clock), but the
Docker harness runs `ref_emitter` and `ref_receiver` in **separate containers** → they are
already cross-device in the §3.2 sense. Use §3.2 there; reserve §3.1 for single-process loopback
tests. When both containers are on the same host you may optionally cross-check with `CLOCK_BOOTTIME`
host monotonic, but the **reported** method must be §3.2 (it is the one that generalizes to
physical).

## 4. What "capture-to-render" means in each context

### 4.1 Acceptance definition
`capture-to-render = T3 − T0`, per frame, after warm-up exclusion (§5). Thresholds apply to the
**p95 of the frame-level distribution** (Balanced ≤150 ms; Low Latency ≤80 ms) on the **clean
controlled reference LAN** (sim: `clean` netem profile; physical: reference LAN in
HARDWARE_VALIDATION.md). Under impairment the threshold is per-mode-documented (PRODUCT_SPEC §8),
not the clean-LAN number.

### 4.2 Simulated (Docker/loopback)
- Emitter synthetic source (timestamped impulse / sine fixture from `wdr_fakes`) stamps T0.
- Transport is the QUIC path over the compose bridge/netem as-is.
- T3 is the `NullRenderSink` accept time (simulated play-out).
- All sim results are labelled `simulation: docker-looback | in-process` (§9).

### 4.3 Physical (real devices)
- T0 = real capture API stamp (WASAPI/PipeWire/etc. sample time).
- T3 = DAC presentation (see §2). Electrical-only measurement requires the rig in R04.
- Start/recover measured as **time-to-audible** (first audible frame at render) with a separate
  monotonic probe — never derived from the frame-latency sample set.

## 5. Warm-up exclusions

Excluded from **all** percentile/statistic computations (not just flagged):
1. **Buffer prefill:** from run start until the receiver jitter buffer reaches the profile's
   target steady-state fill for the first time. Sim: fill target per `BufferProfile` (§in ADR-007
   sizing inequality).
2. **Startup floor:** the first `WARMUP_FRAMES = 100` rendered frames **or** the first 2.0 s of
   rendered audio, whichever is later, measured from first render.
3. **Recovery transients:** every frame rendered within 500 ms after a late-discard burst, a
   reorder-window jump, or an underrun recovery, plus the frame(s) that triggered them.
4. Only warm-up-clean frames count toward `n`; the number excluded is reported (`warmup.excluded`).

## 6. Run length & repetition

- **Minimum run:** ≥5 min (300 s) per cell (codec × frame size × buffer profile × network
  profile) **of warm-up-clean measurement** (§5), per PROTOCOL_SPEC "run length ≥5 min/profile".
- **Repetition:** ≥5 independent runs per cell; each run is a fresh connect+negotiate+stream.
  Report **per-run spread** (min/max/median of run-level p50/p95/p99/max) — never a single run.
- **Sample count guard:** a run is valid only if `n ≥ 1000` clean frame-latency samples
  (a 5-min run at Opus 20 ms ≈ 15 000, Opus 10 ms ≈ 30 000, FLAC 5 ms ≈ 60 000). Fewer → the run
  is discarded and re-run (report the discarded count).
- A run must record device/load/network conditions: network profile id, device/OS/DAC tag,
  observed CPU load (sim: container), channel layout, sample repr, buffer profile, frame size.

## 7. Percentile calculation (precise method)

On the sorted (ascending) sample vector `x_1 ≤ … ≤ x_n` of clean frame-latency values
(§5, §6):
- **Percentile** `Q(p)` for `p ∈ (0,1)`: **linear interpolation between adjacent order statistics
  (R type 7, the Numpy/`PERCENTILE.INC` default)**. Let `h = (n − 1) · p`, `i = ⌊h⌋`, `f = h − i`;
  then `Q(p) = x_i + f · (x_{i+1} − x_i)`, with `i` clamped to `[0, n − 1]`.
- **p50** → `h = (n − 1) · 0.5` (interpolated between the two middle order statistics when `n`
  is even). **p95**, **p99** by the same formula. **max** = `x_n`.
- `n`, the frame-period, and the method tag `linear-interp-order-statistics-r7` MUST be reported
  with every number.
- Run-level aggregates (per-run p95 min/max/median) are plain maxima/minima/medians over the run
  set — report their own `n_runs`.

## 8. Measurement uncertainty

Reported as `value ± U_us`, where:
- `U = max(frame_period / 2, (p99 − p50) / 2) + cross_clock_residual`
  - `frame_period / 2`: T0/T3 edge-quantization (a frame is stamped whole at its boundary;
    ± half a frame-period of inherent quantization in locating "first sample" / "presentation").
  - `(p99 − p50) / 2`: scheduler/driver jitter half-width — the practical jitter-spread term.
  - `cross_clock_residual = 0` under §3.1; otherwise the WLS offset-estimator standard error
    (§3.2) or the measured discipline offset residual (§3.3).
- Clock resolution of the used clock (e.g., `Instant` ~ns; `ClockHandle::now_ms` 1 ms) is listed
  separately and already bounded by `frame_period/2` for frame-sized metrics.
- **Acceptance uses the upper bound**: a target is met only if `Q(p95) + U ≤ 150 ms` (Balanced)
  or `≤ 80 ms` (Low Latency). This keeps pass/fail honest under scheduler noise.

## 9. Simulated vs physical separation

- **Never merge** simulated and physical numbers in a single statistic, chart, or table.
- **Simulated** = compose/Docker (`clean`/`loss1`/`jitter30` … profiles) or single-process
  loopback; tag `simulation: docker-looback | in-process`.
- **Physical** = real OS capture + real DAC render; tag `simulation: physical`.
- A result that mixes a simulated channel with a real device is **physical-partial** and must be
  labelled as such on both the channel and the device legs.
- **RTT ≠ e2e** (TEST_PLAN): transport ping numbers (`t-B0-transport` 149 µs/857 µs) and
  whole-run wall-clock (`latency_us=71838`) are referenced *only* as budget inputs (§12), clearly
  annotated "transport/run-elapsed, not capture-to-render".
- Every reported number carries the full context block (§13); a number without it is rejected by
  review.

## 10. Device-gated Low-Latency probe (ADR-007 / PRODUCT_SPEC §8)

Low Latency (≤80 ms p95) is **opt-in** and gated by a **latency probe on connect**. The probe
decides whether ≤80 ms is achievable for *this device* on *this connection*.

### 10.1 Probe procedure (on connect)
1. After handshake, pre-final-negotiation: emitter sends a **timestamped probe frame** on the
   media path (a `QoS/Probe` marker, same framing as a real frame; may ride the datagram lane).
   It carries `probe_id`, `T0_probe` (emitter wall clock), and the emitter sample counter base.
2. Receiver records `T2_probe = arrival` on its monotonic clock and immediately returns a probe
   reply carrying `T2_probe` + its clock reading.
3. Emitter records `T3_probe` on receipt. Repeat for **≥3 exchanges**; use the **p95 of the
   per-exchange one-way estimates**.
4. Compute `one_way_transit = (T3_probe − T0_probe)/2` (single clock: the emitter's own clock;
   the exchange also seeds the §3.2 offset estimator).
5. **e2e budget estimate:**
   `budget = one_way_transit(p95) + capture_accumulate + encode_delay + decode_rt + jitter_buffer(profile) + render_DAC_profile[device]`
   where the fixed pipeline heads come from the active codec/buffer profile (§12) and
   `render_DAC_profile[device]` starts from the bundled per-OS/DAC-class calibration table and is
   refined in-band by §4.3/§3.2 measurements after the first stream.
6. **Qualify** iff `budget + margin ≤ 80 ms − guard`, `guard = 5 ms` (scheduler allowance), i.e.
   `budget ≤ 75 ms`, at the measured p95 with `n_probes ≥ 3`. The Low-Latency toggle is presented
   only while qualified; otherwise show "Low Latency unavailable on this connection/device" with
   the measured budget and one-action remedy (PRODUCT_SPEC §8 / R07).
7. **Cache per device:** persist `{ device-capability-hash → budget, timestamp, probe profile }`
   to the local device-capability store, keyed by a **non-identifying** model/OS/API capability
   hash (no raw stable device identifier — FR-055 / SECURITY_SPEC redaction). Cache is used only
   to skip re-measurement on reconnect; it is invalidated on DAC hotplug/route change (FR-015)
   and on config change, and enjoys a bounded TTL.
8. The probe is **measurement-only**; it never affects capture/render correctness, the drift
   estimator, or the buffer controller (which stay per ADR-007).

### 10.2 Pass/fail examples
- Opus 10 ms on a capable USB DAC (`render_DAC_profile ≈ 30 ms`, clean LAN `one_way ≈ 1 ms`):
  `1 + 10 + 6.5 + ~0.5 + 20 + 30 = 68 ms ≤ 75 ms` → **qualified** (see §12 Low-Latency row).
- Opus 20 ms on the same device: `74.5 + E` → **not qualified**; Low Latency forces 10 ms frames.
- Opus 10 ms on a high-latency Android USB DAC path (`E ≈ 60 ms`): `68 − 30 + 60 = 98 ms`
  → **not qualified** (R07 mitigated: no false "low latency" claim).

## 11. The executable probe (B1/P2 harness)

The harness implements a latency-recording mode in the reference sim plus a standalone
`lat_probe` driver. This is the **contract**; the worker that builds it (t-B1-SOAK / P2) owns the
code, gated by this spec.

### 11.1 Recording hooks (in `wdr_refsim`, emitter + receiver)
- Emitter: stamp `T0` (per frame, source stamp = unit within `media_ts`/first-sample basis) and
  `T1` (post send-handoff). Persist `(seq, T0, T1)`.
- Receiver: stamp `T2` (handed to decode) and `T3` (render-sink accept, §2). Persist
  `(seq, T2, T3)`. For §3.2, also log receiver arrival clock + `media_ts` pairs for the offset
  estimator.
- Both sides emit per-frame samples into a bounded ring; the probe driver correlates them by
  `seq`+`run_id`, applies §5 exclusions, and computes §7 stats. The existing `latency_us` field
  (whole-run elapsed) is kept but **recategorised** as `run_elapsed_us` in the schema below so it
  is never mistaken for e2e.

### 11.2 Configuration (env-driven, sim-start.sh-compatible — no positional CLI)
- `WDR_LAT_MODE=record|report|probe` · `WDR_LAT_RUN_MS` (default 300000) ·
  `WDR_LAT_RUNS` (default 5) · `WDR_LAT_WARMUP_FRAMES` (default 100) ·
  `WDR_LAT_DEVICE_TAG` (default `sim-none`) · `WDR_LAT_NETWORK_PROFILE` (inherits
  `WDR_NETEM_PROFILE`) · `WDR_METRICS_DIR` (existing) · `WDR_LAT_RUN_ID` (default
  `wall-clock-run-<n>`). Codec/frame-size/buffer come from the existing emitter/receiver config
  so a probe run is byte-identical to a normal e2e run except for the added stamps.

### 11.3 Output schema (metrics storage) — `latency-<profile>-<codec>-<frame_ms>-<buffer>-<run_id>.json`, JSON `v1`

```jsonc
{
  "schema": "wdr.latency.v1",
  "run_id": "…",
  "task": "t-P1-elat",
  "simulation": "docker-looback | in-process | physical | physical-partial",
  "clock_method": "single-clock | offset-estimator | ntp-measurement-only",   // breadth per §3
  "conditions": {
    "network_profile": "clean", "device_tag": "sim-none",
    "os": "linux", "dac_class": "sim-null-sink", "load": "idle",
    "codec": "Opus", "frame_ms": 20, "sample_rate": 48000,
    "channel_layout": "Stereo", "sample_repr": "I16", "buffer_profile": "balanced"
  },
  "warmup": { "policy": "prefill+startup100+recovery500ms", "frames_excluded": 123, "seconds": 2.2 },
  "run": { "planned_ms": 300000, "cleaned_ms": 292000, "n_runs_total": 5, "n_runs_valid": 5, "n_runs_discarded": 0 },
  "pipeline": [ { "T0": 0, "T1": 0, "T2": 0, "T3": 0, "e2e_us": 0, "t_encode_us": 0, "t_transit_us": 0, "t_decode_render_us": 0 }, "… per clean frame" ],
  "stats": {                                   // §7 over each run, then run-level rollup
    "count": 15000,
    "frame_period_us": 20000,
    "percentile_method": "linear-interp-order-statistics-r7",
    "per_run": [ { "n": 15000, "p50_us": 0, "p95_us": 0, "p99_us": 0, "max_us": 0 }, "…" ],
    "p50_us": 0, "p95_us": 0, "p99_us": 0, "max_us": 0,
    "spread": { "p95_min_us": 0, "p95_max_us": 0, "p95_median_us": 0 },
    "uncertainty_us": 0, "cross_clock_residual_us": 0
  },
  "e2e_acceptance": { "target_balanced_us": 150000, "target_lowlat_us": 80000,
    "p95_us_plus_uncertainty_us": 0, "balanced_pass": true, "lowlat_pass": null },
  "run_elapsed_us": 71838,                     // legacy whole-run proxy; KEPT BUT RE-LABELLED, never an e2e p95
  "loss": { "packets": 0 }, "underruns": 0, "late_discard": 0,
  "device_gate": {                             // §10; present only in probe mode
    "probe_exchanges": 3, "one_way_p95_us": 0, "budget_est_us": 0,
    "margin_us": 5000, "qualified": true,
    "cached": { "hit": false, "key_hash": "…", "source": "table|in-band" }
  },
  "device_load_network_notes": "free text ─ every §13 context item"
}
```

### 11.4 Plug into the compose/metrics harness (future worker edits, specified here)
- `docker/sim-start.sh` (DevEx-owned) passes `WDR_LAT_*` through to the ref binaries; sims write
  the `latency-*.json` files into the shared `wdr-metrics` volume (same pattern as
  `emitter-sim.json`).
- `docker/metrics.sh` globs `latency-*.json`, rolls runs up into `collector.json` + `runs.log`
  (p50/p95/p99/max, per-run spread, uncertainty, pass/fail).
- `docker/assert.sh` gains an **envelope assertion**: per cell, `Q(p95) + U ≤ budget`
  (150 ms Balanced, 80 ms device-gated Low Latency) and `n ≥ 1000`/`n_runs ≥ 5`; the existing
  "no-crash + hash" floors are unchanged.
- Each profile/codec cell is run under the existing `clean`/`loss1`/`jitter30`… profiles so the
  simulated envelope is a direct, attributed number, never conflated with physical.
- Nothing in this task edits those files (write scope: this spec + risk row + report); the above
  is the interface the B1/P2 harness implements.

## 12. Budget model — Balanced ≤150 ms p95 and Low Latency ≤80 ms p95

Head contributions (ms), per codec, using measured references:
- `t-B0-transport`: loopback echo p50 149 µs / p99 857 µs → transit p95 (wired LAN) **1 ms**.
- `t-B1` e2e (whole-run `latency_us`, clean): FLAC ≈ 71 ms (rev-5 `71838`); Opus ≈ 21 ms;
  1% loss ≈ 90 ms — these bound the motion but, being run-elapsed, are used as *starting
  references for the envelope sanity check*, not as the frame-level p95.
- Opus algorithmic delay 6.5 ms (RFC 6716 lookahead/pre-skip); FLAC ≈ one block (ADR-005,
  240 samples @48k = 5.0 ms). Capture/accumulate = one frame period. Jitter buffer: Balanced
  40 ms, Low 20 ms (device-independent config per PROTOCOL_SPEC profiles / ADR-007 sizing).
  Render/DAC `E`: **device-dependent** (Android USB DAC historically 20–80 ms; R07).

### 12.1 Balanced ≤150 ms p95 (clean controlled LAN) — frame-level budget
| # | Head | Opus 20 ms | Opus 10 ms | FLAC (5 ms) | Device-dep? |
|---|---|---|---|---|---|
| A | Capture / accumulate buffer (= frame period) | 20.0 | 10.0 | 5.0 | capture-API dep on real HW (PipeWire quantum, WASAPI shared/exclusive) |
| B | Encode algorithmic delay | 6.5 | 6.5 | 5.0 | no (codec) |
| C | QUIC transit p95 (wired LAN; t-B0 loopback p99 0.857 ms → margin) | 1.0 | 1.0 | 1.0 | network/AP dep (Wi-Fi raises this; flag) |
| D | Jitter buffer, Balanced profile | 40.0 | 40.0 | 40.0 | no (config) |
| E | Render / DAC presentation | 25.0 | 25.0 | 25.0 | **device-dependent** (Android USB DAC 20–80 ms) |
| F | Scheduler / RT-callback allowance | 5.0 | 5.0 | 5.0 | no (implementation) |
| | **Total p95 budget** | **97.5** | **87.5** | **81.0** | ≤150 ms ✓; headroom = 150 − total |
| | Headroom | 52.5 | 62.5 | 69.0 | comfortable; E is the only device-sizeable term |

### 12.2 Low Latency ≤80 ms p95 (opt-in, device-gated, clean LAN) — frame-level budget
| # | Head | Opus 20 ms | Opus 10 ms | FLAC (5 ms) | Device-dep? |
|---|---|---|---|---|---|
| A | Capture / accumulate | 20.0 | 10.0 | 5.0 | capture-API dep (real HW) |
| B | Encode algorithmic delay | 6.5 | 6.5 | 5.0 | no |
| C | QUIC transit p95 | 1.0 | 1.0 | 1.0 | network dep |
| D | Jitter buffer, Low profile | 20.0 | 20.0 | 20.0 | no |
| E | Render / DAC (gated capable device) | 30.0 | 30.0 | 30.0 | **gated by probe §10; high-latency DAC → not qualified** |
| F | Scheduler allowance (guard) | 5.0 | 5.0 | 5.0 | no |
| | **Total p95 budget** | **82.5** | **72.5** | **66.0** | 20 ms Opus **fails** → forces 10 ms; 10 ms Opus & FLAC pass with a capable `E` |
| | Verdict | ✗ not qualified | ✓ 72.5+U ≤ 80 (device-gated) | ✓ 66+U ≤ 80 (clean LAN, device-gated) | Low Latency is lossy/10 ms-first; FLAC Low-Latency = clean-LAN best-effort, **not** gated for lossless under impairment |

### 12.3 Impaired / degraded network (Balanced and Resilient)
- Lossy lanes: impairment shows as loss/late-discard, not added latency beyond the jitter buffer
  (bounded by `D`, PROTOCOL_SPEC numeric bounds).
- Lossless reliable stream: retransmit deadline ≤50 ms (ADR-003). With the ±uncertainty envelope,
  1%-loss whole-run ≈90 ms (t-B1) fits the Balanced 150 ms budget. Under jitter30/reorder the
  acceptance is per-mode-documented per PRODUCT_SPEC §8 — never compared against the clean-LAN
  150 ms.

## 13. Reporting checklist (mandatory context block on every latency result)

[ ] timestamp points used (T0/T1/T2/T3) · [ ] clock method (§3.1/§3.2/§3.3) + resolution/residual
[ ] warm-up policy + frames excluded · [ ] run length + n_runs + n per run (+discarded)
[ ] device/load/network conditions · [ ] percentile method (§7) + n · [ ] uncertainty `± U`
[ ] simulated vs physical separation + RTT≠e2e annotation · [ ] per-run spread · [ ] device-gate
result for any Low-Latency claim (probe exchange count, budget estimate, cache status).

## 14. Terminology mismatch versus current code (action note)

The rev-5 `latency_us` (ref_emitter `started.elapsed()`) is **run elapsed**, not capture-to-render
p95. The t-P1-elat/B1 harness ships the T0..T3 hooks and re-labels that field `run_elapsed_us`;
until then no report may quote `latency_us` as an e2e p95. This spec is the governing definition
from today.
