# Development Environment

Verified on this host (2026-09-06): Linux under WSL2 (kernel 6.18.33.2-microsoft-standard-WSL2), Docker 29.7.2 (WSL2 backend), node 24.19.0, python 3.12.3, git 2.43.0. Rust toolchain and `just` not yet installed — installed and pinned by bootstrap.

## WSL2 prerequisites
Windows 11 + WSL2 + Docker Desktop (WSL2 backend). Whole-disk check: Docker-in-WSL works; nothing platform-audio can run in Docker (native adapters are validated on native runners / hardware).

## Bootstrap (`./dev/bootstrap`)
Single idempotent entry point: install `rustup` (pinned stable toolchain), `just`, `cargo-ndk`, `zigbuild`, `cross` as needed and dev deps; verify host capabilities; write env checks. Fresh-clone runnable, noninteractive. Dependency acquisition cached/retryable; offline behavior documented after first bootstrap.

## Task runner (`justfile`)
Targets: `format lint unit integration build test-e2e package clean-verify benchmark selftest coverage`.

## Toolchain pins
Rust stable pinned in `rust-toolchain.toml`; `Cargo.lock` committed. Android: AGP/Kotlin/Gradle pinned (AGP 9.4 / Gradle 9.6 / JDK 17 per P0). Apple: pinned Xcode/Swift in CI. Dependency policy + maintenance rubric in `DEPENDENCY_EVALUATION.md`.

## Docker / dev container
- Dev container for Linux-buildable shared services/tools.
- **Docker Compose local harness**: emitter-simulator, impaired network (`tc netem`, container `NET_ADMIN` capability documented; profiles: loss, jitter, reorder, duplication, bandwidth, disconnect), receiver-simulator, metrics collector, assertion runner.

## Native runners (CI)
- Linux runner/container: core, protocol, Linux adapters, sanitizers, fuzz smoke, netem tests.
- Windows runner: WASAPI capture/render integration, packaging, install/uninstall smoke. Hosted-runner audio-endpoint capability P — probed in first CI sprint with a virtual-endpoint plan.
- macOS runner: macOS + iOS compile/test, permission-state unit tests, simulator-safe tests, packaging, notarization dry-run. Headless macOS capture = Core Audio process-taps path (TCC-free); SCK behind TCC/hardware tasks.
- Android emulator: lifecycle/UI/permissions/network/codec/fake-audio (**no USB audio**).
- iOS simulator: UI/lifecycle/protocol/fake-audio (no real capture/USB).

## Virtual audio devices
PipeWire null sink/loopback; ALSA `snd-aloop`; WASAPI virtual endpoint (P, win); `hci_vhci` virtual BT HCI.

## Signing placeholders
- Windows: unsigned MSIX/EXE now; Azure Artifact Signing / CA-cert paths documented.
- macOS: ad-hoc + unsigned now; Developer ID + notarytool dry-run config ready.
- Android: self-generated release keystore now; store publish gated.
- iOS: dev signing on CI now; provisioning/App Store gated.
Missing signing credentials are NOT a reason to leave packaging unimplemented.

## Clean-machine verification
`./dev/validate-clean` builds + tests + packages from a fresh clone in a clean container/VM.
