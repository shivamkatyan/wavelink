# t-B4-android-emitter — Android Emitter: app-audio capture shell (capture policy, MediaProjection consent, mediaProjection FGS)

**task_id:** t-B4-android-emitter · **root_task_id:** B4-android-emitter ·
**hypothesis_id:** H-B4-AND-1 · **owner_role:** Platform Implementer - Android ·
**date:** 2026-09-10 · **status:** complete (compile + JVM unit-tested on this
macOS host; device/emulator gates open)

## Summary

Delivered `platform/android-emitter` — a SEPARATE Android **emitter** (capture)
app shell that mirrors `platform/android-receiver`'s Gradle layout, version
catalog (AGP 8.7.3 / Kotlin 2.1.0 / JUnit 4.13.2), manifest style, package style
(`dev.wdr.emitter`) and pure-JVM test strategy, implementing permitted app-audio
capture per PLATFORM_MATRIX §A Android row: `AudioPlaybackCaptureConfiguration`
(opt-in USAGE_MEDIA/GAME/UNKNOWN mix, per-UID/usage filter) + per-session
`MediaProjection` consent (single-use token on API 34+) + a `mediaProjection`
foreground service (declared 34+; mediaPlayback-type fallback pre-34). Capture is
behind the `MediaProjectionCaptureAdapter` seam (`CaptureSource` analogue) with a
`FrameSink` integration point clearly left empty (no network transport).

Compiled and JVM unit-tested **on this macOS host**: `assembleDebug` and
`testDebugUnitTest` both BUILD SUCCESSFUL (24/24 tests, 0 failures). Three real
compile fixes were required and are documented (JVM_17 target string, wrong
MediaProjection package, mis-remembered `setAudioPlaybackCaptureConfig` name —
verified via `javap` against android.jar). Real per-app capture, the
MediaProjection consent UX, locked-device behavior and FGS lifecycle on API 34+
remain **device gates**; the emulator has no real capture.

## Files changed

