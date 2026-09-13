# t-B3-macos — macOS Emitter: SCK system capture, HAL USB DAC, TCC permission gate

**task_id:** `t-B3-macos` · **root_task_id:** `B3-macos-emitter` · **hypothesis_id:** `H-B3-MAC-1`
**owner_role:** Platform Implementer - macOS · **status:** complete (compile+unit validated on this macOS host; runtime hardware-gated)
**date:** 2026-09-09

## Summary

Delivered `platform/macos-emitter` — a standalone macOS **emitter** scaffold
mirroring `platform/win-emitter`/`platform/linux-emitter` exactly in structure
and discipline, plus a **genuinely-compiling** macOS capture backend
(`#[cfg(target_os = "macos")]`, compiled on this real macOS host):

- `src/lib.rs` — portable, zero-dependency, buildable-anywhere seam:
  `CaptureSource` (with per-app `set_endpoint(name, per_app)` carried from
  linux-emitter), `FormatMeta`/`EndpointInfo` (with `per_app_capable`)/
  `RouteChange`, macOS `Error` (`MacAudio(String)`), `allow_lossless` fail-closed
  Free/Pro gate, `Frame`, deterministic `FakeCaptureSource` (same byte algorithm
  as siblings), `MacEmitterApp` (RT `push_block` memcpy → worker `emit_frame`
  → stub send), and 9 unit tests mirroring the sibling set + per-app tests.
- `src/backend.rs` (macOS): `permission` (TCC state machine + fake-probe
  unit tests — the `macos-ci` item), `hal` (enumerate/default-output/USB via
  `coreaudio-sys`, **live-validated on this host**), `hotplug` (HAL device-monitor,
  function-pointer listener), `system_capture` (real, compiling SCK adapter with
  an RT sample handler that only copies into a preallocated buffer), and `tap`
  (feature `macos14-taps`) with a **link-validated** `AudioHardwareCreateProcessTap`/
  `DestroyProcessTap` FFI skeleton + an honest lifecycle seam.
- README.md + build-check.md (compiled-vs-hardware ledger, runbooks referencing
  HARDWARE_VALIDATION.md macOS row) and this report.

## Files changed

| Path | Change |
|---|---|
| `platform/macos-emitter/Cargo.toml` | Standalone `[workspace]`; macOS-only deps under `[target.'cfg(target_os="macos")'.dependencies]` (`coreaudio-sys`, `core-foundation`, `screencapturekit`); `[features] macos14-taps`; `lto="thin"` |
| `platform/macos-emitter/Cargo.lock` | Standalone lockfile (generated) |
| `platform/macos-emitter/src/lib.rs` | Portable surface + 9 tests (see Summary) |
| `platform/macos-emitter/src/backend.rs` | macOS backend module root (module map + honest compiled-vs-seam ledger) |
| `platform/macos-emitter/src/backend/permission.rs` | `PermissionState{NotDetermined,Denied,Restricted,Authorized}` + `PermissionProbe` trait + real `ScShareableContentProbe` + 6 state-machine tests |
| `platform/macos-emitter/src/backend/hal.rs` | HAL enumerate/default/USB-transport FFI (live-validated) |
| `platform/macos-emitter/src/backend/hotplug.rs` | `DeviceMonitor` trait + `HALDeviceMonitor` (function-pointer listener) |
| `platform/macos-emitter/src/backend/system_capture.rs` | `SystemCaptureAdapter` seam + real `SystemCaptureHandle` + `PreallocatedCaptureBuffer` RT sink + test |
| `platform/macos-emitter/src/backend/tap.rs` | Process-tap FFI skeleton (link gate test) + `ProcessTapHandle` seam (feature `macos14-taps`) |
| `platform/macos-emitter/README.md` | crate map, honest capabilities, build + hardware runbook |
| `platform/macos-emitter/build-check.md` | exact commands/results, Swift-runtime note, HAL evidence, seam ledger, 3 hardware runbooks |
| `docs/orchestration/reports/t-B3-macos.md` | This report |

