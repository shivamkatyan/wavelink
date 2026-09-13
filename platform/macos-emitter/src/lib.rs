//! `macos-emitter` — standalone macOS emitter scaffold (task t-B3-macos).
//!
//! A Wavelink macOS emitter is a **capture** component: it records
//! the system audio (playback) mix and forwards it to the receiver. This crate
//! is the small, standalone, portable seam — its own trait copies, zero core-
//! crate deps — mirroring `platform/win-emitter` and `platform/linux-emitter`
//! exactly:
//!
//! * [`CaptureSource`] — the platform capture-adapter trait (own copy).
//! * [`FormatMeta`], [`EndpointInfo`], [`RouteChange`] — capture metadata and
//!   the route-change event surface. As on Linux, `EndpointInfo` carries a
//!   `per_app_capable` bool and [`CaptureSource::set_endpoint`] takes a
//!   `per_app` flag — macOS supports per-application capture via Core Audio
//!   process taps (macOS 14.2+; PLATFORM_MATRIX §A, ADR-008).
//! * [`MacEmitterApp`] — ties the capture callback (memcpy into a caller
//!   buffer) to frame production (wrap bytes with a sequence number) and to a
//!   stub `send` closure, with a Free/Pro lossless policy gate via
//!   [`allow_lossless`].
//! * [`FakeCaptureSource`] — deterministic test double producing the same
//!   5 ms / i16 byte pattern as the win/linux siblings (cross-crate golden
//!   agreement).
//! * [`backend`] (`#[cfg(target_os = "macos")]`) — the genuinely-macOS part:
//!   Core Audio HAL device metadata + USB hotplug, the Screen Recording TCC
//!   permission state machine, the ScreenCaptureKit system-capture adapter,
//!   and the (feature-gated) Core Audio process-tap seam.
//!
//! # macOS capture facts (docs/planning/PLATFORM_MATRIX.md §A, RT_CONTRACT.md,
//! ADR-008)
//!
//! * **System-wide capture** — ScreenCaptureKit audio, macOS 13+
//!   (`SCStreamConfiguration.capturesAudio = true`). Screen Recording TCC via
//!   `NSScreenCaptureUsageDescription`. The SCK audio `sample handler` is RT-
//!   ish and may only copy `CMSampleBuffer` audio bytes into a preallocated
//!   buffer (RT_CONTRACT macOS SCK row).
//! * **Per-application capture** — Core Audio process taps, macOS 14.2+
//!   (`AudioHardwareCreateProcessTap` with an ObjC `CATapDescription`),
//!   feature-gated `macos14-taps`.
//! * **USB DAC** — HAL `AudioObject` enumeration / default-output / hotplug;
//!   USB transport heuristic (`kAudioDeviceTransportTypeUSB`). There is **no
//!   `AVAudioSession` on macOS** — the HAL default-output-device is the only
//!   system-wide output handle the app can follow.
//!
//! # What this crate proves on THIS host (honest split)
//!
//! * **Compiled / live-validated on this macOS host**: the portable surface +
//!   its unit tests, the compile of the whole `backend`, the HAL metadata FFI
//!   (live-ran during development — see `build-check.md`), the permission
//!   state-machine unit tests, and the process-tap FFI symbol *link* gate.
//! * **Hardware-gated** (needs a logged-in GUI session / a USB DAC): actually
//!   receiving SCK audio or a process tap end-to-end, live hotplug delivery,
//!   USB-DAC recognition. None of that is claimed here.

#![deny(rust_2018_idioms)]

/// macOS capture backend (`#[cfg(target_os = "macos")]` — compiled on any macOS
/// host, including this one).
///
/// HAL metadata/hotplug, Screen Recording permission state machine,
/// ScreenCaptureKit system capture, and the (14.2+, feature-gated) process-tap
/// seam. On a non-macOS host this module is not compiled at all — see `backend`
/// and `build-check.md` for what is compile-validated here vs hardware-gated.
#[cfg(target_os = "macos")]
pub mod backend;

