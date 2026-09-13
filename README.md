<div align="center">

# 🔊 Wavelink

**Send a computer's live audio over your local Wi-Fi to a phone or tablet — and
play it through a portable USB DAC.** Free lossy mode for everyone; Pro
lossless mode that is *measured*, never assumed.

**Source-available · [Free for personal use — commercial license required](LICENSE) ·
no cloud, no accounts, no telemetry**

</div>

## What it is

An **emitter** app captures permitted system or application audio from macOS,
Windows, Linux, Android, or iOS. A **receiver** app accepts the stream on
another device and renders it through the selected output — especially a
portable USB DAC connected to a phone or tablet. Wi-Fi supports a free lossy
mode (Opus) and a Pro lossless mode (FLAC / raw PCM with sample-perfect
integrity). Bluetooth is supported only where public platform APIs genuinely
allow it (Linux A2DP sink); everything else is said out loud, with a one-action
fallback.

- **Docs site:** <https://shivamkatyan.github.io/wavelink/>
- **End-user guide:** [`docs/user/README.md`](docs/user/README.md) (install,
  getting started, Free vs Pro, Bluetooth, privacy, troubleshooting)
- **Build status & release evidence:** [`docs/orchestration/RELEASE_STATUS.md`](docs/orchestration/RELEASE_STATUS.md)

## Status (honest)

| Area | Status |
|---|---|
| Reference system (headless emitter⇄receiver over QUIC) | ✅ **green** — lossless FLAC hash-perfect (clean + under 1% loss), lossy Opus bounded, policy intersection, reconnect/recovery; **229 tests pass / 0 fail**; **60-min clean soak PASS** (WSL2 + macOS, same `22153f00…` golden) |
| Platform shells | ✅ macOS emitter (ScreenCaptureKit + Core Audio taps) — **refined window + menu-bar UI** (single Stop/Start, metrics card, severity-colored activity log, permission row with System-Settings deep link, dark/light) **and streams real/fixture audio via the `AudioFrameSink` seam**, **rate-aware** (lossless at the true delivered rate — 44.1k bit-exact; Opus native 44.1k or worker-resampled odd rates →48k; fixture gate hash-perfect vs `ref_receiver`; real SCK capture verified on a TCC-granted session = permission gate for fresh installs) · Windows emitter (WASAPI, interactive menu launcher) · Linux emitter (PipeWire, interactive menu launcher) · one combined **Wavelink** app per platform with an in-app role picker (Android + iOS merged; macOS/win/linux emitter + linux A2DP-sink receiver) — each compiles and unit-tests on the build host |
| Cross-platform desktop GUI | 🟡 `platform/desktop-gui` — pure-Rust **iced** scaffold for Windows/Linux sharing the SAME `AudioFrameSink` engine (real fixture driver, hash-perfect; `CaptureOffline` honest until native capture gate). iced window = runner-gated `gui` feature |
| In-app transport wiring | 🟢 **macOS wired end-to-end** (SCK/fixture → `AudioFrameSink`/`FrameSink` → encode → CRC → QUIC → receiver; `--stream` CLI (fixture or SCK) + Start/Stop in the macOS window/menu-bar UI; fixture path hash-perfect, 44.1k lossless bit-exact). Android/iOS `AudioFrameSink` placeholders are still null — that is the next milestone |
| External gates (never overclaimed) | 🟠 fresh-install Screen Recording TCC + real macOS capture on other hardware, Windows native run, Android device, iOS `.usbAudio`/local-network, USB-DAC hotplug, Bluetooth lab, bit-perfect loopback, store signing/notarization — each has a runbook |

## Quick start (developer)

```bash
source dev/env.sh
cargo build --workspace && cargo test --workspace    # 229 tests, 0 fail
just lint fmt

# Packages (real artifacts)
just package            # android (signed APKs) + macos (.app + DMG) here
just package linux      # tarball + .deb via the dev container
just package windows    # win-emitter.exe zip (Windows runner)
just package ios        # unsigned iOS cores + docs
WDR_DIST_DIR=/tmp/dist just package macos

# Static docs site (builds site_build/, deploy via .github/workflows/pages.yml)
just site
```

## Repository map

```
crates/            shared Rust core (proto, crypto, entitlement, codec, session,
                   transport, telemetry, fakes, refsim, dev)  — the proven engine
platform/          per-OS app shells (macos-emitter, win-emitter, linux-emitter,
                   linux-receiver, desktop-gui, and the combined android-wavelink
                   / ios single-app surfaces with in-app role pickers; the older
                   split android/ios role apps are superseded)
docs/user/         end-user documentation (rendered to the static site)
docs/planning/     architecture, protocol, ADRs, platform matrix, security spec
docs/orchestration/ build status, gates, task ledger, DoD reconciliation, packaging
docker/ + compose.yml  netem-impaired reference harness (soak, CC spike)
scripts/package/   packaging pipeline    scripts/site/  static-site build
.github/workflows/ ci · release (packages + GitHub Release) · pages (docs site)
```

## Documentation

- **End user:** [`docs/user/`](docs/user/) — install, first run, Free vs Pro,
  Bluetooth, privacy & security, troubleshooting, platform support.
- **Planning/architecture:** [`docs/planning/`](docs/planning/README-oriented:
  ARCHITECTURE, PROTOCOL_SPEC, ADRS/001–010, PLATFORM_MATRIX, THREAT_MODEL,
  RT_CONTRACT, HARDWARE_VALIDATION, RELEASE_AND_SIGNING, DELIVERY_PLAN).
- **Orchestration:** [`docs/orchestration/`](docs/orchestration/) — release
  status, quality dashboard, acceptance checklist, packaging, decision log,
  incident reports.
- **Packaging:** [`docs/orchestration/PACKAGING.md`](docs/orchestration/PACKAGING.md).

## Contributing & support

Wavelink is a **commercial, source-available** product: the code is published
for transparency and review, but it is not open for redistribution or reuse
(see [`LICENSE`](LICENSE)). Please route questions, feature requests, and bug
reports through **[GitHub issues](https://github.com/shivamkatyan/wavelink/issues)** —
not forked builds or PRs.

The project runs on an evidence-before-claims culture: hardware/SLO claims need
a runbook + named gate, and real-time paths must stay RT-clean
([`docs/planning/RT_CONTRACT.md`](docs/planning/RT_CONTRACT.md)). Dependency
policy stays permissive-only
([`docs/planning/DEPENDENCY_EVALUATION.md`](docs/planning/DEPENDENCY_EVALUATION.md)).

## License

Wavelink is **proprietary, source-available** software — see [`LICENSE`](LICENSE).
Free for personal (non-commercial) use of the official builds; commercial use
requires a paid license. Third-party components remain under their own
permissive licenses — see
[`docs/planning/LICENSE-NOTICES.md`](docs/planning/LICENSE-NOTICES.md) for the
attribution notices and SBOM policy.
