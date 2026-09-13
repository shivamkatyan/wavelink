# Wavelink — Build Status & Release Evidence (in-progress snapshot)

Snapshot date: **2026-09-13 (0.0.1 genesis)** · Fresh single-commit history in the `wavelink` repo, carrying forward the previously verified state (245 tests / 0 fail after the 2026-09-13 ws1/ws2/ws3/ws5 batch; the v0.2.0 release of the retired `wireless-hifi-relay` repo is history) · **2026-09-14 update: ws-a…ws-h landed — workspace now 279 tests / 0 fail, android 35/0, secure lane, i24 e2e, drift estimator, uniffi bridge (host-proven), QR token, RT no_std+watchdog, host latency evidence** · Honest, evidence-backed state per Phase; no simulated claim passed off as physical.

## Wavelink 0.0.1 snapshot (fresh genesis, combined-app era)

- **One Android app** (`android-wavelink`, `dev.wavelink.app`): role picker →
  Emitter (MediaProjection FGS) or Receiver (route-scan + mediaPlayback FGS).
  The split `android-emitter` / `android-receiver` role shells were **retired**
  (verified at 0.1.x, then removed from the tree in the proprietary cleanup;
  `scripts/package/android.sh` ships only `android-wavelink`).
- **One iOS source surface** (`platform/ios`): merged `WavelinkApp` SwiftUI app,
  role picker in-app; type-checks (0 errors) but has **no Xcode project /
  `.ipa` yet** → iOS gate below.
- **macOS** keeps the emitter `.app`/DMG; the window header now has an
  Emitter/Receiver role selector (Receiver staged with an honest alert).
- **Release artifact naming:** `android-wavelink-*-signed.apk`,
  `wavelink-<rev>.dmg`, `linux-emitter[-receiver]-<rev>.tar.gz` / `.deb`,
  `win-emitter-<rev>.zip`, `wavelink-ios-source-<rev>.tar.gz`.
- The flows that were only compile/unit-verified are logged with evidence-or-device-gate in
  [`ACCEPTANCE_CHECKLIST.md`](./ACCEPTANCE_CHECKLIST.md).

