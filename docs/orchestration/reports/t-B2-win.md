# t-B2-win — Wavelink: Windows WASAPI Emitter scaffold report

Task: `t-B2-win` · Root: `R-B2-WIN` · Attempt: 2 · Hypothesis: `H-B2-WIN-2`
Owner role: **Platform Implementer - Windows** · Status: **in_progress** · Date: 2026-09-08

## Deliverable

A standalone Linux-buildable Rust crate `platform/win-emitter` defining its own
`CaptureSource` trait + `FormatMeta`/`EndpointInfo`/`RouteChange`, a
`#[cfg(windows)]`-only WASAPI loopback capture module (`src/wasapi.rs` via the
`windows` crate), a `FakeCaptureSource` Linux test double, a `WinEmitterApp`
(capture callback → byte buffer → seq-wrapped frame → stub `send` closure) with
a Free/Pro lossless policy gate, Linux-runnable tests, and Windows-runner docs.

## Scope (allowed paths only)

- `platform/win-emitter/Cargo.toml`
- `platform/win-emitter/src/lib.rs`
- `platform/win-emitter/src/wasapi.rs`
- `platform/win-emitter/README.md`
- `platform/win-emitter/build-check.md`
- `docs/orchestration/reports/t-B2-win.md`

No core crates are modified. `platform/win-emitter` embeds its own `[workspace]`
declaration so it is standalone and excluded from the root workspace.

## Status so far

Environment verified: Linux host (WSL2), stable `1.98.1` matches
`rust-toolchain.toml`; `x86_64-pc-windows-msvc` std target added so the wasapi
module can be validated with `cargo check --target x86_64-pc-windows-msvc`
(check does not link → no Windows SDK/link.exe required on the runner).

Crate scaffolding and implementation pending (this file is written first per
process rule).

## Supervisor verification (2026-09-09)
- Confirmed: crate standalone (`[workspace]` self-contained), not a root-workspace member.
- `cargo build` + `cargo test` (8 pass) + `cargo clippy --all-targets --all-features -- -D warnings` + `cargo fmt --check` all green on Linux (after `cargo fmt` normalization of wasapi.rs).
- wasapi.rs is `#[cfg(windows)]` and compiles only on a Windows runner; `build-check.md` documents exact commands + limitations.
- **Status: complete** (portable surface validated on Linux; native Windows build gated = `windows-ci`).
