//! `win-emitter` — standalone Windows WASAPI emitter scaffold (task t-B2-win).
//!
//! A Wavelink Windows emitter is a **capture** component: it records
//! the system audio (playback) mix via a WASAPI loopback device and forwards it
//! to the receiver. This crate is the small, standalone, **Linux-buildable**
//! seam:
//!
//! * [`CaptureSource`] — the platform adapter trait (its own copy; no core
//!   crate dependency) exposed at the ARCHITECTURE.md adapter boundary.
//! * [`FormatMeta`], [`EndpointInfo`], [`RouteChange`] — capture metadata and
//!   the route-change event surface.
//! * [`WinEmitterApp`] — ties the capture callback (memcpy into a caller
//!   buffer) to frame production (wrap bytes with a sequence number) and to a
//!   stub `send` closure, with a Free/Pro lossless policy gate via
//!   [`allow_lossless`].
//! * [`FakeCaptureSource`] — deterministic Linux test double.
//!
//! The actually-capturing backend [`wasapi`] (`#[cfg(windows)]`-only) provides
//! [`WasapiLoopbackCapture`], gated to the `windows` crate and the
//! `x86_64-pc-windows-msvc` target; it is compiled/validated on a Windows
//! runner (see [`wasapi`] and `build-check.md`), never on Linux.
//!
//! This "own trait + own metadata" choice is deliberate: the requirement is to
//! keep the shell tiny and Linux-buildable **without** depending on the core
//! WDR crates, while still honouring the shared `CaptureSource` adapter
//! contract documented in `docs/planning/ARCHITECTURE.md`.

#![deny(rust_2018_idioms)]

/// Windows WASAPI capture backend (target-gated; compiled only on Windows).
///
/// On Linux this module is not compiled at all — see its own file docs and
/// `build-check.md` for how it is validated on a Windows runner via
/// `cargo check --target x86_64-pc-windows-msvc`.
#[cfg(windows)]
pub mod wasapi;

/// Capture/format metadata for the selected endpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FormatMeta {
    /// Sampling rate in Hz.
    pub rate: u32,
    /// Bit depth per sample.
    pub bits: u32,
    /// Channel count.
    pub channels: u32,
}

/// Description of one audio endpoint (render device).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EndpointInfo {
    /// Endpoint name / friendly identifier.
    pub name: String,
    /// USB DAC heuristic (see `docs/planning/PLATFORM_MATRIX.md` Windows rows;
    /// true USB classification needs PnP VID/PID correlation, a documented
    /// follow-up).
    pub is_usb: bool,
    /// Whether this endpoint is the current system default.
    pub is_default: bool,
}

/// Route-change events surfaced by the capture adapter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RouteChange {
    /// The default render endpoint changed (name == new default if known).
    DefaultDeviceChanged(Option<String>),
    /// A new render endpoint was added.
    DeviceAdded(String),
    /// A render endpoint was removed.
    DeviceRemoved(String),
    /// The negotiated capture format changed.
    FormatChanged(FormatMeta),
}

/// Fallible capture-adapter operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// `start` was called while already running.
    AlreadyStarted,
    /// `stop`/read attempted without a running capture.
    NotStarted,
    /// `set_endpoint` referenced an endpoint that does not exist.
    EndpointNotFound(String),
    /// The endpoint cannot provide the requested format/settings.
    UnsupportedFormat,
    /// The preallocated capture buffer overflowed (RT push rejected).
    BufferOverflow,
    /// The `send` closure rejected a frame.
    SendFailed,
    /// Underlying WASAPI/COM failure (message decoration only).
    Wasapi(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let msg = match self {
            Error::AlreadyStarted => "capture already started",
            Error::NotStarted => "capture not started",
            Error::EndpointNotFound(n) => return write!(f, "endpoint not found: {n}"),
            Error::UnsupportedFormat => "unsupported capture format",
            Error::BufferOverflow => "capture buffer overflow",
            Error::SendFailed => "frame send rejected",
            Error::Wasapi(m) => return write!(f, "wasapi error: {m}"),
        };
        f.write_str(msg)
    }
}

