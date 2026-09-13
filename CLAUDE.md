# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

Wavelink ("WDR"): an **emitter** app captures system/app audio from a computer, a **receiver** app renders it on another device through a portable USB DAC, over LAN Wi-Fi. Free tier = lossy (Opus); Pro tier = lossless (FLAC/raw PCM, hash-verified). One QUIC connection per session (reliable control stream + unreliable datagrams for lossy media / reliable stream for lossless). No cloud, no accounts, no telemetry.

The project runs on plan–task–gate phases (P0…B7) tracked in `docs/orchestration/`; the reference system (headless emitter⇄receiver over QUIC) is green — **229 tests pass / 0 fail**.

## Commands

```bash
source dev/env.sh            # ALWAYS source first — sets CARGO_HOME + PATH used everywhere
./dev/bootstrap              # idempotent bootstrap: rustup (pinned stable), just, cargo-ndk, zigbuild, cross

cargo build --workspace
cargo test --workspace       # 229 tests, 0 fail (includes proptest + fuzz-smoke)
cargo test -p <crate>        # single crate, e.g. wdr_proto
cargo test -p <crate> <name> # single test/filter
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo fmt --all -- --check

just format lint unit build selftest   # fmt-check / clippy -D warnings / wdr_dev tests / build / unit+lint
just audit license sbom                # cargo audit (RustSec), cargo deny check licenses, SBOM lock inventory
just package [linux|windows|ios]       # packaging via scripts/package/all.sh (see PACKAGING.md)
WDR_DIST_DIR=/tmp/dist just package macos   # native on this host: android + macos by default
just site                              # build static docs site into site_build/ (deploy: pages.yml)

# Reference-sim harness (Docker + netem) — see compose.yml / B1-SOAK-EVIDENCE.md
cargo build -p wdr_refsim --release    # rebuild before any soak
docker compose up -d --build
bash docker/bootstrap-netem.sh   # full profile suite + asserts (idempotent)
bash docker/soak.sh              # 60-min clean soak (see B1-SOAK-EVIDENCE.md)
bash docker/ccspike.sh           # congestion-control spike (WDR_CC=cubic/bbr)
```

Platform shells are **separate workspaces** with their own `Cargo.lock`; build/test them from their own directory:

```bash
cd platform/macos-emitter && cargo build
TOOL=/Applications/Xcode.app/Contents/Developer/Toolchains/XcodeDefault.xctoolchain/usr/lib/swift-5.5/macosx
DYLD_LIBRARY_PATH="$TOOL" cargo test   # macos shell tests link a Swift SCK shim; needs this loader path
cd platform/win-emitter && cargo test  # portable surface (WASAPI behind #[cfg(windows)])
cd platform/android-wavelink && ./gradlew :app:assembleDebug :app:testDebugUnitTest   # single combined app
```

## Architecture

### The core/shell split (the single most important structure)

Two disjoint build graphs:

- **`crates/` — the shared, platform-free Rust core** (the root workspace, `members = ["crates/*"]`). This is the proven engine: `wdr_proto` (wire schema/goldens), `wdr_crypto` (Noise XX pairing, AEAD, keys), `wdr_entitlement` (Free/Pro policy), `wdr_codec` (Opus/FLAC/PCM adapters, TPDF dither), `wdr_session` (session FSM), `wdr_transport` (quinn QUIC), `wdr_telemetry`, `wdr_fakes` (deterministic PCM + HashSink + fake adapters), `wdr_rt` (SPSC ring — the RT contract primitives), `wdr_refsim` (headless emitter/receiver sim), `wdr_dev` (DevEx placeholder).
- **`platform/` — per-OS native shells** (`macos-emitter`, `win-emitter`, `linux-emitter`, `linux-receiver`, the combined `android-wavelink` and `ios` single-app surfaces with in-app role pickers, `desktop-gui`). Each is a **standalone workspace explicitly NOT in the root** with its own `Cargo.lock`. Pattern (see `macos-emitter/README.md`): a host-portable `lib.rs` seam (`CaptureSource` trait + `FakeCaptureSource` + policy gate) builds/tests anywhere; `#[cfg(target_os = "...")]` backend modules hold platform FFI. macOS `win/linux` emitters carry a real launcher UI; `app/main.swift` etc.

**Streaming seam** (the modern shell↔core bridge): shells reach the proven engine through the `AudioFrameSink`/`FrameSink` traits in `wdr_refsim::sink` — `wdr_refsim::sink::QuicAudioSink` owns its tokio+quinn runtime. The macOS shell is **already wired end-to-end** (SCK/fixture → `AudioFrameSink` → encode → CRC → QUIC → receiver, driven by `macos-emitter --stream` and Start/Stop in the AppKit UI); it depends on `wdr_proto`/`wdr_fakes`/`wdr_entitlement`/`wdr_refsim` by path. This deliberately reverses the earlier "zero core-crate deps" stance now that a real stream exists — the seam still keeps shells free of direct tokio/quinn/codec deps. Next milestone: Android/iOS `AudioFrameSink` wiring (their placeholders are still null).

### Audio data flow (both directions)

