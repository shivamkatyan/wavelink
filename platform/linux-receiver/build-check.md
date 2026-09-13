# linux-receiver — Linux-runner build & validation check

The BlueZ receive backend (`src/bluez.rs`) is BOTH `#[cfg(target_os =
"linux")]`- and feature-`bt`-gated: **it is not compiled on this macOS host**
(no Linux target, no BlueZ daemon). The portable surface (`src/lib.rs` — the
`RenderSink` trait, `LinuxReceiverApp`, `BluetoothMesh` FR-033 matrix,
`FakeRenderSink` + 11 tests) builds and runs here with **zero dependencies**
(`bt`'s `zbus` dep is Linux-target-gated, so the default feature is inert on
macOS/Windows). Nothing here claims a live A2DP registration or render happened
on any build host.

## On this host (macOS, verified 2026-09-10)

```bash
cd platform/linux-receiver
cargo build                                                   # PASS
cargo test                                                    # 11 tests, 0 fail
cargo clippy --all-targets --all-features -- -D warnings      # PASS (clean)
cargo fmt --all -- --check                                    # PASS (clean)
```

## Exact runner commands (Linux CI, BlueZ + optional PipeWire)

```bash
# 1) Portable + BlueZ D-Bus surface (zbus is pure-Rust — no libdbus-1-dev needed).
cd platform/linux-receiver
cargo check --features bt

# 2) Media-sink render wiring (needs libpipewire-0.3-dev + a running PipeWire
#    session for live use). Mirrors linux-emitter's `pipewire` feature.
cargo check --all-features
```

- `zbus = "4"` (MIT, pure-Rust D-Bus): pinned for `org.bluez.ProfileManager1`
  / `org.bluez.Profile1` via `zbus::blocking::Connection` (the `blocking-api`
  feature is a default). If a future pinned zbus renames/moves that module, it
  is a one-line runner fix (or fall back to `dbus-rs` 0.9, also permissive);
  the fix gets recorded in build-check.md.
- `pipewire = "0.10"` (optional, feature `pipewire`, NOT default): the
  `pw_stream::process` OUTPUT stream that pops the SPSC ring into the output
  buffer (RT_CONTRACT Linux PipeWire row). Needs the dynamic
  `libpipewire` (LGPL-2.1 OS library — recorded exception in
  DEPENDENCY_EVALUATION.md / SBOM_POLICY.md; never vendored/statically linked).

## What the adapter does (platform surface, honest)

- Opens the D-Bus **system bus** (`zbus::blocking::Connection::system()` —
  real call; fails with a documented `Error::BlueZ` when no BlueZ daemon runs).
- **Register `Profile1` with the `a2dp_sink` role** at
  `/org/bluez/wdr/a2dp_sink` (A2DP Sink UUID `0000110b-…`, "Role":
  "a2dp_sink") so the box is pairable as a Bluetooth speaker
  (ADR-009 standard-sink path). Live D-Bus export + `RegisterProfile` wiring
  is runner-validated (`cargo check --features bt`) and exercised in the
  **bt-lab** — never on a build host.
- **Receive + render (FR-034):** on `NewConnection(device, fd, …)` take the
  A2DP transport, a worker decodes SBC→PCM and fills the SPSC ring; the
  PipeWire `pw_stream::process` (RT, `PW_STREAM_FLAG_RT_PROCESS`) pops into the
  preallocated output buffer targeting the routed USB DAC
  (`alsa_output.usb-*` / `target.object`). RT side: copies/atomics only, no
  blocking (RT_CONTRACT Linux PipeWire row). The render seam the shell models
  (`push_render`/`pull_block`) is unit-tested on this host; the live graph is
  bt-lab gated.
- **Custom product-peer cell (low-bitrate lossy):** RFCOMM/L2CAP classic
  sockets via `ProfileManager` — app decodes and renders to its own output;
  the FR-033 `BluetoothMesh` matrix exposes which cells are supported vs
  honest-unsupported with the one-action free-lossy-Wi-Fi fallback.

## Platform differences the product UI must surface

1. **Only Linux has a standard-sink receive path** — a Linux box running this
   product is a Bluetooth speaker and renders to its own output/USB DAC.
2. **Stock Android/iOS/Windows/macOS can never be A2DP sinks through this
   product** (no public app API; evidence in PLATFORM_MATRIX §B / ADR-009).
   Unsupported cells get one action: free lossy Wi-Fi (FR-020) — never "route
   to a Bluetooth headset" (FR-034).
3. **BT is lossy in Free and Pro** (A2DP SBC/AAC); Pro lossless (FR-021) is
   Wi-Fi only. `BtFidelity::LossyOnly` / `bt_supports_lossless() == false`.
4. Min: Ubuntu 22.04+/glibc ≥ 2.35, BlueZ (system bus), PipeWire ≥ 1.4 for the
   media-sink render wiring; SBC codec from the system.

## Status

- macOS/macOS (this host, portable surface): `cargo build`, `cargo test` (11
  tests), `cargo clippy --all-targets --all-features -- -D warnings`, `cargo
  fmt --check` green (2026-09-10).
- BlueZ D-Bus surface: **gated** to a Linux runner (`cargo check --features bt`).
- Live A2DP-sink receive + render to a USB DAC: **gated** to the bt-lab
  (HARDWARE_VALIDATION.md Bluetooth row). Do not claim it works until a real
  Linux box + BT hardware exercises it and results are recorded.
