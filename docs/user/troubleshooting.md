# Troubleshooting

Every failure in this product is designed to become an **actionable message**
(FR-054), not a cryptic stack trace. If the app shows "grant permission", "plug
in the DAC", "change buffer mode", "pick a capturable source", or "return to
Wi-Fi", do exactly that.

## Common issues

| Symptom | Cause | Action |
|---|---|---|
| macOS capture is silent / no system audio | Screen Recording (TCC) not granted | System Settings → Privacy & Security → Screen Recording → allow. App explains this before the prompt. |
| Android emitter shows consent each time | MediaProjection tokens are single-use on Android 14+ | Not a bug — approve consent when prompted (required by Android). |
| Android emitter can't hear a specific app | App is not in the capturable mix (USAGE_MEDIA/GAME/UNKNOWN opt-in), or content is protected | Only capturable apps appear. Protected content is silenced by the OS by design. |
| "USB DAC" not detected | Route/device enumeration differs per device | Re-plug the DAC (hotplug is handled on supported paths: Android `TYPE_USB_DEVICE`, iOS `.usbAudio`, macOS CoreAudio transport). Check the route panel. |
| Fidelity shows "output path unverified" | The render path hasn't been measured with a loopback/USB analyzer | Expected until bit-perfect hardware verification (ADR-005) is run. It never shows up just because a DAC is attached. |
| No streams / wrong codec shown | Transport is not yet wired into the app shells (demo values) | Reference system (headless) already streams; the in-app transport lands via the `FrameSink` seam — see project status. |
| BT says "unsupported on stock phones" | True — public APIs do not allow stock phones to act as A2DP sinks | Use the one-action **free lossy Wi-Fi** fallback instead (FR-033/034). |
| First launch warning on macOS | Developer (unsigned) build | Control-click → Open, or `xattr -d com.apple.quarantine` (dev builds only). |
| Android sideload blocked | "Install unknown apps" not enabled for source | Enable for that source, or `adb install`. |
| High latency / dropouts | Wrong buffer profile for a busy network | Use **Balanced** or **Resilient** (Low Latency is for cleanest networks; 80 ms p95 is opt-in and device-gated). |
| Receiver health values seem static | Demo values until transport wiring | Reference CLIs report the real measured latency/loss/underruns today. |

## Diagnosing for support

Use the **redacted diagnostic export** (FR-055): it includes versions,
capabilities, state transitions, metrics and recent errors — and explicitly not
audio, secrets, or raw peer/device identities. Share that, plus what you were
doing and the exact on-screen message.

## Reporting a bug

Open an issue with:

1. Platform + OS version + app build (run `--version` / app About).
2. The exact steps and the on-screen message.
3. A redacted diagnostic export.
4. Anything that contradicts this page — odds are it's a real defect.
