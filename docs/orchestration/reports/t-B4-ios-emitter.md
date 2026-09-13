# t-B4-ios-emitter — iOS Emitter Shell: honest capture-capability matrix, FR-052 consent FSM → system picker, Free/Pro gate, FR-055 redaction, App-Review 2.5.14 record

**task_id:** t-B4-ios-emitter · **root_task_id:** B4-ios-emitter ·
**hypothesis_id:** H-B4-IOS-EM-1 · **owner_role:** Platform Implementer - iOS ·
**date:** 2026-09-10 · **status:** complete (on-host validation done; ReplayKit
broadcast runtime, SCK 27+, TCC, App Review = device/store gates — see
acceptance table)

## Summary

Standing up the iOS **emitter** (capture) shell (`platform/ios-emitter`) as the
last mobile-breadth cell, mirroring the iOS receiver shell in structure and the
Android emitter in role: (1) `WDRiOSEmitterCore` — a pure-Foundation core that is
unit-tested on this macOS host AND type-checks against the iOS 14.0
iphonesimulator SDK with the very same sources, encoding the honest iOS capture
boundary (PLATFORM_MATRIX §A iOS + ADR-008): the **system bus is UNCAPTURABLE via
public API** (there is no iOS system-wide tap); per-app/self audio = self
`installTap` (12+), ReplayKit broadcast **via the system picker** (12–26),
ScreenCaptureKit (27+, replaces ReplayKit). `CapturePolicy` (capability matrix +
`acceptableCaptureModes(forOSMajor:)` + consent/indicator/background/protected
honesty strings), `EmissionStatus` (FR-053 + FR-055 redaction + 2.5.14 indicator
honesty), `ConsentFlowState` (FR-052 FSM: `.needsExplanation →
.awaitingSystemPicker → .authorized/.denied`, authorised unreachable without the
explanation), `PolicyGate` (Free/Pro, fails closed, REQUIRES_CONFIRM
no-silent-downgrade), `AudioSessionPolicy` (`[audio, screen-capture]`); (2)
`WDRiOSEmitterApp` — SwiftUI shell (persistent top-level Free/Pro toggle
FR-040/FR-048, FR-052 explain-before-prompt then the **system picker**
`RPSystemBroadcastPickerView` / `SCContentSharingPicker` 27+ conditional, system
recording-indicator honesty row, FR-053 status panel, Palette AA >= 4.5:1,
a11y modifiers FR-056); (3) `WDRiOSEmitterBroadcast` — an honest broadcast-
extension scaffold: a compile-checked `RPBroadcastSampleHandler` subclass + plist
declarations + the exact Xcode app+extension build runbook, stated plainly as
requiring an app+extension embedding, App Group IPC and a physical device; (4)
`store-compliance.md` — the App Review 2.5.14 record; (5) README + build-check.md.

**On this host:** `swift test` = 50/50 pass, exit 0; the same core sources
compile/emit against the iPhoneSimulator 26.5 SDK (exit 0, also with
`-warnings-as-errors`) and link into a simulator executable; the SwiftUI app
type-checks with `-warnings-as-errors`; the broadcast-handler file type-checks
with `-warnings-as-errors` (ReplayKit resolves in the 26.5 SDK — both real
fixes recorded). No simulator runtime and no device are installed, and
**ScreenCaptureKit does not exist in the 26.5 SDKs at all**, so ReplayKit
broadcast runtime, per-other-app capture, protected-content silence, the SCK
picker (27+) and App Review are explicitly **device/store gates** — never claimed
validated here, and never claimed "the extension broadcasts".

## Files changed

