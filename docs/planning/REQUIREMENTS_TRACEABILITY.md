# Requirements Traceability — Honest Status Audit (2026-09-09)

Legend: ✅ Verified (automated/measured evidence) · 🟡 Portable-core implemented,
native gate pending · 🔒 Blocked on external gate (runbook exists) · 🚫 Unsupported
by public API (evidence + fallback) · ⬜ Not yet implemented · ➖ N/A at this stage.

## Roles & sessions
| ID | Status | Evidence / gate |
|----|--------|-----------------|
| FR-001 Role selection | 🟡/✅ | **Android + iOS combined into a single Wavelink app with a role picker (RoleLauncherActivity / RolePickerView)**; win/linux keep launcher menus; macOS has an Emitter/Receiver role selector (Receiver render staged); full desktop-receiver merging = follow-up |
| FR-002 Single stream, fan-out-ready | ✅ | session FSM single-stream; protocol has SESSION_DESCRIPTOR/CAPABILITY per-receiver (fan-out-ready) |
| FR-003 Local discovery + manual/QR | 🟡/✅ | **mDNS discovery wired (2026-09-13, WS5): new `wdr_discovery` crate (`_wdr._tcp.local.` advertise/browse via mdns-sd 0.21.3, privacy-min TXT), `ref_receiver --advertise` + `ref_emitter --discover`, loopback advertise→browse→dial e2e hash-perfect + cross-host loopback unit test on this host**; **manual-IP/QR fallback token implemented host-side (2026-09-14, WS-F: `wdr_discovery::manual` — `wdr://host:port?nonce=…&fp=…` ≤512 B, IPv6-bracket + label, malformed-rejection matrix; QR render/scan = device/shell gate)**; mobile-shell browsing = shell/device gate |
| FR-004 Secure pairing + revoke | 🟡 | crypto Noise XX + ed25519 + replay + key-sep implemented+tested; pairing UX/trust-store gated to shells |
| FR-005 Session control | ✅ | `wdr_session` FSM (Idle→…→Terminated) deterministic tests |
| FR-005A Recovery policy (numeric) | ✅ | bounds in PROTOCOL_SPEC + FSM timing tests (backoff 1.5x→30s, idle 60s, pairing 60s, control 5s) |
| FR-006 Capability negotiation | 🟡 | wdr_proto Capability/SessionDescriptor; negotiation core in session; endpoint wiring gated |

