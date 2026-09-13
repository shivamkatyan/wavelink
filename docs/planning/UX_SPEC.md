# UX_SPEC — the shared user-layer design for all Wavelink apps

Single source of truth for the "refined, platform-appropriate" user layer across
every shell (macOS AppKit, Android framework Views, iOS SwiftUI, win/linux iced).
Tokens, the streaming state model, the single-action principle, permission UX,
activity logging, and the latency-honesty policy.

## 1. Brand palette (one set of tokens, 4 platforms)

The fixed sRGB pairs below are the ONLY palette. Each platform maps them to its
native token system. All pairs are WCAG-AA contrast-checked (≥4.5:1) in both
schemes (documented ratios in `palette` docs; the iOS `Palette` records them).

| Role | Dark | Light | Notes |
|---|---|---|---|
| background | `#1C1C1E` | `#F5F5F7` | window/bg |
| surface | `#26262A` | `#FFFFFF` | cards |
| textPrimary | `#F2F2F7` | `#1C1C1E` | |
| textSecondary | `#C7C7CC` | `#545458` | |
| accent | `#0A84FF` | `#0040DD` | primary actions |
| good/success | `#32D74B` | `#00754C` | streaming ok |
| warn | `#FFD60A` | `#8A5A00` | paused/overflow |
| error | `#FF453A` | `#C0262B` | fatal |
| free tier | `#8E8E93` | `#6E6E73` | |
| pro tier | `#FF9F0A` | `#9A5B00` | |

Platform homes: iOS `Palette` enum; macOS `Wd` (`NSColor` dynamic providers +
`values/colors.xml`/`values-night`); Android `values(+night)/colors.xml`;
iced `Theme`-tinted colors.

Type/spacing/radius: system-typographic scale per platform (Dynamic Type,
Android fontScale, SF/System fonts); spacing = 8/12/16/24; card radius 10–12.

## 2. Streaming state model (single source of truth)

`Disconnected → Connecting → Streaming → (Error | Paused) → Stopped`. Every
shell renders these with the same label + colour + (for a11y) shape/icon:
Idle(textSecondary) · Connecting(accent) · Streaming(good) · Error(error) ·
Paused/Stopping(warn). No colour-only meaning (FR-056): a spoken label/shape
always accompanies the dot.

## 3. One contextual Start/Stop

Never show "Start" and "Stop" simultaneously. A single primary action toggles,
mutually exclusive, disabled while transitioning. (macOS `actionButton`, Android
`actionButton`, iOS `PrimaryAction` (shared), iced single button.)

## 4. Screen IA (per app)

- **Emitter:** Status header → "Streaming to" (receiver address) → Tier + single
  Start/Stop → Session metrics (codec, capture rate, wire rate, resampled,
  frames, bytes, est. send latency, last error) → permission row → activity log.
- **Receiver:** Status header → output route/DAC → buffer profile → fidelity →
  single Start/Stop → metrics → activity log.
- Win/linux (iced): same IA as emitter (fixture/capture-offline driver until
  native capture).

## 5. Permission UX (FR-052)

Explain BEFORE the OS prompt. macOS: probe Screen Recording; `NotDetermined` →
proactively prompt; `Denied|Restricted` → explainer + "Open System Settings"
(`x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture`);
`ev:fatal` for a missing grant renders as an actionable state + permission row,
never a wall of text. Android: in-app explainer → MediaProjection consent.
iOS: `ConsentExplainerView` → system picker.

## 6. Activity log (replaces raw output boxes)

Timestamped, severity-coloured entries (accent=info, warn, error); collapsible;
auto-scroll; Clear; **Copy diagnostics** (FR-055-redacted: no peer identity
beyond what the window already shows). Cap ~500 lines.

## 7. Latency honesty

The status card's latency is the **worker-side encode+send** time (`ev:metric
send_ms` — measured, not fabricated), explicitly NOT end-to-end (which needs
receiver timestamps). Never present simulated latency as measured
(LATENCY_MEASUREMENT.md is the record).

## 8. Demo-vs-real labelling

Any health value that is demo/simulated is visibly marked "(demo)" until the
transport wires it (iOS receiver tone, Android receiver summary, iced
capture-offline). Real fixtures (hash-perfect) are labelled as such.
