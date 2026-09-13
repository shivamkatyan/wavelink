# t-B5-linux-receiver — Linux standard A2DP-sink receiver shell (BlueZ Profile1 + PipeWire media sink)

**task_id:** t-B5-linux-receiver · **root_task_id:** B5-linux-receiver ·
**hypothesis_id:** H-B5-LIN-RX-1 · **owner_role:** Platform Implementer - Linux ·
**date:** 2026-09-10 · **status:** complete (portable surface validated on macOS host; native BlueZ/PipeWire evidence is Linux-runner/bt-lab gated)

## Summary

Standing up the **Linux receiver** shell (`platform/linux-receiver`) for the ONLY
full public Bluetooth receive+render path the product supports (ADR-009 /
PLATFORM_MATRIX §B Linux row): a Linux box running this product registers a BlueZ
`org.bluez.Profile1` with the **`a2dp_sink`** role (pairable as a Bluetooth
speaker), receives A2DP (SBC/AAC — lossy by design), and renders the decoded PCM
through a PipeWire media sink to the selected output — its own USB DAC. This is
the FR-034 product-receiver definition (the product device receives AND renders
to its own output/DAC; routing to an ordinary BT headset does NOT count, and the
honesty matrix below says so).

Delivered: (1) `Cargo.toml` — standalone `[workspace]` mirroring the
`win/linux/macos-emitter` shell pattern; portable surface has **zero deps**;
native deps live under `[target.'cfg(target_os = "linux")'.dependencies]`
(`zbus` 4.x MIT pure-Rust for the BlueZ D-Bus surface, optional `pipewire` 0.10
for the media-sink render wiring) and are feature-`bt`-gated so the crate builds
anywhere without a Linux BT stack; (2) `src/lib.rs` — `RenderSink` trait
(start/stop/format/route), `FormatMeta`/`EndpointInfo{name,is_usb,sink_role}`/
`SinkRole`, BT-receiver `RouteChange` (SinkConnected, SinkDisconnected,
A2DPProfileReady, RFCOMMPeerConnected{name}, FormatChanged), `Error` (+Display +
std::error::Error), `LinuxReceiverApp<S,R>` (RT `push_render` memcpy into a
preallocated buffer → worker `pull_block` → seq-numbered `Frame` → render
closure), policy (`allow_lossless` gates **Wi-Fi** lossless only; BT path is
lossy in Free AND Pro), the **`BluetoothMesh` FR-033 support matrix + one-action
free-lossy-Wi-Fi fallback** and `ReceiverRole`, `BtFidelity::LossyOnly` +
`bt_supports_lossless() == false`, and a deterministic `FakeRenderSink` — 11
unit tests; (3) `src/bluez.rs` — the Linux+bt-gated native skeleton (system-bus
connect via real `zbus::blocking::Connection::system()`, Profile1 a2dp_sink
registration surface, NewConnection fd→worker handoff, RT–ring discipline per
RT_CONTRACT Linux row, feature-`pipewire` media-sink `RenderSink`), honestly
runner/bt-lab-validated with zero live-registration claims; (4) README.md +
build-check.md; (5) this report.

**On this host (macOS):** `cargo build` PASS, `cargo test` **11/11 pass**,
`cargo clippy --all-targets --all-features -- -D warnings` PASS, `cargo fmt
--all -- --check` PASS. The native BlueZ/PipeWire part is NOT compiled here
(target + feature gates) — exactly the honest split the sibling
`linux-emitter`/`pipewire.rs` uses; live A2DP-sink receive+render to a USB DAC
is bt-lab hardware-gated (HARDWARE_VALIDATION.md Bluetooth row / `bt-lab`).

## Files changed

| Path | Change |
|---|---|
| `platform/linux-receiver/Cargo.toml` | Standalone `[workspace]`; `default = ["bt"]`; `bt = ["dep:zbus"]`, `pipewire = ["dep:pipewire"]`; native deps under `[target.'cfg(target_os = "linux")'.dependencies]` |
| `platform/linux-receiver/Cargo.lock` | Locked 146 pkgs incl. target-gated `zbus 4.4.0` + `pipewire` (resolved, not compiled on this host); committed per repo dependency policy |
| `platform/linux-receiver/src/lib.rs` | Portable surface (trait, app, matrix, fakes) + 11 unit tests + A2DP-honesty docs |
| `platform/linux-receiver/src/bluez.rs` | Linux+`bt`-gated BlueZ A2DP-sink / PipeWire media-sink native skeleton (runner/bt-lab validated) |
| `platform/linux-receiver/README.md` | Role, honest-BT contract, on-host build/test vs native gates |
| `platform/linux-receiver/build-check.md` | Exact on-host commands + Linux-runner/bt-lab split |
| `docs/orchestration/reports/t-B5-linux-receiver.md` | This report |

