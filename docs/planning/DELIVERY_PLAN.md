# Delivery Plan (Dependency DAG)

Task packets carry the full §13.3 contract (task_id, root_task_id, hypothesis_id, objective, mode, owner_role, dependencies, baseline_revision, allowed_paths, read_only_paths, inputs, constraints, acceptance_criteria, validation_commands, scope_validation_command, required_evidence, out_of_scope, report_path). Consolidated delivery status lives in `docs/orchestration/RELEASE_STATUS.md` / `QUALITY_DASHBOARD.md` (the one-file-per-task ledger and phase-gate sheets were retired in the proprietary cleanup).

## Waves (parallelization = disjoint write paths)

| Phase | Objective | Entry → Exit criteria | Owned by | Validation |
|---|---|---|---|---|
| P0 Evidence | OS×role×transport + BT matrices timestamped; capture probes; ADR-001..010; bootstrap/clean-clone design; traceability | matrix rows evidence-dated; no reusable product work before gate; gate rows green in RELEASE_STATUS | Platform Feasibility ×5, OSS/Licensing | research notes + probes (largely DONE 2026-09-06) |
| P1 Decisions/spikes | transport/lossless sizing spike (ADR-003/005), drift/jitter spike (ADR-007), RT contract table, protocol freeze, golden vectors | ADRs accepted; numeric bounds locked; fuzz targets stubbed | Audio/Proto/Security/DevEx | benchmark + netem spike reports |
| B0 Foundation | cargo workspace, contracts/schemas, fake adapters, synthetic source/hash sink, telemetry+redaction, CI skeleton, Compose harness, EntitlementProvider | unit+property green; bootstrap from fresh clone; CI green | DevEx, Platform Implementer (core) | `just unit` etc. |
| B1 Reference system | headless emitter+receiver; discovery/pairing/negotiation/encrypted control/media; Opus lossy + FLAC/PCM lossless; jitter/drift/backpressure/reconnect/observability/entitlement testable; full topology automated | golden + impairment + security-of-reference green; soak 60-min | Proto/Audio/Reliability | `just e2e`, `just soak` |
| B2 MVP | Windows emitter (WASAPI loopback) + Android receiver (AudioTrack/Oboe, USB DAC route/hotplug, API34+ BIT_PERFECT gated, mediaPlayback FGS) + Free/Pro toggle + installers/APK + focused E2E | real Win capture; Android renders to output + USB DAC; Free lossy + toggled Pro lossless e2e; reconnect/route/lifecycle/soak/reference-hw | Win/And impl, UX, Reliability, Integration | hardware-lab gated for USB |
| B3 Desktop breadth | macOS emitter (SCK/taps), Linux emitter (PipeWire per-app), desktop receiver roles; permission flows; virtual-device/native-runner tests; limits surfaced | release-capable within documented limits | mac/lin impl | virtualization + native-runner + lab |
| B4 Mobile breadth | iOS receiver (local net TCC, .playback background, .usbAudio, TestFlight path); Android emitter (playback-capture consent/FGS); iOS emitter (ReplayKit + SCK, system pickers); unsupported capture detected before start | store-policy + lifecycle tests pass | ios/android impl | simulator + device lab |
| B5 Bluetooth | Linux a2dp_sink receiver; custom RFCOMM/L2CAP product-peer cells (throughput budgets); honest matrix + one-action fallback; never imply stock-phone sink | approved cells implemented with physical evidence; others explicit | BT impl, UX | BT lab (virtual HCI + device) |
| B6 Hardening | security/privacy gates, accessibility (WCAG AA), performance+soak budgets, crash/leak/malformed/chaos suites, upgrade/compat policy tested, install/uninstall + clean-machine, SBOM/notices/user+operator docs | all gates green; flake ≤ threshold; no critical/high findings | Reliability, Security, UX, Docs | full matrix |
| B7 Release audit | independent requirement/security/licensing/clean-machine audits; repair until clean or explicit block | no unresolved critical/high; every requirement Verified/evidence-unsupported/gated-with-runbook; signing jobs ready | Independent Reviewer | audit reports + DoD checklist |

## Critical path
P0 → P1 (ADR-003/005/007 spikes) → B0 (protocol freeze) → B1 (reference green) → B2 (Win capture + Android render/USB) → B6 → B7. Desktop/mobile breadth and BT parallelize off the path.

## Risks & fallback
quinn CC under loss (→ BBR/redundancy/UDP fallback); libopus/FLAC cross-compile friction (→ pinned NDK/Xcode toolchains); Android USB latency variance (→ device-gated Low-Latency); hosted-runner audio absence (→ virtual endpoints / hardware lab); iOS App Review for capture features (→ system pickers + compliance record). Each has an owner in `RISK_REGISTER.md`.

## Do your own lock check
No phase may be declared complete without its gate row green in `RELEASE_STATUS.md` (binary criterion + command/evidence + reviewer disposition). Foreground waves B0–B2 sequential where file-contract dependencies require; background waves (B3–B5 breadth) overlap B2–B6 where write scopes are disjoint.
