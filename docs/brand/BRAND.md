# Wavelink — brand

**Wavelink** is the product: it makes a phone or tablet a wireless DAC for any
computer on your Wi-Fi — capture the computer's audio, stream it lossy (Free)
or lossless (Pro, hash-verified), play it through your portable USB DAC. No
cloud, no accounts, no telemetry.

## Naming

| Layer | Name |
|---|---|
| Product / app display name | **Wavelink** |
| Bundle / application ids | `dev.wavelink.app` (mobile), `dev.wavelink.macos` (macOS), package `wavelink` (Linux .deb) |
| Engineering crates | `wdr_*` (the audio kernel — proto/codec/transport/refsim). Publicly invisible; "Wavelink" is the brand, `wdr_*` is the engine. |
| mDNS service | `_wdr._tcp` (ADR-006; same trampoline, only the app name changed) |

## Mark

A wave glyph breaking a horizontal line inside a rounded square — "a wireless
wave crossing the gap". Brand colour is the UX accent blue; the wave uses
surface/white.

Master: `scripts/brand/make_icons.swift` draws and rasterizes the mark (no
external tooling). Generated assets are committed under each platform's assets.

## Colour & type

The UX palette in `docs/planning/UX_SPEC.md` IS the brand palette (background /
surface / textPrimary / textSecondary / accent / good / warn / error / free /
pro — dark + light, WCAG-AA). Type: system-native per platform. No third-party
fonts, no Web fonts.

## Voice

- Honest, measured, never overwrought: "Free lossy for everyone; Pro lossless
  that is measured, never assumed."
- Role-first: a device can be a Wavelink **Emitter** (captures audio) or a
  Wavelink **Receiver** (renders to its own output/DAC) — picked at launch
  (FR-001), because the same app does both.
- Never imply what the OS won't do (capture scope, copy protection, bit-perfect
  claims stay gated to evidence).

## Iconography

See each platform's generated asset (macOS `.icns`, Android adaptive +
monochrome `ic_launcher`, iOS `AppIcon`, Linux `wavelink.png`, Windows
`wavelink.ico`). Status/severity iconography follows the UX_SPEC non-color
rule (FR-056): a shape or label always accompanies colour.
