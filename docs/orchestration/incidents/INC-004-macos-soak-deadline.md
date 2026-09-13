# INC-004 — macOS 60-min soak deadline overrun + stale metrics

- **Date:** 2026-09-10 (macOS-host 60-min clean soak re-validation)
- **Root task:** B1 reference-system soak re-validation on macOS (Docker Desktop)
- **Classification:** Environment/monitor-config defect (not a product defect) —
  container-based deadline pad calibrated on WSL2 under-calibrated macOS
  virtualization; plus a stale-metrics footgun in soak.sh.

## Reproduction
- `WDR_SOAK_SECONDS=3600 bash docker/soak.sh` on macOS (Docker Desktop, Linux VM):
  emitter "did not finish in time; killing" → RESULT=FAIL, emitter exit 137 (SIGKILL by monitor).
- Receiver ran ~62 min clean: 62 RSS samples, no underrun/fatal signal; lossless hash
  preserved on the portions that completed (receiver side complete for prior 30s run,
  hash `51b6fd5c…`).

## Root cause
1. **Deadline pad under-calibrated:** `WDR_SOAK_DEADLINE_PAD` default was the fixed
   600s from WSL2 tuning. A 3600s real-time-paced run on Docker Desktop took >4200s
   wall (per-frame encode + QUIC + VM scheduling overhead >16%), so the monitor killed
   a healthy emitter. Same class as the documented WSL2 attempts 1–4.
2. **Stale metrics footgun:** soak.sh never cleared the shared metrics volume, so the
   summary read the PREVIOUS run's `emitter-sim.json`/`receiver-sim.json` (both the 30s
   run's hashes) — masking what actually occurred. ccspike.sh already clears.

## Repair (evidence-producing, durable)
- soak.sh now clears `*-sim.json` before every run (mirrors ccspike.sh).
- soak.sh default deadline pad is ratio-based: `SOAK_SECS * 50% + 120s` (env-overridable
  via `WDR_SOAK_DEADLINE_PAD`) — comfortably covers WSL2 (~11% overhead) and macOS
  (~16% overhead) with one mechanism.
- soak.sh RSS reader honored `WDR_SOAK_RSS` (was hard-coded to `/tmp/opencode/soak-rss.log`)
  and parsed the correct `MemUsage` token (was `split()[1]`, i.e. the "/").
- Status: **closed** — re-run PASSED: full 60-min clean soak on macOS (emitter exit 0,
  receiver complete, underruns 0, fatal 0, lossless FLAC hash preserved `22153f00…`,
  RSS flat 1.35–7.02 MiB / 62 samples). Evidence appended to B1-SOAK-EVIDENCE.md.
