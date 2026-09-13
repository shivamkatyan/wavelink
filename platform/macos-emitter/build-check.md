# macos-emitter — macOS build & validation check

This crate is developed/validated on a **real macOS host** (macOS 26.5.2, arm64,
Xcode 26.6, Rust 1.98.1 + `aarch64-apple-darwin`). Unlike the win/linux siblings
(whose native backends are cross-gated to other builders), the whole `backend`
**is compiled here**, so this page splits "compiled/validated on this host" from
"hardware-gated (logged-in GUI session / USB DAC)".

## Exact commands + results (ran 2026-09-09 on this host)

```bash
cd platform/macos-emitter
cargo build                                # ✅
cargo test                                 # ✅ 17 passed, 0 failed
cargo test --features macos14-taps         # ✅ 20 passed, 0 failed
cargo clippy --all-targets --all-features -- -D warnings   # ✅ clean
cargo fmt --all -- --check                  # ✅ clean
```

> **Swift runtime note (real host quirk, not a code issue):** running the test
> binaries requires the screencapturekit Swift shim's runtime. This box has
> macOS 26.5 dev-image where `libswift_Concurrency.dylib` is **not** in
> `/usr/lib/swift` or the dyld shared cache, so tests must be launched with:
>
> ```bash
> TOOL=/Applications/Xcode.app/Contents/Developer/Toolchains/XcodeDefault.xctoolchain/usr/lib/swift-5.5/macosx
> DYLD_LIBRARY_PATH="$TOOL" cargo test        # (+ --features macos14-taps)
> ```
>
> Root cause: the SCK shim links `@rpath/libswift_Concurrency.dylib`; the binary
> has no rpath and the host has no system copy. `cargo build`/`clippy`/`fmt` need
> **no** such variable (link-time resolution is satisfied by the SDK stubs). On
> a normal dev Mac this step is usually unnecessary (`/usr/lib/swift/libswift_Concurrency.dylib`
> exists there); the env var is harmless.
>
> The **same loader-path requirement applies to the `--stream` CLI at runtime**
> (it reaches SCK through the same shim) and to the packaged app's child
> process: `app/main.swift` sets `DYLD_LIBRARY_PATH` from the toolchain Swift
> lib dir before running the bundled CLI (see `swiftLibRpath()`).

## First real stream — `--stream` + the `AudioFrameSink` seam (2026-09-12)

The crate now wires real audio into the proven core: `--stream` drives capture
(SCK, or a `wdr_fakes` fixture for the hardware-free gate) through the shared
`wdr_refsim::sink::QuicAudioSink`/`AudioFrameSink` seam → encode → per-frame
CRC → QUIC (Opus datagram / FLAC·PCM reliable stream) → `ref_receiver`.

```bash
# software gate (no TCC/hardware) — fixture path, hash-perfect:
bash scripts/verify/macos-stream-smoke.sh
#   PASS pro/flac: packets=4 fatal=0 underruns=0 hash=b7a3c25c…   (canonical golden)
#   PASS free/opus: packets=50 fatal=0 underruns=0                (bounded datagrams)
# real SCK capture (TCC-granted logged-in session, this host):
./target/release/ref_receiver --role receiver --buffer balanced 127.0.0.1:9100 &
DYLD_LIBRARY_PATH="$TOOL" platform/macos-emitter/target/release/macos_emitter \
    --stream --addr 127.0.0.1:9100 --tier free --codec opus --duration 1
#   JSON events: start → end(ok) — receiver-sim.json: packets_recv=68,
#   loss/dup/reorder/late/fatal/underruns all 0
bash scripts/verify/macos-launch-smoke.sh   # packaged .app opens with a window
```

**CLI contract** (newline-delimited JSON on stdout, parsed by the Swift GUI):
`{"ev":"start",...}` · `{"ev":"stats",...,"period_ms":1000}` ·
`{"ev":"end","status":"ok",...}` · `{"ev":"fatal","message":...}` (exit 1).

