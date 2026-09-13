//! `macos-emitter` — macOS capture backend
//! (`#[cfg(target_os = "macos")]`; compiled on macOS hosts, including this one).
//!
//! This is the genuinely-macOS part of the crate. All construction/setup here
//! is **off the data path**; the only RT piece is the
//! [`system_capture::PreallocatedCaptureBuffer`] the ScreenCaptureKit sample
//! handler memcpys into (RT_CONTRACT macOS SCK row).
//!
//! # Modules
//!
//! * [`permission`] — Screen Recording TCC state machine backed by
//!   `SCShareableContent` availability (`screencapturekit`), driven through an
//!   injected [`permission::PermissionProbe`] so every transition is
//!   unit-tested with a fake probe.
//! * [`hal`] — Core Audio HAL output-device metadata: enumerate, default
//!   output, USB transport heuristic. The exact FFI reads were **live-validated
//!   on this host** during development (see `build-check.md`).
//! * [`hotplug`] — USB DAC add/remove detection via
//!   `AudioObjectAddPropertyListener` on `kAudioHardwarePropertyDevices`
//!   (function-pointer form; the block form is a documented follow-up).
//! * [`system_capture`] — ScreenCaptureKit system-audio capture adapter
//!   (`capturesAudio`); RT sample handler copies only.
//! * [`tap`] — Core Audio process taps (macOS 14.2+, feature `macos14-taps`):
//!   FFI symbol link gate + lifecycle seam. CATapDescription construction is
//!   ObjC-gated → hardware gate.
//!
//! # Honesty at the top
//!
//! * **Compiled here** (this is a real macOS host): every module below;
//!   **live-validated here**: the `hal` FFI reads (they ran during
//!   development). **Unit-tested here**: the `permission` state machine.
//! * **Hardware-gated** (needs a logged-in GUI session / USB DAC): actually
//!   receiving SCK audio, a process tap, live hotplug, E2E DAC — see
//!   `build-check.md`. Nothing below claims runtime capture.

pub mod hal;
pub mod hotplug;
pub mod permission;
pub mod system_capture;

#[cfg(feature = "macos14-taps")]
pub mod tap;
