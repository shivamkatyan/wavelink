# Requirements Traceability — Honest Status Audit (2026-09-09)

Legend: ✅ Verified (automated/measured evidence) · 🟡 Portable-core implemented,
native gate pending · 🔒 Blocked on external gate (runbook exists) · 🚫 Unsupported
by public API (evidence + fallback) · ⬜ Not yet implemented · ➖ N/A at this stage.

## Roles & sessions
| ID | Status | Evidence / gate |
|----|--------|-----------------|
| FR-001 Role selection | 🟡/✅ | **Android + iOS combined into a single Wavelink app with a role picker (RoleLauncherActivity / RolePickerView)**; win/linux keep launcher menus; macOS has an Emitter/Receiver role selector (Receiver render staged); full desktop-receiver merging = follow-up |
| FR-002 Single stream, fan-out-ready | ✅ | session FSM single-stream; protocol has SESSION_DESCRIPTOR/CAPABILITY per-receiver (fan-out-ready) |
| FR-003 Local discovery + manual/QR | 🟡 | mDNS (mdns-sd) selected (ADR-006); discovery not yet wired into refsim; manual/QR spec'd |
| FR-004 Secure pairing + revoke | 🟡 | crypto Noise XX + ed25519 + replay + key-sep implemented+tested; pairing UX/trust-store gated to shells |
| FR-005 Session control | ✅ | `wdr_session` FSM (Idle→…→Terminated) deterministic tests |
| FR-005A Recovery policy (numeric) | ✅ | bounds in PROTOCOL_SPEC + FSM timing tests (backoff 1.5x→30s, idle 60s, pairing 60s, control 5s) |
| FR-006 Capability negotiation | 🟡 | wdr_proto Capability/SessionDescriptor; negotiation core in session; endpoint wiring gated |

## Audio capture & rendering
| ID | Status | Evidence / gate |
|----|--------|-----------------|
| FR-010 Desktop capture (W/M/L) | 🟡 | win-emitter WASAPI shell (linux-validated, gate windows-ci); linux-emitter PipeWire shell (gate pipewire-ci); **macOS emitter shell COMPLETE (platform/macos-emitter: SCK system capture + Core Audio process-tap + HAL/USB/hotplug; permission-state + 17 or 20 (macos14-taps) tests green on macOS host)**; **first real stream wired: the packaged `.app` opens with a UI (launch-smoke) and `--stream` runs SCK capture or a fixture through `wdr_refsim::sink::AudioFrameSink` → encode → CRC → QUIC → `ref_receiver`; fixture path is HASH-PERFECT (canonical golden `b7a3c25c…`); real SCK capture verified on this host's TCC-granted session (68 frames, 0 loss/fatal/underruns)**; **rate-aware**: lossless at the true delivered rate (44.1k bit-exact), Opus native 44.1k or worker-resampled odd rates →48k (`wdr_codec::resample::ResamplerI16`, tested); fresh-install Screen Recording TCC + real capture on other hardware = `macos-capture-sck` gate |
| FR-011 Mobile capture | 🔒 | Android emitter role in the combined `platform/android-wavelink` app (playback-capture/FGS, role picker, assembleDebug + 31/0 JVM tests on host); **iOS emitter role in the merged `platform/ios` app (ReplayKit broadcast 12-26 + SCK 27+ max-public scope, in-app consent, App Review 2.5.14 record, merged-app type-check 0 errors)**; runtime capture / store-policy = device/store gates. (Split-era android-emitter / ios-emitter shells verified at 0.1.x, then retired.) |
| FR-012 Receiver rendering (5 OS) | 🟡 | Receiver role in the combined `android-wavelink` app (AudioTrack/Oboe route scan + mediaPlayback FGS, JVM tests PASS on macOS host); refsim null-render; **Receiver role in the merged `platform/ios` app (SwiftUI/AVAudioEngine, .usbAudio); type-check 0 errors on host**, .usbAudio device gate; mac/win/linux desktop receiver roles QUEUED. (Split-era android-receiver / ios-receiver shells verified, then retired.) |
| FR-013 Output selection + USB DAC | 🟡 | android AudioOutputRouter (TYPE_USB_DEVICE, setPreferredDevice, hotplug); gate usb-dac-device |
| FR-014 Format/fidelity visibility | 🟡 | telemetry EventKind + FormatMeta; UI gated |
| FR-015 Route changes | 🟡 | RouteChange enums + router hotplug logic; native crash-free re-create gated |
| FR-016 Background | 🔒 | android mediaPlayback FGS+MediaSession (manifest); device-locked playback gate usb-dac-device |