impl std::error::Error for Error {}

/// The platform **capture** adapter trait (own copy; mirrors the
/// ARCHITECTURE.md `CaptureSource` adapter boundary).
pub trait CaptureSource {
    /// Begin capturing audio on the currently selected endpoint.
    fn start(&mut self) -> Result<(), Error>;
    /// Stop capturing; idempotent.
    fn stop(&mut self);
    /// The negotiated capture format.
    fn format(&self) -> FormatMeta;
    /// Select which render endpoint is looped-back by friendly name.
    fn set_endpoint(&mut self, name: &str) -> Result<(), Error>;
}

/// Tier gate mirroring `wdr_entitlement` semantics (Free=false, Pro=true)
/// without depending on the core crate. Unknown tier strings fail closed.
pub fn allow_lossless(tier: &str) -> bool {
    matches!(tier, "Pro")
}

/// One produced audio frame: payload bytes wrapped with a sequence number.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    /// Session-scoped stream identifier.
    pub stream_id: u32,
    /// Monotonic, wrap-safe sequence number.
    pub seq: u64,
    /// Raw interleaved PCM payload bytes.
    pub bytes: Vec<u8>,
}

/// Deterministic Linux test double implementing [`CaptureSource`].
///
/// Produces a fixed byte production per call, so the app's push→frame→send
/// chain is fully deterministically testable on Linux.
#[derive(Debug)]
pub struct FakeCaptureSource {
    fmt: FormatMeta,
    running: bool,
    phase: u8,
}

impl FakeCaptureSource {
    /// New source with the given format.
    pub fn new(fmt: FormatMeta) -> Self {
        FakeCaptureSource {
            fmt,
            running: false,
            phase: 0,
        }
    }

    /// Deterministic next capture block: 5 ms @ `fmt.rate` of interleaved
    /// i16-byte samples with a fixed per-phase byte pattern. Repeated calls
    /// advance the phase; identical for a fresh source of the same format.
    /// This models the bytes a real platform capture callback hands to
    /// [`WinEmitterApp::push_block`].
    pub fn next_block(&mut self) -> Vec<u8> {
        let samples = (self.fmt.rate as usize / 200) * self.fmt.channels as usize;
        let mut out = Vec::with_capacity(samples * 2);
        for i in 0..samples {
            let i8 = (i % 256) as u8;
            let b = self
                .phase
                .wrapping_add(i8.wrapping_mul(17))
                .wrapping_add(i8);
            out.push(b);
            out.push(b.wrapping_mul(3).wrapping_add(7));
        }
        self.phase = self.phase.wrapping_add(1);
        out
    }
}

impl CaptureSource for FakeCaptureSource {
    fn start(&mut self) -> Result<(), Error> {
        if self.running {
            return Err(Error::AlreadyStarted);
        }
        self.running = true;
        Ok(())
    }

    fn stop(&mut self) {
        self.running = false;
    }

    fn format(&self) -> FormatMeta {
        self.fmt
    }

    fn set_endpoint(&mut self, name: &str) -> Result<(), Error> {
        if name.is_empty() {
            return Err(Error::EndpointNotFound(name.to_string()));
        }
        Ok(())
    }
}

/// The emitter application: capture callback → preallocated buffer → frame
/// (bytes + seq) → stub `send` closure.
///
/// The **data-path** discipline mirrors `docs/planning/RT_CONTRACT.md`:
/// [`push_block`](Self::push_block) is the RT-side callback and only memcpys
/// into the preallocated buffer (overflow returns an error; it never allocates
/// or grows). [`emit_frame`](Self::emit_frame) is the worker-side step that
/// wraps buffered bytes into a [`Frame`] with the next [`Frame::seq`] and hands
/// it to the injected `send` closure.
pub struct WinEmitterApp<S, F> {
    source: S,
    tier: String,
    lossless: bool,
    stream_id: u32,
    seq: u64,
    capacity: usize,
    buf: Vec<u8>,
    send: F,
}

