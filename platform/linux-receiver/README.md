# linux-receiver — Wavelink Linux Bluetooth A2DP-sink receiver shell

The **only full public Bluetooth receive+render path** this product supports
(ADR-009 / PLATFORM_MATRIX §B Linux row): a Linux box running this product
registers a BlueZ `a2dp_sink` profile, receives A2DP (SBC/AAC — lossy), and
renders the decoded PCM through a PipeWire media sink to the selected output —
its own USB DAC (FR-034 product-receiver definition: the product device
receives AND renders; routing to an ordinary Bluetooth headset does NOT count).

Part of the B5 Bluetooth wave.

## What's here
- `src/lib.rs` — platform-agnostic, Linux-buildable seam: `RenderSink` trait,
  `LinuxReceiverApp` (RT `push_render` memcpy → preallocated buffer →
  worker `pull_block` → frame(seq) → render closure), Free/Pro lossless policy
  via `allow_lossless` (Wi-Fi lossless **only**; BT is lossy in every tier —
  `bt_supports_lossless()` is always `false`), the honest
  `BluetoothMesh` FR-033 support matrix + one-action free-lossy-Wi-Fi fallback
  (FR-034), deterministic `FakeRenderSink` test double. Builds + tests on this
  host — no BlueZ or PipeWire needed.
- `src/bluez.rs` — the native **BlueZ A2DP-sink** backend, BOTH
  `#[cfg(target_os = "linux")]`- AND feature-`bt`-gated: opens the D-Bus system
  bus (`zbus`, MIT, pure Rust), registers `org.bluez.Profile1` with the
  `a2dp_sink` role, receives the A2DP transport, and renders via a PipeWire
  media sink (feature `pipewire`) targeting the USB DAC. Compiled only on a
  Linux runner; live registration/render is bt-lab validated — never claimed on
  a build host.
- `build-check.md` — exact on-host commands vs Linux-runner/bt-lab gates.
- `src/bin/linux_receiver.rs` — the **runnable entrypoint** (new 2026-09-12; the
  crate previously had NO binary — an installed box had nothing to launch). A
  no-args launch with a TTY opens an interactive menu (render format,
  Bluetooth-support honesty matrix, **register A2DP sink**, version) that stays
  open until quit; non-TTY keeps usage+exit. `--register` really calls BlueZ
  only on a Linux + `bt` build with a live system bus; everywhere else it
  prints the concrete bt-lab gate. `scripts/package/linux.sh` now ships
  `linux-receiver` (tarball + `.deb` with a `wdr-linux-receiver.desktop`
  entry) alongside the emitter.

## Launching / "opens" UX
- Double-click (or app menu / dash via the `.desktop` entry): the interactive
  menu opens in a terminal and stays until `q` — no more "nothing happened".
- `linux-receiver --register` on a Linux + `bt` build: registers the
  BlueZ `a2dp_sink` profile (needs a live system bus — bt-lab gate).
- Everything else is headless *by design* (a service/device role): the honest
  limit is that live A2DP receive+render is bt-lab/runner-gated, never claimed
  on the build host.

## Bluetooth honesty (the contract of this API)
- **Linux standard A2DP sink = supported** and is the only full public
  receive+render cell (product device renders to its own output/DAC).
- **Stock Android / iOS / Windows / macOS A2DP sink = unsupported by public
  API** (evidence in PLATFORM_MATRIX §B / ADR-009). The product never implies a
  stock phone/OS can be a Bluetooth speaker through us.
- **Custom classic-socket product peers (RFCOMM/L2CAP) on Android /
  Windows-RFCOMM / Linux = supported, low-bitrate lossy** — the app decodes and
  renders to its own output/DAC; never claimed as hi-fi.
- Every unsupported cell offers **one action: free lossy Wi-Fi** (FR-020).
  Routing received audio on to a BT headset never satisfies the
  product-receiver definition (FR-034).
- The BT receive path is **lossy-only in Free AND Pro** — Pro's lossless
  (FR-021) is delivered over Wi-Fi, never Bluetooth. `BtFidelity::LossyOnly` /
  `bt_supports_lossless() == false` make that mechanically checkable.

## Build / validate here (no BlueZ, no PipeWire)
```bash
cd platform/linux-receiver
cargo build && cargo test && cargo clippy --all-targets --all-features -- -D warnings && cargo fmt --all -- --check
```

## Native BlueZ / PipeWire build (gated)
See `build-check.md`; requires a Linux runner with a BlueZ system bus (zbus is
pure Rust — no libdbus needed) and, for the media-sink render wiring,
`libpipewire-0.3-dev` (`cargo check --features bt,pipewire`). Live
A2DP-sink receive + render to a USB DAC is bt-lab hardware-gated
(HARDWARE_VALIDATION.md Bluetooth row).
