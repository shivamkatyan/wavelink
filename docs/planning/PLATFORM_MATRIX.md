# Platform Feasibility Matrix

**Research verified: 2026-09-06** (official docs; evidence seeds in `docs/planning/research/`). Status tags:
`S` Supported · `SL` Supported with limitations · `E` Experimental · `U` Unsupported by public API · `P` Pending probe.

Revalidate affected rows whenever an OS SDK, target version, entitlement, dependency, or store policy changes.

## Revalidation — macOS/iOS build & test environment (2026-09-10, macOS host)

Revalidated on a physical macOS host: **Xcode 26.6** (SDKs: macOS 26.5, iPhoneOS 26.5, iPhoneSimulator 26.5),
**Rust 1.98.1** (targets aarch64-apple-darwin, aarch64-apple-ios, aarch64-apple-ios-sim, x86_64-apple-ios),
**Docker Desktop 29.4.0**, JDK 17 + Android SDK (platform-34/build-tools 34.0.0/NDK 27.2). Verified facts
that CONFIRM the macOS/iOS rows below without changing their verdicts:
- ScreenCaptureKit is present and the macOS 13+ floor holds on this SDK (system capture path compile-capable on host);
- Core Audio process taps (`AudioHardwareCreateProcessTap`) are present in the macOS 26.5 SDK (14.2+ per-app path compile-capable, feature-gated);
- no `AVAudioSession` on macOS (CoreAudio HAL path remains the route surface); `.usbAudio` remains an iOS-only AVAudioSession port;
- iOS 26.5 simulator SDK supports AVAudioEngine/AVAudioSession compile + simulator-safe tests (device `.usbAudio`/local-net TCC remain hardware gates).
The macOS/iOS research verdicts above are un-changed; the compile/build + unit-test environment is now verified ON a native macOS host with the shells being built in B3/B4 (reports in `docs/orchestration/reports/t-B3-macos.md`, `t-B4-ios-receiver.md`).

## A. Capture / render / DAC (Wi-Fi relay)

| OS / min | System capture | Per-app capture | Consent | Protected content | Background | Public render | USB DAC route/visible | Capture status |
|---|---|---|---|---|---|---|---|---|
| **Windows 11 24H2+** (10 22H2 = EOL legacy/own-risk) | S — WASAPI loopback (no "Stereo Mix" needed) | U — no per-process PCM API (session control only) | none for loopback; packaged-app exemption nuance P | muted by design in loopback (DRM) | S — Win32 service (session-0 loopback documented) | S — WASAPI shared/exclusive, endpoint enum, default-route change, format negotiate | SL — endpoint + friendly/instance/container ID; VID/PID needs PnP correlation (P single canonical doc) | **S** |
| **macOS 13** (taps 14.2+) | S — ScreenCaptureKit audio (macOS 13+) | S (14.2+) — Core Audio process taps (public, documented) | Screen Recording TCC (`NSScreenCaptureUsageDescription`; `NSAudioCaptureUsageDescription`) | excluded/muted by design | SL — accessory activation policy; no macOS background-modes capability | S — AVAudioEngine/AU; **no AVAudioSession on macOS**; HAL default-device or AU-level device | S — HAL AudioObject enum/select/hotplug; class-compliant USB | **S** |
| **Linux** Ubuntu 22.04+/glibc≥2.35, PipeWire≥1.4 | S — PipeWire monitor/loopback | S — per-node targeting (`target.object`) | none for audio (screen portal on Wayland) | decrypted PCM capturable at PipeWire (CDM decrypts in-process; no PMP audio gate) — P exact service policy | S — headless user service / Docker | S — ALSA/PipeWire sink, rate/format negotiation, RTKit scheduling | S — snd-usb-audio, hotplug, deterministic node names | **S** |
| **Android** minSdk 29, target 34+ | U — REMOTE_SUBMIX requires system `CAPTURE_AUDIO_OUTPUT` (not 3rd-party) | SL — AudioPlaybackCaptureConfiguration (opt-in USAGE_MEDIA/GAME/UNKNOWN mix, per-UID/usage filter) + MediaProjection | MediaProjection per-session; renew each session; 14+ single-use token; must start from visible activity; FGS type `mediaProjection` on 34+ | excluded via capture policy (silence) | SL — mediaProjection FGS (while-in-use); 15+ no BOOT_COMPLETED start | S — AudioTrack/AAudio/Oboe; `getDevices`; `setPreferredDevice`; API 34+ `AudioMixerAttributes` BIT_PERFECT (media-over-USB only) | S — `TYPE_USB_DEVICE`, `registerAudioDeviceCallback`; emulator has **no USB audio** | **SL** |
| **iOS/iPadOS** 14 floor (SCK emitter 27+) | U — system bus via `installTap` | S — self-playback tap (no mic needed); SL — other-app audio via ReplayKit broadcast ext (system picker, iOS10+) or SCK (iOS27+, system picker, replaces ReplayKit) | Local network TCC (iOS14+); broadcast = `RPSystemBroadcastPickerView` (no bypass); SCK = `SCContentSharingPicker`; mic only if `.playAndRecord`/inputNode | excluded (ReplayKit incompatible w/ AVPlayer content; output protection) | S — `.playback` + `UIBackgroundModes audio` (receiver); `screen-capture`+`audio` (SCK emitter) | S — AVAudioEngine output; `.usbAudio` port; route-change notif; session sample rate typically 8–48k (P >48k) | S — `.usbAudio` visibility; system auto-selects; cannot force specific DAC | **SL** |

