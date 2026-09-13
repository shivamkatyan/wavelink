//! PipeWire capture backend (feature-gated, `#[cfg(feature="pipewire")]`).
//!
//! This module is compiled only when the crate is built with `--features
//! pipewire` on a Linux host that has PipeWire + `libpipewire-0.3-dev` and the
//! `pipewire` crate available (see `build-check.md`). On the WSL2 dev host it
//! is **not** compiled, and must not be required here.
//!
//! Design (mirrors `RT_CONTRACT.md` PipeWire row):
//! * The data path runs on PipeWire's RT (`PW_STREAM_FLAG_RT_PROCESS` stream)
//!   callback: it only memcpys the captured buffer into a caller-provided,
//!   preallocated byte buffer (no allocation, no locks, no syscalls).
//! * System-wide capture = a capture stream targeting the graph monitor /
//!   default sink capture port (`capture.sink=true` semantics).
//! * Per-application capture = linking the capture stream to a specific node
//!   via `target.object` (node.name / object.serial) — supported on Linux.
//! * Audio capture needs NO xdg portal consent (the portal gates Camera/screen,
//!   not audio) — a documented difference vs macOS/Android.
//!
//! The `pipewire` crate API surface used below follows pipewire-rs 0.7.x
//! (`pw_main_loop`, `pw_context`, `pw_core`, `pw_stream`). A CI runner with
//! PipeWire must compile it; exact availability is verified there, not here.

use crate::{EndpointInfo, Error, FormatMeta, RouteChange};

/// A single capturable PipeWire node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PwNodeInfo {
    /// `node.name` or serial-derived identifier.
    pub name: String,
    /// `media.class` (e.g. `Audio/Sink`, `Audio/Source`, `Stream/Output/Audio`).
    pub media_class: String,
    /// Whether a USB audio device backs this node (`device.product`/`alsa.usb`).
    pub is_usb: bool,
    /// Whether this node can be targeted as an individual app stream
    /// (`media.class == Stream/Output/Audio` → per-app targetable).
    pub per_app: bool,
}

/// PipeWire capture source: system-wide (default sink) or per-application
/// (target.node) capture into a preallocated buffer, with RT-discipline
/// memcpy-only in the process callback.
#[cfg(feature = "pipewire")]
pub struct PipeWireCaptureSource {
    fmt: FormatMeta,
    running: bool,
    selected: Option<EndpointInfo>,
    capacity: usize,
}

#[cfg(feature = "pipewire")]
impl PipeWireCaptureSource {
    /// Create a source with a fixed on-RT capture buffer `capacity`.
    pub fn new(capacity: usize) -> Self {
        // 48 kHz / 16-bit / stereo is the common initial negotiation; the real
        // negotiated format comes from the pw_stream params at connect time.
        let fmt = FormatMeta {
            rate: 48_000,
            bits: 16,
            channels: 2,
        };
        PipeWireCaptureSource {
            fmt,
            running: false,
            selected: None,
            capacity,
        }
    }

    /// Enumerate nodes (a thin, documented shell — CI runner wires this to
    /// `pw_core` registry enumeration; the pipewire crate's exact API is
    /// confirmed at build time on the runner).
    pub fn enumerate(&self) -> Vec<PwNodeInfo> {
        // Placeholder: construction/wiring happens on the runner. Never claim
        // a live enumeration here.
        Vec::new()
    }

    /// Attach the PipeWire main loop and connect the capture stream to the
    /// selected node. Compiled/validated on the Linux CI runner.
    fn attach_stream(&mut self, _target: &str) -> Result<(), Error> {
        // The pipewire-rs wiring goes here: pw_main_loop::new(),
        // pw_context::new(), pw_core::connect(), pw_stream with
        // PW_STREAM_DIRECTION_INPUT + PW_STREAM_FLAG_RT_PROCESS, params set
        // to the negotiated format, and `set_process_func` that memcpys the
        // data into `self.buf` (RT discipline). Exact calls depend on the
        // pinned pipewire crate version and are asserted on the CI runner.
        Err(Error::PipeWire(
            "attach_stream wiring is runner-validated (no daemon on this host)".into(),
        ))
    }
}

#[cfg(feature = "pipewire")]
impl crate::CaptureSource for PipeWireCaptureSource {
    fn start(&mut self) -> Result<(), Error> {
        if self.running {
            return Err(Error::AlreadyStarted);
        }
        let target = match &self.selected {
            Some(e) => e.name.clone(),
            None => return Err(Error::NotStarted),
        };
        self.attach_stream(&target)?;
        self.running = true;
        Ok(())
    }

    fn stop(&mut self) {
        self.running = false;
    }

    fn format(&self) -> FormatMeta {
        self.fmt
    }

    fn set_endpoint(&mut self, name: &str, per_app: bool) -> Result<(), Error> {
        if name.is_empty() {
            return Err(Error::EndpointNotFound(name.to_string()));
        }
        self.selected = Some(EndpointInfo {
            name: name.to_string(),
            is_usb: name.contains("usb"),
            per_app_capable: per_app,
        });
        Ok(())
    }
}