Notes, kept honest:
- A benign `SwiftNativeNSObject … implemented in both …` objc startup warning
  appears when the CLI runs with the toolchain Swift lib on the loader path
  (class-probing only; harmless, no behaviour change).
- Real SCK capture was exercised **on this host** with Screen Recording granted
  to the calling terminal (68 Opus frames, 0 fatal/underruns). A **fresh
  install** must still grant Screen Recording TCC to this app (the permission
  gate fails fast with a typed `fatal` event when missing) — that remains the
  `macos-capture-sck` gate. Per-app process taps (14.2+) are still a seam.

## Test inventory

| Group | # tests | What they prove |
|---|---|---|
| `tests::*` (portable `lib.rs`) | 9 | `allow_lossless` Free/Pro fail-closed gate; app gate; deterministic 960-byte blocks shared with win/linux; seq/send round-trip; overflow; lifecycle; `set_endpoint` empty-reject; **per-app targeting**; route-change exhaustive match |
| `backend::permission` | 6 | State machine with a scripted **fake probe**: grant, deny-never-authorizes, grant-after-denial, revoke, **Restricted sticky**, inconclusive keeps prior state, `classify` heuristics |
| `backend::system_capture` | 1 | `PreallocatedCaptureBuffer` RT copy/overflow/reset (the exact ops the SCK sample handler performs) |
| `backend::tap` (`macos14-taps`) | 3 | `AudioHardwareCreateProcessTap`/`DestroyProcessTap` **symbol link gate**; detached-handle honest refusal; start/stop intent lifecycle |

## What is compiled / validated on this host

1. **Portable surface** (`lib.rs`) — builds on any host; unit tests run here.
2. **`backend` compile** — the real macOS gate this crate satisfies: everything
   in `permission`/`hal`/`hotplug`/`system_capture` (+ `tap` under the feature)
   compiles and links on this host. The `screencapturekit` 10.0.3 and
   `coreaudio-sys` 0.2.18 deps resolve and build.
3. **HAL FFI live-validated** (off the data path, no TCC needed; exact calls are
   the same selectors/addresses as `backend/hal.rs`). Annotated run this host:

   ```
   devices data size status=0 size=12
   devices status=0 count=3 first=Some(79)
   device0 name = Mac mini Speakers
   device0 transport status=0 transport=626c746e usb=false   # 'bltn' built-in
   default output status=0 id=79                              # == device0 ✅
   streams data-size status=0 size=4 (output-scope, non-zero ⇒ output-capable)
   ```
   Enumeration (3 devices), `kAudioObjectPropertyName` → CFString → UTF-8,
   `kAudioDevicePropertyTransportType`, default-output device and
   output-scope stream presence all read real values. USB detection on a real
   **USB DAC** remains hardware-gated (no USB DAC attached to this box).
4. **Permission state machine** unit tests (the `macos-ci` gate item from the
   handoff) — all pass with the scripted fake probe; the real
   `ScShareableContentProbe` compiles against `screencapturekit`.

## What is SEAM vs COMPILED, and why (the honest ledger)

| Piece | Status here | Reason / exact evidence |
|---|---|---|
| HAL device metadata FFI | **compiled + live-validated** | ran above |
| Hotplug (device add/remove) | **compiled** (function-pointer listener) | `AudioObjectAddPropertyListener` + `devices_listener` atomic bump; live *delivery* needs plug/unplug a device (hardware gate) |
| Hotplug block form | **not used** — documented follow-up | `AudioObjectAddPropertyListenerBlock` needs a libdispatch **block literal** (no safe pure-Rust block constructor in our dep set); the C-callback form is the cleanly-compiling real impl |
| ScreenCaptureKit system capture | **compiled** (`SystemCaptureHandle`) | `SCShareableContent::create().get()`, `SCContentFilter::with_display`, `SCStreamConfiguration.capturesAudio`, `SCStream` + `start_capture`; actual bytes require TCC GUI session (hardware gate `macos-capture-sck`) |
| Process-tap FFI | **compiled + link-validated** | `AudioHardwareCreateProcessTap`/`DestroyProcessTap` declared per the macOS 26 SDK header `AudioHardwareTapping.h`; the symbols **resolve at link time** (asserted by `tap_ffi_symbols_link`). `coreaudio-sys` bindgen does **not** emit them (grep of generated bindings: no `ProcessTap`), so the declarations are ours |
| Working process tap | **seam** | `CATapDescription` is an ObjC `NSObject` with no C-struct form (SDK header is `#ifdef __OBJC__`); constructing one needs ObjC messaging (`initStereoMixdownOfProcesses:` etc.) — a swift/objc2 shim is the O-S follow-up, then the 14.2+ hardware gate `macos-capture-taps` |