| Path | Change |
|---|---|
| `platform/android-emitter/settings.gradle.kts` | Plugin repos + `:app` include; `rootProject.name = "android-emitter"` |
| `platform/android-emitter/build.gradle.kts` | Root build; plugin aliases `apply false` |
| `platform/android-emitter/gradle/libs.versions.toml` | Version catalog (same AGP 8.7.3 / Kotlin 2.1.0 / JUnit 4.13.2 pins as the receiver) |
| `platform/android-emitter/app/build.gradle.kts` | minSdk 29 / targetSdk + compileSdk 34 / namespace `dev.wdr.emitter` / applicationId `dev.wdr.emitter` / JUnit dep; **`jvmTarget = "17"`** (fixed vs receiver's broken `JvmTarget.JVM_17.toString()`) |
| `platform/android-emitter/app/src/main/AndroidManifest.xml` | `RECORD_AUDIO`, `FOREGROUND_SERVICE`, `FOREGROUND_SERVICE_MEDIA_PROJECTION` (API 34+ enforced); `EmitService` with `foregroundServiceType="mediaProjection"`; launcher `MainActivity` |
| `.../java/dev/wdr/emitter/PolicyGate.kt` | Pure-JVM Free/Pro gate (FR-040..FR-048) + **UNKNOWN fails closed** + `requiresConfirmForDowngrade(lossless→lossy)` (FR-047, receiver semantics) |
| `.../java/dev/wdr/emitter/CapturePolicy.kt` | Usage/UID allow-list model, pure `shouldInclude()`/`uidAllowed()`/`shouldCapture()`, `CaptureUsage` compile-time constants, `PlaybackCaptureConfigFactory` (builds `AudioPlaybackCaptureConfiguration`) |
| `.../java/dev/wdr/emitter/StatusModel.kt` | FR-053 fields + `statusLine()` (a11y, FR-056) + `redactedDescription()` (FR-055) + `VisualIndicator` |
| `.../java/dev/wdr/emitter/FakeSoundSource.kt` | Deterministic seeded PCM block generator (16/24-bit) for tests + pre-device demo |
| `.../java/dev/wdr/emitter/AudioCaptureAdapter.kt` | Seam: `MediaProjectionCaptureAdapter` → `AudioPlaybackCaptureConfiguration` + `AudioRecord` on a dedicated thread; `SourceStarted`/`Format`/`FrameMeta`/`FramesAvailable`; honest single-use-token / silenced-content comments |
| `.../java/dev/wdr/emitter/EmitService.kt` | mediaProjection FGS (mediaPlayback type pre-34), per-session consent, `FrameSink` seam (null), FR-053 status publication, `SharedPrefsPolicyStore` |
| `.../java/dev/wdr/emitter/MainActivity.kt` | Minimal framework-only UI: Free/Pro toggle (FR-040/048), FR-052 explain-before-prompt dialog, `createScreenCaptureIntent()` consent, status display with FR-056 a11y labels |
| `.../test/java/dev/wdr/emitter/PolicyGateTest.kt` | 9 JVM JUnit tests (free reject / pro allow / unknown fails closed / REQUIRES_CONFIRM / confirmed downgrade) |
| `.../test/java/dev/wdr/emitter/CapturePolicyTest.kt` | 7 JVM JUnit tests against `android.media.AudioAttributes` USAGE constants (inlined, no framework call) |
| `.../test/java/dev/wdr/emitter/StatusModelTest.kt` | 4 JVM JUnit tests: FR-055 redaction, FR-053 fields, FR-056 indicator |
| `.../test/java/dev/wdr/emitter/FakeSoundSourceTest.kt` | 4 JVM JUnit tests: determinism contract |
| `platform/android-emitter/README.md` | Rationale, app-audio capture design, consent runbook, honest limits, test list |
| `platform/android-emitter/build-check.md` | Exact host commands + results + compile-fix log + device/emulator gates |
| `platform/android-emitter/gradlew`/`gradlew.bat`/`gradle/wrapper/*` | Gradle 8.13 wrapper (agent-installed; verified on this host) |
| `platform/android-emitter/local.properties` | `sdk.dir=/opt/homebrew/share/android-commandlinetools` |
| `docs/orchestration/reports/t-B4-android-emitter.md` | This report |

## Decisions

1. **minSdk 29 / targetSdk + compileSdk 34, own applicationId `dev.wdr.emitter`**
   (ADR-008; PLATFORM_MATRIX Android row). `AudioPlaybackCaptureConfiguration` +
   `AudioRecord.Builder.setAudioPlaybackCaptureConfig` are API-29 floor;
   `foregroundServiceType="mediaProjection"` / single-use token /
   `FOREGROUND_SERVICE_MEDIA_PROJECTION` enforcement are API 34+ (gated by
   `SDK_INT >= 34`; pre-34 uses the mediaPlayback FGS type).
2. **Mirror the receiver's file pattern — pure decision + framework composition
   in one file**: `CapturePolicy.shouldInclude(usage)` / `uidAllowed(uid)` /
   `shouldCapture(uid, usage)` are framework-free and JVM-tested (USAGE constants
   as compile-time inlined ints; tests reference `android.media.AudioAttributes`
   USAGE constants only, no framework call). Only `PlaybackCaptureConfigFactory`
   touches `AudioPlaybackCaptureConfiguration`.
3. **PolicyGate mirrors the receiver exactly and adds "unknown fails closed"**:
   an unresolved `EntitlementTier.UNKNOWN` resolves to FREE for every gate
   decision (never silently grants Pro); `toggle()` surfaces
   `REQUIRES_CONFIRM` for PRO→FREE mid-lossless and leaves the tier unchanged;
   `requiresConfirmForDowngrade(requested)` is exposed as the named FR-047
   predicate. Persistence behind `PolicyStore` (Android impl = `SharedPrefsPolicyStore`),
   so the gate class remains pure JVM.
4. **Transport deliberately NOT implemented.** `EmitService` forwards captured PCM
   to a `FrameSink` seam (null here; blocks only counted for FR-053 status),
   marked as the INTEGRATION POINT for the core-wiring task (encode → AEAD →
   QUIC). No network code in this shell.
5. **`jvmTarget = "17"` instead of the receiver's `JvmTarget.JVM_17.toString()`**,
   which fails the Kotlin Gradle plugin ("Unknown Kotlin JVM target: JVM_17") —
   the receiver's config never compiled on its authoring host; fixed here and
   verified. (System Homebrew Gradle 9.7.1 and the agent-pinned Gradle 8.13
   wrapper both build after this fix.)
6. **Framework-only** (no AndroidX/Media3/Compose), same as the receiver — fewer
   moving parts for a capture shell; a UI/AAC migration is a follow-up.
7. **Volume of evidence is honest**: JVM compile + unit tests on this host are
   real; real per-app capture / consent UX / locked-device / FGS-on-34 are
   DEVICE GATES, and the emulator has no real capture.

## Commands run (2026-09-10, macOS host)

```bash
cd platform/android-emitter
export JAVA_HOME=/opt/homebrew/opt/openjdk@17/libexec/openjdk.jdk/Contents/Home
export ANDROID_HOME=/opt/homebrew/share/android-commandlinetools

# System Homebrew Gradle 9.7.1 (after the jvmTarget fix):
gradle --no-daemon :app:assembleDebug        # BUILD SUCCESSFUL in 9s (33 tasks)
gradle --no-daemon :app:testDebugUnitTest    # BUILD SUCCESSFUL in 8s (22 tasks)

# Clean rebuild + tests, explicit exit code:
gradle --no-daemon clean >/dev/null 2>&1
gradle --no-daemon :app:assembleDebug :app:testDebugUnitTest
#   GRADLE_EXIT_CODE=0 ; BUILD SUCCESSFUL in 9s

# Agent-installed Gradle 8.13 wrapper (primary reproducible path):
./gradlew --no-daemon :app:assembleDebug :app:testDebugUnitTest
#   BUILD SUCCESSFUL in 17s — 39 actionable tasks executed
#   WRAPPER_EXIT=0 ; testDebugUnitTest reran: 24 tests, 0 failures/errors/skips
```

First compile downloaded AGP/Kotlin from Google Maven (network; minutes).

## Validation results

- **`assembleDebug`: BUILD SUCCESSFUL** — exit 0 on system Gradle 9.7.1 and on the
  committed Gradle 8.13 wrapper (`./gradlew --no-daemon :app:assembleDebug :app:testDebugUnitTest`,
  17s, 39 tasks). Output: `app/build/outputs/apk/debug/app-debug.apk`.
- **`testDebugUnitTest`: BUILD SUCCESSFUL, 24/24 tests pass** — exit 0,
  0 failures / 0 errors / 0 skipped:

| Class | tests | skipped | failures | errors |
|---|---|---|---|---|
| `dev.wdr.emitter.CapturePolicyTest` | 7 | 0 | 0 | 0 |
| `dev.wdr.emitter.FakeSoundSourceTest` | 4 | 0 | 0 | 0 |
| `dev.wdr.emitter.PolicyGateTest` | 9 | 0 | 0 | 0 |
| `dev.wdr.emitter.StatusModelTest` | 4 | 0 | 0 | 0 |
| **Total** | **24** | **0** | **0** | **0** |

- **Compile fixes required (all real, all verified by a green build):**
  1. `jvmTarget = JvmTarget.JVM_17.toString()` → `"JVM_17"` rejected: fixed to
     `"17"`.
  2. `MediaProjection` is `android.media.projection.MediaProjection` — import
     fixed in `AudioCaptureAdapter.kt` / `CapturePolicy.kt`.
  3. `AudioRecord.Builder`'s method is `setAudioPlaybackCaptureConfig` (not
     `setAudioPlaybackCaptureConfiguration`) — confirmed via `javap` against
     `platforms/android-34/android.jar`, which also confirmed
     `AudioPlaybackCaptureConfiguration.Builder.addMatchingUsage(int)` /
     `addMatchingUid(int)` / `build()`.
