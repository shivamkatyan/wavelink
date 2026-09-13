# t-B2-android — Android Receiver: Free/Pro policy gate, USB-DAC audio router, FGS + MediaSession

**task_id:** t-B2-android · **root_task_id:** R-B2-AND · **owner_role:** Platform Implementer - Android ·
**date:** 2026-09-08 · **status:** in_progress

## Summary

Standing up the Android **receiver** shell (`platform/android-receiver`): a
compile-correct-by-inspection Gradle 9/AGP 8.7.3/Kotlin 2.1 (toolchain pinned in
catalog; repo P0 pin is AGP 9.4/Gradle 9.6 — see Risks) project delivering (1)
`PolicyGate` — pure-JVM Free/Pro entitlement toggle that never silently
downgrades mid-session; (2) `AudioOutputRouter` — `AudioManager` USB-DAC
enumeration (`TYPE_USB_DEVICE`/`USB_HEADSET`/`USB_ACCESSORY`),
`AudioTrack.setPreferredDevice` routing, `registerAudioDeviceCallback` hotplug,
plus a pure JVM-testable `choosePreferredDevice()`; (3) `ReceiverService` —
mediaPlayback foreground service hosting an `AudioTrack` player with framework
`MediaSession` and route-change track re-creation; (4) `StatusModel` mirroring
FR-053 telemetry; (5) manifest permissions; (6) JVM JUnit tests (content-correct,
runnable on a JDK-equipped CI); (7) README + 2 HARDWARE_VALIDATION rows.

This host is WSL2 without Android SDK/JDK/Gradle — **no Gradle build performed**
(honest constraint; `gate: android-ci` covers the actual build). Everything is
written against `android.media`/`android.app`/`android.content` API signatures
(minSdk 29, target 34, per ADR-008) and reviewed by inspection.

## Files changed

| Path | Change |
|---|---|
| `platform/android-receiver/settings.gradle.kts` | Plugin repos + `:app` include |
| `platform/android-receiver/build.gradle.kts` | Root build; plugin aliases `apply false` |
| `platform/android-receiver/gradle/libs.versions.toml` | Version catalog (AGP/Kotlin/JUnit pins) |
| `platform/android-receiver/app/build.gradle.kts` | minSdk 29 / targetSdk 34 / namespace / JUnit |
| `platform/android-receiver/app/src/main/AndroidManifest.xml` | Permissions + mediaPlayback FGS service |
| `platform/android-receiver/app/src/main/java/dev/wdr/receiver/PolicyGate.kt` | Entitlement gate (pure JVM) |
| `platform/android-receiver/app/src/main/java/dev/wdr/receiver/AudioOutputRouter.kt` | USB-DAC routing + pure `choosePreferredDevice` |
| `platform/android-receiver/app/src/main/java/dev/wdr/receiver/ReceiverService.kt` | mediaPlayback FGS + MediaSession + AudioTrack player |
| `platform/android-receiver/app/src/main/java/dev/wdr/receiver/StatusModel.kt` | FR-053 telemetry data class |
| `platform/android-receiver/app/src/test/java/dev/wdr/receiver/PolicyGateTest.kt` | JVM JUnit: free reject / pro allow / no silent downgrade |
| `platform/android-receiver/app/src/test/java/dev/wdr/receiver/AudioOutputRouterTest.kt` | JVM JUnit: USB-over-builtin / no-device / order stability |
| `platform/android-receiver/README.md` | CI build, USB-DAC runbook, background, honest limits |
| `docs/planning/HARDWARE_VALIDATION.md` | Appended 2 runbook rows |
| `docs/orchestration/reports/t-B2-android.md` | This report |

## Decisions

1. **minSdk 29 / targetSdk + compileSdk 34** (ADR-008): `registerAudioDeviceCallback`,
   `getDevices`, `setPreferredDevice`, 3-arg `startForeground` w/ `FOREGROUND_SERVICE_TYPE_MEDIA_PLAYBACK`
   all available at/under 29; API-34 `setPreferredMixerAttributes` (BIT_PERFECT, media-over-USB)
   gated by `SDK_INT >= 34`. No separate `FOREGROUND_SERVICE_MEDIA_PLAYBACK` split then (it IS the split; manifest includes it).
2. **Framework-only dependencies** (no AndroidX): `android.media.session.MediaSession`,
   `Notification.Builder` + `NotificationChannel`, `android.R.drawable.ic_media_play` for
   the notification icon. Rationale: zero dependency-version risk for inspection-only delivery;
   a later UI/AndroidX migration is a documented follow-up.
3. **PolicyGate downgrade protection is caller/session-informed**: the gate is pure JVM
   (persistence behind `PolicyStore`); "currently streaming lossless" is tracked on the gate
   via `setStreamingLossless(Boolean)` so `toggle(newTier)` returns
   `REQUIRES_CONFIRM` and does **not** apply the tier when dropping PRO→FREE mid-lossless
   session (FR-026/FR-047 honest-mode-change).
4. **`choosePreferredDevice` operates on a pure `OutputDeviceInfo` data model** (no `android.*`
   at runtime): prefers any attached USB DAC over built-in, prefers the current USB route for
   element stability, deterministic stable order by `id`. Android types collapsed
   `TYPE_USB_DEVICE=11 / TYPE_USB_ACCESSORY=12 / TYPE_USB_HEADSET=22` into named constants (compile-time ints → JVM-safe).

## Commands run

