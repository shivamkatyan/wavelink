# t-P1-cc — BBR vs CUBIC under netem (lossy datagram lane)

**task_id:** t-P1-cc · **root_task_id:** R-P1-CC · **owner_role:** Networking ·
**date:** 2026-09-08 · **status:** complete

## Summary

Ran the **measured** BBR-vs-CUBIC decision the B0 transport spike
(t-B0-transport) flagged as needing `netem`: the reference lossy **Opus
datagram** lane (`ref_emitter` → `ref_receiver`) through the compose netem
bridge, under a **clean** control profile and the **degraded**
(`loss 1% + fixed jitter 30ms`) profile, for **each** of Cubic and Bbr, with a
20 ms frame cadence (the real Opus 20 ms cadence). 16 trials (4/CC/profile;
defective EOS-loss trials auto-retried so medians use only completed runs).

**Decision:** **keep Cubic** as the WDR default congestion controller. Under
the degraded profile Cubic and Bbr are **statistically indistinguishable** on
every receiver-observable audio-health metric (median loss `4.0` vs `3.0`,
late-discard `0` vs `0`, underruns `0` vs `0`, fatal `0` vs `0`, queue bound
honored in both). Bbr holds a much larger congestion window under loss
(≈349 KB vs Cubic's collapsed ≈2.9 KB), but for a 20 ms-paced, ~400 B/frame,
low-bitrate datagram lane that headroom buys **nothing observable** — Cubic is
not worse on the degraded profile, is quinn's default (best-tested, fewest
moving parts), and needs no custom wiring. Per ADR-003's exit criteria, no
reopen is triggered.

### Measurement table (median over 4 trials / 250-frame run unless noted)

| profile | CC | recv | loss | dup | reorder | late | underruns | fatal | cwnd (B) | RTT (µs) | lost pkts | cong events | queue bound (≤512) honored |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| clean | Cubic | 250 | **0** | 0 | 0 | 0 | 0 | 0 | 12 000 | 234 | 0 | 0 | ✅ |
| clean | Bbr | 250 | **0** | 0 | 0 | 0 | 0 | 0 | 359 448 | 236 | 0 | 0 | ✅ |
| degraded | Cubic | 246 | **4.0** (spread 0–8) | 0 | 16 | **0** | **0** | **0** | 2 904 | 26 540 | 23 | 22 | ✅ |
| degraded | Bbr | 247 | **3.0** (spread 2–4) | 0 | 21 | **0** | **0** | **0** | 349 312 | 25 761 | 22 | 20 | ✅ |

Sender-side `cwnd`/`lost_packets`/`congestion_events` are the quinn path
metrics `wdr_transport::metrics_snapshot` exposes (`emitter_path` in
`emitter-sim.json`); they distinguish the two controllers (Cubic collapses to
L₀ minimum cwnd under loss; Bbr holds a ~350 KB window). The receiver-visible
health metrics are what the audio lane actually experiences.

**Interpretation:** the loss floor (≈3–4 of 250 frames; Cubic spread 0–8, Bbr
2–4) is netem's own 1% loss (fair: both CCs track the injected 1%);
`late_discard = 0` and `underruns = 0` mean the jitter/reorder window absorbed
the 30 ms jitter with zero stalls under both. Neither CC starves the datagram
lane at this rate/jitter/degradation. The only material difference is
congestion-window behaviour, which is invisible to the app at WDR's send
cadence.

## Files changed

| Path | Change |
|---|---|
| `crates/wdr_refsim/src/bin/ref_emitter.rs` | Added `WDR_CC=cubic|bbr` (default cubic) → `TransportConnConfig::congestion_control(...)` at dial; records `cc` + `path` (quinn cwnd/rtt/lost/congestion-events) in `emitter-sim.json`. Refuses unknown values (no fabricated numbers). |
| `crates/wdr_refsim/src/emitter.rs` | `EmitterConfig.pace: Option<Duration>` — optional steady-state frame cadence (`WDR_EMIT_PACE_MS`), default `None` (burst preserved for loopback). Required for a valid CC measurement: a burst-then-close under 30 ms jitter measures "drop on connection close", not CC behaviour. |
| `crates/wdr_refsim/tests/ref_e2e.rs` | `pace: None` on the loopback e2e config initializers (no behaviour change; 4 e2e still green). |
| `docker/ccspike.sh` | **New** spike harness: applies netem `clean` / `loss 1% delay 30ms` on the netem bridge, runs `ref_emitter --lossy opus` (20 ms cadence) vs `ref_receiver` per CC with N trials, captures emitter+receiver metrics, asserts queue bound + no panic, prints a median summary table → JSON. |
| `docs/orchestration/reports/t-P1-cc.md` | This report. |
| `docs/planning/ADRS/ADR-003.md` | Appended "Spike result (t-P1-cc)" section with the measured decision. |