- **Framework-light tests**: only `android.media.AudioAttributes` USAGE int
  constants are referenced from tests (compile-time inlined) — no Robolectric,
  plain JVM.

## Acceptance criteria

| Criterion | Status | Evidence |
|---|---|---|
| Emitter project tree complete under `platform/android-emitter` (settings/build/catalog/app/manifest/src/tests/README/build-check) | ✅ | Tree listed in Files changed |
| `assembleDebug` BUILD SUCCESSFUL on this host | ✅ | exit 0; system Gradle 9.7.1 + 8.13 wrapper; `app-debug.apk` |
| `testDebugUnitTest` all-passing | ✅ | 24/24, 0 fail/error/skip (test-results XML) |
| No write outside `platform/android-emitter/**` + the report | ✅ | git-verifyable; receiver/core/planning untouched |
| `PolicyGate`: Free rejects lossless, Pro allows, UNKNOWN fails closed, `requiresConfirmForDowngrade` (PRO→FREE mid-lossless) REQUIRES_CONFIRM with tier unchanged | ✅ | `PolicyGateTest` (9 tests, passing) |
| `CapturePolicy`: usage allow-list ⊆ {USAGE_MEDIA, GAME, UNKNOWN}, per-UID filter, `shouldInclude(usage)` pure + JVM-testable; builder produces `AudioPlaybackCaptureConfiguration` | ✅ | `CapturePolicyTest` (7 tests, passing); `PlaybackCaptureConfigFactory` compiled |
| `StatusModel`: FR-053 fields, FR-055 redacted description (no peer/secrets), a11y `statusLine()` + non-color `VisualIndicator` (FR-056) | ✅ | `StatusModelTest` (4 tests, passing) |
| `FakeSoundSource`: deterministic PCM generator | ✅ | `FakeSoundSourceTest` (4 tests, passing) |
| `MediaProjectionCaptureAdapter` seam: granted projection + allow list → config → `AudioRecord`; per-session consent comments; API 34+ single-use branch documented | ✅ | source compiled; honest comments |
| `EmitService`: mediaProjection FGS (34+) with mediaPlayback fallback (29–33), `FrameSink` integration point, FR-053 status publication | ✅ | source compiled; manifest declares `foregroundServiceType="mediaProjection"` |
| Manifest: RECORD_AUDIO, FOREGROUND_SERVICE, FOREGROUND_SERVICE_MEDIA_PROJECTION (API-34+ enforcement noted), emitter service declared | ✅ | manifest compiled through `processDebugMainManifest` |
| UI shell: Free/Pro toggle (FR-040/048), FR-052 explain-before-prompt, `createScreenCaptureIntent()`, FR-056 a11y labels | ✅ | source compiled |
| README + build-check mirror the receiver's with this host's real evidence + honest gates | ✅ | both delivered |
| No GPL/AGPL; framework + JUnit only | ✅ | no third-party deps beyond AGP/Kotlin/JUnit |
| Real per-app capture / consent UX / locked-device / FGS-on-34 device behavior | ⬜ device gate `android-capture-device` | not claimed |