| Path | Change |
|---|---|
| `platform/ios-emitter/Package.swift` | SPM package `WDR-Emitter`: `WDRiOSEmitterCore` + `WDRiOSEmitterCoreTests` (macOS-runnable; App/broadcast targets intentionally excluded) |
| `platform/ios-emitter/.gitignore` | Local ignores (`.build/`, scratch xcodeproj) |
| `platform/ios-emitter/Sources/WDRiOSEmitterCore/CapturePolicy.swift` | Honest capability matrix: system bus UNCAPTURABLE; selfTap 12+ / ReplayKit 12–26 / SCK 27+; `acceptableCaptureModes(forOSMajor:)`, `primaryOtherAppMode`, honesty strings (system-picker consent, system red indicator, no-silent-background, protected-content) |
| `platform/ios-emitter/Sources/WDRiOSEmitterCore/EmissionStatus.swift` | FR-053 emitter telemetry + FR-055 redacted export + `systemIndicatorVisible` honesty |
| `platform/ios-emitter/Sources/WDRiOSEmitterCore/ConsentFlowState.swift` | FR-052 consent FSM (struct + `transition(to:denial:)`, redacted `ConsentDenial`, rejection of illegal transitions) |
| `platform/ios-emitter/Sources/WDRiOSEmitterCore/PolicyGate.swift` | Free/Pro gate (mirror of receiver + android emitter): `allowsLosslessCapture()`, fails closed, REQUIRES_CONFIRM |
| `platform/ios-emitter/Sources/WDRiOSEmitterCore/AudioSessionPolicy.swift` | `.playback`/`.playAndRecord` + `UIBackgroundModes [audio, screen-capture]` declaration set |
| `platform/ios-emitter/Tests/WDRiOSEmitterCoreTests/CapturePolicyTests.swift` | OS-version windows, orderings, system-bus honesty, picker-only consent, honesty strings (19) |
| `platform/ios-emitter/Tests/WDRiOSEmitterCoreTests/EmissionStatusTests.swift` | FR-053 fields + FR-055 redaction + indicator honesty (8) |
| `platform/ios-emitter/Tests/WDRiOSEmitterCoreTests/ConsentFlowStateTests.swift` | FR-052 transitions/invariants + redacted denials + rejection coverage (14) |
| `platform/ios-emitter/Tests/WDRiOSEmitterCoreTests/PolicyGateTests.swift` | Free/Pro/fail-closed/REQUIRES_CONFIRM (9) |
| `platform/ios-emitter/Sources/WDRiOSEmitterApp/WDRiOSEmitterApp.swift` | `@main` SwiftUI entry |
| `platform/ios-emitter/Sources/WDRiOSEmitterApp/EmissionModel.swift` | ObservableObject wiring core→UI (tier/consent/status) + device-only `systemAuthorized`/`systemDenied` seams |
| `platform/ios-emitter/Sources/WDRiOSEmitterApp/ContentView.swift` | Persistent Free/Pro toggle + FR-052 → system-picker capture flow + indicator row + FR-047 confirmation, a11y (FR-056) |
| `platform/ios-emitter/Sources/WDRiOSEmitterApp/ConsentExplainerView.swift` | FR-052 explain-before-prompt (copy from core honesty strings) |
| `platform/ios-emitter/Sources/WDRiOSEmitterApp/CaptureStatusView.swift` | FR-053 panel (demo telemetry; redacted) |
| `platform/ios-emitter/Sources/WDRiOSEmitterApp/SystemBroadcastPicker.swift` | `RPSystemBroadcastPickerView` wrapper (a11y single-button element) + `#if canImport(ScreenCaptureKit)` SCK scaffold + honest fallback |
| `platform/ios-emitter/Sources/WDRiOSEmitterApp/Palette.swift` | Contrast-safe palette (15 pairs >= 4.5:1) |
| `platform/ios-emitter/Sources/WDRiOSEmitterApp/Info.plist` | UIBackgroundModes [audio, screen-capture] + NSLocalNetworkUsageDescription + NSBonjourServices [_wdr._tcp] (+ broadcast-extension-owns-its-plist comment) |
| `platform/ios-emitter/Sources/WDRiOSEmitterBroadcast/BroadcastSampleHandler.swift` | Compile-checked `RPBroadcastSampleHandler` subclass: audio sample-buffer broker → null `AudioFrameSink` seam; video/mic deliberately ignored |
| `platform/ios-emitter/Sources/WDRiOSEmitterBroadcast/BroadcastExtension-Info.plist` | broadcast-upload-extension declarations (`com.apple.broadcast-services-upload`, `RPBroadcastProcessModeSampleBuffer`) + honesty comments |
| `platform/ios-emitter/README.md` | Rationale, capture design, FR-052→picker flow, device runbook + extension build runbook, honest limits, a11y, tests |
| `platform/ios-emitter/build-check.md` | Exact on-host commands + results, symbol table, device-gated table, contrast table |
| `platform/ios-emitter/store-compliance.md` | App Review 2.5.14 compliance record (consent/indicator/background/protected) |
| `docs/orchestration/reports/t-B4-ios-emitter.md` | This report |

