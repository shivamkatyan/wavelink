# Hardware Validation

Runbooks for what simulators/emulators/docker cannot prove. Tier 3 gates: B2 (MVP), B4 (mobile breadth), B5 (BT), B6 (hardening, bit-perfect).

| Gate | Device/rig | Tests | Evidence |
|---|---|---|---|
| Windows capture | Real Win11 machine | WASAPI loopback end-to-end; endpoint/route changes; protected-content silence; session-0 service capture | capture log + audio integrity |
| Android receiver + USB DAC | Phone/tablet + portable USB DAC | discovery/pair; route select; hotplug attach/detach; BIT_PERFECT (API34+, media-over-USB); background FGS; locked-device | DAC output + telemetry |
| macOS capture | Intel + Apple Silicon Macs | SCK system; per-process taps (14.2+); TCC prompts/denials; hardened+sandboxed build | capture log; permission states |
| iOS capture/render | iPhones (Lightning/USB-C) | ReplayKit broadcast + SCK 27+ system pickers; local-net deny; .usbAudio DAC; background audio | screenrec + route |
| Bluetooth | Reference BT devices + virtual HCI | Linux a2dp_sink receive + render; custom RFCOMM/L2CAP product-peer cells; never-a-headset fallback | BT lab reports |
| E2E latency | Reference LAN + timers | start ≤3 s; recover ≤5 s; Balanced ≤150 ms; Low-Latency device-gate | timestamped loopback |
| Background/thermal | Locked devices | long session battery/thermal/dropout | soak logs |
| **Bit-perfect** | DAC w/ digital loopback or USB analyzer | source samples bit-identical into DAC; bounded silent slip; clock relationship + slip budget recorded; receiver-side only | analyzer trace + hash |
| Signed installs | Clean machines | install/uninstall; clean-clone validate; signed-store dry-run | package logs |

Every hardware gate has a runbook (steps, expected results, pass/fail criteria) authored by the owning impl before the lab run; results recorded in `docs/orchestration/` and reflected as `Verification: hardware` or `pending gate` in traceability.
