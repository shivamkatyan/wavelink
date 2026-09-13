# Test Plan

Layers (fast layers runnable in WSL2/Docker).

## Unit & property (fast)
Protocol encode/decode + version negotiation; session/state-machine transition tables (deterministic virtual clock — never sleep-and-assume); capability/entitlement intersection + mid-session Free/Pro transitions; sequence arithmetic + timestamp wrap (u64, prop-tested); jitter-buffer bounds/adaptation; drift estimator + correction policy (drift injection); ring overflow/underflow; codec round-trips + malformed frames; redaction; permission/route-change FSMs; proptest arbitrary valid/invalid messages; fuzz targets: postcard control, mDNS TXT, audio-frame header (cargo-fuzz).

## Golden audio (exact hashes)
Fixtures: silence, impulse trains, sine sweeps, full-scale edge values, seeded pseudo-random PCM, mono/stereo channel-identification patterns, 44.1/48 kHz at required bit depths. Canonical byte representations defined (i16/f32 interleave order; no dither except the documented 24→16 lossy path). Lossless: `hash(source) == hash(decoded)` for every delivered frame across golden vectors + randomized formats. Lossy: bounded objective metrics (SNR/PESQ-floor oracle — oracle definition P, locked in B1). Transport-integrity tests kept separate from hardware output-path verification. Toolchain + lib versions pinned (no encoder-version drift).

## Integration
Sim emitter → sim receiver over loopback; cross-process over netem-impaired virtual network; native capture → null sink via virtual devices (PipeWire null sink, ALSA loopback, WASAPI virtual endpoint P on hosted runners); synthetic source → full network → native render; restart/peer-restart/network-change/DAC-hotplug/permission-denial-regrant/Free-Pro changes; current client ↔ previous protocol fixtures (compat policy). Impairment matrix: loss {0,0.5,5}%, jitter, reorder, duplication, packet tamper, bandwidth cap, disconnect; seeded exact profiles stored in the sim harness.

## Security (gated P6, named cases)
Pairing MITM; fresh key establishment; rejected untrusted sender; replay + duplicate session; malformed/oversized/truncated/high-rate control; fuzz parsers/codec/transport boundaries; key-rotation boundary; downgrade-attempt rejection (persisted pattern+version); revoked-identity rejection; unauthenticated mDNS saturation; redaction + secret scans; dependency vulnerability (`cargo audit`) + license scans (`cargo-deny`) + SBOM; 0-RTT-off assertion.

## UI
Emitter + receiver journeys; permission denial + recovery; offline/manual-address; unsupported-BT messaging; Free/Pro policy; route + fidelity labels; accessibility semantics/keyboard/dynamic-type/reduced-motion/narrow-wide; screenshot/visual regression on representative mobile + desktop sizes.

## B6 Accessibility (FR-056) — per-shell WCAG 2.1 AA acceptance (gate defined 2026-09-10)
Each shell ships semantics + a checklist; a shell's FR-056 row is green only when its items pass on its native platform.

- **Shared (all shells):** every FR-053 status uses a shape/icon + label + color triad (never color-only) · ≥4.5:1 contrast in light and dark variants · honors system dynamic-text and reduced-motion settings · text/controls scale at 200% without clipping · min target size (44pt iOS / 48dp Android) · screen-reader names/roles/values on all interactive + status elements.
- **iOS (SwiftUI):** VoiceOver via `.accessibilityLabel/Value/Hint` on every control and status element; `@Environment(\.accessibilityReduceMotion)` to disable decorative animation; Dynamic Type through system fonts + minimum legible sizes; contrast-safe palette constant; Full Keyboard Access reachable; `UITest` for VoiceOver order in the receiver journey.
- **macOS (AppKit/SwiftUI + Accessibility API):** `NSAccessibility` role/label/value on all controls; `NSWorkspace.shared.accessibilityDisplayShouldReduceMotion` honored; Full Keyboard Access + focus rings; contrast-checked appearance-aware colors; VoiceOver spot-check on emitter role/route/fidelity.
- **Android (View/Compose):** `contentDescription`/`contentLabel` on all controls + status; `fontScale` + sp units; TalkBack focus order; reduced-motion via `Settings.Global.ANIMATOR_*`; min 48dp targets; contrast-checked theme.
- **Windows/Linux:** Narrator/Orca names+help; keyboard nav; theme contrast; text scaling.
- **Automated where feasible:** static checks (accessibility-label presence, contrast constants), UITest reaches; **manual:** VoiceOver/TalkBack/orchestrated keyboard + contrast survey on a real session. Recorded per shell in its README/build-check.md and in `docs/orchestration/reports/`.

## Reliability & performance
60-min clean soak in routine CI; longer scheduled soak; repeated connect/disconnect + role switching; leak checks (memory/handle/thread/socket/audio resources); CPU/mem/bandwidth/battery/thermal benches on reference devices; packet-loss/jitter sweeps plotted (latency, buffer fill, underruns, recovery time); slow-receiver/slow-encoder/backpressure; RT-callback worst-case (not average) instrumentation incl. abort-on-allocation guards.

## Environment tiers
- Tier 1: pure-Rust core + synthetic PCM (Docker/WSL2).
- Tier 2: Linux e2e (PipeWire null/loopback, virtual HCI for BT).
- Tier 3: hardware lab (real Windows endpoints, Android + USB DAC, macOS taps + SCK TCC, iOS device, BT, end-to-end latency, battery/thermal, signed installers).

## Requirement mapping
Every quality target in §8 of the contract maps to the layers above; the exact FR↔test links are in `REQUIREMENTS_TRACEABILITY.md` (Automated tests / Manual tests columns).