## Decisions

1. **Core stays pure Foundation** so the same sources are (a) unit-tested on this
   macOS host with `swift test` and (b) type-checked/emitted against the iOS 14.0
   iphonesimulator SDK in one honest command; all iOS-only runtime (system picker,
   RPBroadcastController callbacks, AVAudioSession) lives in the App/broadcast
   layer — exactly the split the receiver shell established.
2. **The capture truth is encoded as a tested capability matrix**
   (`CapturePolicy`): system bus = UNCAPTURABLE (no mode can represent it; a test
   pins `CaptureMode` to exactly three and `CaptureScope` to two); selfTap 12+,
   ReplayKit 12–26, SCK 27+; `acceptableCaptureModes(forOSMajor:)` and
   `primaryOtherAppMode` drive what the UI may offer. The UI can never present a
   mode the matrix forbids.
3. **Consent ALWAYS goes through the SYSTEM picker (App Review 2.5.14).** The
   shell embeds `RPSystemBroadcastPickerView` (12–26) or presents
   `SCContentSharingPicker` (27+, `#if canImport(ScreenCaptureKit)` — not compiled
   on this 26.5 SDK), and never starts a broadcast programmatically for other
   apps' audio. `ConsentFlowState` enforces `.authorized` unreachable without the
   FR-052 explanation, unit-tested.
4. **The recording indicator is the SYSTEM's red one, always.** The app draws no
   fake indicator; `EmissionStatus.systemIndicatorVisible` reports it honestly
   (a self-session tap has no system overlay and the UI says so).
5. **No silent background capture.** Every mode requires a visible foreground
   start (`requiresForegroundStart == true`); `UIBackgroundModes [audio,
   screen-capture]` only allow an ACTIVE session to continue while backgrounded.
6. **PolicyGate mirrors the receiver/android semantics**: Fails closed on unknown
   tier (no lossless capture); PRO→FREE mid-lossless-capture returns
   `.requiresConfirm` and never applies silently (FR-026/FR-047).
7. **FR-055 redaction is real:** `EmissionStatus.redactedExport()`/`summaryLine()`
   never include the raw capture source label; scope is coarse (`self`/`other-app`).
8. **Broadcast extension shipped as an honest scaffold, not an unverified
   binary:** the compile-checked handler + plist declarations + exact Xcode
   app+extension build runbook (App Group IPC, `preferredExtension`, device
   requirement) are in README/build-check; the `AudioFrameSink` seam is null —
   nothing leaves the device until the transport task wires it.
9. **A11y is shipped, not prose (FR-056):** labels/values/hints on every control
   (incl. the system picker exposed as ONE labelled button — its internal button
   is otherwise unreachable), Dynamic Type, Reduce Motion honoured, non-color
   status (shape+label), palette >= 4.5:1 (15 measured pairs).
10. **Free/Pro toggle is persistent at the top of the shell UI** (FR-040/FR-048),
    documented as a dev/demo switch, not tamper-resistant enforcement (FR-045).
