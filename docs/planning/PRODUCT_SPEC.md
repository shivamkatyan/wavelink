# Wavelink — Product Specification

**Status:** Approved (plan-mode review: 3 independent reviewers, 0 unresolved critical/high).
**Product name:** Wavelink (branding replaceable).
**Role:** Emitter (captures permitted audio and streams it) / Receiver (renders to selected output / portable USB DAC). Apps may expose one or both roles per `PLATFORM_MATRIX.md`.

## Problem & target users
An owner of a portable USB DAC wants to hear a desktop/laptop's live audio through that DAC without buying a dedicated network streamer. Current options are proprietary dongles or closed ecosystems. Target users: music listeners with a laptop/desktop + portable DAC; users who want computer audio on a phone-attached DAC elsewhere; Linux/self-host users.

## Journeys
- **Receiver:** launch → pick Receiver → choose output (default / USB DAC) → choose buffer profile (Low Latency / Balanced / Resilient) → become discoverable → approve pairing (SAS or QR) → stream with live health (latency, fill, loss, fidelity).
- **Emitter:** launch → pick Emitter → choose capture source (system output and, where platform permits, per-app: macOS 14.2+, Linux PipeWire) → discover (mDNS) or manual address / QR → choose allowed quality mode → pair → start.

## Free / Pro behavior
Persistent top-of-UI toggle on every shell (FR-040). Policy centralized behind `EntitlementProvider` (FR-041). Free = lossy Wi-Fi + supported Bluetooth cells (FR-042). Pro adds lossless Wi-Fi (FR-043). Negotiated session uses the intersection of both peers' policies (FR-046). Mid-session toggle change triggers explicit renegotiation; disabling Pro during lossless never silently changes fidelity — pause-and-confirm or saved downgrade preference (FR-047/FR-026). The toggle is a development/demonstration switch, not tamper-resistant enforcement (FR-045). Commerce/billing accounts deferred with a documented adapter boundary (FR-044). The toggle must remain functional in all shipped artifacts and be replaced only in a separate, approved commerce project (FR-048).

## Functional requirements (IDs)
Full traceability in `REQUIREMENTS_TRACEABILITY.md`. Summary:
- FR-001 role selection; FR-002 single active stream, fan-out-ready wire; FR-003 mDNS discovery + manual/QR fallback; FR-004 secure pairing + revocation + no unauthenticated injection; FR-005 session control; FR-005A numeric recovery windows, expiry, sleep/wake (state-machine tested); FR-006 capability negotiation (version, codec, sample format, channel layout, frame duration, transport, output, entitlement policy).
- FR-010 desktop capture (Win/mac/Linux public APIs); FR-011 mobile capture to max public extent with clear limitations; FR-012 receiver rendering all five OSes; FR-013 output selection + USB DAC identification; FR-014 format/fidelity visibility; FR-015 route changes handled; FR-016 background behavior per platform.
- FR-020 Free Wi-Fi lossy (Opus default); FR-021 Pro Wi-Fi lossless (FLAC/PCM evaluated in ADR-005); FR-022 fidelity semantics ("lossless" = decoded PCM sample-identical to agreed encoded PCM; "bit-perfect output" only when full digital path verified); FR-023 adaptive buffers (Low/Balanced/Resilient, measurable, no oscillation); FR-024 clock drift bounded policy (resampled path never labeled bit-perfect); FR-025 network recovery bounded or actionable error; FR-026 honest mode changes (never silent downgrade).
- FR-030 Free Bluetooth lossy where public APIs make emitter→receiver viable; FR-031 standards-first (A2DP/LE Audio before custom); FR-032 custom transport gate (no private APIs/root/jailbreak/unsigned drivers/MFi-only for the general product); FR-033 explicit BT support matrix + honest unsupported cells with one-action fallback to free lossy Wi-Fi; FR-034 equivalent-outcome gate (product device receives AND renders to its own output/DAC; routing to a BT headset does not satisfy).

## Non-goals (excluded unless an ADR changes scope)
Cloud relay / WAN / hosted accounts / remote streaming; multiroom sync and one-to-many; audio recording/storage/library/media-server; DSP/effects/EQ/normalization beyond transport-compatibility conversion; billing and production entitlement enforcement; private APIs, kernel mods, root, jailbreak, unsigned drivers; DRM circumvention or capturing non-capturable content; claims that every Android device bypasses the system mixer or produces bit-perfect USB output.