/// The `--stream` driver: real capture (SCK) or a deterministic fixture →
/// [`wdr_refsim::sink::AudioFrameSink`] seam → QUIC receiver. macOS-only
/// (SCK backend + POSIX signals); see `bin/macos_emitter.rs` / `stream.rs`.
#[cfg(target_os = "macos")]
pub mod stream;

/// The `--receive` driver (WS3 desktop receiver render): `QuicRenderReceiver`
/// seam → a selectable `RenderSink` (null hash sink on the host; the
/// device-gated Core Audio output sink on real hardware). macOS-only.
/// See `bin/macos_emitter.rs` / `receive.rs`.
#[cfg(target_os = "macos")]
pub mod receive;

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

/// Description of one capturable output endpoint (HAL `AudioObject`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EndpointInfo {
    /// Endpoint name / friendly identifier (HAL `kAudioObjectPropertyName`).
    pub name: String,
    /// USB DAC heuristic: device transport type reports
    /// `kAudioDeviceTransportTypeUSB`. Not authoritative for every DAC
    /// (some class-compliant devices report a different transport; see
    /// build-check.md).
    pub is_usb: bool,
    /// Whether the endpoint is the current system default output
    /// (`kAudioHardwarePropertyDefaultOutputDevice`).
    pub is_default: bool,
    /// Whether per-application capture is available for this endpoint (macOS
    /// Core Audio process taps, 14.2+; feature-gated `macos14-taps`). Carried
    /// like linux-emitter because macOS supports per-app.
    pub per_app_capable: bool,
}

/// Route-change events surfaced by the capture adapter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RouteChange {
    /// The default output device changed (name == new default if known).
    DefaultDeviceChanged(Option<String>),
    /// An output device was added (e.g. a USB DAC hotplug).
    DeviceAdded(String),
    /// An output device was removed.
    DeviceRemoved(String),
    /// The negotiated capture format changed.
    FormatChanged(FormatMeta),
}

/// Fallible capture-adapter operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// `start` while already running.
    AlreadyStarted,
    /// `stop`/read attempted without a running capture.
    NotStarted,
    /// `set_endpoint` referenced an endpoint that does not exist.
    EndpointNotFound(String),
    /// The endpoint cannot provide the requested format/settings.
    UnsupportedFormat,
    /// Preallocated capture buffer overflowed (RT push rejected).
    BufferOverflow,
    /// The `send` closure rejected a frame.
    SendFailed,
    /// macOS CoreAudio / ScreenCaptureKit failure (message decoration only).
    MacAudio(String),
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
            Error::MacAudio(m) => return write!(f, "macos audio error: {m}"),
        };
        f.write_str(msg)
    }
}

impl std::error::Error for Error {}

/// The platform **capture** adapter trait (own copy; mirrors ARCHITECTURE.md).
pub trait CaptureSource {
    /// Begin capturing audio from the currently selected endpoint.
    fn start(&mut self) -> Result<(), Error>;
    /// Stop capturing; idempotent.
    fn stop(&mut self);
    /// The negotiated capture format.
    fn format(&self) -> FormatMeta;
    /// Select the capture endpoint by name; if `per_app`, target a specific
    /// application's audio (macOS: Core Audio process taps, 14.2+,
    /// feature-gated `macos14-taps`; see `backend::tap`).
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

/// Deterministic test double implementing [`CaptureSource`].
///
/// Produces a fixed byte production per call using the same 5 ms / i16
/// algorithm as the win/linux siblings, so the push→frame→send chain is fully
/// deterministically testable and cross-crate goldens agree.
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
    /// i16-byte samples with a fixed per-phase byte pattern. Repeated calls
    /// advance the phase; identical for a fresh source of the same format.
    /// This models the bytes a real platform capture callback hands to
    /// [`MacEmitterApp::push_block`].
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
/// (bytes + seq) → stub `send` closure.
///
/// The **data-path** discipline mirrors `docs/planning/RT_CONTRACT.md`
/// (macOS rows): [`push_block`](Self::push_block) is the RT-side callback and
/// only memcpys into the preallocated buffer (overflow returns an error; it
/// never allocates or grows). [`emit_frame`](Self::emit_frame) is the
/// worker-side step that wraps buffered bytes into a [`Frame`] with the next
/// [`Frame::seq`] and hands it to the injected `send` closure.
pub struct MacEmitterApp<S, F> {
    source: S,
    tier: String,
    lossless: bool,
    stream_id: u32,
    seq: u64,
    capacity: usize,
    buf: Vec<u8>,
    send: F,
}

impl<S, F> std::fmt::Debug for MacEmitterApp<S, F>
where
    S: std::fmt::Debug,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MacEmitterApp")
            .field("tier", &self.tier)
            .field("lossless", &self.lossless)
            .field("stream_id", &self.stream_id)
            .field("seq", &self.seq)
            .field("capacity", &self.capacity)
            .field("buffered", &self.buf.len())
            .finish()
    }
}