## Decisions

1. **Standalone `[workspace]` + zero portable deps**, mirroring the
   `win/linux/macos-emitter` shells exactly; `default = ["bt"]` is inert off-Linux
   because `bluez` is `#[cfg(target_os = "linux")]`-gated, so the crate builds on
   any host with no Linux BT stack.
2. **`zbus = "4"` (MIT, pure-Rust) for the BlueZ D-Bus surface** — no `libdbus`
   system dependency, so `cargo check --features bt` is one command and clean on a
   fresh Linux runner. `PipeWireMediaSink` wiring is a separate non-default
   `pipewire` feature (mirror of linux-emitter), so a bare runner without
   libpipewire-dev still builds the `bt` surface. Both are permissive per
   deny.toml; libpipewire itself is LGPL-2.1 dynamically-linked OS library
   (recorded exception) — never vendored.
3. **Honest runner-validated seam, identical discipline to the sibling**: the only
   "real" native call in the portable-build path is
   `zbus::blocking::Connection::system()` (fails → documented `Error::BlueZ`);
   Profile1 export/`RegisterProfile`/NewConnection fd handoff and the pw_stream
   render wiring return documented `Error::BlueZ`/`Error::PipeWire`
   "runner/bt-lab-validated" errors — nothing ever claims a live registration or
   render happened on a build host.
4. **BT path is lossy-only in Free AND Pro** — `bt_supports_lossless()` is a const
   `false`; `allow_lossless("Pro")` gates Wi-Fi lossless (FR-021) only. `BtFidelity`
   Display text makes the rule auditable.
5. **Honesty matrix is pure data (`BluetoothMesh`)**, not runtime probing: every
   cell is a documented public-API fact (PLATFORM_MATRIX §B / ADR-009). Exactly one
   standard-sink cell is `Supported` (Linux); all other stock-OS standard-sink
   cells are `UnsupportedByPublicApi` with a `FreeLossyWifi` one-action fallback;
   RFCOMM/L2CAP product peers are `SupportedLossy` on Android/Windows-RFCOMM/Linux;
   iOS peer is MFi-scoped/unsupported. A test forbids any fallback from suggesting
   routing to a BT headset (FR-034).
6. **`Error` mirrors emitter but BT-flavored** (`BlueZ(String)` / `PipeWire(String)`
   decorations) and implements `Display`/`std::error::Error`.
7. **`RenderSink` kept to exactly {start, stop, format, route}**; `route` targets the
   output by device name (USB DAC), fails `EndpointNotFound` on empty/unknown, and
   a failed route never clobbers the previous target (tested).

## Commands run (this host, exact)

```bash
source /Users/sk/Developer/relay/ps/dev/env.sh        # CARGO_HOME/RUSTUP_HOME/PATH

cd platform/linux-receiver
cargo build                                           # exit 0; zbus/pipewire resolved, NOT compiled (target gate)
cargo test                                            # exit 0; 11 passed / 0 failed (+ 0 doc-tests)
cargo clippy --all-targets --all-features -- -D warnings   # exit 0; clean
cargo fmt --all -- --check                            # exit 0; clean
# environment: Rust 1.98.1 (aarch64-apple-darwin), macOS; no Linux target / BlueZ / PipeWire here
```

## Validation results

- `cargo build`: PASS (ported surface, zero compiled deps on this host).
- `cargo test`: **11/11, exit 0** — allow_lossless Free/unknown refusals; BT never
  lossless even on Pro (+ fidelity text); FakeRenderSink deterministic (5 ms/960
  bytes at 48k/stereo); app seq + deliver to render closure; `push_render`
  overflow no-panic; route select/reject/keep-previous; render-sink lifecycle
  (AlreadyStarted, idempotent stop); route-change all-variants exhaustive match;
  A2DP matrix Linux-only-full-path + honest unsupported cells; matrix never offers
  a headset fallback; receiver roles lossy-only.
- `cargo clippy --all-targets --all-features -- -D warnings`: clean (bluez.rs not
  compiled here; its own future runner lint pass is part of the `bt` gate).
- `cargo fmt --all -- --check`: clean.
- **Not verifiable here (native gates):** BlueZ Profile1 registration, real A2DP
  sink receive + render to a USB DAC on a Linux box, RFCOMM low-bitrate fallback
  cell, PipeWire media-sink render RT behaviour — all bt-lab / Linux-runner
  (HARDWARE_VALIDATION.md Bluetooth row).

## Acceptance criteria