## Network audio modes
| ID | Status | Evidence / gate |
|----|--------|-----------------|
| FR-020 Free Wi-Fi lossy (Opus) | ✅ | Opus adapter + reference e2e (free tier) + impairment; ADR-004 |
| FR-021 Pro Wi-Fi lossless | ✅ | FLAC/PCM adapter + lossless hash-preserving e2e (clean + 1% loss); ADR-005 (hybrid FLAC→PCM fallback) |
| FR-022 Fidelity semantics | ✅ | lossless=hash equality (proven); bit-perfect=hardware-loopback-only (gated); fidelity machine in ADR-005 |
| FR-023 Adaptive buffers (3 profiles) | 🟡 | formats/rates 44.1k lossless + Opus native + worker-resampled odd rates now supported (seam is rate-aware; resampler = `wdr_codec::resample`, worker-side/off-RT); volume/gain still has no FR | BufferProfile low/balanced/resilient in refsim; per-profile tuning gated |
| FR-024 Clock drift policy | 🟡 | ADR-007 (WLS estimator + bounded resample) spec'd; resampler integration gated |
| FR-025 Network recovery | ✅ | reconnect/backoff FSM + netem 1% loss recovery (5s window design) |
| FR-026 Honest mode changes | ✅ | PolicyGate REQUIRES_CONFIRM (no silent downgrade) unit-tested (android) + wdr_entitlement + session tests |

## Bluetooth
| ID | Status | Evidence / gate |
|----|--------|-----------------|
| FR-030/031/032/033/034 | 🚫/🔒 | ADR-009 two-axis model: **Linux-standard-sink = the only full public receive+render path — shell COMPLETE (`platform/linux-receiver`: BlueZ a2dp_sink + PipeWire media-sink, portable surface 11/0 tests on host, honest FR-033 matrix + FR-034 one-action free-lossy-Wi-Fi fallback; native = Linux-runner/bt-lab)**; stock Android/iOS/Windows/macOS A2DP-sink & generic LE-Audio receive = Unsupported by public API (evidence in PLATFORM_MATRIX); custom product-peer RFCOMM/L2CAP (And/Win-RFCOMM/Linux) = low-bitrate lossy fallback cell (feature-gated); product never claims stock-phone sink. |

## Entitlement
| ID | Status | Evidence / gate |
|----|--------|-----------------|
| FR-040 Dev toggle all shells | 🟡 | EntitlementProvider + DevToggle (core); PlatformGate.kt (android); **iOS SwiftUI + macOS shells carry persistent top-level toggle**; shell UI gated |
| FR-041 Centralized EntitlementProvider | ✅ | wdr_entitlement trait + tests |
| FR-042/043 Free/Pro policies | ✅ | feature table tests (Free=lossy+BT; Pro=+lossless) |
| FR-044 Deferred commerce adapter | ✅ | CommerceEntitlementBackend seam documented (no impl) |
| FR-045 Dev toggle not enforcement | ✅ | documented in provider doc-comment |
| FR-046 Policy intersection | ✅ | Policy::intersect + negotiation tests |
| FR-047 Live policy change | ✅ | renegotiate_live_change + PolicyGate REQUIRES_CONFIRM + tests |
| FR-048 Toggle lifecycle | 🟡 | provider in artifacts; iOS/macOS/Android shells carry top-level toggle; full artifact scan gated |