Capture → memcpy into a preallocated SPSC ring (RT thread: only this) → worker: normalize → accumulate → encode Opus (lossy) or FLAC/raw PCM with per-frame CRC (lossless) → AEAD → QUIC (**lossy = unreliable datagram, lossless = reliable stream with retransmit deadline**) → receiver: reader → SPSC ring → reorder window → decode → jitter buffer → drift estimator + bounded resample → render-format conversion → dedicated render thread.

### RT_CONTRACT (non-negotiable)

A platform **RT audio callback may only** copy bytes in/out of `wdr_rt::spsc::SpscRing` plus trivial atomic/int math. No alloc, no locks, no syscalls, no logging, no codec/encrypt/transport calls on RT threads — all of that lives on dedicated workers. The RT-side API is `SpscRing::try_push` (capture producer) / `try_pop_exact` (render consumer): no-alloc, no-lock, no-syscall by construction, and `with_capacity` allocates exactly once, off-RT. Enforcement is by the contract itself — the `panic = "abort"` profiles and `rt-guard` abort-on-alloc allocator described in `docs/planning/RT_CONTRACT.md` §4 are **designed, not yet implemented** (no such profile/feature exists in any Cargo.toml today; `wdr_rt`'s own docs say the discipline is "enforced by the RT contract, not by runtime guards"). Per-platform allowed/forbidden tables: `docs/planning/RT_CONTRACT.md`. Never put encode/decode/transport in a platform callback.

### Entitlement seam

`EntitlementProvider` in `wdr_entitlement` is the only seam through which session/network/UI code asks the tier question. **Free tier never sends lossless** — enforced in `wdr_refsim`. The shipped `DevToggleEntitlementProvider` is a dev/demo toggle (FR-045), not a security control; a real commerce backend layers behind the same trait.

### Golden losslessness

`wdr_fakes` owns deterministic fixtures (silence, impulse, sine sweep, full-scale edge, seeded pseudo-random PCM, mono/stereo channel-id, 16/24-bit) with the canonical byte form (16-bit = i16 LE interleaved; 24-bit = i32-by-value, top byte zero). `NullSource → PcmSource → HashSink` hashes the canonical stream; `hash(decoded) == golden` proves true losslessness. Goldens live in `docs/orchestration/reports/t-B0-fakes.md`.

## The reference-sim harness

`wdr_refsim` (`ref_emitter` / `ref_receiver` bins) is the end-to-end evidence pipeline: headless emitter encodes `wdr_fakes` fixtures over quinn QUIC loopback, receiver runs jitter buffer + decode + HashSink, and both publish `*-sim.json` metrics; `docker/` scripts drive it under `tc netem` impairment profiles (loss/jitter/reorder/duplication/bandwidth/disconnect). **Before any soak, rebuild the refsim release binary** (`cargo build -p wdr_refsim --release`). Dockerfile (`wdr-dev`, Ubuntu 24.04) does NOT install libclang — bindgen feature builds need `LIBCLANG_PATH` or a dependency added to the image.

## Evidence-before-claims culture (read this before changing behavior)

- **`docs/planning/` is the source of truth**: ARCHITECTURE, PROTOCOL_SPEC, ADRS/001–010, PLATFORM_MATRIX, RT_CONTRACT, SECURITY_SPEC, TEST_PLAN, RISK_REGISTER, and the FR-by-FR honest status audit in REQUIREMENTS_TRACEABILITY.md (✅/🟡/🔒/🚫/⬜).
- **`docs/orchestration/` tracks gates**: RELEASE_STATUS, QUALITY_DASHBOARD, PACKAGING, ACCEPTANCE_CHECKLIST, DECISION_LOG, plus `reports/` and `incidents/`.
- Hardware/SLO claims (real capture on hardware, native RT timing, signing/notarization, USB-DAC hotplug, bit-perfect loopback) are **external gates that cannot be satisfied on any single host** — never mark them done without evidence. A "pending gate" is allowed only with its runbook (HARDWARE_VALIDATION.md / RELEASE_AND_SIGNING.md). Mark simulated runs as simulated.
- Most CI jobs (ci.yml, license-audit.yml, reference-sim.yml) are deliberately `if: false` until a self-hosted runner/credentials exist; release.yml and pages.yml are live. Read the workflow headers before "fixing" the disabled ones.

## Conventions

- Cargo.lock committed; toolchain pinned in `rust-toolchain.toml` (stable + rustfmt + clippy).
- License policy (deny.toml + DEPENDENCY_EVALUATION.md): **permissive-only**. GPL/AGPL/SSPL denied; MPL-2.0 allowed only for the recorded uniffi decision. Record every new dependency in `docs/planning/DEPENDENCY_EVALUATION.md`.
- Institutional honesty is explicit in the docs ("measured, never assumed", "render-format conversion vs bit-perfect never conflated") — keep strong real-time claims tied to the measured reports in `docs/orchestration/reports/`.
- Shells' packaging: `scripts/package/*.sh` (honor `WDR_DIST_DIR`, `WDR_NO_INTERACTIVE`); Windows EXE can only link on the Windows runner — locally just `cargo check --target x86_64-pc-windows-msvc`.