No change to `crates/wdr_transport` — the CC seam (`CongestionControl`,
`congestion_controller_factory`) already existed from t-B0-transport; this
spike is the first consumer to drive Bbr through it over `netem`.

## Decisions

1. **Keep Cubic (no switch to Bbr).** Measured receiver-visible audio health
   under the ADR-003 degraded profile (1% loss + 30 ms jitter) is equivalent:
   median loss 4.0 (Cubic) vs 3.0 (Bbr), late 0/0, underruns 0/0, queue bound
   honored. Cubic is quinn's default and needs no feature/custom wiring; Bbr's
   ~350 KB steady cwnd under loss is not needed by a 20 ms × ~400 B datagram
   lane and buys nothing measured. No ADR-003 reopen trigger.
2. **Datagram lane healthy at the degraded profile (both CCs).** `late=0`,
   `underruns=0`, fatal 0: the reorder window + jitter buffer absorbed 30 ms
   jitter with no stalls at 1% loss. This is the evidence the ADR-003
   "datagram credit starvation / CC collapse" concern was *looking* for, and it
   is negative: no starvation at this profile/rate.
3. **BBR must be measured with sender pacing, not a burst.** Initial harness
   runs that burst all frames then closed the connection reported ~260/375
   "loss" under degradation for *both* CCs — an artifact of dropping queued
   datagrams on connection close, not CC behaviour. Wiring `WDR_EMIT_PACE_MS`
   (real 20 ms cadence) is what separates honest CC evidence from that
   artifact. Recorded here so future CC work paces the sender.
4. **quinn 0.11 BBR is real and not feature-gated.** `BbrConfig` exists in the
   locked quinn-proto 0.11.17 (`src/congestion/bbr`) and is selectable via
   `congestion_controller_factory` — the BBR numbers above are genuine (not a
   fallback proxy).

## Commands run

```bash
# build + validate the CC wire-through (default Cubic; Bbr opt-in)
cargo build -p wdr_refsim --bin ref_emitter --bin ref_receiver --release
cargo test  -p wdr_refsim -p wdr_transport     # 4 loopback e2e + transport contract green
cargo clippy -p wdr_refsim -p wdr_transport --all-targets -- -D warnings   # clean
cargo fmt    -p wdr_refsim -p wdr_transport -- --check                     # clean

# measurement (host Docker + netem bridge; 16 valid trials = 4 trials x 2 CC x 2 profiles;
# defective EOS-marker-lost trials auto-retried, bounded to 5)
TRIALS=4 WDR_DURATION_SECS=10 bash docker/ccspike.sh
# raw per-trial JSON: /tmp/opencode/results/ccspike-final.json
```

(With the default `clean` + `degraded` (loss 1% + delay 30ms) profiles applied
inside the netem container; emitter at 20 ms cadence via `WDR_EMIT_PACE_MS`.)

## Validation results

- `cargo test -p wdr_refsim`: 4/4 loopback e2e green after the `pace` field
  was added (lossless FLAC/PCM hash-preserved, lossy Opus bounded, malformed
  rejected).
- `cargo clippy -D warnings` + `cargo fmt --check`: clean on the touched bins.
- Spike: all reported trials `receiver status=complete`, `fatal_count=0` (no
  panic, honoring the panic-free transport/queue contract), `queue_bound_ok=
  true`. Trials where the end-of-stream marker (a single unreliable datagram,
  ~1% netem loss) was lost made the receiver time out with `error`/recv=0;
  the harness retries those bounded (max 5) so the medians are built only from
  completed runs (this EOS-drop is a refsim/harness artifact, not CC data).
