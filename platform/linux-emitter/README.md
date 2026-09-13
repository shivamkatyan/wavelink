# linux-emitter — Wavelink Linux emitter shell

Emit Linux system audio (or a specific application's audio) to a Wireless DAC
Relay receiver. Part of the B3 desktop-breadth wave.

## What's here
- `src/lib.rs` — platform-agnostic, Linux-buildable seam: `CaptureSource` trait,
  `LinuxEmitterApp` (RT memcpy → frame(seq) → send), Free/Pro lossless policy
  gate, deterministic `FakeCaptureSource` test double. Builds + tests on this
  host (no PipeWire needed).
- `src/pipewire.rs` — the native **PipeWire** backend, `#[cfg(feature="pipewire")]`
  gated. System-wide loopback **and** per-application capture (node targeting —
  a Linux strength). Compiled on a PipeWire-capable runner only.
- `build-check.md` — exact runner commands and platform notes.

## Linux capture capabilities (honest)
- **System-wide**: loopback/monitor of the default sink → whole desktop mix.
- **Per-application**: target a `Stream/Output/Audio` node → that app's audio
  (NOT possible on Windows — a differentiator).
- **No portal consent for audio** (portal gates camera/screen, not audio).
- **Protected content**: no per-stream DRM mute at the graph layer (unlike
  Windows loopback which silences protected streams); product policy still
  governs what you may capture.

## Installing (the .deb)

`apt install ./linux-emitter_*.deb` installs the `linux-emitter` binary plus a
`wdr-linux-emitter.desktop` entry, so the app shows up in the desktop's app
menu and opens a terminal with the CLI when launched (no GUI yet — the capture
backend is hardware/PipeWire-gated). Run directly from a terminal instead:
`linux-emitter --list-format`.

## Build / validate here (no PipeWire)
```bash
cd platform/linux-emitter
cargo build && cargo test && cargo clippy --all-targets -- -D warnings && cargo fmt --check
```

## Native PipeWire build (gated)
See `build-check.md`; requires a Linux runner with `libpipewire-0.3-dev` and
libclang (bindgen). `cargo check --features pipewire`.
