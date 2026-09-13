# B1 60-minute clean soak — PASS (2026-09-09)

## Gate (§8): "Clean-network soak: 60 minutes with zero crashes, deadlocks,
## unbounded memory growth, or buffer underruns on the reference setup."

**Result: PASS**

Setup: docker bridge `wdr_wdr-net` (receiver) + netem shared-netns (emitter),
clean profile (no netem qdisc), lossless FLAC i16/48k stereo, pseudo-random
source, balanced buffer, real-time pacing (WDR_EMIT_REAL_TIME=1),
`--seconds 3600`.

Evidence (from /tmp/opencode/soak5.json + soak5-rss.log):
- emitter: exit 0, status ok, run elapsed ~4,020,684,561 µs (~67 min wall for
  3600 s of audio; ~7 min = per-frame encode+QUIC+rt overhead on the shared
  emulated host — expected, explainable), hash 22153f00…
- receiver: status complete, underruns 0, fatal 0, hash 22153f00… (== emitter)
- loss: 0, duplicate 0, reorder 0, late 0 (lossless reliable stream)
- RSS over run (107 samples): min 1.59 MiB, max 3.76 MiB, delta 2.17 MiB —
  flat, no unbounded memory growth
- no crash, no deadlock (ran to clean completion), no underrun

Method honesty: this is a SIMULATED clean soak on the Docker reference system
(a real-time-paced reference emitter/receiver over a real OS/bridge path), not
a physical-device soak. It proves the portable core's long-run stability and
lossless integrity. Physical-device long-duration/battery/thermal remains a
hardware gate (HARDWARE_VALIDATION.md).

Retries recorded (INCs): attempts 1–4 invalidated by distinct environment/
script defects (stale binary, pacing overrun, container-name collision,
monitor-deadline overrun); attempt 5 (rev-10 binary, unique names, 80-min
monitor, correct pacing) PASSED. Retry tax tracked by root+hypothesis per
ledger.

---

# B1 clean soak — macOS-host revalidation (2026-09-10)

Harness made hermetic for macOS: Dockerfile.dev now prebuilds the Linux
`ref_emitter`/`ref_receiver` binaries INTO the image (soak.sh/ccspike.sh/
sim-start.sh previously mounted the host `target/release` — ELF on WSL2, but
Mach-O on macOS — so they exec the in-image Linux binaries instead). Reference
harness re-validated under Docker Desktop on this host.

- **Shortened clean soak PASS (30 s audio, real-time paced):** emitter exit 0
  status ok · receiver complete · underruns 0 · fatal 0 · lossless FLAC hash
  preserved (emitter == receiver == `51b6fd5c…`) through the netem clean bridge.
- **FULL 60-min clean soak PASS (2026-09-10, macOS host):** emitter exit 0
  status ok · receiver complete · **underruns 0 · fatal 0** · **lossless FLAC
  hash preserved (`22153f00…` — the SAME canonical golden as the WSL2 full-soak
  PASS, cross-host determinism)** · wall ≈ 84 min for 3600 s of audio (≈ +15%
  Docker-Desktop virtualization overhead — the reason INC-004's ratio-based
  monitor pad replaced the fixed 600 s) · RSS flat/bounded: 62 samples, min
  1.35 MiB, max 7.02 MiB (the peak is the startup sample; steady state ~2–3 MiB),
  no growth trend. First macOS attempt was monitor-killed (INC-004 — deadline
  + stale-metrics); after the soak.sh fixes (in-image binaries, metrics cleared,
  ratio pad, correct RSS parse) the run is a clean PASS.

Method honesty (unchanged): simulated clean soak on the Docker reference
system; not a physical-device soak. Physical long-duration/battery/thermal
remains a hardware gate.

---

# B1 clean soak — re-run on macOS host after the i24/rt-guard/receiver-seam batch (2026-09-13)

Re-run of the FULL 60-min clean soak on this macOS host under Docker Desktop,
after the WS1/WS2/WS3 core changes (24-bit codec adapter, `rt-guard` +
`panic=abort`, receiver `RenderSink` seam) — proving the reference behaviour is
unchanged by the new work.

- **FULL 60-min clean soak PASS (2026-09-13, macOS host):** emitter exit 0
  status ok · receiver complete · **underruns 0 · fatal 0** · **lossless FLAC
  hash preserved (`22153f00…` — identical to the previous macOS and WSL2 full-soak
  hashes, cross-host cross-batch determinism)**. `bootstrap-netem.sh` profile
  suite ALSO passed: all 7 profiles (clean/loss0.5/loss1/loss5/jitter30/
  reorder/duplication) green with asserts.
- RSS over run: 62 samples, min 1.54 MiB, max 4.83 MiB — flat, no unbounded
  growth. No crash, no deadlock, no underrun (ran to clean completion).

Method honesty (unchanged): simulated clean soak on the Docker reference
system; not a physical-device soak. Physical long-duration/battery/thermal
remains a hardware gate (HARDWARE_VALIDATION.md).

Evidence: `/tmp/opencode/soak-result.json` (result=PASS, soak_seconds=3600) +
`/tmp/opencode/soak-rss.log` + `/tmp/opencode/ws6-bootstrap.log` (7/7 profiles).