11. **ScreenCaptureKit is honestly flagged for a newer SDK:** the 26.5 SDKs lack
    the framework entirely, so the SCK branch is `#if canImport(ScreenCaptureKit)`
    — it cannot be compile-verified on this host (`ios-27-sdk` gate), recorded in
    build-check.md.

## Commands run (this host, exact)

```bash
# ---- Core unit tests (macOS SPM package; pure Foundation) ----
cd platform/ios-emitter
swift test
# exit 0 · 50 tests, 0 failures (CapturePolicy 19 / ConsentFlowState 14 / EmissionStatus 8 / PolicyGate 9)

# ---- iOS simulator-SDK compile of the core (primary iOS gate) ----
mkdir -p /tmp/wdriosem
xcrun -sdk iphonesimulator swiftc -target arm64-apple-ios14.0-simulator \
  -parse-as-library -emit-module -emit-library -module-name WDRiOSEmitterCore \
  -emit-module-path /tmp/wdriosem/WDRiOSEmitterCore.swiftmodule \
  -emit-library -o /tmp/wdriosem/WDRiOSEmitterCore.a Sources/WDRiOSEmitterCore/*.swift
# exit 0 (212 KB .a); also exit 0 with -warnings-as-errors

# ---- App target type-check (SwiftUI + ReplayKit, -warnings-as-errors) ----
xcrun -sdk iphonesimulator swiftc -target arm64-apple-ios14.0-simulator \
  -parse-as-library -typecheck -warnings-as-errors -I /tmp/wdriosem \
  Sources/WDRiOSEmitterApp/*.swift
# exit 0, 0 diagnostics

# ---- Broadcast-extension handler type-check (-warnings-as-errors) ----
xcrun -sdk iphonesimulator swiftc -target arm64-apple-ios14.0-simulator \
  -parse-as-library -typecheck -warnings-as-errors \
  Sources/WDRiOSEmitterBroadcast/BroadcastSampleHandler.swift
# exit 0 (ReplayKit resolves in the 26.5 SDK)

# ---- Link smoke: core consumed by an iOS-simulator executable ----
xcrun -sdk iphonesimulator swiftc -target arm64-apple-ios14.0-simulator \
  -I /tmp/wdriosem -o /tmp/wdriosem/smoke_em \
  /tmp/wdriosem/smoke_main.swift /tmp/wdriosem/WDRiOSEmitterCore.a
# exit 0; Mach-O 64-bit executable (86 KB)

# ---- Environment facts + symbol availability ----
xcodebuild -version        # Xcode 26.6 · Build 17F113
swift --version            # Swift 6.3.3
xcrun simctl list runtimes # EMPTY — no simulator runtime on this host
grep RPBroadcastSampleHandler / SDK/ReplayKit/Headers/RPBroadcastExtension.h   # iOS 10.0+
grep RPSystemBroadcastPickerView / SDK/ReplayKit/Headers/RPBroadcast.h          # iOS 12.0+
grep -r SCContentSharingPicker / SDK/**/ScreenCaptureKit.framework               # NOT PRESENT in 26.5 iOS/iOS-sim SDKs
# xcodebuild app build NOT run: no committed .xcodeproj; xcodegen not installed (build-check.md)
```

## Validation results

- `swift test`: **50/50 pass, exit 0** (macOS, core logic): CapturePolicy 19 ·
  ConsentFlowState 14 · EmissionStatus 8 · PolicyGate 9.
- Core vs iPhoneSimulator 26.5 SDK (target `arm64-apple-ios14.0-simulator`):
  module + static library emitted (212 KB, exit 0; also `-warnings-as-errors`);
  linked into an arm64 simulator executable (exit 0).
- App layer type-check: exit 0 with `-warnings-as-errors` (0 diagnostics).
- Broadcast-handler type-check: exit 0 with `-warnings-as-errors`. Two real
  compile fixes required and documented: override name
  `finishBroadcastWithError(_:)` (selector-preserved, not `finishBroadcast(with:)`)
  and `CFRelease` unavailable under ARC-managed Core Foundation (now
  `withExtendedLifetime`).