impl<S, F> MacEmitterApp<S, F>
where
    S: CaptureSource,
    F: FnMut(Frame) -> Result<(), Error>,
{
    /// Build the app with a preallocated `capacity`-byte capture buffer. The
    /// lossless path is refused up front when the tier gate
    /// ([`allow_lossless`]) denies it — lossless never silently slips through,
    /// mirroring `wdr_entitlement` FR-047 semantics (no core dependency).
    pub fn new(source: S, tier: &str, stream_id: u32, capacity: usize, send: F) -> Self {
        MacEmitterApp {
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

    /// Delegate: select the capture endpoint (optionally a per-application
    /// endpoint).
    pub fn set_endpoint(&mut self, name: &str, per_app: bool) -> Result<(), Error> {
        self.source.set_endpoint(name, per_app)
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
        let free = MacEmitterApp::new(FakeCaptureSource::new(stereo48()), "Free", 1, 4096, |_| {
            Ok(())
        });
        assert!(!free.lossless_allowed());

        let pro = MacEmitterApp::new(FakeCaptureSource::new(stereo48()), "Pro", 1, 4096, |_| {
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
        // 480 interleaved frames × 2 bytes = 960 bytes. Matches win/linux.
        assert_eq!(a.next_block().len(), 960);
    }

    #[test]
    fn app_increments_seq_and_delivers_to_send() {
        let mut src = FakeCaptureSource::new(stereo48());
        let blocks = [src.next_block(), src.next_block(), src.next_block()];

        let expected: Vec<Vec<u8>> = blocks.to_vec();
        let mut sent: Vec<Frame> = Vec::new();
        let mut app = MacEmitterApp::new(src, "Pro", 7, 8192, |f| {
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
        let mut app = MacEmitterApp::new(src, "Pro", 1, 16, |_| Ok(()));
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
            src.set_endpoint("", false),
            Err(Error::EndpointNotFound(String::new()))
        );
        assert_eq!(src.set_endpoint("USB DAC", false), Ok(()));
    }

    #[test]
    fn per_app_targeting_is_supported() {
        let mut src = FakeCaptureSource::new(stereo48());
        assert!(!src.is_per_app());
        src.set_endpoint("app:music", true).unwrap();
        assert!(
            src.is_per_app(),
            "macOS per-app targeting (Core Audio process taps) must be representable"
        );
        assert_eq!(
            src.set_endpoint("", false),
            Err(Error::EndpointNotFound(String::new()))
        );
    }

    #[test]
    fn route_change_matches_all_variants() {
        let cases = [
            RouteChange::DefaultDeviceChanged(Some("Mac mini Speakers".into())),
            RouteChange::DefaultDeviceChanged(None),
            RouteChange::DeviceAdded("USB DAC".into()),
            RouteChange::DeviceRemoved("Old DAC".into()),
            RouteChange::FormatChanged(stereo48()),
        ];
        // Match every variant at least once (exhaustiveness enforced by compiler).
        match &cases[0] {
            RouteChange::DefaultDeviceChanged(Some(n)) => assert_eq!(n, "Mac mini Speakers"),
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
