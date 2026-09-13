//! `linux-emitter` — standalone Linux PipeWire emitter scaffold (t-B3-lin).
//!
//! A Wavelink Linux emitter is a **capture** component: it records
//! the system audio graph (PipeWire) and forwards it to the receiver. This
//! crate is the small, standalone, buildable-on-this-host seam that mirrors the
//! sibling `platform/win-emitter` (same "own trait, no core crate dep" design):
//!
//! * [`CaptureSource`] — the platform capture-adapter trait (own copy).
//! * [`FormatMeta`], [`EndpointInfo`], [`RouteChange`] — metadata + route-event
//!   surface. `EndpointInfo` carries a `per_app`-capable bool because on Linux
//!   per-application capture is supported via PipeWire node targeting (a
//!   documented strength vs Windows, which is system-loopback-only).
//! * [`LinuxEmitterApp`] — the RT-side callback (memcpy into preallocated
//!   buffer) → frame (bytes + seq) → stub `send` closure, with the Free/Pro
//!   lossless policy gate via [`allow_lossless`].
//! * [`FakeCaptureSource`] — deterministic Linux test double.
//!
//! The actually-capturing backend [`pipewire`] is `#[cfg(feature="pipewire")]`-
//! gated: it is compiled/validated on a Linux runner with the PipeWire daemon
//! and the `pipewire` crate (see `build-check.md`), never required here.

#![deny(rust_2018_idioms)]

/// PipeWire capture backend (feature-gated; compiled only with
/// `--features pipewire` on a Linux host). On this host (no PipeWire daemon)
/// it is not compiled by default.
#[cfg(feature = "pipewire")]
pub mod pipewire;

/// Capture/format metadata for the selected node/endpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FormatMeta {
    /// Sampling rate in Hz.
    pub rate: u32,
    /// Bit depth per sample.
    pub bits: u32,
    /// Channel count.
    pub channels: u32,
}

/// Description of one capturable audio node / output device.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EndpointInfo {
    /// Node/device name (e.g. `alsa_output.usb-...analog-stereo` or a node.name).
    pub name: String,
    /// USB DAC heuristic (deterministic node naming on Linux — `device.product`/
    /// `alsa_output.usb-*`; see PLATFORM_MATRIX Linux rows).
    pub is_usb: bool,
    /// Whether per-application capture is available for this node (PipeWire
    /// `target.object` — supported on Linux; see ARCHITECTURE/per-app).
    pub per_app_capable: bool,
}

/// Route-change events surfaced by the capture adapter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RouteChange {
    /// The default sink/node changed.
    DefaultSinkChanged(String),
    /// A node was added.
    NodeAdded(String),
    /// A node was removed.
    NodeRemoved(String),
    /// The negotiated format changed.
    FormatChanged(FormatMeta),
}

/// Fallible capture-adapter operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// `start` while already running.
    AlreadyStarted,
    /// `stop`/read without a running capture.
    NotStarted,
    /// `set_endpoint` referenced a node that does not exist.
    EndpointNotFound(String),
    /// The node cannot provide the requested format.
    UnsupportedFormat,
    /// Preallocated capture buffer overflowed (RT push rejected).
    BufferOverflow,
    /// The `send` closure rejected a frame.
    SendFailed,
    /// PipeWire failure (message decoration only).
    PipeWire(String),
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
            Error::PipeWire(m) => return write!(f, "pipewire error: {m}"),
        };
        f.write_str(msg)
    }
}

impl std::error::Error for Error {}

/// The platform **capture** adapter trait (own copy; mirrors ARCHITECTURE.md).
pub trait CaptureSource {
    /// Begin capturing audio from the currently selected node.
    fn start(&mut self) -> Result<(), Error>;
    /// Stop capturing; idempotent.
    fn stop(&mut self);
    /// The negotiated capture format.
    fn format(&self) -> FormatMeta;
    /// Select the capture node by name; if `per_app`, target a specific
    /// application node (Linux PipeWire `target.object` — no portal consent
    /// needed for audio; per-app is a Linux capability).
    fn set_endpoint(&mut self, name: &str, per_app: bool) -> Result<(), Error>;
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
#[derive(Debug)]
pub struct FakeCaptureSource {
    fmt: FormatMeta,
    running: bool,
    phase: u8,
    per_app: bool,
}

impl FakeCaptureSource {
    /// New source with the given format.
    pub fn new(fmt: FormatMeta) -> Self {
        FakeCaptureSource {
            fmt,
            running: false,
            phase: 0,
            per_app: false,
        }
    }

    /// Whether per-app targeting is active (set via `set_endpoint`).
    pub fn is_per_app(&self) -> bool {
        self.per_app
    }

    /// Deterministic next capture block: 5 ms @ `fmt.rate` of interleaved
    /// i16-byte samples with a fixed per-phase byte pattern.
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