No other path was modified (root workspaces, `crates/*`, other `platform/*`,
`docs/planning/**`, CI, Docker, justfile all untouched).

## Decisions

1. **Crate name/pin `screencapturekit` 10.0.3** (MIT OR Apache-2.0, from the
   same doom-fish repo as `screencapturekit-rs`; the crates.io name is
   `screencapturekit` — acceptable per DEPENDENCY_EVALUATION). It **builds on
   this host**, so the SCK backend is real and compiling (not a seam).
2. **`coreaudio-sys` 0.2.18** (MIT, bindgen vs host SDK 26). Process taps are
   **not** in its generated bindings → we declare `AudioHardwareCreateProcessTap`/
   `DestroyProcessTap` ourselves per the SDK header; linkage asserted by a test
   (symbols resolve on this host).
3. **Hotplug uses the function-pointer listener form**
   (`AudioObjectAddPropertyListener`) — the block form needs a libdispatch block
   literal (no safe constructor in our dep set); the C-callback form is the
   cleanly-compiling real impl, block form = documented follow-up.
4. **Per-app capture is a seam + link gate**, not fabricated runtime: the API's
   first argument is an ObjC `CATapDescription*` (`#ifdef __OBJC__` header), so a
   working tap needs a Swift/objc2 shim. `ProcessTapHandle::start()` refuses
   honestly when no tap is created; `SystemCaptureHandle.set_endpoint(..., true)`
   refuses with a pointer to the runbook.
5. **`PermissionState` has 4 states** (no `Limited`): Screen Recording TCC on
   macOS is a single grant/deny (no partial-audio mode); documented in-code.
6. **Portable surface has zero deps**; all FFI is `[target.'cfg(target_os =
   "macos")'.dependencies]` — mirrors win/linux.

## Commands run (this host, macOS 26.5.2 / Xcode 26.6 / Rust 1.98.1 / arm64)

```bash
cd platform/macos-emitter
cargo build
TOOL=/Applications/Xcode.app/Contents/Developer/Toolchains/XcodeDefault.xctoolchain/usr/lib/swift-5.5/macosx
DYLD_LIBRARY_PATH="$TOOL" cargo test
DYLD_LIBRARY_PATH="$TOOL" cargo test --features macos14-taps
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --all -- --check
# + a scratch live probe of the exact hal.rs FFI calls (see build-check.md evidence)
```

## Validation results

- `cargo build` — ✅ (incl. whole `backend`; `screencapturekit` 10.0.3 and
  `coreaudio-sys` build).
- `cargo test` — ✅ **17 passed, 0 failed** (9 portable + 6 permission + 1
  system_capture; the 3 tap tests are feature-gated).
- `cargo test --features macos14-taps` — ✅ **20 passed, 0 failed**.
- `cargo clippy --all-targets --all-features -- -D warnings` — ✅ clean.
- `cargo fmt --all -- --check` — ✅ clean.
- HAL FFI live run on this host: enumerate (3 devices), name "Mac mini Speakers",
  transport `0x'bltn'` (built-in, usb=false), default output id 79, output-scope
  streams non-zero — all match `backend/hal.rs` calls (full transcript in
  build-check.md).
- Swift-runtime note: tests need `DYLD_LIBRARY_PATH` to the Xcode swift lib dir
  because this dev image lacks `/usr/lib/swift/libswift_Concurrency.dylib`
  (helpful, harmless on normal dev Macs; build/clippy/fmt need no env).

## Acceptance criteria

