# Manual Acceptance Checklist — role-selector / role-picker flows

**Status: living checklist.** The 0.0.1 genesis milestone was **compile- and
unit-verified** on build hosts; the UI flows below were **not** click-through
authenticated by the build agent (no physical Android device, no iOS device /
Xcode project, and macOS capture TCC on this host only). This file records
exactly what a human (or a future device-gate run) must verify, and what
evidence currently exists. Nothing here is claimed as a finished device test.

Legend for the **Status** column:

- ✅ **evidenced** — something automated already proved it (smoke script,
  unit test, compile gate); evidence cited.
- 🟡 **device gate** — must be run on the named hardware/grant; the runbook is
  `docs/planning/HARDWARE_VALIDATION.md` / `docs/planning/RELEASE_AND_SIGNING.md`
  and the named gate in `docs/orchestration/RELEASE_STATUS.md`.

Each row is **manual** unless it cites a script/evidence. When you run a row,
record the result next to it (date, device/OS, pass/fail) — or drop it from the
checklist and promote the flow to "evidenced" in the orchestration docs.

---

## (a) Android — role picker → Emitter / Receiver

App: `android-wavelink` (`dev.wavelink.app`), artifact
`android-wavelink-release-signed.apk`. Launcher is the role picker
(`RoleLauncherActivity`); Emitter and Receiver live in one app.

| # | Step | Expected | Status |
|---|---|---|---|
| a1 | Install the APK and open **Wavelink** | Role picker shows two tiles: **Emitter** and **Receiver** | 🟡 device gate (`android-device`) |
| a2 | Tap **Emitter** | `EmitterActivity` opens: status card, one contextual Start/Stop, Free/Pro toggle | 🟡 device gate |
| a3 | Tap Start | In-app FR-052 explanation appears **before** the system prompt; then MediaProjection (`createScreenCaptureIntent`) consent; on grant, `EmitterService` FGS starts (`mediaProjection` type) and status goes live | 🟡 device gate |
| a4 | Deny MediaProjection once | In-app explanation confirmed beforehand; a denial produces an actionable error, not a silent hang | 🟡 device gate |
| a5 | Android 14+ second session | MediaProjection consent is requested afresh each capture session (single-use tokens) | 🟡 device gate |
| a6 | Back → role picker → tap **Receiver** | `ReceiverActivity` opens with route card + "Scan output routes" + Free/Pro toggle; tapping Start launches `ReceiverService` FGS (`mediaPlayback`) | 🟡 device gate |
| a7 | Receiver route scan with a USB DAC attached | `AudioOutputRouter` lists the USB DAC (FR-013); without one it reports "none (no USB DAC detected)" | 🟡 device gate (`usb-dac-device`) |
| a8 | Receiver route change (unplug DAC) | Route-change event surfaces (FR-015); no crash | 🟡 device gate |
| a9 | Free vs Pro toggle in either role | FR-047 downgrade confirm fires when switching Pro→Free | 🟡 device gate |

**Evidence already held (NOT a click-through):** `./gradlew :app:assembleDebug
:app:testDebugUnitTest` — combined app builds, 31 unit tests pass (supervised
on the build host, 2026-09-13); role/status unit tests cover the state model,
not the OS consent UI. Compile + unit ≠ the steps above.

---

## (b) iOS — role picker → Emitter / Receiver

App: merged single-app source under `platform/ios` (`dev.wavelink.app`).
**There is no Xcode project and no `.ipa` yet** — running any of this is itself
a gate (see `docs/planning/RELEASE_AND_SIGNING.md` and the iOS gate in
`RELEASE_STATUS.md`). Complete (a)…(g) on a simulator/device **once an Xcode
project wraps `platform/ios`**.