## Risks / limitations

- **No device evidence yet** (real capture, MediaProjection UX, locked-device,
  FGS lifecycle on API 34+, API 29–33 fallback band). Compile + JVM tests do not
  exercise `AudioRecord`/`MediaProjection` at runtime.
- **The Android emulator has NO real capture** (PLATFORM_MATRIX); a demo on an
  emulator must use `FakeSoundSource`.
- **No transport**: `FrameSink` is null — nothing leaves the device yet; that is
  the integration task's seam.
- **Toolchain delta**: repo P0 docs pin AGP 9.4 / Gradle 9.6; this tree ships
  AGP 8.7.3 / Kotlin 2.1.0 (known-good) with versions centralized in
  `libs.versions.toml` for a one-place bump. Homebrew Gradle 9.7.1 also builds,
  but the committed wrapper is pinned to 8.13 (agent-installed) for
  reproducibility.
- **Receiver config bug found**: `JvmTarget.JVM_17.toString()` in
  `platform/android-receiver/app/build.gradle.kts` would fail a real build the
  same way; the emitter uses the corrected `"17"`. Reporting for the receiver's
  android-ci gate.
- Free/Pro toggle is a dev/demo switch, not tamper-resistant (FR-045).

## Follow-up tasks

- Run the consent/capture runbook on a real Android device (API 34 preferred) →
  flip the `android-capture-device` gate; verify protected-content silence.
- Wire `FrameSink` to the emitter core (encode → AEAD → QUIC) — the integration
  task's boundary is `EmitService.FrameSink` / `MediaProjectionCaptureAdapter.FramesAvailable`.
- Bump catalog to the repo P0 pin (AGP 9.4 / Gradle 9.6) and re-verify; also
  apply the `jvmTarget` fix to the receiver's build script.
- AndroidX/Media3/Result-API UI migration + proper service binder for live FR-053
  updates are follow-ups for the emitter UI shell.
- Verify the power/thermal behavior of always-on capture (device-gated).

## Blockers

- None blocking delivery. Device/emulator-gated behavior is blocked on hardware
  (`android-capture-device`), by design — explicitly not claimed as validated.
