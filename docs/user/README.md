# Wavelink — End-User Documentation

Turn an ordinary device connected to a portable DAC into a wireless audio
receiver — send your computer's audio over Wi-Fi to a phone or tablet, lossy
(Free) or lossless (Pro).

This is the user-facing manual. It covers **installing**, **setting up** and
**using** the Wavelink apps. Everything here is honest about what the
current builds do today and what is still behind a lab/device/store gate.

## What this project is right now

| Level | What exists today | Who is it for |
|---|---|---|
| **Reference system** (working end-to-end) | Headless emitter + receiver sims over a QUIC link: lossless FLAC hash-perfect, lossy Opus bounded, policy gate, recovery. 229 tests, 60-min clean soak PASS. | Developers, CI, verification |
| **Platform shells** (app surfaces) | macOS emitter (ScreenCaptureKit/taps) with a role selector; Windows emitter (WASAPI); Linux emitter (PipeWire) + Linux BT A2DP-sink receiver; **one combined Wavelink app per mobile platform** with an in-app role picker (Android emitter/receiver in a single APK; iOS merged emitter+receiver source). Each compiles + unit-tested; capture/consent/status surfaces real. | Beta testers, developers |
| **Wired streaming in the app shells** | The in-app network transport still plugs into the shells via a `FrameSink` seam (reference system already proves the transport works). | Next milestone |

So: install + exercise the shells today, and know that the proven streaming
core is being wired into the shells next.

## Documentation map

- **[Setup & install](setup-and-install.md)** — get each app onto a device/machine.
- **[Getting started](getting-started.md)** — the receiver + emitter journey.
- **[Free vs Pro](free-vs-pro.md)** — the two tiers and the dev toggle.
- **[Bluetooth](bluetooth.md)** — what works and what never will (honestly).
- **[Privacy & security](privacy-and-security.md)** — local-only, encrypted, redacted.
- **[Troubleshooting](troubleshooting.md)** — common issues and fixes.
- **[Platform support](platform-support.md)** — the full emitter/receiver matrix.

## Quick links

- **Project home / downloads:** the GitHub Pages site (Home → Download).
- **Developer docs:** `docs/planning/` (architecture, protocol, ADRs) and
  `docs/orchestration/` (build status, gates, DoD).
- **Building the apps yourself:** see `docs/orchestration/PACKAGING.md` (`just package`).

## Honest gates (so no one is surprised)

- Real per-app / system capture on macOS (Screen Recording TCC), Windows WASAPI
  native check, Android device capture, iOS `usbAudio`/local-network, USB-DAC
  hotplug, Bluetooth lab, bit-perfect loopback, and signing/notarization/App
  Store submissions all still require **physical hardware, a runner, or
  credentials** — they are not vendored silently. Each has a runbook in
  `docs/planning/HARDWARE_VALIDATION.md` and `docs/planning/RELEASE_AND_SIGNING.md`.
- No app in this repo is a finished, store-signed release build yet. Android
  ships sideloadable signed APKs; macOS ships unsigned (ad-hoc) developer DMGs.