## What works (verified, portable core)
| Item | Evidence |
|------|----------|
| Rust shared core (11 crates) | Workspace green on macOS host: `cargo build/test/clippy/fmt` — **218 passed / 0 failed** (INC-003 repaired ref_e2e golden desync + rustfmt style drift on intake) |
| B0 fixtures + lossless golden equality | `wdr_fakes` canonical blake3 goldens; FLAC/PCM roundtrips hash-preserving (t-B0-wire) |
| B1 reference system (emitter+receiver sim) | Loopback e2e green (215-suite); Docker harness re-validated on macOS: image has hermetic prebuilt Linux refsim binaries (Dockerfile.dev), topology + NET_ADMIN verified, escaped the WSL2 host-binary-mount assumption (soak.sh/ccspike.sh/sim-start mounted Mach-O on macOS — fixed to in-image binaries) |
| P1 spikes | ADR-005 codec bench (FLAC-with-hybrid-PCM fallback recommended); ADR-003 CC spike (keep CUBIC); LATENCY_MEASUREMENT.md; RT_CONTRACT + wdr_rt SPSC |
| B2 shells | `platform/win-emitter` (Linux-validated, 8 tests), `platform/android-wavelink` (combined app: 31/0 unit tests + assembleDebug/Release; split-era shells retired) |
| B3 Linux shell | `platform/linux-emitter` (Linux-validated, 7 tests; PipeWire backend cfg-gated) |
| B3 macOS shell | `platform/macos-emitter` **UI gate closed on host**: the packaged `.app` opens with a real window (launch-smoke) and `--stream` runs SCK capture or a fixture through the core `AudioFrameSink` seam → encode → CRC → QUIC → `ref_receiver` (fixture path **hash-perfect** against the canonical golden; real SCK capture verified on this host's TCC-granted session, 68 frames / 0 loss / 0 fatal / 0 underruns). Fresh-install Screen Recording TCC + capture on other hardware = `macos-capture-sck` gate; process taps (14.2+) still a seam |
| B4 iOS receiver | `platform/ios-receiver` was **complete on host** (SwiftUI/AVAudioEngine + .usbAudio route + a11y; 43/43 tests + iOS-sim-SDK gates) — split-era shell **retired**; the merged `platform/ios` single app is the surface; .usbAudio/local-net = device gate |
| B4 Android emitter | `platform/android-emitter` was **complete on host** (playback-capture + MediaProjection FGS; assembleDebug + 24/0 tests) — split-era shell **retired**; `android-wavelink` (role picker; 31/0 unit tests + assembleDebug/Release) is the surface; capture = device gate |
| B4 iOS emitter | `platform/ios-emitter` was **complete on host** (ReplayKit/SCK max-public scope + App Review 2.5.14 record) — split-era shell **retired**; merged into `platform/ios`; ReplayKit runtime/SCK 27+/App Review = device/store gates |
| B5 Linux A2DP-sink receiver | `platform/linux-receiver` **complete on host** (portable surface 11/0 tests; BlueZ/PipeWire native = Linux-runner/bt-lab). **Now ships a runnable launcher** (`src/bin/linux_receiver.rs`: interactive menu / `--register` / version) **and is packaged by `scripts/package/linux.sh`** (tarball + `.deb` with `.desktop` entry) — the receiver no longer has "no binary to open" |
| SBOM mechanism | `just sbom` (273-pkg inventory) + `just audit` (0 vulns) + `just license` (deny.toml OK) |

## Soak status
- 60-minute clean soak (lossless FLAC, real-time paced): **PASS** on the source WSL2 host (B1-SOAK-EVIDENCE.md: 67-min wall, lossless hash preserved, 0 underruns/fatal, RSS flat).
- macOS-host: harness fixed for hermetic Linux binaries (in-image ELF; host binaries are Mach-O). **Full 60-min clean soak re-run on macOS PASSED 2026-09-13** (this batch): hash `22153f00…` identical to the prior WSL2/macOS full soaks (cross-host + cross-batch deterministic), RSS 1.54–4.83 MiB / 62 samples, 0 underruns/fatal, `bootstrap-netem.sh` 7/7 profiles PASS. See B1-SOAK-EVIDENCE.md.

## Gates still external (honest)
- **windows-ci** (WASAPI native check), **usb-dac-device** (physical), **macos TCC/SCK/tap physical logged-in session**, **ios-device** (.usbAudio + local-network), **android-emulator/device capture**, **bt-lab** (virtual HCI/device), **native RT evidence** (WASAPI/Oboe/SCK/AU on runner/lab), **bit-perfect hardware loopback** (bit-perfect = receiver-side, lab-verified, bounded-slip).
- Credential gates (signing/notarization/store) per RELEASE_AND_SIGNING.md.

### Remaining product gates (scheduled follow-ups — NOT done; no claim of completion)

| Gate | What it needs | Status |
|---|---|---|
| **macOS desktop Receiver render path** | Wired at the seam (2026-09-13, WS3): refsim `RenderSink` trait + live `QuicRenderReceiver` server + macOS `--receive` driver. `--sink null` = host-smoked hash==golden (`macos-receive-smoke.sh`); `--sink audio` = `CoreAudioRenderSink` (preflights a HAL output device, typed error otherwise). Actual AudioUnit playback I/O = TCC/USB-DAC session (`usb-dac-device`) | 🟡 seam+driver real on host; audio I/O device-gated |
| **win/linux combined GUI + native receiver roles** | `platform/desktop-gui` is a driver scaffold (pure-Rust iced, shares the `AudioFrameSink` engine with a real fixture driver); the iced window is a runner-gated `gui` feature. A combined win/linux app + native receiver render roles need a Windows/Linux runner and native capture gate | 🔒 runner-gated (`windows-ci` / linux runner) |
| **iOS `.ipa` + signing** | The merged `platform/ios` app type-checks but ships as source/unsigned cores only — publishing needs an Xcode project wrapper + Apple Developer credential (`RELEASE_AND_SIGNING.md`) | 🔒 credential + Xcode-project gate |
| **Store metadata / screenshots** | App Store / Play Store listing assets (screenshots are device-gated — real device screenshots, not renders) | 🔒 not started |

Each row is a deliberate pending gate with a runbook; per `ACCEPTANCE_CHECKLIST.md`,
device-gated flows are logged with an exact hardware/OS gate or evidence, never
silently marked tested.

## Not yet done / in progress (explicit)
- **B3 macOS emitter native runtime validation**: real SCK capture was exercised on this host (TCC-granted logged-in session → 68 frames/0 fatal/0 underruns via `--stream`); fresh installs needing Screen Recording TCC + real capture on other hardware + process taps (14.2+) = hardware gates. Desktop receiver roles (macOS first) queued.
- **B4**: iOS receiver / Android emitter shells in progress on this host; iOS emitter (ReplayKit/SCK) queued (store-policy + device).
- **B5** Bluetooth cells — only Linux-standard-sink is full path; others honest-unsupported w/ fallback per ADR-009 (bt-lab).
- **B6** full hardening (a11y acceptance defined 2026-09-10; security/chaos/perf gates on runners).
- **B7** independent audit re-run (first audit closed 0 crit/high; final DoD reconciliation in progress).

## How to reproduce the B1 e2e
```bash
source dev/env.sh
cargo build -p wdr_refsim --release
# topology: wdr-netem + wdr_wdr-net bridge + receiver + emitter (see docker/soak.sh)
bash docker/soak.sh   # 60-min clean soak; WDR_SOAK_SECONDS=N for shorter
```

## Artifact locations
- Planning/ADR/RT/latency/security: `docs/planning/`
- Orchestration/release/dashboards/checklists: `docs/orchestration/`
- Worker reports: `docs/orchestration/reports/`
- Platform shells: `platform/{macos-emitter,win-emitter,linux-emitter,linux-receiver,android-wavelink,ios,desktop-gui}`
- Sim + fixtures: `crates/wdr_refsim`, `crates/wdr_fakes`
- Compose/netem/soak: `compose.yml`, `docker/`
- SBOM: `docs/orchestration/sbom/` (+ `just sbom`)

---

# Definition of Done — B7 reconciliation (2026-09-10, macOS-host session)

Per governing contract §18. `✅` = evidenced now · `🟡` = partial (named gate) · `🚫` = evidence-backed unsupported + fallback.

| # | DoD item | Status | Evidence |
|---|----------|--------|----------|
| 1 | Every FR has traceable outcome | ✅ | REQUIREMENTS_TRACEABILITY audit (every ID: Verified / portable+gate / external-gate / unsupported+fallback) |
| 2 | Reference system + MVP automated & green | ✅/🟡 | B1 reference e2e + 60-min soak PASS on WSL2 **and** macOS (`22153f00…`); shells green; native MVP (WASAPI/USB) = gates |
| 3 | Win/mac/Linux desktop emitters complete within verified API limits | 🟡 | three shells complete + tested (win 8, linux 7, macos 17 + 20 taps); **macOS additionally opens with a UI and streams via the `AudioFrameSink` seam (`macos-stream-smoke.sh` fixture gate hash-perfect + real SCK on host), and now RECEIVES via the `RenderSink` seam (`macos-receive-smoke.sh` hash-perfect, WS3)**; runtime capture elsewhere = windows-ci / TCC-SCK-tap gates |
| 4 | Android + iOS receiver paths complete within verified limits | 🟡 | android-receiver + ios-receiver were JVM/iOS-sim tested (split-era shells, since **retired**); the surface is now the combined `android-wavelink` receiver role + merged `platform/ios`; device = gates |
| 5 | Desktop-receiver + mobile-emitter cells complete OR evidence-unsupported + fallback | 🟡/🚫 | ios/android emitters + linux A2DP-sink receiver shells complete; stock-phone-sink = unsupported-by-public-API + free-lossy-WiFi fallback; mac/win/linux desktop-receiver roles queued |
| 6 | Free lossy + Pro lossless Wi-Fi work end-to-end | ✅ | B1 e2e: Opus lossy bounded + FLAC lossless hash-preserving (clean + 1% netem loss) |
| 7 | Dev Free/Pro toggle → one centralized policy, tested | ✅ | `wdr_entitlement` EntitlementProvider/DevToggle + policy tests; all shells carry the top-level toggle |
| 8 | Every viable BT cell implemented; nonviable honest | 🟡 | Linux standard A2DP-sink receiver shell implemented (11/0 tests); custom RFCOMM cell feature-gated; stock-sink unsupported+fallback (ADR-009/matrix); bt-lab for physical evidence |
| 9 | Lossless integrity via PCM-equality tests | ✅ | canonical blake3 goldens, hash(source)==hash(decoded), e2e + soak; INC-003 made it honest on this host |
| 10 | Output-conversion vs bit-perfect never conflated | ✅ | fidelity ladder (converted / unverified / bitPerfect via hardware-loopback token) in core + iOS shell |
| 11 | Discovery/pairing/encryption/revocation/malformed/redaction pass security tests | ✅/🟡 | crypto/session/telemetry tests green (Noise XX, replay, key-sep, AEAD tamper, redaction poison); **mDNS discovery wired at the crate + refsim level (2026-09-13: `wdr_discovery` + `--advertise`/`--discover` + e2e)**; mobile-shell pairing/trust-store UI = shell/device gate |
| 12 | WSL2 bootstrap + container sim + native CI reproducible | 🟡 | bootstrap + compose validated on this host (hermetic in-image binaries); CI jobs ready-but-gated (`if:false` + `macos-shell` job ready; runners needed) |
| 13 | Native packages produced or ready except documented credentials | 🟡 | installers/packaging stubs ready; signing/notarization gated (RELEASE_AND_SIGNING) |
| 14 | Hardware-only checks: retained evidence or precise pending gate | ✅ | every hardware item has a runbook (HARDWARE_VALIDATION / RELEASE_AND_SIGNING) + named gate |
| 15 | No unresolved critical/high independent finding | ✅/🟡 | plan reviews closed 0 crit/high; B7 final audit = the requirement-mapping + this DoD + external gates (no open critical/high raised) |
| 16 | Phases 3–7 each executed | ✅ | B3 (macos+linux emitters) · B4 (ios rx/em, android em) · B5 (linux bt rx) · B6 (a11y+sec+soak) · B7 (reconciliation here) — all executed this + prior sessions |
| 17 | Docs match behavior | ✅ | shell READMEs/build-checks record real commands + gates; orchestration current |
| 18 | Repo validated + resumable, orchestration current | ✅ | workspace 215/0 green; clean tree; commits 0244629, d8c90df, 7be8667 (+pending closeout) |

**Verdict:** no MVP-only result; Phases 3–7 executed with shells compile/unit-validated on this host. Remaining 🟡 are exclusively **named external gates with runbooks** (native runners, physical devices, creds) — consistent with §18's allowance for "current official evidence that public APIs prevent the required behavior, plus the nearest supported fallback" and "precision pending gate".
