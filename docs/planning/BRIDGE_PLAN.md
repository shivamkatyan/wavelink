# Mobile FFI Bridge Plan (FR-011/FR-012 follow-up — WS4 seam-level wiring)

Owner: Platform Implementer (mobile). Gate: `android-ci` / `ios-device` /
`usb-dac-device`. Status: **planned** (2026-09-13) — the seam is wired
host-side; the Rust-in-app FFI is NOT yet implemented and is a named gate.

## What is already real (this batch, host-verified)

- **The seam exists on all three shells** with identical shape —
  `onFormat` once → `onBlock` (whole interleaved LE i16 blocks) → `finish`:
  - Android: `app/.../emitter/SinkSeam.kt` `FrameSink` + `FixtureFrameSink`
    (whole-frame CRC-32 readout). Wired into `EmitterService`; JVM tests
    `SinkSeamTest.kt` (**35/0 unit tests + assembleDebug pass on host**).
  - iOS: `Sources/WavelinkApp/Emitter/FrameSink.swift` `FrameSink` +
    `FixtureFrameSink` (CRC-32), driven from `EmissionModel.systemAuthorized()`
    → sink-derived FR-053 status, `finish()` on stop. Merged-app type-check
    0 errors on host (`ios-simulator` CI gate).
  - macOS (the proven reference): `wdr_refsim::sink::FrameSink` →
    `QuicAudioSink` → QUIC, golden hash-verified (`macos-stream-smoke.sh`).
- **The engine's receive half** is likewise reachable through
  `wdr_refsim::sink::RenderSink` + `QuicRenderReceiver` (WS3), golden-verified.

## The gap being gated

The Android/iOS shells drive `FixtureFrameSink` (local counts + CRC) instead of
the Rust engine. Wiring the real engine in-app requires an FFI bridge that does
NOT exist yet anywhere (no cargo-ndk task, no uniffi, no jniLibs in
`build.gradle.kts`; iOS has zero Rust linkage). The macOS pattern (child-process
+ NDJSON) is not viable on mobile where the capture callbacks are in-process.

## Plan (ADR-001/002: uniffi for control-plane only, never RT render callbacks)

1. **Android — JNI/uniffi Kotlin bindings + cargo-ndk packaging**
   - Add `uniffi` (recorded MPL-2.0 exception, DEPENDENCY_EVALUATION) with a
     `#[uniffi::export]` control-plane surface; or a hand-written JNI bridge.
     Keep it **control-plane only**: the runtime `onBlock` path must not cross
     FFI per ADR-002 (RT discipline) — instead the FFI surface drives a
     **running engine instance** that pushes into the same seam.
   - `build.gradle.kts`: add a `cargo-ndk` task producing `libwdr_core.so`
     (arm64-v8a / armeabi-v7a / x86_64) via `jniLibs`; `abiFilters` pinned;
     `uses-native-library` + packaging config in the manifest.
   - Runtime wiring: `EmitterService` constructs the engine once per session,
     `FixtureFrameSink` is replaced by the engine-backed sink behind the same
     `FrameSink` interface — `SinkSeamTest.kt` remains valid.
   - Verification: `:app:testDebugUnitTest :app:assembleDebug` on host +
     `android-ci` assembly job; capture/transport runtime = device gate.

2. **iOS — uniffi Swift bindings + Xcode wrapper**
   - uniffi-generated Swift module linked in the `platform/ios` app via the
     recorded `xcodegen` wrapper (repo intentionally holds no `.xcodeproj`).
   - Same control-plane discipline: `EmissionModel`/`ReceiverModel` exchange
     status via FFI; media data stays inside the process.
   - Verification: simulator type-check (already a gate) → simulator runtime →
     device/store gates.

3. **Naming honesty**: until step 1/2 ships, traceability keeps FR-011/FR-012
   at 🟡 ("seam fixture-wired on host; Rust-in-app FFI = named gate `android-ci`
   / `ios-device`") — no claim that the app streams over the network.

## DEPENDENCY_EVALUATION notes

- `uniffi 0.32` / `cbindgen` / `cargo-ndk` already recorded (FFI/bindings tools,
  permissive policies documented there). No new runtime dependency is expected
  beyond the existing core crates already present at the shell seams.