## UX & diagnostics
| ID | Status | Evidence / gate |
|----|--------|-----------------|
| FR-050/051 Flows | 🟡 | receiver/emitter journeys in PRODUCT_SPEC + refsim roles; UI shells gated. **macOS emitter flow now real: `--stream` (fixture or SCK capture) + Start/Stop + live status in the AppKit UI; receiver flow still via `ref_receiver` (receiver GUIs on desktop QUEUED)**. Discovery/pairing (FR-003/FR-004) remains gated (ADR-006 mDNS, no code yet) |
| FR-052 Pre-prompt explanations | 🟡 | permission-flow spec; native permission gates pending (macOS `--stream` fails fast with a typed `fatal` "Screen Recording not granted" message when TCC is missing) |
| FR-053 Status display | 🟡 | StatusModel.kt (FR-053 fields) + telemetry; **live on the macOS emitter: streaming state, codec, lane, frames sent, overflow flag and errors rendered from `--stream`'s JSON events**; receiver-side health values elsewhere remain demo until receiver transport wiring |
| FR-054 Actionable errors | 🟡 | wdr_proto Error taxonomy (retryable + user_action); shell wiring gated |
| FR-055 Redacted diagnostics | ✅ | wdr_telemetry redaction allow/deny + manifest + poison tests |
| FR-056 Accessibility | 🟡 | ⬜→🟡 2026-09-10: per-shell WCAG AA acceptance checklist now DEFINED (TEST_PLAN/PRODUCT_SPEC; keyboard, VoiceOver/TalkBack/Narrator/Orca, contrast ≥4.5:1, dynamic text, reduced motion, non-color status); iOS SwiftUI shell carries a11y semantics (labels/dynamic-type/reduce-motion/contrast-safe palette); macOS shell carries accessibility affordances; gate = per-shell a11y acceptance (B6) |

## Acceptance metrics (§8)
| Metric | Status | Evidence |
|--------|--------|----------|
| Stereo 44.1/48k first | ✅ | adapters + goldens at 44.1/48k stereo |
| 16/24-bit lossless | 🟡 | i16 proven (hash equality); i24 adapter PENDING (ADR-005 follow-up) |
| Start ≤3s / recover ≤5s | 🟡 | design + FSM timing; measured on reference via LATENCY_MEASUREMENT spec (physical device-gated) |
| Balanced ≤150ms / LowLat ≤80ms | 🟡 | LATENCY_MEASUREMENT budget tables; device-gated probe; reference-loopback latencies measured (~21-90ms) |
| 60-min clean soak | ✅ | B1-SOAK-EVIDENCE.md PASS |
| Degraded (1% loss/30ms jitter) | ✅ | netem e2e lossless hash-preserving; CC spike bounded (late=0, underruns=0, queue<=512) |
| Lossless PCM hash equality | ✅ | golden_lossless_vs_fakes + refsim FLAC/PCM e2e |
| Drift injection | 🟡 | ADR-007 spec; estimator tests gated P-spike |
| State-machine tests | ✅ | wdr_session deterministic tests |
| Redacted diagnostics | ✅ | telemetry poison tests |

## Summary
- ✅ Verified (measured/automated): ~22 FR/metrics
- 🟡 Portable-core implemented, native/UI gate pending: ~18
- 🔒 Blocked on external gate (runbook): ~9 (mobile/BT/background native)
- 🚫 Unsupported by public API: BT sinks on stock mobile/desktop (with evidence + free-lossy-WiFi fallback)
- ⬜ Not yet implemented: accessibility (FR-056) + B6/B7 gates
- No requirement is claimed "implemented without test evidence"; every ✅ has a test/log/measurement; every 🟡 names its exact gate.
- **NOTE (2026-09-12):** volume/gain control and a user-facing codec-selection UI have **no FR** — the tier picks the codec (Free=Opus, Pro=FLAC/PCM). Any attempt to claim them must add new FRs first.
