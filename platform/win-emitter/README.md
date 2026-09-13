# win-emitter — Windows WASAPI emitter scaffold

The Windows **emitter** (capture) side of Wavelink. It records the
system audio (playback) mix via a WASAPI loopback device and forwards frames to
a receiver. This crate is the small standalone seam: the portable
`CaptureSource` surface builds and unit-tests on any host; the actually
capturing WASAPI backend is `#[cfg(windows)]`-gated and compiles/validates on a
Windows runner or machine.

See `docs/orchestration/reports/t-B2-win.md` for the original delivery and
`build-check.md` for the current validate-vs-gate ledger.

## What it does

- Captures the **system-wide** playback mix over WASAPI loopback (no per-process
  PCM API exists on Windows — "system output", not "app X audio").
- Enumerates render endpoints; selects the default or by friendly name.
- Follows the RT discipline (`docs/planning/RT_CONTRACT.md`): the capture
  callback only memcpys into a preallocated buffer; frame production/send
  happens on a worker.
- Exposes a Free/Pro lossless policy gate (`allow_lossless`) that fails closed.

## Build

```bash
# Portable surface (works on any host):
cd platform/win-emitter && cargo test          # 8 tests

# Windows compile gate (needs the windows-msvc target; link needs Windows):
rustup target add x86_64-pc-windows-msvc
cargo check --target x86_64-pc-windows-msvc

# Release EXE (Windows runner — this is what `just package windows` produces):
cargo build --release --target x86_64-pc-windows-msvc --bin win_emitter
```

## Run

```bat
win_emitter.exe --list-format   :: enumerate render endpoints
win_emitter.exe --version
```

No-arg invocation prints usage; when the exe is double-clicked from Explorer
(console process count == 1) it holds the window open until Enter so the text
is readable instead of flashing and closing.

## Honest limitations (surface in UI + docs)

1. **System-wide only** — no per-application PCM capture on Windows via public
   API (session control is not PCM).
2. **Protected content returns silence** in loopback (OS-enforced).
3. **USB-DAC identification** is by friendly/instance name; VID/PID correlation
   is a documented follow-up.
4. Min supported: Windows 11 24H2+ (Win10 22H2 is EOL/own-risk per ADR-008).

## License

Wavelink is proprietary, source-available software (see the repo [`LICENSE`](../../LICENSE)).
Free for personal use of the official builds; commercial use requires a paid
license. Third-party attribution notices: `docs/planning/LICENSE-NOTICES.md`.