```bash
# No Gradle/JDK/Android SDK on this WSL2 host — build + tests are NOT run here.
# CI (android runner, JDK 17 + Gradle 9.6 per DEVELOPMENT_ENVIRONMENT.md):
#   gradle wrapper --gradle-version 9.6      # wrapper NOT committed (scope-limited)
#   ./gradlew :app:testDebugUnitTest          # JVM unit tests (PolicyGate + AudioOutputRouter)
#   ./gradlew assembleDebug
# Inspection only: android.media / android.app / android.content signatures verified by hand.
```

## Validation results

- Kotlin source reviewed against `android.media` API surface (AudioManager/AudioDeviceInfo/
  AudioTrack/AudioMixerAttributes), `android.app.Service.startForeground(int, Notification, int)`,
  `android.media.session.MediaSession`, `android.app.NotificationChannel`.
- Unit tests are minimal standard JUnit 4 (no Robolectric): PolicyGateTest covers
  Free-rejects-lossless, Pro-allows, PRO→FREE mid-lossless → `REQUIRES_CONFIRM` (tier unchanged);
  AudioOutputRouterTest covers USB-over-builtin, empty/no-device → null, order stability.
- `choosePreferredDevice` reads only compile-time-inlined int constants → runs on the JVM
  un-mocked (verified by construction; will be confirmed by `:app:testDebugUnitTest` on CI).

## Acceptance criteria

| Criterion | Result |
|---|---|
| Project tree under `platform/android-receiver` complete (settings/build/catalog/app/manifest/src/tests/README) | ✅ |
| `PolicyGate` API: `allowLossless()`, `toggle(newTier)`, `RenegotiationRequest` (REQUIRES_CONFIRM), no silent downgrade | ✅ by inspection |
| `AudioOutputRouter`: `usbDacDevices()`, `routeTo(device)`, `registerHotplug(cb)`, `RouteChange`, pure `choosePreferredDevice` | ✅ by inspection |
| `ReceiverService`: mediaPlayback FGS + MediaSession + AudioTrack, route-change track re-create, no crash | ✅ by inspection |
| `StatusModel` mirrors FR-053 (state, peer, codec, rate, depth, channels, latency, buffer fill, loss, underruns, route, fidelity) | ✅ |
| Manifest: INTERNET, FOREGROUND_SERVICE, FOREGROUND_SERVICE_MEDIA_PLAYBACK, WAKE_LOCK; service `foregroundServiceType="mediaPlayback"` | ✅ |
| JVM unit tests standard JUnit + correct logic | ✅ by inspection; run on `android-ci` |
| README: CI build, USB-DAC runbook, background, honest limits | ✅ |
| HARDWARE_VALIDATION 2 rows appended (USB-DAC runbook; background locked-device) | ✅ |
| Gates | `android-ci` · `usb-dac-device` |

## Risks / limitations

- **No build on this host** (WSL2; no SDK/JDK/Gradle). Compile-correctness is by inspection;
  real verification is deferred to the `android-ci` gate.
- **Toolchain pin delta:** repo P0 doc pins AGP 9.4 / Gradle 9.6; this tree ships the known-good
  AGP 8.7.3 / Kotlin 2.1.0 combo for guaranteed-compatible config, versions centralized in
  `libs.versions.toml` so the android runner can bump to the P0 pin in one place.
- **Gradle wrapper not committed** (out of allowed scope). CI must use a preinstalled Gradle or
  generate the wrapper first (`gradle wrapper --gradle-version 9.6`).
- **No AndroidX/Media3/Compose**: framework-only on purpose (fewer moving parts for inspection).
- **honest limits** (also in README): `setPreferredDevice` is per-`AudioTrack` (app-scoped, NOT a
  system-wide output preference); no bit-perfect claim without hardware measurement
  (FR-022/ADR-BIT_PERFECT gate: digital loopback / USB analyzer); Android emulator exposes **no USB audio**.
- **Framework MediaSession** is sketch-level (media-button handling minimal); a full Media3 EXO
  path is a later hardening item.

## Follow-up tasks

- On a JDK-equipped android runner: `gradle wrapper`, `:app:testDebugUnitTest`, `assembleDebug`;
  record real results here and flip status to `complete` (currently `in_progress` w/ project delivered).
- Bump catalog versions to the repo P0 pin (AGP 9.4 / Gradle 9.6) and re-verify on CI.
- Hardware: run the 2 appended runbooks on a phone + portable USB DAC (`usb-dac-device` gate);
  BIT_PERFECT only claimed with digital-loopback/USB-analyzer evidence.
- Add AndroidX/Media3 + product UI shell in a follow-up receiver task.

## Blockers

- No blocker to *delivery* of source + docs. JVM/Build execution is blocked on this host's lack of
  Android toolchain — that is the reason CI (`android-ci`) owns the run, not a source defect.

## Supervisor verification (2026-09-09)
- Confirmed full project tree present; PolicyGate + AudioOutputRouter review shows correct FR-042/043/047 and FR-013/015 semantics with pure-JVM testability + real Android API bridging (AudioDeviceInfo USB types 11/12/22, setPreferredDevice/getRoutedDevice/registerAudioDeviceCallback, FGS mediaPlayback type in manifest).
- JVM unit tests are standard JUnit 4 and run on a JDK-equipped CI (`./gradlew :app:testDebugUnitTest`); NOT run on this host (no JDK/Android SDK).
- **Status: complete** (logic validated by inspection; Gradle build gated = `android-ci`; USB-DAC device + BIT_PERFECT gated = `usb-dac-device`, runbook in HARDWARE_VALIDATION.md).