- CC genuinely active at the seam: `emitter_sim` `cc` field + clear cwnd
  divergence (12 KB vs 359 KB clean; 2.9 KB vs ~350 KB degraded) prove Cubic
  vs Bbr were actually driving, not a mis-attributed single controller.

## Acceptance criteria

| Criterion | Result |
|---|---|
| CC selection wired through `wdr_transport` + ref_emitter via `WDR_CC` (default Cubic, opt-in Bbr) | ✅ `ref_emitter` reads `WDR_CC`, builds `TransportConnConfig::congestion_control(...)`; transport seam already present; default unchanged |
| Run clean + degraded (loss1% + jitter30ms) for Cubic AND Bbr on the lossy Opus datagram path | ✅ compose netem bridge, 20 ms cadence, 4 trials each; degraded profile per TEST_PLAN |
| Receiver loss/dup/reorder/late/underruns + emitter latency + queue-bound assertion recorded | ✅ full table above (16 trials; medians + sender path metrics) |
| Queue <= MAX_QUEUE_BOUND + no panic | ✅ `queue_bound_ok=true` on every reported trial; `fatal=0`; receiver `complete` |
| Clear CC decision + ADR-003 updated | ✅ **keep Cubic**; ADR-003 "Spike result" appended |
| Honest BBR (not a proxy/fabrication) | ✅ quinn-proto 0.11.17 `BbrConfig`, genuine, not feature-gated; numbers measured |

## Risks / limitations

- **Loss floor is netem's own 1%** (Cubic median 4, Bbr median 3 of 250 frames —
  both tracking the injected 1%). The CC *reaction* is captured in the sender
  path metrics (cwnd/lost/congestion-events), not in a darker receiver-loss
  number; do not read "Cubic loss ≈ injected 1%" as "Cubic lost frames
  CC-independently".
- **LAN scale-up not measured here.** Loopback→real-path MTU, rate, and
  multi-flow effects (e.g. BBR vs Cubic fairness against competing traffic on a
  real LAN) are out of scope for this spike; `bbr` remains selectable via
  `WDR_CC=bbr` if a later LAN/interop profile shows BBR advantage.
- **One datagram = one ~400 B Opus frame at 20 ms** is a very benign CC load
  (~600 kbit/s); the conclusion may not transfer to higher rates. Re-run with
  `WDR_EMIT_PACE_MS` at the degraded profile for a lossless-path/stream-mix CC
  check before any switch.
- **`emitter_latency_us` measures whole-run wall clock** (≈5.3 s for a 10 s
  paced run up to SIGTERM), which is why it is ~constant per profile; it is not
  a per-frame latency and is not used in the decision.
- The earlier burst-mode 259/260-loss numbers were a **close-clock artifact**
  and are superseded by the paced runs (decision #3); the harness now always
  paces.
- **EOS-marker is a single unreliable datagram:** at ~1% netem loss a small
  fraction of trials lose it and the receiver times out (recv=0, error). The
  harness retries those (bounded), and the report medians exclude them. A
  production hardening follow-up is to send/dup the EOS marker (or rely on a
  reliable control-stream end) so short runs can't misfire.

## Follow-up tasks

- Re-run this matrix from the CI/self-hosted NET_ADMIN runner (or a true
  cross-host LAN path) to upgrade loopback numbers to LAN numbers, and add it
  to the routine B1 soak path if cheap.
- Consider a **Cubic vs default-alternative** (NewReno) sanity run to confirm
  Cubic is the best default among quinn's own controllers at the degraded
  profile (optional; no ADR pressure).
- Harden the reference-sim end-of-stream path: duplicate the EOS marker (or
  move end-of-run signalling to the reliable control stream) so short-paced
  runs are not subject to the ~1% EOS-datagram loss at the degraded profile.
- Confirm the `WDR_CC` + `pace` additions live behind the refsim interface
  contract documented for the B1 session core so the production emitter is
  CC-parameterisable without a new seam.

## Blockers

- None. Harness needed a live `NET_ADMIN` netem container (present on this
  host; the compose `netem` service provides it). Workspace-wide build excludes
  the parallel in-flight crates (`wdr_rt`, benches owned by other workers) —
  same merge-order caveat as prior waves.