## Audio capture & rendering
| ID | Status | Evidence / gate |
|----|--------|-----------------|
| FR-010 Desktop capture (W/M/L) | 🟡 | win-emitter WASAPI shell (linux-validated, gate windows-ci); linux-emitter PipeWire shell (gate pipewire-ci); **macOS emitter shell COMPLETE (platform/macos-emitter: SCK system capture + Core Audio process-tap + HAL/USB/hotplug; permission-state + 17 or 20 (macos14-taps) tests green on macOS host)**; **first real stream wired: the packaged `.app` opens with a UI (launch-smoke) and `--stream` runs SCK capture or a fixture through `wdr_refsim::sink::AudioFrameSink` → encode → CRC → QUIC → `ref_receiver`; fixture path is HASH-PERFECT (canonical golden `b7a3c25c…`); real SCK capture verified on this host's TCC-granted session (68 frames, 0 loss/fatal/underruns)**; **rate-aware**: lossless at the true delivered rate (44.1k bit-exact), Opus native 44.1k or worker-resampled odd rates →48k (`wdr_codec::resample::ResamplerI16`, tested); fresh-install Screen Recording TCC + real capture on other hardware = `macos-capture-sck` gate |
| FR-011 Mobile capture | 🔒 | Android emitter role in the combined `platform/android-wavelink` app (playback-capture/FGS, role picker, assembleDebug + **35/0** JVM tests on host incl. new `SinkSeamTest`); **iOS emitter role in the merged `platform/ios` app (ReplayKit broadcast 12-26 + SCK 27+ max-public scope, in-app consent, App Review 2.5.14 record, merged-app type-check 0 errors)**; **Rust-in-app FFI bridge landed, host-proven (2026-09-14, WS-G, BRIDGE_PLAN step 1: `crates/wdr_bridge` uniffi control-plane over `QuicAudioSink` — worker-engine Send/Sync handle, loopback golden `b7a3c25c…` through the exported fns; android cargo-ndk task + abiFilters + `uses-native-library` + `WdrEngineLoader`; assembleDebug + 35/0 green on host without NDK; `.so` production + runtime capture = `android-ci`/device gate — no claim the app streams yet)**; runtime capture / store-policy = device/store gates. (Split-era android-emitter / ios-emitter shells verified at 0.1.x, then retired.) |
| FR-012 Receiver rendering (5 OS) | 🟡 | Receiver role in the combined `android-wavelink` app (AudioTrack/Oboe route scan + mediaPlayback FGS, JVM tests PASS on macOS host); **receiver render seam (WS3): refsim sinks are now a real `RenderSink` trait + the live `QuicRenderReceiver` server (loopback roundtrip hash-perfect), and the macOS shell gained `--receive` (`--sink null` = null device host-smoked hash==golden; `--sink audio` = preflight-gated Core Audio output, device gate)**; **secure lane landed (2026-09-14, WS-D: Noise XX pairing + per-frame AEAD + fingerprint pinning — loopback golden hash preserved, wrong-fingerprint rejected on either side, lossless-only)**; Receiver role in the merged `platform/ios` app (SwiftUI/AVAudioEngine, .usbAudio); type-check 0 errors on host, .usbAudio device gate; win/linux desktop receiver roles queued | (Split-era android-receiver / ios-receiver shells verified, then retired.) |
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
| FR-023 Adaptive buffers (3 profiles) | 🟡 | i16 **and i24** lossless now ride the full live lane end-to-end (2026-09-14, WS-A: `SinkFormat::canonical_24`, ×3 canonical stride, receiver `Decoder::I24`; loopback golden `edf3016e…` FLAC+PCM); formats/rates 44.1k lossless + Opus native + worker-resampled odd rates supported (seam rate-aware; resampler = `wdr_codec::resample`, polyphase windowed-sinc since WS-E); volume/gain still has no FR | BufferProfile low/balanced/resilient in refsim; per-profile tuning gated |
| FR-024 Clock drift policy | ✅/🟡 | **WLS drift estimator + bounded resample IMPLEMENTED (2026-09-14, WS-B: `wdr_refsim::drift`, MAD outlier rejection + deadband + slew + prefill + excessive-drift signal, `DriftReport.bit_exact` honesty; deterministic drift-injection tests incl. receiver-level skewed-clock run; closes R11)**; physical-device clock skew evidence remains lab-gated |
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
| FR-050/051 Flows | 🟡 | receiver/emitter journeys in PRODUCT_SPEC + refsim roles; UI shells gated. **macOS emitter flow now real: `--stream` (fixture or SCK capture) + Start/Stop + live status in the AppKit UI; macOS receiver flow now real via `--receive` (`--sink null` hash-verified; `--sink audio` device-gated); receiver GUIs on win/linux QUEUED**. Discovery (FR-003) wired at the core+refsim level (2026-09-13); mobile-shell pairing/trust-store UI = shell/device gate |
| FR-052 Pre-prompt explanations | 🟡 | permission-flow spec; native permission gates pending (macOS `--stream` fails fast with a typed `fatal` "Screen Recording not granted" message when TCC is missing) |
| FR-053 Status display | 🟡 | StatusModel.kt (FR-053 fields) + telemetry; **live on the macOS emitter: streaming state, codec, lane, frames sent, overflow flag and errors rendered from `--stream`'s JSON events, and on the receiver via `--receive`'s `complete` event (hash, loss/dup/reorder/underruns)**; Android/iOS status now derived from the wired `FixtureFrameSink` seam (WS4) instead of demo randomness; network transport wiring = FFI gate |
| FR-054 Actionable errors | 🟡 | wdr_proto Error taxonomy (retryable + user_action); shell wiring gated |
| FR-055 Redacted diagnostics | ✅ | wdr_telemetry redaction allow/deny + manifest + poison tests |
| FR-056 Accessibility | 🟡 | ⬜→🟡 2026-09-10: per-shell WCAG AA acceptance checklist now DEFINED (TEST_PLAN/PRODUCT_SPEC; keyboard, VoiceOver/TalkBack/Narrator/Orca, contrast ≥4.5:1, dynamic text, reduced motion, non-color status); iOS SwiftUI shell carries a11y semantics (labels/dynamic-type/reduce-motion/contrast-safe palette); macOS shell carries accessibility affordances; gate = per-shell a11y acceptance (B6) |