## Permission / failure journeys
- Every OS permission is explained in-app immediately before the OS prompt (FR-052): capture (Windows: none for loopback; macOS: Screen Recording / NSAudioCaptureUsageDescription / mic if input used; Linux: none for audio, screen portal on Wayland only if screen capture; Android: RECORD_AUDIO + MediaProjection consent + FGS; iOS: local network, broadcast/SCK system picker, mic only if `.playAndRecord`).
- Failures → actionable errors (FR-054): permission denied → guidance + retry; DAC missing/route change → detect/re-route/actionable error (FR-015); network path lost → bounded recovery or terminal error with "return to Wi-Fi" action; unsupported Bluetooth cell → precise explanation + one action to free lossy Wi-Fi (FR-033).

## Status / diagnostics
Live panel: state, peer, transport, codec, sample rate, bit depth, channels, est. end-to-end latency, buffer fill, packet loss, underruns, output route, fidelity state (FR-053). Verbose diagnostics for one session without rebuild; bounded local logs; redacted diagnostic export with manifest (FR-055). No captured-audio persistence by default; telemetry local-only.

## Accessibility (FR-056)
WCAG 2.1 AA is the acceptance baseline. Concrete per-shell checklist below (defined gate, 2026-09-10; carried into each shell and TEST_PLAN). A shell passes FR-056 only when its checklist is green on its native platform (automated where feasible, manual otherwise); until then the row is 🟡, never ⬜.

| Criterion (WCAG 2.1 AA) | Android | iOS | macOS | Windows | Linux |
|---|---|---|---|---|---|
| Keyboard operation (2.1.1) | TalkBack focus + DPAD/TV | Full Keyboard Access / hardware KB | Full Keyboard Access | Tab/arrow + focus rings | Keyboard + GTK shortcuts |
| Screen reader (4.1.2 names/roles/values) | `contentDescription`, TalkBack | `.accessibilityLabel/Value/Hint`, VoiceOver | Accessibility API (NSAccessibility), VoiceOver | UIA: Name/Help, Narrator | AT-SPI, Orca |
| Contrast ≥4.5:1 (1.4.3) | theme colors te…checked | semantic colors contrast-checked | appearance-aware + contrast-check | theme-checked | theme-checked |
| Dynamic text (1.4.4 zoom 200%) | fontScale, sp units | Dynamic Type sizes (accessibility-…) | AX larger text | text scaling | text scaling |
| Reduced motion (2.3.3) | `Settings.ANIMATOR…` | `@Environment(\.accessibilityReduceMotion)` | `NSWorkspace accessibilityDisplayShouldReduceMotion` | `SystemParametersInfo` SPI | GTK accessibility |
| Non-color status (1.4.1) | icon+label+shape for state | SF Symbols + label (never color-only) | icon+label | icon+label | icon+label |
| Target size ≥44pt/48dp (2.5.5) | min touch target | min 44pt | — | — | — |

Each shell surfaces FR-053 status with an explicit **shape/icon + label + color** triad so no state is color-only, keeps ≥4.5:1 foreground/background contrast in both light/appearance variants, and honors system reduce-motion/dynamic-text settings. Concrete automated/manual checks live in `TEST_PLAN.md` §B6.

## Acceptance metrics (§8)
- Stereo first; 44.1/48 kHz first, higher rates supported by codec/OS/DAC. 16/24-bit integer preserved in lossless when supported end to end.
- Start ≤3 s from already-trusted receiver selection on a healthy LAN; recover ≤5 s on a usable replacement path (time-to-audible-stream, not time-to-handshake).
- Balanced ≤150 ms p95 capture-to-render on the controlled reference LAN; Low Latency ≤80 ms p95, **opt-in and device-gated** (latency probe on connect).
- Every latency result reports timestamp points, clock method, warm-up, run length, device/load/network conditions, percentile basis, uncertainty; simulated and physical reported separately.
- 60-minute clean soak: zero crashes, deadlocks, unbounded memory growth, buffer underruns on the reference setup; longer scheduled soak.
- Degraded network: controlled + observable under 1% loss / 30 ms jitter / reorder / brief disconnect; mode-specific acceptance documented.
- Lossless codec: decoded PCM hashes equal source PCM for every delivered frame across golden vectors and randomized supported formats.
- Drift: measured synthetic positive/negative drift injection; ADR-007 quantifies convergence, max correction rate, buffer bounds, audible-artifact tests, applicable fidelity state.
- Startup/reconnect/mode/route changes covered by deterministic state-machine tests. Diagnostics contain no audio payload, pairing secret, private key, or raw stable device identifier.
