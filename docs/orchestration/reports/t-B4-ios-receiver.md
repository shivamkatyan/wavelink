# t-B4-ios-receiver — iOS Receiver Shell: SwiftUI + AVAudioEngine, `.usbAudio` reporting, Free/Pro gate, honest fidelity, a11y semantics

**task_id:** t-B4-ios-receiver · **root_task_id:** B4-ios-receiver ·
**hypothesis_id:** H-B4-IOS-1 · **owner_role:** Platform Implementer - iOS ·
**date:** 2026-09-10 · **status:** complete (on-host validation done; device gates pending — see acceptance table)

## Summary

Standing up the iOS **receiver** shell (`platform/ios-receiver`) as the Swift
counterpart of the Android receiver: (1) `WDRReceiverCore` — a pure-Foundation
core (no UIKit/SwiftUI/AVFAudio/AudioToolbox imports) that is unit-tested on this
macOS host AND type-checks against the iOS 14.0 iphonesimulator SDK with the very
same sources: `PolicyGate` (Free/Pro, `allowsLossless()`, fails closed on unknown,
REQUIRES_CONFIRM no-silent-downgrade), `FidelityStatus` (honest ladder:
lossy → output-path-converted → output-path-unverified → bit-perfect; bit-perfect
reachable ONLY via a hardware-loopback token, ADR-005), `StatusModel` (FR-053
fields + FR-055 redacted export), `AudioSessionPolicy` (enum-safe `.playback` +
`UIBackgroundModes [audio]`), `RouteSnapshot` (`.usbAudio` detection + FR-015
route-change classification per the Android `AudioOutputRouter` role, pure logic);
(2) `WDRReceiverApp` — SwiftUI shell (ContentView with the persistent top-level
Free/Pro toggle FR-040/FR-048, output/buffer/fidelity sections; SessionView FR-053;
PermissionExplainerView FR-052; OutputRouter reporting `AVAudioSession.currentRoute`
— NEVER forcing a DAC; Renderer = AVAudioEngine player + USBAudioPlayer; Palette
AA >= 4.5:1; a11y modifiers throughout incl. Reduce Motion + Dynamic Type +
non-color status indicators per FR-056); (3) `Info.plist` with background audio +
local-network TCC + `_wdr._tcp` Bonjour declarations; (4) 43 unit tests; (5)
README + build-check.md.

**On this host:** `swift test` = 43/43 pass, exit 0; core emits a static library
and links into an executable against the iPhoneSimulator 26.5 SDK (deployment
target 14.0); the SwiftUI app type-checks with `-warnings-as-errors`. No iOS
simulator *runtime* is installed and no physical device is attached, so
`.usbAudio` DAC output, local-network TCC, and background-on-locked-device are
explicitly **PENDING device gates** (runbook in README + HARDWARE_VALIDATION iOS
row). Honest boundary, never claimed validated here.

## Files changed