## B. Bluetooth receive (product gate: target device running this product receives emitted audio and renders to its own output/DAC)

| OS | Standard A2DP sink (app) | Standard A2DP source (assess) | LE Audio to 3rd-party apps | Custom classic sockets (RFCOMM/L2CAP) | BT receive verdict |
|---|---|---|---|---|---|
| **Windows 11** | U (OS source-only; sink = vendor kernel driver) | OS-managed; app picks render endpoint only | E/P — driver-level VSAP(ACX); no WinRT/Win32 app API | RFCOMM S (WinRT/Winsock); L2CAP app-level U | **U** standard sink; **SL** custom RFCOMM product-peer (low-bitrate lossy) |
| **macOS** | U (no A2DP sink app API; AirPlay Receiver is receive path but no app PCM) | OS-managed | U/P — none documented | S — IOBluetooth RFCOMM/L2CAP data (not audio sink) | **U** standard; **P** custom transport worth later validation |
| **Linux** | **S** — BlueZ `a2dp_sink` + PipeWire media-sink (Linux = Bluetooth speaker) | S | E — BAP unicast/broadcast + BASS, BlueZ `--enable-experimental`, PipeWire 1.4+/1.6 | S — ProfileManager/Profile1 D-Bus, RFCOMM/L2CAP | **S** (only full receive+render path) |
| **Android** | **U** (hidden/removed `BluetoothA2dpSink`; verified 404/AOSP) | OS-managed | U for generic receive (LE Audio = hearing-aid/endpoint classes; no app BAP receive, no broadcast classes) | S — `BluetoothSocket` RFCOMM/L2CAP (bandwidth-limited, FGS+permissions) | **U** standard; **SL** custom RFCOMM/L2CAP product-peer (app renders to DAC) |
| **iOS** | **U** (verified; no public sink API) | OS-managed | U (hearing-device only, read-only status) | U for generic (GATT-over-BR/EDR; ExternalAccessory MFi-scoped) | **U** standard; **P** MFi-scoped custom (not for general product) |

## C. Capture-component survey (eqMac)

- **"emac" interpreted as eqMac — confirmed** (github.com/bitgapp/eqMac). Open snapshot: **Apache-2.0**; last driver code 2021-12 (v1.3.2-era); current capture core ships **closed-source from a private fork** (Free/Pro split). Never copy or reverse-engineer the closed binaries.
- **Decision: adopt native APIs; reuse none of eqMac.** Windows = WASAPI (Rust `windows-rs`); macOS = SCK + Core Audio taps (optionally Apache-2.0 `screencapturekit-rs`); Linux = PipeWire (MIT). Rationale recorded in `DEPENDENCY_EVALUATION.md`.
- GPL virtual audio drivers (BlackHole GPL-3.0, BackgroundMusic GPL-2.0) rejected for embedding; may appear only as separately-installed external components with a recorded licensing decision.