## Acceptance metrics (§8)
| Metric | Status | Evidence |
|--------|--------|----------|
| Stereo 44.1/48k first | ✅ | adapters + goldens at 44.1/48k stereo |
| 16/24-bit lossless | ✅ | **i16 + i24 proven (hash equality via `CodecAdapter24` FLAC/PCM against the recorded i24 goldens, golden_lossless_vs_fakes, 2026-09-13)**; **full live-lane e2e since 2026-09-14 (WS-A): `QuicAudioSink → QuicRenderReceiver` driven at `SinkFormat::canonical_24` hashes to the canonical i24 golden `edf3016e…` for FLAC and PCM, 4 frames, 0 loss/underrun** |
| Start ≤3s / recover ≤5s | 🟡 | design + FSM timing; measured on reference via LATENCY_MEASUREMENT spec (physical device-gated) |
| Balanced ≤150ms / LowLat ≤80ms | 🟡 | LATENCY_MEASUREMENT budget tables; device-gated probe; reference-loopback latencies measured (~21-90ms) |
| 60-min clean soak | ✅ | B1-SOAK-EVIDENCE.md PASS (incl. 2026-09-13 macOS re-run, hash `22153f00…` cross-batch stable) |
| Degraded (1% loss/30ms jitter) | ✅ | netem e2e lossless hash-preserving; CC spike bounded (late=0, underruns=0, queue<=512) |
| Lossless PCM hash equality | ✅ | golden_lossless_vs_fakes + refsim FLAC/PCM e2e |
| Drift injection | 🟡 | ADR-007 spec; estimator tests gated P-spike |
| State-machine tests | ✅ | wdr_session deterministic tests |
| Redacted diagnostics | ✅ | telemetry poison tests |

## Summary
- ✅ Verified (measured/automated): ~24 FR/metrics
- 🟡 Portable-core implemented, native/UI gate pending: ~20
- 🔒 Blocked on external gate (runbook): ~9 (mobile/BT/background native)
- 🚫 Unsupported by public API: BT sinks on stock mobile/desktop (with evidence + free-lossy-WiFi fallback)
- ⬜ Not yet implemented: accessibility (FR-056) + B6/B7 gates
- No requirement is claimed "implemented without test evidence"; every ✅ has a test/log/measurement; every 🟡 names its exact gate.
- **NOTE (2026-09-13):** this pass moved to ✅: FR-003 discovery (mdns-sd crate + refsim discover/advertise + e2e), 16/24-bit lossless (i24 `CodecAdapter24` goldens), 60-min soak re-run; moved to 🟡-with-real-seam: FR-012/FR-050-053 (receiver render seam + macOS `--receive`), FR-011/FR-053 (mobile `FrameSink` seam fixture-wired).
- **NOTE (2026-09-14):** this pass landed the end-to-end 24-bit lane (WS-A, golden `edf3016e…` FLAC+PCM), the WLS drift estimator + injection tests (WS-B, FR-024 core implemented, R11 closed), the `wdr_crypto` secure lane (WS-D), F32/I32 PCM + windowed-sinc resampler (WS-E), the manual/QR fallback token (WS-F, FR-003), and the host-proven uniffi bridge + android packaging (WS-G). RT_CONTRACT §4 runtime layer is complete (rt-guard abort-on-alloc + no_std + deny-alloc probe + stall watchdog, WS-C). F32/I32 remains adapter-surface only (FLAC-32 is a recorded follow-up); the `.so` production stays `android-ci`; physical-device drift/native-RT/latency evidence stays lab-gated.
- **PROPOSED FRs (no owning FR today — would require new FRs before any claim, per 2026-09-12 note):** `FR-0xx volume/gain control`, `FR-0xx user-facing codec-selection UI` — status ⬜ drafted-only, no implementation. |
- **NOTE (2026-09-12):** volume/gain control and a user-facing codec-selection UI have **no FR** — the tier picks the codec (Free=Opus, Pro=FLAC/PCM). Any attempt to claim them must add new FRs first.
