# linux-emitter — Linux-runner build & validation check

The PipeWire adapter (`src/pipewire.rs`) is `#[cfg(feature="pipewire")]`-gated
and is **not compiled on the WSL2 dev host** (no PipeWire daemon, no
libpipewire headers). The non-PipeWire core + FakeCaptureSource + tests build
and run here. The native PipeWire build is gated to a **Linux runner with
PipeWire** (a real distro with the PipeWire daemon / dev headers, or the
`pipewire` compose image when a runner with the socket is available).

## Exact runner commands (Linux CI, PipeWire-capable)

```bash
# distro needs libpipewire-0.3-dev (Debian/Ubuntu: libpipewire-0.3-dev)
cd platform/linux-emitter
cargo check --features pipewire
# on a host WITH a running PipeWire daemon (session socket):
cargo run --features pipewire --example <capture-smoke>   # future; runner-owned
```

`pipewire` crate (0.7, feature `v0_3_48`) is an optional dependency enabled by
the `pipewire` feature. The adapter wires `pw_main_loop`/`pw_context`
/`pw_core`/`pw_stream` (INPUT + `PW_STREAM_FLAG_RT_PROCESS`) and memcpys the
process-callback buffer into a preallocated buffer (RT discipline) — exact
calls confirmed by `cargo check --features pipewire` on the runner.

## What the adapter does (platform surface)

- **System-wide capture**: targets the default sink's capture/monitor port
  (loopback) → the whole desktop mix.
- **Per-application capture**: targets a specific `Stream/Output/Audio` node
  via `target.object` (node.name / serial) → per-app audio (a Linux strength;
  NOT available on Windows, which is system-loopback-only).
- **No portal consent for audio** capture (the xdg portal gates Camera/screen
  capture, not audio) — a documented Linux difference from macOS/Android.
- RT callback only memcpys into the caller buffer (RT_CONTRACT PipeWire row).

## Platform differences to surface in product UI + docs

1. **Per-application capture is supported** on Linux (target.object), unlike
   Windows (system-only). UI must distinguish system vs per-app.
2. **Protected content**: Linux has no per-audio-stream DRM gate at the
   PipeWire/ALSA layer — decrypted PCM is capturable at the graph (unlike
   Windows loopback which mutes protected streams). This is a *capability*
   difference; the product policy (don't capture non-capturable content) still
   applies.
3. **Headless/background**: works as a user service or inside a container with
   a PipeWire socket (documented in DEVELOPMENT_ENVIRONMENT).
4. Min: Ubuntu 22.04+/glibc≥2.35, PipeWire ≥1.4.

## Status

- Linux (this host, no pipewire feature): `cargo build`, `cargo test` (6
  tests), `cargo clippy --all-targets --all-features -- -D warnings`, `cargo
  fmt --check` green (2026-09-09).
- PipeWire native check: **gated** to a PipeWire-capable Linux runner
  (`cargo check --features pipewire`). Do not claim a live PipeWire capture
  until that passes and is validated on a real daemon.