| Criterion | Result | Evidence |
|---|---|---|
| Standalone crate, own `[workspace]`, zero root/core deps, macOS deps target-gated | ✅ | `Cargo.toml`; `cargo` runs self-contained |
| Portable surface mirrors win/linux (trait, `FormatMeta`, `EndpointInfo`+`per_app_capable`, `RouteChange`, macOS `Error`, `allow_lossless`, `Frame`, `FakeCaptureSource`, `MacEmitterApp` RT/worker + Free/Pro gate) | ✅ | `src/lib.rs` (+9 tests) |
| `set_endpoint(name, per_app)` carried (per-app Capability like linux) | ✅ | trait + tests `per_app_targeting_is_supported` |
| `#![deny(rust_2018_idioms)]` | ✅ | lib.rs top |
| Permission state machine + injected `PermissionProbe` + real `SCShareableContent` probe + unit tests (denied/authorized/pending etc.) | ✅ | `backend/permission.rs` — 6 tests pass |
| HAL enumerate / default output / USB heuristic / hotplug | ✅ compiled; HAL reads live-validated; hotplug delivery hardware-gated | `hal.rs` + `hotplug.rs` + build-check evidence |
| System capture via SCK (`capturesAudio`) with RT-only sample handler | ✅ compiled (`SystemCaptureHandle`); runtime TCC-gated | `system_capture.rs`; `macos-capture-sck` pending |
| Process taps 14.2+ FFI skeleton (feature `macos14-taps`), no fabrication, O-S marked | ✅ link gate test passes; construction = ObjC seam | `tap.rs`; `macos-capture-taps` pending |
| README + build-check mirror sibling style; exact commands + results; HARDWARE_VALIDATION reference | ✅ | both files |
| `cargo build` / `test` / `test --features macos14-taps` / `clippy -D warnings` / `fmt --check` all pass | ✅ | counts above |
| Report with acceptance table, risks, follow-ups | ✅ | this file |
| Gates | `macos-ci` (unit tests) **satisfiable** · `macos-capture-sck` / `macos-capture-taps` / `usb-dac-device` **pending hardware** |

## Risks / limitations

- **Runtime capture is not validated here** (hardware gate): SCK bytes, process
  taps, live hotplug, real USB-DAC recognition. The adapter **compiles**; the
  runtime needs a logged-in TCC session / 14.2+ box / USB DAC
  (HARDWARE_VALIDATION.md macOS row). Claimed nowhere.
- **Per-app taps are a seam**: `AudioHardwareCreateProcessTap` links, but a
  working tap requires constructing ObjC `CATapDescription` (Swift/objc2 shim) —
  the O-S follow-up, then the 14.2+ hardware gate. `start()` fails closed today.
- **USB heuristic** uses the reported HAL transport (`kAudioDeviceTransportTypeUSB`);
  some class-compliant DACs report a different transport — PLATFORM_MATRIX row
  caveat; VID/PID correlation is a documented follow-up.
- **Hotplug block form** (libdispatch) not implemented; the C-callback form is
  used and compiles; block-form delivery is a follow-up if needed.
- **Swift runtime on this dev image**: `cargo test` needs `DYLD_LIBRARY_PATH`
  (host lacks the system concurrency dylib); normal dev Macs usually don't.
  Documented in build-check.md; not a code defect.
- **macOS 13 note**: SCK sample-rate/channel config is set as requested; exact
  delivered format read-back from the `CMSampleBuffer` format description is a
  follow-up (defaults 48k/16-bit/stereo negotiated).

## Follow-up tasks

- Re-check `screencapturekit` (10.0.3) on each bump; record MSRV/API drift if any.
- O-S shim for `CATapDescription` (Swift/objc2) so `macos14-taps` becomes a real
  per-app capture, then the 14.2+ hardware run.
- Wire `PreallocatedCaptureBuffer` → the `wdr_rt` SPSC ring + worker encode as
  the transport path lands (B5/B6), matching RT_CONTRACT handoff §3.
- Read the delivered SCK format from the sample buffer's ASBD (vs assumed
  requested format).
- Hardware gates: `macos-capture-sck` (TCC grant/deny + 10-min soak),
  `macos-capture-taps` (single-app isolation), `usb-dac-device` (plug/unplug
  within 2 s + default-change event). Record results, then flip status.
- Sibling consumers (integration): the supervisor's receiver-side B-series can
  consume `MacEmitterApp` frames unchanged (stream_id/seq/bytes contract).
- Mobile equivalents are separate tasks (android/iOS emitters) — not in scope here.

## Blockers

None. Dependency crates reachable; both macOS-native deps build on this host.
(Only runtime-capture validation is intentionally deferred to hardware gates.)