| Path | Change |
|---|---|
| `platform/ios-receiver/Package.swift` | SPM package: `WDRReceiverCore` + `WDRReceiverCoreTests` (macOS-runnable; app target intentionally excluded) |
| `platform/ios-receiver/.gitignore` | Local ignores (`.build/`, scratch xcodeproj) |
| `platform/ios-receiver/Sources/WDRReceiverCore/PolicyGate.swift` | Entitlement toggle (Free/Pro, fails-closed, no silent downgrade) |
| `platform/ios-receiver/Sources/WDRReceiverCore/FidelityStatus.swift` | Fidelity ladder + loopback-token-gated bit-perfect |
| `platform/ios-receiver/Sources/WDRReceiverCore/StatusModel.swift` | FR-053 status + FR-055 redaction |
| `platform/ios-receiver/Sources/WDRReceiverCore/AudioSessionPolicy.swift` | `.playback` category + background-modes declaration |
| `platform/ios-receiver/Sources/WDRReceiverCore/RouteSnapshot.swift` | Output-route report + `.usbAudio` detection/classification |
| `platform/ios-receiver/Tests/WDRReceiverCoreTests/PolicyGateTests.swift` | Free-reject/Pro-allow/unknown-fails-closed/no-silent-downgrade (13) |
| `platform/ios-receiver/Tests/WDRReceiverCoreTests/FidelityStatusTests.swift` | Ladder transitions + token gate (9) |
| `platform/ios-receiver/Tests/WDRReceiverCoreTests/StatusModelTests.swift` | FR-053 fields + redaction (7) |
| `platform/ios-receiver/Tests/WDRReceiverCoreTests/RouteSnapshotTests.swift` | `.usbAudio` detection + route-change FSM (14) |
| `platform/ios-receiver/Sources/WDRReceiverApp/WDRReceiverApp.swift` | `@main` SwiftUI entry |
| `platform/ios-receiver/Sources/WDRReceiverApp/ReceiverModel.swift` | ObservableObject wiring core→UI (tier/session/route/renderer) |
| `platform/ios-receiver/Sources/WDRReceiverApp/ContentView.swift` | Role/output/buffer/fidelity sections + persistent Free/Pro toggle |
| `platform/ios-receiver/Sources/WDRReceiverApp/SessionView.swift` | FR-053 stream-health panel |
| `platform/ios-receiver/Sources/WDRReceiverApp/PermissionExplainerView.swift` | FR-052 why-this-permission before the prompt |
| `platform/ios-receiver/Sources/WDRReceiverApp/OutputRouter.swift` | AVAudioSession.currentRoute reporting + route-change observation (no force) |
| `platform/ios-receiver/Sources/WDRReceiverApp/Renderer.swift` | AVAudioEngine attach/connect/start + USBAudioPlayer |
| `platform/ios-receiver/Sources/WDRReceiverApp/Palette.swift` | Contrast-safe palette (documented ratios) |
| `platform/ios-receiver/Sources/WDRReceiverApp/Info.plist` | UIBackgroundModes [audio] + NSLocalNetworkUsageDescription + NSBonjourServices [_wdr._tcp] |
| `platform/ios-receiver/README.md` | Role/runbook/limits/test summary (mirrors android-receiver README style) |
| `platform/ios-receiver/build-check.md` | Exact on-host commands + device-gated split + contrast table |
| `docs/orchestration/reports/t-B4-ios-receiver.md` | This report |

## Decisions

1. **Core stays pure Foundation (no UIKit/AVFAudio/AudioToolbox),** so the *same*
   core sources are (a) unit-tested on this macOS host with `swift test` and (b)
   type-checked/emitted against the iOS 14.0 iphonesimulator SDK in one honest
   command. The AVAudioSession→model mapping lives in the App layer
   (`OutputRouter.swift`), exactly mirroring the Android split (pure
   `OutputDeviceInfo`/`choosePreferredDevice` vs the `android.media`-bound
   `AudioOutputRouter`).
2. **App target is NOT an SPM target** (it imports SwiftUI + AVFoundation
   AVAudioSession/AVAudioEngine, iOS-only runtime). It is type-checked against
   the simulator SDK instead; `swift test` stays macOS-green. Documented in
   Package.swift and build-check.md.
3. **`.usbAudio` is detected/reported, never claimed forceable** (PLATFORM_MATRIX
   §A iOS: system auto-selects; cannot force). `OutputRouter` reports
   `currentRoute`; `USBAudioPlayer` reflects the route and pauses on NO_OUTPUT.
4. **Fidelity ladder per ADR-005/FR-022/FR-024:** `lossyTransport` →
   `losslessTransportOutputPathConverted` (terminal honest cap, never
   bit-perfect) → `losslessTransportOutputPathUnverified` → `bitPerfectVerified`
   **reachable only via `LoopbackVerificationToken.hardwareLoopbackGate(source:)`**;
   a known-converting path rejects bit-perfect even with a token; observed
   conversion demotes bit-perfect honestly.
5. **PolicyGate mirrors the Android REQUIRES_CONFIRM semantics**: unknown/
   deserialised tier fails closed to Free (no lossless); PRO→FREE mid-lossless
   returns `.requiresConfirm` and never applies silently (FR-026/FR-047).
6. **StatusModel redaction is real (FR-055):** `redactedExport()` and
   `summaryLine()` never include the peer identity or raw device names; route is
   coarse-classified (`usb-audio`/`built-in-other`/`none`).
7. **A11y is shipped, not prose (FR-056):** `.accessibilityLabel/.accessibilityValue/
   .accessibilityHint` on every control, `.isHeader` sections, VoiceOver-combined
   status badges (shape+label, never colour alone), `@Environment(
   .accessibilityReduceMotion)` gating animations, system text styles for Dynamic
   Type, and a palette whose every pair was measured >= 4.5:1 (table in
   build-check.md).