- Contrast: all 15 palette pairs >= 4.5:1 (min 4.66; WCAG relative luminance on
  this host; table in build-check.md).
- SDK facts: `RPBroadcastSampleHandler` iOS 10.0+, `RPSystemBroadcastPickerView`
  iOS 12.0+, `installTap` iOS 8+, `.playAndRecord` present — valid under the
  ADR-008 iOS 14 floor. **ScreenCaptureKit absent from both 26.5 SDKs** → SCK is
  `ios-27-sdk` + device.
- **Not verifiable here (device gates):** ReplayKit broadcast runtime + picker UX,
  per-other-app capture, protected-content silence, indicator transitions, SCK
  picker, local-network TCC prompt, App Review behaviour.
- **Not available here:** no simulator runtime (`simctl list runtimes` empty).

## Acceptance criteria

| Criterion (task brief) | Status | Evidence |
|---|---|---|
| Project tree under `platform/ios-emitter` complete (Package/core/app/broadcast/tests/Info.plists/README/build-check/store-compliance) | ✅ | Files changed table |
| `swift test` green (core macOS-runnable, Foundation/AVFoundation-safe imports) | ✅ | 50 tests, 0 failures, exit 0 |
| Core compiles for the iOS simulator SDK (exact swiftc command) | ✅ | exit 0 + 212 KB `.a` + module; `-warnings-as-errors` clean; link smoke exit 0 |
| `CapturePolicy`: honest capability matrix (system bus UNCAPTURABLE; selfTap 12+ / ReplayKit 12–26 / SCK 27+); `acceptableCaptureMode(forOS:)`; protected-content/background honesty strings | ✅ | CapturePolicyTests (19); PLATFORM_MATRIX §A iOS |
| `EmissionStatus`: FR-053 fields + FR-055 redacted export (no raw peer/id/name/audio) | ✅ | EmissionStatusTests (8) |
| `ConsentFlowState`: FR-052 `.needsExplanation → .awaitingSystemPicker → .authorized/.denied` with redacted reason; system picker = device-only; transitions unit-tested | ✅ | ConsentFlowStateTests (14) |
| `PolicyGate`: Free/Pro, fails closed on unknown, REQUIRES_CONFIRM no-silent-downgrade | ✅ | PolicyGateTests (9) |
| App layer compiles against iPhoneSimulator SDK with `-warnings-as-errors`; persistent Free/Pro toggle; FR-052 explain-then-SYSTEM-picker start (RP picker, no private capture); SCK 27+ conditional; system-indicator-is-system's row; a11y modifiers (FR-056) | ✅ | exit 0 (`-warnings-as-errors`); ContentView/ConsentExplainerView/SystemBroadcastPicker/CaptureStatusView |
| `Info.plist`: NSLocalNetworkUsageDescription + NSBonjourServices [_wdr._tcp] + comment that the broadcast extension carries its own plist | ✅ | Info.plist |
| Broadcast-extension scaffold: compile-checked `RPBroadcastSampleHandler` file (ReplayKit resolves) + plist declarations + README runbook stating app+extension embedding, App Group IPC, physical device | ✅ | BroadcastSampleHandler.swift exit 0; BroadcastExtension-Info.plist; README §Broadcast-extension build runbook |
| App-Review compliance record (2.5.14): system-picker consent ✓ · system red indicator ✓ · no silent background ✓ · protected content excluded ✓ · foreground requirement ✓ | ✅ | store-compliance.md (with honest device/store tags) |
| t-B4 report with worker-return fields | ✅ | This report |
| No write outside `platform/ios-emitter/**` + the report | ✅ | `git status` clean except `platform/ios-emitter/` |
| ReplayKit broadcast runtime (picker → extension captures), per-other-app capture | 🔒 PENDING `ios-replaykit`/`ios-device` | runbook in README; not claimed |
| SCK `SCContentSharingPicker` (iOS 27+) | 🔒 PENDING `ios-27-sdk` (missing from 26.5 SDKs) + `ios-device` | `#if canImport(ScreenCaptureKit)` scaffold |
| Protected-content silence / indicator transitions / TCC prompt / App Review | 🔒 PENDING device/store | device runbooks + store-compliance.md |
| Frames leaving the device (transport) | 🔲 by design | `AudioFrameSink` null; transport task |