| # | Step | Expected | Status |
|---|---|---|---|
| b1 | Open the app | Role picker (`RolePickerView`) lists Emitter + Receiver with blurbs and SF Symbols | 🟡 device gate (`ios-device` / no Xcode project) |
| b2 | Tap **Emitter** | `EmitterContentView` opens; consent explainer (`ConsentExplainerView`) precedes local-network TCC | 🟡 device gate |
| b3 | Emitter capture flow | System broadcast/SCK picker appears; a session can start and the system red indicator shows; audio continues only while the session is actively capturing | 🟡 device gate |
| b4 | Tap **Receiver** | `ReceiverContentView` opens; `OutputRouter` lists the route; `.usbAudio` DAC output selectable | 🟡 device gate (`usb-dac-device`) |
| b5 | Switch role from the toolbar | "Choose role" button returns to the picker without state corruption | 🟡 device gate |
| b6 | Dynamic Type / VoiceOver sweep | Role tiles + status rows are reachable and labelled (FR-056 iOS row) | 🟡 device gate |

**Evidence already held:** the merged app compiles against the iOS SDK — `0
errors` type-check gate (supervised on the build host, 2026-09-13;
`platform/ios/build-check.md`). Compile ≠ a run.

---

## (c) macOS — role selector + Start/Stop streaming (TCC-granted session)

App: `macos-emitter` (`.app` inside `wavelink-<rev>.dmg`). The window header
carries a `NSSegmentedControl` role selector (**Emitter** default; **Receiver**
staged with an honest alert).

| # | Step | Expected | Status |
|---|---|---|---|
| c1 | Launch the `.app` | Window opens with an Emitter/Receiver segmented role selector and a Start Stream action | ✅ `scripts/verify/macos-launch-smoke.sh` (window present, GUI session) |
| c2 | Select **Receiver** | An alert explains the desktop receiver render path is the next milestone (no silent "running") | ✅ source-verified (`app/main.swift` `roleChanged`); not clicked-through → mark as manual |
| c3 | Grant Screen Recording TCC | Permission row shows granted; `--permission-state` reports allowed | ✅ on this host (TCC-granted session, 2026-09-13); fresh installs = `macos-capture-sck` gate |
| c4 | Emitter → Start Stream (fixture) | `scripts/verify/macos-stream-smoke.sh`: fixture → `ref_receiver` decode hashes to the canonical golden (lossless FLAC hash-perfect; Free/Opus bounded) | ✅ automated (no hardware) |
| c5 | Emitter → Start Stream (system audio) | Real SCK capture on a TCC-granted logged-in session streams without fatal/underruns | ✅ on this host (68 frames / 0 loss / 0 fatal / 0 underruns, 2026-09-13); other hardware = `macos-capture-sck` gate |
| c6 | Stop Stream mid-run | Clean stop path (flushes end-of-stream); UI returns to "Start Stream" | ✅ covered by the stream smoke's clean-stops `0 fatal`; manual spot-check recommended |
| c7 | Set Receiver address + tier change | Session metrics card updates honestly (FR-053); Pro→Free shows the FR-047 downgrade confirm | 🟡 manual spot-check (alert dialog not smoke-tested) |
| c8 | Full Keyboard Access / VoiceOver spot-check | Role selector + action buttons reachable and labelled (FR-056 macOS row) | 🟡 manual on hardware |

**Evidence already held:** launch smoke + stream smoke (fixture) are automated
in `scripts/verify/` and pass on this macOS host; real SCK capture verified on
the host's TCC-granted session (RELEASE_STATUS B3 row). The **click-through of
c2, c7, c8 is not automated** — a human should exercise them on any real
session and record it here.

---

## Recording a flow as done

1. Run the row's steps on the named hardware/OS (record device, OS version,
   app version).
2. On pass: update the row to `✅` and add the date/evidence; update
   `docs/orchestration/RELEASE_STATUS.md` and `QUALITY_DASHBOARD.md` if it
   closes one of their named gates.
3. A flow that cannot run on available hardware stays 🟡 with its gate name —
   a pending gate with a runbook is allowed; a silent claim of "tested" is not.