8. **Free/Pro toggle is persistent at the top of the shell UI** (FR-040/FR-048);
   documented as a dev/demo switch, not tamper-resistant enforcement (FR-045;
   commerce adapter is a separate later project per FR-044).

## Commands run (this host, exact)

```bash
# ---- Core unit tests (macOS SPM package; pure Foundation) ----
cd platform/ios-receiver
swift test
# exit 0 · 43 tests, 0 failures (PolicyGate 13 / Fidelity 9 / RouteSnapshot 14 / StatusModel 7)

# ---- iOS simulator-SDK compile gate for the core (primary gate) ----
xcrun -sdk iphonesimulator swiftc \
  -target arm64-apple-ios14.0-simulator \
  -parse-as-library -emit-library \
  -o /tmp/WDRReceiverCore.a Sources/WDRReceiverCore/*.swift
# exit 0 (also clean with -warnings-as-errors)

# ---- Core as an iOS-simulator module (feeds app type-check) ----
mkdir -p /tmp/wdrbuild
xcrun -sdk iphonesimulator swiftc -target arm64-apple-ios14.0-simulator \
  -parse-as-library -emit-module -emit-library -module-name WDRReceiverCore \
  -emit-module-path /tmp/wdrbuild/WDRReceiverCore.swiftmodule \
  -emit-library -o /tmp/wdrbuild/WDRReceiverCore.a Sources/WDRReceiverCore/*.swift
# exit 0

# ---- App target type-check (SwiftUI + AVAudioSession against the sim SDK) ----
xcrun -sdk iphonesimulator swiftc -target arm64-apple-ios14.0-simulator \
  -parse-as-library -typecheck -I /tmp/wdrbuild \
  Sources/WDRReceiverApp/*.swift
# exit 0 (also clean with -warnings-as-errors)

# ---- Link smoke: core consumed by an iOS-simulator executable ----
xcrun -sdk iphonesimulator swiftc -target arm64-apple-ios14.0-simulator \
  -I /tmp/wdrbuild -o /tmp/wdrbuild/smoke_app \
  /tmp/wdrbuild/smoke_main.swift /tmp/wdrbuild/WDRReceiverCore.a
# exit 0; Mach-O 64-bit executable arm64 produced

# ---- Environment facts recorded ----
uname -m                      # arm64
xcodebuild -version           # Xcode 26.6 (Build 17F113)
swift --version               # Swift 6.3.3
xcrun -sdk iphonesimulator --show-sdk-path   # iPhoneSimulator26.5.sdk
xcrun simctl list runtimes    # EMPTY — no simulator runtime on this host (documented)
# xcodebuild NOT run: no committed .xcodeproj; xcodegen not installed (optional gate)
```

## Validation results

- `swift test`: **43/43 pass, exit 0** (macOS, core logic).
- Core vs iPhoneSimulator 26.5 SDK (target `arm64-apple-ios14.0-simulator`):
  type-check exit 0; static library emitted (236 KB); module emitted for
  `-I` consumption; linked into an arm64 simulator executable (exit 0).
- App target type-check: exit 0, also with `-warnings-as-errors` (0 diagnostics).
- Contrast: all 17 recorded palette pairs >= 4.5:1 (WCAG relative luminance,
  computed on this host; table in build-check.md).
- Reported iOS availability of the API surface used (grep of
  AVAudioSessionTypes.h/AVAudioSessionRoute.h in iPhoneSimulator26.5.sdk):
  `AVAudioSessionPortUSBAudio` iOS 6.0+ (Swift `AVAudioSession.Port.usbAudio`),
  `currentRoute`, `routeChangeNotification` all < iOS 14 → valid under the
  ADR-008 iOS 14 floor.
- **Not verifiable here (device gates):** `.usbAudio` DAC audio, real hotplug,
  local-network TCC prompt, background/locked-device playback.
- **Not available here:** no simulator runtime (`simctl list runtimes` empty).

## Acceptance criteria