## Risks / limitations

- **No simulator runtime / no device → all runtime behaviour is device-gated**
  (HARDWARE_VALIDATION iOS row). The code routes ONLY through public
  AVAudioSession/ReplayKit APIs; `RPSystemBroadcastPickerView` +
  `RPBroadcastSampleHandler` availability is compile-verified against the 26.5
  SDK headers.
- **ScreenCaptureKit cannot even be compile-checked on this host** — the 26.5 SDK
  ships no ScreenCaptureKit.framework (iOS 27+). The SCK branch is behind
  `#if canImport(ScreenCaptureKit)`; until it is built against an iOS 27+ SDK and
  run on a device it is a documented scaffold, never a validated capture path.
- **A Broadcast Upload Extension needs an Xcode app+extension embedding** (App
  Group IPC, `preferredExtension` bundle id, device run); this host delivers
  source + plist + runbook, not a runnable binary — stated plainly in README.
  `showsMicrophoneButton = false` and no `NSMicrophoneUsageDescription` keep the
  mic surface honest (no mic consent in this shell).
- **Health/telemetry values are demo-simulated** until the transport task wires
  real capture telemetry — recorded honestly in README/build-check.
- **`xcodebuild` not run** (no `.xcodeproj` committed; `xcodegen` absent). The
  simulator-SDK type-checks + test pass is the agreed honest compile gate; a
  generated-project simulator build (`CODE_SIGNING_ALLOWED=NO`) is a follow-up for
  a CI macOS/iOS runner; the extension adds the app+extension project requirement.
- **Info.plists are source-only** — a real app/extension bundle build must copy
  them (Xcode build phases).
- **ReplayKit/SCK capture is best-effort per-app by design:** not every app's
  audio is capturable; the OS decides. The shell never claims otherwise.

## Follow-up tasks

- `ios-replaykit`/`ios-device` gates: build the app+extension Xcode project per
  the README runbook on a device; verify picker UX, red indicator transitions,
  per-other-app capture, protected-content silence, no-silent-background start;
  record results in HARDWARE_VALIDATION.
- `ios-27-sdk` gate: rebuild against an iOS 27+ SDK to import ScreenCaptureKit and
  compile-check/resolve the `SCContentSharingPicker` branch, then device-verify
  on iOS 27+.
- Wire `AudioFrameSink` to the emitter core (encode → AEAD → LAN) — the
  integration boundary; replace demo FR-053 telemetry with real capture counts.
- On a macOS/iOS CI runner: generate an `.xcodeproj` (e.g. via `xcodegen`) and run
  a simulator `xcodebuild` (`CODE_SIGNING_ALLOWED=NO`); copy Info.plists
  correctly; add the broadcast extension target.
- FR-056 shell a11y acceptance (B6): device VoiceOver + Voice Control walkthrough
  for the system-picker flow + status panel.

## Blockers

- None blocking *delivery* of source + docs. On-host validation was executable
  (toolchain present). Device evidence (ReplayKit/SCK runtime, per-other-app
  capture, TCC, App Review) is blocked on hardware/os-version/store — that is why
  the `ios-replaykit` / `ios-device` / `ios-27-sdk` / store gates own those
  claims, not a source defect. ScreenCaptureKit additionally needs an iOS 27+ SDK
  (absent here) before it is even compilable.