impl<S, F> std::fmt::Debug for WinEmitterApp<S, F>
where
    S: std::fmt::Debug,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WinEmitterApp")
            .field("tier", &self.tier)
            .field("lossless", &self.lossless)
            .field("stream_id", &self.stream_id)
            .field("seq", &self.seq)
            .field("capacity", &self.capacity)
            .field("buffered", &self.buf.len())
            .finish()
    }
}

impl<S, F> WinEmitterApp<S, F>
where
    S: CaptureSource,
    F: FnMut(Frame) -> Result<(), Error>,
{
    /// Build the app with a preallocated `capacity`-byte capture buffer.
    ///
    /// The lossless path is refused up front when the tier gate
    /// ([`allow_lossless`]) denies it — lossless never silently slips through,
    /// mirroring `wdr_entitlement` FR-047 semantics (no core dependency).
    pub fn new(source: S, tier: &str, stream_id: u32, capacity: usize, send: F) -> Self {
        WinEmitterApp {
            source,
            tier: tier.to_string(),
            lossless: allow_lossless(tier),
            stream_id,
            seq: 0,
            capacity,
            buf: Vec::with_capacity(capacity),
            send,
        }
    }

    /// Whether the lossless path is allowed for this app's tier.
    pub fn lossless_allowed(&self) -> bool {
        self.lossless
    }

    /// The configured tier name.
    pub fn tier(&self) -> &str {
        &self.tier
    }

    /// Delegate: begin capture on the source.
    pub fn start(&mut self) -> Result<(), Error> {
        self.source.start()
    }

    /// Delegate: stop capture.
    pub fn stop(&mut self) {
        self.source.stop();
    }

    /// Delegate: select the capture endpoint by name.
    pub fn set_endpoint(&mut self, name: &str) -> Result<(), Error> {
        self.source.set_endpoint(name)
    }

    /// Delegate: negotiated capture format.
    pub fn current_format(&self) -> FormatMeta {
        self.source.format()
    }

    /// **RT-side** capture callback: memcpy `block` into the preallocated
    /// buffer. Overflow (would exceed `capacity`) is rejected with
    /// [`Error::BufferOverflow`]; this path performs no allocation.
    pub fn push_block(&mut self, block: &[u8]) -> Result<(), Error> {
        if self.buf.len().saturating_add(block.len()) > self.capacity {
            return Err(Error::BufferOverflow);
        }
        self.buf.extend_from_slice(block);
        Ok(())
    }

    /// Drain the buffered bytes into the next seq-numbered [`Frame`] and
    /// deliver it to the `send` closure. Runs on the worker, not the RT
    /// callback. Returns an error if there is nothing buffered or the `send`
    /// closure rejects the frame.
    pub fn emit_frame(&mut self) -> Result<Frame, Error> {
        if self.buf.is_empty() {
            return Err(Error::BufferOverflow);
        }
        let bytes = std::mem::take(&mut self.buf);
        let frame = Frame {
            stream_id: self.stream_id,
            seq: self.seq,
            bytes,
        };
        self.seq = self.seq.wrapping_add(1);
        // Restore the preallocated capacity on the worker (not the RT path).
        self.buf.reserve(self.capacity);
        (self.send)(frame.clone())?;
        Ok(frame)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stereo48() -> FormatMeta {
        FormatMeta {
            rate: 48_000,
            bits: 16,
            channels: 2,
        }
    }

    #[test]
    fn allow_lossless_refuses_free_and_unknown() {
        assert!(!allow_lossless("Free"));
        assert!(!allow_lossless("free"));
        assert!(!allow_lossless("AnythingUnknown"));
        assert!(allow_lossless("Pro"));
    }

    #[test]
    fn app_gate_refuses_lossless_on_free_allows_on_pro() {
        let free = WinEmitterApp::new(FakeCaptureSource::new(stereo48()), "Free", 1, 4096, |_| {
            Ok(())
        });
        assert!(!free.lossless_allowed());

        let pro = WinEmitterApp::new(FakeCaptureSource::new(stereo48()), "Pro", 1, 4096, |_| {
            Ok(())
        });
        assert!(pro.lossless_allowed());
    }

    #[test]
    fn fake_source_produces_deterministic_blocks() {
        let mut a = FakeCaptureSource::new(stereo48());
        let mut b = FakeCaptureSource::new(stereo48());
        for _ in 0..4 {
            assert_eq!(
                a.next_block(),
                b.next_block(),
                "production must be deterministic"
            );
        }
        // Same phase-length every call: 5 ms @ 48k stereo = 240 samples/channel,
        // 480 interleaved frames × 2 bytes = 960 bytes.
        assert_eq!(a.next_block().len(), 960);
    }

    #[test]
    fn app_increments_seq_and_delivers_to_send() {
        let mut src = FakeCaptureSource::new(stereo48());
        let blocks = [src.next_block(), src.next_block(), src.next_block()];

        let expected: Vec<Vec<u8>> = blocks.to_vec();
        let mut sent: Vec<Frame> = Vec::new();
        let mut app = WinEmitterApp::new(src, "Pro", 7, 8192, |f| {
            sent.push(f.clone());
            Ok(())
        });

        app.start().unwrap();
        for block in &blocks {
            app.push_block(block).unwrap();
            app.emit_frame().unwrap();
        }

        assert_eq!(sent.len(), 3, "every push must become a sent frame");
        for (i, f) in sent.iter().enumerate() {
            assert_eq!(f.stream_id, 7);
            assert_eq!(f.seq, i as u64, "seq must increment");
            assert_eq!(&f.bytes, &expected[i], "payload must round-trip unchanged");
        }
    }

    #[test]
    fn push_block_overflows_never_panics() {
        let src = FakeCaptureSource::new(stereo48());
        let mut app = WinEmitterApp::new(src, "Pro", 1, 16, |_| Ok(()));
        app.push_block(&[0u8; 16]).unwrap();
        // One byte too many: rejected, buffer untouched.
        assert_eq!(app.push_block(&[0u8; 1]), Err(Error::BufferOverflow));
    }

    #[test]
    fn fake_start_stop_lifecycle() {
        let mut src = FakeCaptureSource::new(FormatMeta {
            rate: 44_100,
            bits: 16,
            channels: 1,
        });
        assert_eq!(src.start(), Ok(()));
        assert_eq!(src.start(), Err(Error::AlreadyStarted));
        src.stop();
        assert_eq!(src.start(), Ok(()));
    }

    #[test]
    fn set_endpoint_rejects_empty() {
        let mut src = FakeCaptureSource::new(stereo48());
        assert_eq!(
            src.set_endpoint(""),
            Err(Error::EndpointNotFound(String::new()))
        );
        assert_eq!(src.set_endpoint("USB DAC"), Ok(()));
    }

    #[test]
    fn route_change_matches_all_variants() {
        let cases = [
            RouteChange::DefaultDeviceChanged(Some("Speakers".into())),
            RouteChange::DefaultDeviceChanged(None),
            RouteChange::DeviceAdded("USB DAC".into()),
            RouteChange::DeviceRemoved("Old DAC".into()),
            RouteChange::FormatChanged(stereo48()),
        ];
        // Match every variant at least once (exhaustiveness enforced by compiler).
        match &cases[0] {
            RouteChange::DefaultDeviceChanged(Some(n)) => assert_eq!(n, "Speakers"),
            RouteChange::DefaultDeviceChanged(None) => unreachable!(),
            _ => unreachable!(),
        }
        match &cases[1] {
            RouteChange::DefaultDeviceChanged(None) => {}
            _ => unreachable!(),
        }
        match &cases[2] {
            RouteChange::DeviceAdded(n) => assert_eq!(n, "USB DAC"),
            _ => unreachable!(),
        }
        match &cases[3] {
            RouteChange::DeviceRemoved(n) => assert_eq!(n, "Old DAC"),
            _ => unreachable!(),
        }
        match &cases[4] {
            RouteChange::FormatChanged(m) => assert_eq!(*m, stereo48()),
            _ => unreachable!(),
        }
    }
}