| Criterion (task brief) | Status | Evidence |
|---|---|---|
| Project tree under `platform/linux-receiver` complete (Cargo.toml standalone `[workspace]`, lib.rs, bluez.rs, README, build-check) | ✅ | Files changed table |
| Portable surface compiles/tests on this host — `cargo build` && `cargo test` | ✅ | build PASS; 11 tests, 0 fail, exit 0 |
| `cargo clippy --all-targets --all-features -- -D warnings` clean | ✅ | clippy PASS |
| `cargo fmt --all -- --check` clean | ✅ | fmt PASS |
| Native deps under `[target.'cfg(target_os = "linux")'.dependencies]`, feature-`bt` (default) so crate builds anywhere without a Linux BT stack | ✅ | Cargo.toml; default build green on macOS |
| `linux-receiver` = only full public BT receive+render path per ADR-009; FR-034 (product device receives AND renders; no headset routing) honored | ✅ | lib.rs docs, `BluetoothMesh` + fallback test, README/build-check |
| `RenderSink { start, stop, format, route }` + `LinuxReceiverApp` RT `push_render`/worker `pull_block` + policy gate | ✅ | lib.rs; unit-tested |
| A2DP capability honesty: per-cell matrix + one-action fallback to free lossy Wi-Fi (FR-033) | ✅ | `BluetoothMesh`/`ReceiverRole`; matrix tests |
| Lossy-only-BT policy exposed (`bt_supports_lossless()` = false / `BtFidelity::LossyOnly`) | ✅ | lib.rs + test |
| FakeRenderSink determinism + route-change exhaustive match | ✅ | tests |
| `src/bluez.rs` — Linux+bt-gated REAL skeleton: Profile1 (a2dp_sink) via D-Bus, object expose, PipeWire media-sink render, RT–ring handoff; marked runner/bt-lab gated; NOT claimed compiling-validated here | ✅ | bluez.rs; build-check honest split |
| Report `docs/orchestration/reports/t-B5-linux-receiver.md` | ✅ | this file |
| Live BlueZ registration + A2DP receive + USB-DAC render on a real Linux box | 🔒 PENDING | `bt-lab` / linux-runner gate; runbook in build-check + HARDWARE_VALIDATION |
| RFCOMM low-bitrate fallback cell live test | 🔒 PENDING | `bt-lab` gate |
| Lossless-over-BT claim | ✅ (by design impossible) | `bt_supports_lossless() == false`, ADR-009 |

## Risks / limitations

- **Native side is compile-gated AND evidence-gated**: `bluez.rs` is only
  compiled on a Linux runner (`cargo check --features bt`), and a live
  registration/render is only valid on a real Linux box with BT hardware
  (bt-lab). The pinned `zbus 4.x`/`pipewire 0.10` APIs are the documented target;
  exact macro/signature mating is confirmed on the runner (an escape hatch —
  `zbus::blocking` moved/renamed → dbus-rs 0.9 or a one-line feature/version
  change — is recorded in build-check.md). Until that passes, the Linux native
  path must not be claimed build-green.
- **SBC decode + PipeWire RT render** (fd read, decode, SPSC fill/RT pop
  budget) is entirely runner/bt-lab territory; RT_CONTRACT §5 states simulation
  cannot substitute. The shell's RT seam (`push_render` no-alloc/no-block
  contract) is unit-tested here, the live graph is not.
- **Custom RFCOMM/L2CAP product-peer cells** (And/Win-RFCOMM/Linux) are
  bandwidth-limited low-bitrate lossy and never claimed hi-fi — stated in the
  matrix and UI-facing labels.
- **`Cargo.lock` includes target-gated deps** (zbus/pipewire resolved, not
  compiled here) — matches the emitter convention; SBOM/license gate treats them
  as permissive (MIT; pipewire lib = LGPL-2.1 dynamic-only recorded exception).
- Zero-dependency portable surface keeps this crate cheap to audit on any host.

## Follow-up tasks

- Linux-runner gate: `cargo check --features bt` and `--all-features` on an
  Ubuntu 22.04+ runner; fix any pinned-zbus/pipewire-0.10 API drift; record
  exact versions in build-check.md.
- bt-lab gate: run the Bluetooth row of HARDWARE_VALIDATION.md — real
  A2DP-sink receive + PipeWire render to a USB DAC (FR-034), plus the RFCOMM
  low-bitrate product-peer cell — and record results.
- Wire live SPSC ring handoff (worker SBC-decode fill → `pw_stream::process` RT
  pop) into `BluezProfileServer`/`PipeWireMediaSink` once the runner validates
  the skeleton.
- Update REQUIREMENTS_TRACEABILITY FR-030–034 row + QUALITY_DASHBOARD once the
  runner/bt-lab gates land (write scope for this task was limited to
  `platform/linux-receiver/**` + this report).

## Blockers

- None for delivery of source + docs: the on-host validation was executable
  (toolchain present) and all four commands pass. Native BlueZ/PipeWire
  evidence is blocked on a Linux runner + bt-lab hardware — that is the reason
  the `bt-lab` gate owns those claims, not a source defect.
