# win-emitter — Windows-runner build & validation check

The WASAPI adapters in this crate are `#[cfg(windows)]`-gated and **cannot be
compiled or validated on the Linux/WSL2 host** (no x86_64-pc-windows-msvc
target, no Windows SDK). The native build is gated to a **GitHub Actions
Windows runner** via `.github/workflows/ci.yml` (`windows-basic`, currently
`if: false` — enable when a windows runner is available / credentials allow).

## Exact CI commands (windows runner)

```bash
rustup target add x86_64-pc-windows-msvc
# from repository root
cargo check --target x86_64-pc-windows-msvc -p win-emitter   # if part of a workspace
# OR (this crate declares its own [workspace], so):
cd platform/win-emitter
cargo check --target x86_64-pc-windows-msvc
cargo test --target x86_64-pc-windows-msvc                 # unit tests incl. wasapi logic
```

`windows` crate dependency (0.62.2, target `cfg(windows)` only) features used:
`Win32_Foundation, Win32_Media_Audio, Win32_Media_KernelStreaming,
Win32_UI_Shell_PropertiesSystem, Win32_System_Com, Win32_System_Com_StructuredStorage,
Win32_System_Variant`.

## What the WASAPI adapter does (platform-surface)

- Enumerates render endpoints via `IMMDeviceEnumerator::EnumAudioEndpoints`.
- Captures the default/selected endpoint with `IAudioClient::Initialize` in
  **shared mode** + `AUDCLNT_STREAMFLAGS_LOOPBACK` → system-wide mix (no
  per-app capture — that is NOT available on Windows via public API).
- Reads via `IAudioCaptureClient::GetBuffer`, **only memcpy** into a
  caller-provided buffer (RT discipline, see `docs/planning/RT_CONTRACT.md`).
- Endpoint selection by friendly name; a `RouteChange` surface for default-route
  changes (FR-015) — no crash, re-connect capture.

## Platform limitations (must appear in product UI + docs)

1. **Protected content returns silence** in loopback (OS-enforced; not a bug).
2. **No per-application PCM capture** — system-wide loopback is the only public
   route. UI must say "system output", not "app X audio".
3. **USB-DAC identification** is by friendly/instance/container name only;
   VID/PID correlation via SetupAPI is a documented pending follow-up.
4. Min supported: Windows 11 24H2+ (Win10 22H2 is EOL legacy, own-risk).

## Status

- Linux: `cargo build` + `cargo test` (8 tests) + `cargo clippy -D warnings` +
  `cargo fmt` all green (2026-09-08/09), non-WASAPI logic + FakeCaptureSource.
- Windows native check: **gated** to a windows runner (enable `windows-basic` CI
  job). Do not claim a Windows build until that passes.