| Criterion (task brief) | Status | Evidence |
|---|---|---|
| Project tree under `platform/ios-receiver` complete (Package/core/app/tests/Info.plist/README/build-check) | ✅ | Files changed table |
| Core compiles for the iOS simulator SDK (exact swiftc command) | ✅ | exit 0 + 236 KB `.a` |
| `swift test` green (core macOS-runnable, Foundation/AVFoundation-safe imports) | ✅ | 43 tests, 0 failures, exit 0 |
| `PolicyGate`: Free refuses lossless / Pro allows / unknown fails closed / no silent downgrade | ✅ | PolicyGateTests (13) |
| `FidelityStatus`: honest ladder; bit-perfect ONLY via hardware-loopback token; converting path capped | ✅ | FidelityStatusTests (9); ADR-005 |
| `StatusModel`: FR-053 fields + redaction-friendly description | ✅ | StatusModelTests (7) |
| `RouteSnapshot`: `.usbAudio` detection + route-change representation (pure logic, fake snapshots) | ✅ | RouteSnapshotTests (14) |
| `OutputRouter` reports `.usbAudio` but NEVER claims to force a DAC | ✅ | OutputRouter.swift doc/HARD rule; build-check |
| `Renderer`: AVAudioEngine attach/connect/start + USBAudioPlayer reflecting route | ✅ | Renderer.swift; compiles iOS sim SDK |
| `Info.plist`: UIBackgroundModes [audio], NSLocalNetworkUsageDescription, NSBonjourServices [_wdr._tcp] | ✅ | Info.plist |
| FR-052 pre-prompt explanation view before local-network access | ✅ | PermissionExplainerView + ContentView sheet gating |
| A11y (FR-056): labels/values/hints, Dynamic Type, Reduce Motion, >=4.5:1 palette, non-color indicators | ✅ | SwiftUI modifiers throughout; Palette + measured table |
| Free/Pro toggle persistent top-level (FR-040/048) | ✅ | ContentView tier section |
| `.usbAudio` DAC output on a real device | 🔒 PENDING | `ios-device` gate; runbook in README + HARDWARE_VALIDATION |
| Local-network TCC prompt on a real device | 🔒 PENDING | `ios-device` gate |
| Background playback on a locked device | 🔒 PENDING | `ios-device` gate |
| Bit-perfect claim | 🔒 PENDING | `bit-perfect` hardware loopback gate (ADR-005); by-design unreachable from shell code |

## Risks / limitations

- **No simulator runtime / no device on this host** → UI runtime, TCC prompt,
  real DAC output, and locked-device background all remain device-gated
  (HARDWARE_VALIDATION iOS row). The code routes ONLY through public
  AVAudioSession/AVAudioEngine APIs; .usbAudio detection uses the verified
  `"USB-Audio"` port literal (iOS 6+ constant) — compile-verified against the
  26.5 SDK.
- **`xcodebuild` not run** (no `.xcodeproj` committed; `xcodegen` absent). The
  simulator-SDK type-check + test pass is the agreed honest compile gate;
  a generated-project build (simulator + `CODE_SIGNING_ALLOWED=NO`) is a
  follow-up for a CI macOS/iOS runner.
- **Health-panel values are demo-simulated** (latency/fill/loss/underruns) until
  the network-transport task wires real telemetry — recorded honestly in README.
- **No signing/provisioning** here (dev signing is a CI/device concern per
  DEVELOPMENT_ENVIRONMENT).
- **Info.plist is source-only here** — a real app bundle build (Xcode/xcodebuild)
  must copy it; documented in build-check.md.
- **`Lower` risk:** `OutputRouter` uses `AVAudioSession.currentRoute` — on the
  simulator (if one is later installed) the route is the host's; DAC behaviour
  still requires a device.

## Follow-up tasks

- `ios-device` gate: run the README device runbook (real DAC attach/detach/hotplug,
  local-network TCC allow/deny, locked-device background) and record results.
- Wire the network transport (mDNS `_wdr._tcp`, frames) into `Renderer`/model,
  replacing demo values with real FR-053 telemetry.
- On a macOS/iOS CI runner: generate an `.xcodeproj` (e.g. via `xcodegen`) and run
  a simulator `xcodebuild` (CODE_SIGNING_ALLOWED=NO); copy Info.plist correctly.
- `bit-perfect` gate: hardware loopback / USB-analyzer measurement, then mint the
  loopback token in a validation-only build path.
- FR-056 shell a11y acceptance (B6): device VoiceOver + Voice Control walkthrough
  and screenshot/Visual-Regression on representative sizes.

## Blockers

- No blocker to *delivery* of source + docs. On-host validation was executable
  (toolchain present). Device evidence (`.usbAudio` DUT output, local-network TCC,
  locked-device background) is blocked on physical hardware — that is the reason
  the `ios-device` / `bit-perfect` gates own those claims, not a source defect.