Nothing in the scaffold claims runtime capture on *arbitrary* hardware: stream
creation (`SCShareableContent::get()`) fails closed without consent — that is
the point of `permission::PermissionGate` (fail closed, never authorizes on an
ambiguous probe). Since 2026-09-12, **real SCK capture through `--stream` has
been exercised on this host's TCC-granted session** (see above); fresh
installs / other hardware remain the `macos-capture-sck` gate.

## Hardware gate runbook (logged-in GUI session; per `docs/planning/HARDWARE_VALIDATION.md` "macOS capture" row)

### 1. `macos-capture-sck` — ScreenCaptureKit end-to-end
1. Login to a GUI session on the Mac; run the app/binary once → System Settings →
   Privacy & Security → Screen Recording → grant.
2. Re-run; expect `SCShareableContent` non-empty → `SystemCaptureHandle::start`
   creates the stream; audio bytes arrive in the preallocated sink (poll
   `buffered()`/`reset_fill()` on the worker; check `overflowed()` == false at the
   negotiated rate — default 48 kHz / stereo / 16-bit).
3. Pass criteria: non-zero PCM captured while a tone plays; no overflow under a
   10-min soak; capture-log + permission-states recorded.
4. Also verify **denial**: revoke TCC → probe maps to `Denied`/`Restricted` and
   `start()` fails closed (no silent silent-silence).

### 2. `macos-capture-taps` — per-app process taps (macOS 14.2+)
1. Build with `--features macos14-taps`. Construct a `CATapDescription` from
   ObjC/Swift (the seam shim) for one app (e.g. `initStereoMixdownOfProcesses`)
   or the global-minus list; hand the resulting `AudioObjectID` to
   `ProcessTapHandle::for_tap_id`.
2. Expect only that app's mix in the preallocated buffer; `destroy()` calls
   `AudioHardwareDestroyProcessTap` cleanly.
3. Pass criteria: single-app PCM isolation verified by muting the source app;
   clean start/stop cycles. This is the O-S (open-source) follow-up gate — the
   shim does not exist yet in this scaffold.

### 3. `usb-dac-device` — HAL USB DAC hotplug
1. With the monitor running, plug a USB DAC into the Mac.
2. Expect the listener generation to bump → `poll_changes()` emits
   `RouteChange::DeviceAdded(name)` with `is_usb == true` (transport
   `kAudioDeviceTransportTypeUSB`); unplug → `DeviceRemoved`.
3. Pass criteria: add/remove within ~2 s of plug/unplug; default-output change
   also surfaced as `DefaultDeviceChanged`.

## Status

- This host: `cargo build`, `cargo test` (17 pass), `cargo test --features
  macos14-taps` (20 pass), `cargo clippy --all-targets --all-features -- -D
  warnings`, `cargo fmt --check` all green. HAL FFI live-validated.
- **First real stream wired (2026-09-12):** the packaged `.app` opens with a
  window (`scripts/verify/macos-launch-smoke.sh` PASS) and `--stream` carries a
  fixture (hash-perfect vs the canonical golden) or **real SCK capture**
  (68 frames/0 fatal/0 underruns, TCC-granted terminal session) to `ref_receiver`
  (`scripts/verify/macos-stream-smoke.sh` PASS).
- Hardware gates still pending: fresh-install Screen Recording TCC, real capture
  on other hardware, `macos-capture-taps` (14.2+ ObjC shim), `usb-dac-device`.
  Do not over-claim beyond this host's TCC-granted evidence.
