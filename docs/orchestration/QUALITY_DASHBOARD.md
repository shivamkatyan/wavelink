# QUALITY DASHBOARD (0.0.1 genesis 2026-09-13, macOS host)

> Host revalidation: macOS (Xcode 26.6/iOS 26.5 SDKs, Rust 1.98.1, Docker 29.4.0, JDK17+Android SDK).
> Inherited green corrected on intake (INC-003): ref_e2e golden desync + rustfmt style drift repaired; workspace now genuinely green.

| Axis | Status | Detail |
|------|--------|--------|
| Platform matrix | green (P0), revalidating macOS/iOS rows | evidence-dated 2026-09-06; macOS/iOS build+test env revalidated 2026-09-10 (Xcode 26.6) |
| Requirements coverage | ~24 Verified · ~23 portable+gate (now incl. 4 new shells on this host) · ~9 external-gate · ~2 unsupported-by-public-API · FR-056 a11y acceptance DEFINED (shells carry semantics; device walkthroughs = gates) | REQUIREMENTS_TRACEABILITY.md audit |
| Build (core) | green | cargo build --workspace; 11 crates |
| Tests (workspace) | **245 passed, 0 failed** | unit/property/golden/e2e/security incl. proptest+fuzz (count grew past the 184 recorded on WSL2; +3 = `wdr_refsim/tests/ref_live_sink.rs` `QuicAudioSink`/`AudioFrameSink` roundtrip: FLAC golden hash, Opus datagram, Free-refuses-lossless) |
| Shell compile+tests (macOS host) | green | **combined-app era (2026-09-13):** `android-wavelink` single app **35/0** unit tests (incl. `SinkSeamTest`) + assembleDebug · merged `platform/ios` single app type-checks **0 errors** · macos-emitter 17(+20 taps)/0 (+ `--receive` driver, smoke-gated) · linux-receiver 11/0 + runnable launcher · win-emitter 8/0 (Linux-validated surface) · linux-emitter 7/0. The split android/ios role apps are superseded by the combined apps (their flows are device-gated, see ACCEPTANCE_CHECKLIST) |
| Smoke gates (`scripts/verify/`) | green on macOS host | `macos-launch-smoke.sh` (window present) + `macos-stream-smoke.sh` (fixture pro/flac → **hash == canonical golden**) + `macos-receive-smoke.sh` (WS3 receiver role: `--receive --sink null` ← `--stream` fixture, hash == canonical golden) — the "opens with a UI AND really streams/renders" regression guards |
| Golden lossless | green | canonical blake3 hashes, hash(source)==hash(decoded), channel order (INC-003 repaired) |
| Integration/netem | green | clean + 1% loss hash-preserving; Opus lossy bounded; policy gate; metrics contract |
| Soak 60-min | **PASS** | 67-min wall (WSL2 2026-09-09) + **macOS re-runs 2026-09-10 & 2026-09-13** (`22153f00…` cross-host stable; 2026-09-13: RSS 1.54–4.83 MiB / 62 samples, 0 underruns/fatal, 7/7 netem profiles) |
| CC decision | keep CUBIC | measured equivalence under loss1%+jitter30ms (ADR-003 appended) |
| Codec decision | FLAC w/ hybrid PCM fallback | ADR-005 bench measured (worst-case FLAC loses to PCM) |
| RT contract | green | wdr_rt SPSC + stress test; RT_CONTRACT.md per-platform whitelist |
| Security tests | green (core) | Noise XX handshake, replay, key separation, AEAD tamper, redaction poison, 0-RTT-off |
| SBOM/audit | **green on host (2026-09-10)**: `just audit` → cargo-audit 0 vulnerabilities (273 crates); `just license` → cargo-deny licenses OK (committed deny.toml permissive policy); `just sbom` → lock inventory | cargo-cyclonedx/syft remain CI-runner-gated |
| Flake rate | 0 known (soak retries documented as env/script defects, not product flakes) | — |
| Perf budgets | reference-loopback measured (Opus ~21ms, FLAC ~71ms, 1% loss ~90ms); device budgets gated | LATENCY_MEASUREMENT |
| Open review findings | 0 critical/high (plan reviews); B7 final audit pending native gates | — |
| Pending external gates | wasapi/windows-capture · android-ci (FFI runtime) · usb-dac-device · ios-device · bt-lab · native-RT-evidence · bit-perfect-loopback · signing creds · (windows-basic + ios-simulator CI jobs now ENABLED with real steps) | runbooks in HARDWARE_VALIDATION / RELEASE_AND_SIGNING |