    fn set_endpoint(&mut self, name: &str, per_app: bool) -> Result<(), Error> {
        if name.is_empty() {
            return Err(Error::EndpointNotFound(name.to_string()));
        }
        self.per_app = per_app;
        Ok(())
    }
}

/// The emitter application: capture callback → preallocated buffer → frame
/// (bytes + seq) → stub `send` closure. Mirrors the win-emitter data-path
/// discipline (RT_CONTRACT.md): RT-side only memcpys into the preallocated
/// buffer; frame production/send runs on the worker.
pub struct LinuxEmitterApp<S, F> {
    source: S,
    tier: String,
    lossless: bool,
    stream_id: u32,
    seq: u64,
    capacity: usize,
    buf: Vec<u8>,
    send: F,
}

impl<S, F> std::fmt::Debug for LinuxEmitterApp<S, F>
where
    S: std::fmt::Debug,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LinuxEmitterApp")
            .field("tier", &self.tier)
            .field("lossless", &self.lossless)
            .field("stream_id", &self.stream_id)
            .field("seq", &self.seq)
            .field("capacity", &self.capacity)
            .field("buffered", &self.buf.len())
            .finish()
    }
}

impl<S, F> LinuxEmitterApp<S, F>
where
    S: CaptureSource,
    F: FnMut(Frame) -> Result<(), Error>,
{
    /// Build the app with a preallocated `capacity`-byte capture buffer. The
    /// lossless path is refused up front when the tier gate denies it.
    pub fn new(source: S, tier: &str, stream_id: u32, capacity: usize, send: F) -> Self {
        LinuxEmitterApp {
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

    /// Delegate: select the capture node (optionally a per-application node).
    pub fn set_endpoint(&mut self, name: &str, per_app: bool) -> Result<(), Error> {
        self.source.set_endpoint(name, per_app)
    }

    /// Delegate: negotiated capture format.
    pub fn current_format(&self) -> FormatMeta {
        self.source.format()
    }

    /// **RT-side** capture callback: memcpy `block` into the preallocated
    /// buffer; overflow is rejected with [`Error::BufferOverflow`]; no
    /// allocation on this path.
    pub fn push_block(&mut self, block: &[u8]) -> Result<(), Error> {
        if self.buf.len().saturating_add(block.len()) > self.capacity {
            return Err(Error::BufferOverflow);
        }
        self.buf.extend_from_slice(block);
        Ok(())
    }

    /// Drain the buffered bytes into the next seq-numbered [`Frame`] and
    /// deliver it to the `send` closure. Worker-side, not RT.
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
        let free =
            LinuxEmitterApp::new(FakeCaptureSource::new(stereo48()), "Free", 1, 4096, |_| {
                Ok(())
            });
        assert!(!free.lossless_allowed());

        let pro = LinuxEmitterApp::new(FakeCaptureSource::new(stereo48()), "Pro", 1, 4096, |_| {
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
        assert_eq!(a.next_block().len(), 960);
    }

    #[test]
    fn app_increments_seq_and_delivers_to_send() {
        let mut src = FakeCaptureSource::new(stereo48());
        let blocks = [src.next_block(), src.next_block(), src.next_block()];

        let expected: Vec<Vec<u8>> = blocks.to_vec();
        let mut sent: Vec<Frame> = Vec::new();
        let mut app = LinuxEmitterApp::new(src, "Pro", 7, 8192, |f| {
            sent.push(f.clone());
            Ok(())
        });

        app.start().unwrap();
        for block in &blocks {
            app.push_block(block).unwrap();
            app.emit_frame().unwrap();
        }

        assert_eq!(sent.len(), 3);
        for (i, f) in sent.iter().enumerate() {
            assert_eq!(f.stream_id, 7);
            assert_eq!(f.seq, i as u64);
            assert_eq!(&f.bytes, &expected[i]);
        }
    }

    #[test]
    fn push_block_overflows_never_panics() {
        let src = FakeCaptureSource::new(stereo48());
        let mut app = LinuxEmitterApp::new(src, "Pro", 1, 16, |_| Ok(()));
        app.push_block(&[0u8; 16]).unwrap();
        assert_eq!(app.push_block(&[0u8; 1]), Err(Error::BufferOverflow));
    }

    #[test]
    fn per_app_targeting_is_supported() {
        let mut src = FakeCaptureSource::new(stereo48());
        assert!(!src.is_per_app());
        src.set_endpoint("app:spotify", true).unwrap();
        assert!(
            src.is_per_app(),
            "Linux per-app node targeting must be representable"
        );
        assert_eq!(
            src.set_endpoint("", false),
            Err(Error::EndpointNotFound(String::new()))
        );
    }

    #[test]
    fn route_change_matches_all_variants() {
        let cases = [
            RouteChange::DefaultSinkChanged("alsa_output.builtin".into()),
            RouteChange::NodeAdded("alsa_output.usb-0d8c_1234".into()),
            RouteChange::NodeRemoved("alsa_output.old".into()),
            RouteChange::FormatChanged(stereo48()),
        ];
        match &cases[0] {
            RouteChange::DefaultSinkChanged(n) => assert_eq!(n, "alsa_output.builtin"),
            _ => unreachable!(),
        }
        match &cases[1] {
            RouteChange::NodeAdded(n) => assert_eq!(n, "alsa_output.usb-0d8c_1234"),
            _ => unreachable!(),
        }
        match &cases[2] {
            RouteChange::NodeRemoved(n) => assert_eq!(n, "alsa_output.old"),
            _ => unreachable!(),
        }
        match &cases[3] {
            RouteChange::FormatChanged(m) => assert_eq!(*m, stereo48()),
            _ => unreachable!(),
        }
    }
}
