# Platform support

The honest, evidence-backed matrix. A cell is only "Supported" when a real,
public API path exists and is implemented; otherwise it is either
"Supported with limitations", "Unsupported by public API" (with the nearest
fallback), or a pending hardware/runner/credential gate.

## Wi-Fi (the relay itself)

| Platform | Emitter (capture) | Receiver (render) |
|---|---|---|
| **macOS 13+** | ✅ **Supported** — ScreenCaptureKit system capture (13+), Core Audio process taps for per-app (14.2+); Screen Recording TCC. Hardware validation pending on a logged-in session. | 🔊 In progress (desktop receiver role queued) |
| **Windows 11 24H2+** | ✅ **Supported** — WASAPI loopback (system-wide; no per-process PCM API). Native runner check pending. | 🔊 Queued |
| **Linux** (Ubuntu 22.04+ / PipeWire ≥ 1.4) | ✅ **Supported** — PipeWire system + per-app node targeting | ✅ Receiver core; BT path below |
| **Android 10+ (API 29), target 34** | 🟡 **Supported w/ limitations** — app-audio capture via MediaProjection consent + playback-capture config (opt-in USAGE_MEDIA/GAME/UNKNOWN); new consent each session; device gates | ✅ **Supported** — AudioTrack/Oboe, USB DAC detection/hotplug (`TYPE_USB_DEVICE`, `setPreferredDevice`) |
| **iOS 14+** | 🟡 **Supported w/ limitations** — ReplayKit broadcast extension (12–26) + ScreenCaptureKit (27+); system picker + red indicator; device/store gates | ✅ **Supported** — AVAudioEngine, `.usbAudio` route (system chooses; can't force a specific DAC) |

The reference system proves the transport on every platform's behalf: lossless
FLAC **hash-perfect** (clean + under 1% loss), lossy Opus bounded, policy
intersection, reconnect/recovery.

## Bluetooth (the strict rules, FR-030…034)

| Cell | Verdict |
|---|---|
| Linux **standard A2DP sink** receiver (BlueZ + PipeWire media-sink) | ✅ Supported (the only full public receive+render path) |
| Custom product-peer RFCOMM/L2CAP (Android / Windows-RFCOMM / Linux) | 🟡 Low-bitrate lossy fallback (feature-gated, lab-validated) |
| Stock **Android/iOS/Windows/macOS** phone-or-desktop as an A2DP sink | 🚫 **Unsupported by public API** — with one-action fallback: free lossy Wi-Fi |
| LE-Audio generic receive for third-party apps | 🚫 Unsupported by public API (hearing-device classes only) |

## What "Supported" means here

Each green cell is backed by **official API research + timestamped evidence**
(`docs/planning/PLATFORM_MATRIX.md`) and — for the shells — compile- and
unit-validated builds in this repository. Cells marked "pending hardware/
runner/credential" are honest gates with runbooks
(`docs/planning/HARDWARE_VALIDATION.md`, `RELEASE_AND_SIGNING.md`); no cell is
marked supported merely because a similarly named API exists.
