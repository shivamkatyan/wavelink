# macos-emitter — Wavelink macOS emitter shell

Emit macOS system audio (ScreenCaptureKit) — and, on macOS 14.2+, a specific
application's audio (Core Audio process taps) — to a Wavelink receiver.
Part of the B3 desktop-breadth wave.

## What's here
- `src/lib.rs` — platform-agnostic, buildable-on-any-host seam: `CaptureSource`
  trait (with `per_app` endpoint targeting, mirroring linux-emitter),
  `MacEmitterApp` (RT memcpy → frame(seq) → send), Free/Pro lossless policy
  gate, deterministic `FakeCaptureSource` test double. Builds + tests on this
  host.
- `src/stream.rs` (`#[cfg(target_os = "macos")]`) — the **`--stream` driver**:
  real SCK capture (or a `wdr_fakes` fixture for the hardware-free gate) →
  the core `AudioFrameSink` seam (`wdr_refsim::sink::QuicAudioSink`, which owns
  its tokio+quinn runtime) → encode → CRC → QUIC → a receiver. Backed by the
  PROVEN core via **path dependencies on `wdr_proto`/`wdr_fakes`/
  `wdr_entitlement`/`wdr_refsim`** — this deliberately reverses the old
  "zero core-crate deps" stance now that a real stream exists; the seam keeps
  the shell free of direct tokio/quinn/codec deps.
- `src/backend.rs` (`#[cfg(target_os = "macos")]` — **compiled on this host**):
  - `permission` — Screen Recording TCC state machine (`NotDetermined / Denied /
    Restricted / Authorized`) driven by an injected `PermissionProbe` + a real
    `SCShareableContent` probe; every transition is unit-tested with a fake probe.
  - `hal` — Core Audio HAL output-device metadata (enumerate, default output,
    USB transport heuristic). Live-validated on this host.
  - `hotplug` — HAL device add/remove monitor (`AudioObjectAddPropertyListener`,
    function-pointer form; atomic generation bump on the listener thread).
  - `system_capture` — ScreenCaptureKit system-audio adapter (`capturesAudio`)
    whose **RT sample handler only copies** `CMSampleBuffer` audio bytes into a
    caller-preallocated buffer (RT_CONTRACT macOS SCK row).
  - `tap` (feature `macos14-taps`) — Core Audio process-tap FFI skeleton
    (link-validated here) + a lifecycle **seam**; the `CATapDescription`
    constructor is ObjC-gated → hardware gate.
- `app/main.swift` — the AppKit UI (no storyboards). It opens with a real
  window (explicit `static func main()` installs the delegate — the "no UI"
  bug fix), shells out to the bundled CLI for short queries, and **drives
  `--stream` with Start/Stop + live status** (address field, tier toggle,
  JSON status parsing; `applicationWillTerminate` SIGTERMs the child so the
  end-of-stream marker flushes).
- `build-check.md` — exact commands + results, and the hardware-gate runbooks.

## macOS capture capabilities (honest)
- **System-wide**: ScreenCaptureKit audio on macOS 13+, requires Screen
  Recording TCC (`NSScreenCaptureUsageDescription`) in a logged-in GUI session.
  **Actual bytes are hardware-gated** — only the adapter compiles here.
- **Per-application**: Core Audio process taps, macOS 14.2+, feature `macos14-taps`.
  The FFI symbols **link** on this host; creating a working tap needs the ObjC
  `CATapDescription` shim + a 14.2+ box (O-S follow-up; NOT validated here).
- **USB DAC**: HAL `AudioObject` enumeration + default-output + hotplug; USB
  heuristic is the reported transport (`kAudioDeviceTransportTypeUSB`).
- **No `AVAudioSession` on macOS**; the HAL default output device is the only
  system-wide output the app can follow (PLATFORM_MATRIX §A).
- **Protected content**: muted/excluded by the OS (same policy caveat as the
  Windows loopback row).

## Streaming for real — `macos-emitter --stream`

Captures system audio (ScreenCaptureKit) or a deterministic `wdr_fakes`
fixture, pushes it through the core `AudioFrameSink` seam to a QUIC receiver
(e.g. `ref_receiver` on the same host — the software gate). No discovery/pairing
yet (ADR-006 follow-up); the receiver address is explicit.

```text
macos-emitter --stream --addr <ip:port> [--tier free|pro] [--codec opus|flac|pcm]
              [--fixture pseudo-random|silence|impulse|full-scale|sine|channel-left|channel-right]
              [--duration <secs>]
env: WDR_ENT_TIER / WDR_STREAM_ADDR (default 127.0.0.1:9100) / WDR_STREAM_DURATION_SECS / WDR_CC
```

**Status contract** — newline-delimited JSON on stdout (the Swift GUI parses it):
`{"ev":"start",tier,codec,rate,channels,lane,fixture,addr}` →
`{"ev":"stats",seq,packets_sent,bytes_sent,overflowed,period_ms}` (≤1/s) →
`{"ev":"end",status:"ok",packets_sent,bytes_sent}` (clean) or
`{"ev":"fatal",message}` (exit 1: policy gate refused, connect failed, or
Screen Recording TCC not granted for real capture). SIGINT/SIGTERM stop
cleanly: `finish()` flushes the end-of-stream marker so the receiver completes.

**GUI**: receiver-address field (default `127.0.0.1:9100`), tier toggle
(Free=Opus lossy, Pro=FLAC lossless — enforced by the core policy gate), **Start
Stream** / **Stop Stream**, live status line parsed from the JSON events.
Stop/quit SIGTERM the child so the marker flushes.

**Verification** (2026-09-12, on this host):
- `bash scripts/verify/macos-stream-smoke.sh` — fixture pro/flac → `ref_receiver`
  is **hash-perfect** vs the canonical `b7a3c25c…` golden (4 frames, 0
  loss/fatal/underruns); free/opus datagrams bounded (50 frames, 0 fatal).
- `bash scripts/verify/macos-launch-smoke.sh` — the packaged `.app` opens with a
  real window (regression guard for the "opens with no UI" bug).
- Real SCK capture verified on this host's TCC-granted terminal session
  (68 Opus frames, 0 fatal/underruns). A fresh install must grant Screen
  Recording TCC to the app first — `--stream` fails fast with a typed `fatal`
  event when missing.
- `cmake` is now a **packaging prerequisite** (`flac-bound` builds vendored
  libFLAC via cmake; see `scripts/package/macos.sh`).

## Validate on this host
```bash
cd platform/macos-emitter
cargo build
# tests link screencapturekit's Swift shim; point the loader at the Swift runtime
TOOL=/Applications/Xcode.app/Contents/Developer/Toolchains/XcodeDefault.xctoolchain/usr/lib/swift-5.5/macosx
DYLD_LIBRARY_PATH="$TOOL" cargo test
DYLD_LIBRARY_PATH="$TOOL" cargo test --features macos14-taps
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --all -- --check
```

## Hardware (logged-in GUI session; see `build-check.md` + HARDWARE_VALIDATION.md)
- Screen Recording TCC grant → `SCShareableContent` non-empty → SCK stream
  delivers audio bytes to the preallocated sink (gate `macos-capture-sck`).
- macOS 14.2+ + ObjC `CATapDescription` shim → process tap delivers a single
  app's mix (gate `macos-capture-taps`).
- Plug/unplug a USB DAC → HAL hotplug adds/removes the device (gate `usb-dac-device`).
