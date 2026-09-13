//! `linux-receiver` — standalone Linux Bluetooth A2DP-sink receiver scaffold
//! (t-B5-linux-receiver).
//!
//! A Wavelink Linux **receiver** is the ONLY full public Bluetooth
//! receive+render component this product supports (ADR-009 two-axis model,
//! PLATFORM_MATRIX §B Linux row): a Linux box running THIS product registers a
//! BlueZ `a2dp_sink` profile, receives A2DP (SBC/FastStream/AAC — lossy), and
//! renders the decoded PCM through a PipeWire media sink to the selected output
//! (its own USB DAC). This is the FR-034 product-receiver definition — a device
//! running this product receives AND renders to its own output/DAC. Routing the
//! received audio to an ordinary Bluetooth headset does NOT satisfy it, and the
//! [`BluetoothMesh`] honesty matrix below says so.
//!
//! This crate is the small, standalone, buildable-on-this-host seam that mirrors
//! the sibling `platform/linux-emitter` (same "own trait, no core crate dep"
//! design):
//!
//! * [`RenderSink`] — the platform render-adapter trait (own copy).
//! * [`FormatMeta`], [`EndpointInfo`], [`RouteChange`], [`Error`] — the receiver
//!   metadata / route-event / error surface. `EndpointInfo.sink_role` describes
//!   the output a received stream is rendered to (USB DAC vs PipeWire
//!   media-sink vs built-in).
//! * [`LinuxReceiverApp`] — the RT-side render callback (memcpy into a
//!   preallocated buffer) → [`Frame`] (bytes + seq) → stub `render` closure,
//!   with the Free/Pro lossless policy gate via [`allow_lossless`].
//! * [`BluetoothMesh`] / [`ReceiverRole`] — the honest per-cell A2DP-sink
//!   support matrix (FR-033) with the one-action fallback to free lossy Wi-Fi
//!   (FR-033/FR-034). A stock Android/iOS/Windows/macOS device can never be an
//!   A2DP sink through this product — that limitation is part of this API's
//!   contract, not an aside.
//! * [`FakeRenderSink`] — deterministic Linux test double.
//!
//! Callers must not claim lossless receive over Bluetooth: standard A2DP has no
//! lossless mode, and Pro's lossless (FR-021) is a Wi-Fi capability only —
//! [`bt_supports_lossless`] and [`BtFidelity`] make that mechanically checkable.
//!
//! The actually-receiving backend [`bluez`] is BOTH `#[cfg(target_os =
//! "linux")]`- and feature-`bt`-gated: it is compiled/validated only on a Linux
//! runner with BlueZ (and the PipeWire daemon for the `pipewire` feature), never
//! on this macOS host (see `build-check.md`). Nothing here implies a live A2DP
//! registration or render has been validated on the build host.

#![deny(rust_2018_idioms)]

/// BlueZ A2DP-sink receive backend (Linux-only, feature `bt`). On this macOS
/// host — no Linux target, no BlueZ daemon — it is NOT compiled by the target
/// gate; see `build-check.md` for the honest split.
#[cfg(all(target_os = "linux", feature = "bt"))]
pub mod bluez;

/// Render/format metadata for the selected output endpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FormatMeta {
    /// Sampling rate in Hz.
    pub rate: u32,
    /// Bit depth per sample.
    pub bits: u32,
    /// Channel count.
    pub channels: u32,
}

/// The role of an output endpoint the received audio is rendered to. Kept
/// separate from raw names so product UI can show an honest route label
/// (FR-014/FR-015) without guessing from strings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SinkRole {
    /// A class-compliant USB DAC (`alsa_output.usb-*`, `device.product`
    /// contains "usb") — the FR-034 product-receiver render target.
    UsbDac,
    /// A PipeWire media-sink node (e.g. `bluez_output.…` or `alsa_output.…`)
    /// that itself renders to a physical device.
    PipeWireMediaSink,
    /// The built-in / default output (never the product-receiver gate).
    BuiltIn,
}

/// Description of one renderable audio output endpoint (a sink the received
/// PCM can be routed to).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EndpointInfo {
    /// Node/device name (e.g. `alsa_output.usb-0d8c_1234.analog-stereo`, a
    /// `node.name`, or a BT device for the low-bitrate product-peer cell).
    pub name: String,
    /// USB DAC heuristic (deterministic node naming on Linux —
    /// `device.product`/`alsa_output.usb-*`; see PLATFORM_MATRIX §A/B).
    pub is_usb: bool,
    /// What this endpoint is (drives the honest route label).
    pub sink_role: SinkRole,
}

/// Route-change events surfaced by the Bluetooth receiver / render adapter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RouteChange {
    /// A render output (PipeWire/ALSA sink, usually the USB DAC) became
    /// available.
    SinkConnected(String),
    /// The render output was lost (detach/unplug, BT device departed).
    SinkDisconnected(String),
    /// BlueZ `Profile1` with the `a2dp_sink` role is registered and the box is
    /// pairable/discoverable as a Bluetooth speaker (ADR-009 standard sink
    /// path — Linux only).
    A2DPProfileReady,
    /// A custom classic-socket product peer connected (RFCOMM/L2CAP, Android /
    /// Windows-RFCOMM / Linux; low-bitrate lossy fallback cell — the app
    /// decodes and renders to its own output).
    RFCOMMPeerConnected { name: String },
    /// The negotiated format changed (A2DP codec negotiation or sink format).
    FormatChanged(FormatMeta),
}

/// Fallible render-adapter operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// `start` while already running.
    AlreadyStarted,
    /// `stop`/pull without a running render or output.
    NotStarted,
    /// `route` referenced an output endpoint that does not exist.
    EndpointNotFound(String),
    /// The output cannot render the requested format.
    UnsupportedFormat,
    /// Preallocated render buffer overflowed (RT push rejected).
    BufferOverflow,
    /// The `render` closure rejected a frame.
    RenderFailed,
    /// BlueZ/D-Bus failure (registration, Profile1, daemon unavailable).
    BlueZ(String),
    /// PipeWire failure (message decoration only).
    PipeWire(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let msg = match self {
            Error::AlreadyStarted => "render already started",
            Error::NotStarted => "render not started",
            Error::EndpointNotFound(n) => return write!(f, "endpoint not found: {n}"),
            Error::UnsupportedFormat => "unsupported render format",
            Error::BufferOverflow => "render buffer overflow",
            Error::RenderFailed => "frame render rejected",
            Error::BlueZ(m) => return write!(f, "bluez error: {m}"),
            Error::PipeWire(m) => return write!(f, "pipewire error: {m}"),
        };
        f.write_str(msg)
    }
}

impl std::error::Error for Error {}

/// The platform **render** adapter trait (own copy; receiver-flipped sibling of
/// linux-emitter's `CaptureSource`). The received, decoded PCM is handed to the
/// adapter, which renders it to the selected output (USB DAC via the PipeWire
/// media sink on Linux).
pub trait RenderSink {
    /// Begin rendering to the currently selected output.
    fn start(&mut self) -> Result<(), Error>;
    /// Stop rendering; idempotent.
    fn stop(&mut self);
    /// The negotiated render format.
    fn format(&self) -> FormatMeta;
    /// Route the render output to `target` (e.g. the ALSA/PipeWire name of a
    /// USB DAC). Empty/unknown targets fail with [`Error::EndpointNotFound`].
    fn route(&mut self, target: &str) -> Result<(), Error>;
}

/// Tier gate mirroring `wdr_entitlement` semantics (Free=false, Pro=true)
/// without depending on the core crate. Unknown tier strings fail closed.
///
/// NOTE this gates **Wi-Fi** lossless (FR-021) only. The Bluetooth receive
/// path (standard A2DP sink) is lossy in Free AND Pro — see [`BtFidelity`].
pub fn allow_lossless(tier: &str) -> bool {
    matches!(tier, "Pro")
}

/// Honest fidelity classification of the Bluetooth receive path. Standard A2DP
/// is SBC/AAC — lossy by design; there is no standard lossless A2DP mode, and
/// Pro's lossless (FR-021) is delivered over Wi-Fi, never Bluetooth
/// (ADR-009 / PLATFORM_MATRIX §B). Product UI/telemetry must never claim
/// "lossless over BT"; this type makes the claim mechanically impossible.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BtFidelity {
    /// The BT receive path is lossy-only (Free and Pro alike).
    LossyOnly,
}

impl std::fmt::Display for BtFidelity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(
            "Bluetooth receive is lossy-only (A2DP SBC); lossless (FR-021) is Pro Wi-Fi only",
        )
    }
}

/// Whether the Bluetooth receive path can ever carry lossless audio. Always
/// `false` — exposed so any code path or UI can be asserted against it.
pub const fn bt_supports_lossless() -> bool {
    false
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

// ---------------------------------------------------------------------------
// Honest Bluetooth receive matrix (FR-033/FR-034, ADR-009)
// ---------------------------------------------------------------------------

/// A cell of the Bluetooth receive matrix — one (platform, transport) pairing
/// the product UI can show.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PlatformCell {
    /// Linux standard A2DP sink (BlueZ `a2dp_sink` + PipeWire media-sink): a
    /// Linux box running THIS product becomes a Bluetooth speaker and renders
    /// to its own output/DAC — the only full public receive+render path.
    LinuxStandardSink,
    /// Stock Android as an A2DP sink.
    AndroidStandardSink,
    /// Stock iOS/iPadOS as an A2DP sink.
    IosStandardSink,
    /// Stock Windows as an A2DP sink.
    WindowsStandardSink,
    /// Stock macOS as an A2DP sink.
    MacOsStandardSink,
    /// Custom classic-socket product peer over RFCOMM/L2CAP on Android
    /// (app decodes and renders to its own output/DAC).
    AndroidRfcommPeer,
    /// Custom classic-socket product peer over RFCOMM on Windows.
    WindowsRfcommPeer,
    /// Custom classic-socket product peer over RFCOMM/L2CAP on Linux.
    LinuxRfcommPeer,
    /// iOS/RFCOMM MFi-scoped custom — not part of the general product.
    IosRfcommPeer,
}

impl PlatformCell {
    /// Human-readable cell label, in the same wording the product UI uses.
    pub fn label(self) -> &'static str {
        match self {
            PlatformCell::LinuxStandardSink => "Linux standard A2DP sink (free)",
            PlatformCell::AndroidStandardSink => "Android standard A2DP sink",
            PlatformCell::IosStandardSink => "iOS standard A2DP sink",
            PlatformCell::WindowsStandardSink => "Windows standard A2DP sink",
            PlatformCell::MacOsStandardSink => "macOS standard A2DP sink",
            PlatformCell::AndroidRfcommPeer => "Android RFCOMM/L2CAP product peer",
            PlatformCell::WindowsRfcommPeer => "Windows RFCOMM product peer",
            PlatformCell::LinuxRfcommPeer => "Linux RFCOMM/L2CAP product peer",
            PlatformCell::IosRfcommPeer => "iOS RFCOMM product peer",
        }
    }
}

/// Verdict for a [`PlatformCell`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CellVerdict {
    /// Full public receive+render path exists (FR-034 satisfied).
    Supported,
    /// Works, but low-bitrate lossy, at the data-transport level; the app
    /// itself decodes and renders to its own output/DAC.
    SupportedLossy,
    /// No public OS/app API exists for this cell — it can never be enabled by
    /// this product on that (stock) OS. Precisely explained + one-action
    /// fallback to free lossy Wi-Fi.
    UnsupportedByPublicApi,
}

/// The one-action fallback offered for unsupported cells (FR-033).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FallbackAction {
    /// None — the cell is genuinely supported.
    None,
    /// Free lossy Wi-Fi (FR-020): switch the stream to the Wi-Fi path. This is
    /// the ONLY fallback the product offers from an unsupported Bluetooth cell.
    /// It is never "route to a Bluetooth headset" — that would not be the
    /// product-receiver definition (FR-034).
    FreeLossyWifi,
}

/// One honest row of the FR-033 matrix.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MatrixRow {
    /// The cell this row describes.
    pub cell: PlatformCell,
    /// Verdict.
    pub verdict: CellVerdict,
    /// One-sentence why (shown verbatim in product UI + docs).
    pub note: &'static str,
    /// One-action fallback for unsupported cells.
    pub fallback: FallbackAction,
}

/// The Bluetooth receive-capability matrix the product UI renders (FR-033), as
/// a pure, portable data object — no OS probing, because every cell is a
/// *documented* fact of the public APIs (PLATFORM_MATRIX §B + ADR-009
/// evidence), not a runtime observation.
///
/// Contract: exactly one standard-sink cell is [`CellVerdict::Supported`]
/// (Linux). Every other standard-sink cell is
/// [`CellVerdict::UnsupportedByPublicApi`] with a [`FallbackAction::FreeLossyWifi`].
/// This product never implies a stock Android/iOS/Windows/macOS device can be
/// an A2DP sink — the matrix says the opposite, verbatim.
#[derive(Debug, Clone, Copy)]
pub struct BluetoothMesh;

impl BluetoothMesh {
    /// The full FR-033 rows, in display order.
    pub fn rows() -> [MatrixRow; 9] {
        [
            MatrixRow {
                cell: PlatformCell::LinuxStandardSink,
                verdict: CellVerdict::Supported,
                note: "BlueZ a2dp_sink + PipeWire media-sink: a Linux box running this \
                       product is a Bluetooth speaker and renders to its own output/USB DAC \
                       (the only full public receive+render path, ADR-009).",
                fallback: FallbackAction::None,
            },
            MatrixRow {
                cell: PlatformCell::AndroidStandardSink,
                verdict: CellVerdict::UnsupportedByPublicApi,
                note: "No public/3rd-party A2DP sink API on stock Android \
                       (BluetoothA2dpSink is hidden/removed; verified against AOSP).",
                fallback: FallbackAction::FreeLossyWifi,
            },
            MatrixRow {
                cell: PlatformCell::IosStandardSink,
                verdict: CellVerdict::UnsupportedByPublicApi,
                note: "No public A2DP sink API on iOS (verified); hearing-device status is \
                       read-only.",
                fallback: FallbackAction::FreeLossyWifi,
            },
            MatrixRow {
                cell: PlatformCell::WindowsStandardSink,
                verdict: CellVerdict::UnsupportedByPublicApi,
                note: "Windows A2DP sink is an OS/vendor-driver role; no WinRT/Win32 app \
                       sink API (OS is source-only from an app's view).",
                fallback: FallbackAction::FreeLossyWifi,
            },
            MatrixRow {
                cell: PlatformCell::MacOsStandardSink,
                verdict: CellVerdict::UnsupportedByPublicApi,
                note: "No A2DP sink app API on macOS (AirPlay Receiver is a receive path \
                       with no app PCM access).",
                fallback: FallbackAction::FreeLossyWifi,
            },
            MatrixRow {
                cell: PlatformCell::AndroidRfcommPeer,
                verdict: CellVerdict::SupportedLossy,
                note: "Custom classic sockets (BluetoothSocket RFCOMM/L2CAP): the app \
                       decodes and renders to its own output/DAC; low-bitrate lossy \
                       (bandwidth-limited), never claimed as hi-fi.",
                fallback: FallbackAction::None,
            },
            MatrixRow {
                cell: PlatformCell::WindowsRfcommPeer,
                verdict: CellVerdict::SupportedLossy,
                note: "Custom RFCOMM via WinRT/Winsock (L2CAP at app level is \
                       unsupported); app decodes and renders; low-bitrate lossy.",
                fallback: FallbackAction::None,
            },
            MatrixRow {
                cell: PlatformCell::LinuxRfcommPeer,
                verdict: CellVerdict::SupportedLossy,
                note: "Custom RFCOMM/L2CAP (ProfileManager/Profile1 D-Bus); app decodes \
                       and renders; low-bitrate lossy.",
                fallback: FallbackAction::None,
            },
            MatrixRow {
                cell: PlatformCell::IosRfcommPeer,
                verdict: CellVerdict::UnsupportedByPublicApi,
                note: "iOS generic classic-socket access is MFi-scoped (ExternalAccessory); \
                       not part of the general product.",
                fallback: FallbackAction::FreeLossyWifi,
            },
        ]
    }

    /// Verdict for a single cell.
    pub fn verdict(cell: PlatformCell) -> CellVerdict {
        BluetoothMesh::rows()
            .into_iter()
            .find(|r| r.cell == cell)
            .map(|r| r.verdict)
            .unwrap_or(CellVerdict::UnsupportedByPublicApi)
    }

    /// The one-action fallback the product offers from an unsupported cell —
    /// always free lossy Wi-Fi (FR-033/FR-034); never routing to a headset.
    pub fn fallback(cell: PlatformCell) -> FallbackAction {
        BluetoothMesh::rows()
            .into_iter()
            .find(|r| r.cell == cell)
            .map(|r| r.fallback)
            .unwrap_or(FallbackAction::FreeLossyWifi)
    }
}

/// The receiver role a given session is operating as (ADR-009 two-axis model).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReceiverRole {
    /// Standard A2DP sink (the full public path — Linux only).
    StandardA2DPSink,
    /// Custom classic-socket product peer (low-bitrate lossy fallback cell).
    RfcommProductPeer,
}

impl ReceiverRole {
    /// Display label for product UI.
    pub fn label(self) -> &'static str {
        match self {
            ReceiverRole::StandardA2DPSink => {
                "Standard A2DP sink (BlueZ a2dp_sink + PipeWire media-sink)"
            }
            ReceiverRole::RfcommProductPeer => {
                "Custom RFCOMM/L2CAP product peer (low-bitrate lossy)"
            }
        }
    }

    /// Every Bluetooth receive role is lossy-only (free tier). Pro's lossless
    /// is a Wi-Fi capability — never surfaced on a BT role.
    pub fn fidelity(self) -> BtFidelity {
        BtFidelity::LossyOnly
    }
}

// ---------------------------------------------------------------------------
// Application shell: RT push_render → preallocated buffer → worker pull_block
// ---------------------------------------------------------------------------

/// Deterministic Linux test double implementing [`RenderSink`].
#[derive(Debug)]
pub struct FakeRenderSink {
    fmt: FormatMeta,
    running: bool,
    route_target: Option<String>,
    rendered: Vec<u8>,
    phase: u8,
}

impl FakeRenderSink {
    /// New render sink with the given format.
    pub fn new(fmt: FormatMeta) -> Self {
        FakeRenderSink {
            fmt,
            running: false,
            route_target: None,
            rendered: Vec::new(),
            phase: 0,
        }
    }

    /// The currently routed output target (set via [`RenderSink::route`]).
    pub fn route_target(&self) -> Option<&str> {
        self.route_target.as_deref()
    }

    /// Whether the sink is currently rendering.
    pub fn is_running(&self) -> bool {
        self.running
    }

    /// Total bytes handed to [`FakeRenderSink::deliver`] so far (deterministic
    /// accounting for tests).
    pub fn rendered_bytes(&self) -> usize {
        self.rendered.len()
    }

    /// Deterministic next render block: 5 ms @ `fmt.rate` of interleaved i16
    /// samples with a fixed per-phase byte pattern (mirror of
    /// `FakeCaptureSource::next_block`).
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

    /// Accept a rendered frame's bytes (the seam the app's `render` closure
    /// calls). Deterministic accumulation — no real audio is produced,
    /// matching the sibling fake contract.
    pub fn deliver(&mut self, bytes: &[u8]) -> Result<(), Error> {
        self.rendered.extend_from_slice(bytes);
        Ok(())
    }
}

impl RenderSink for FakeRenderSink {
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

    fn route(&mut self, target: &str) -> Result<(), Error> {
        if target.is_empty() {
            return Err(Error::EndpointNotFound(target.to_string()));
        }
        self.route_target = Some(target.to_string());
        Ok(())
    }
}

/// The receiver application: RT render callback → preallocated buffer → frame
/// (bytes + seq) → stub `render` closure that drives the [`RenderSink`]
/// (e.g. the PipeWire media-sink writing to the USB DAC). Mirrors the
/// linux-emitter data-path discipline (RT_CONTRACT.md): the RT side only
/// memcpys into the preallocated buffer; frame production/delivery runs on the
/// worker.
pub struct LinuxReceiverApp<S, R> {
    sink: S,
    tier: String,
    lossless: bool,
    stream_id: u32,
    seq: u64,
    capacity: usize,
    buf: Vec<u8>,
    render: R,
}

impl<S, R> std::fmt::Debug for LinuxReceiverApp<S, R>
where
    S: std::fmt::Debug,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LinuxReceiverApp")
            .field("tier", &self.tier)
            .field("lossless", &self.lossless)
            .field("stream_id", &self.stream_id)
            .field("seq", &self.seq)
            .field("capacity", &self.capacity)
            .field("buffered", &self.buf.len())
            .finish()
    }
}

impl<S, R> LinuxReceiverApp<S, R>
where
    S: RenderSink,
    R: FnMut(Frame) -> Result<(), Error>,
{
    /// Build the app with a preallocated `capacity`-byte render buffer. The
    /// lossless path (Wi-Fi, FR-021) is refused up front when the tier gate
    /// denies it; the Bluetooth path is lossy-only regardless of tier (see
    /// [`bt_supports_lossless`]).
    pub fn new(sink: S, tier: &str, stream_id: u32, capacity: usize, render: R) -> Self {
        LinuxReceiverApp {
            sink,
            tier: tier.to_string(),
            lossless: allow_lossless(tier),
            stream_id,
            seq: 0,
            capacity,
            buf: Vec::with_capacity(capacity),
            render,
        }
    }

    /// Whether the lossless (Wi-Fi) path is allowed for this app's tier.
    pub fn lossless_allowed(&self) -> bool {
        self.lossless
    }

    /// The configured tier name.
    pub fn tier(&self) -> &str {
        &self.tier
    }

    /// Honest fidelity of this app's Bluetooth receive path — always
    /// [`BtFidelity::LossyOnly`], independent of tier.
    pub fn bt_fidelity(&self) -> BtFidelity {
        BtFidelity::LossyOnly
    }

    /// Delegate: begin rendering on the sink.
    pub fn start(&mut self) -> Result<(), Error> {
        self.sink.start()
    }

    /// Delegate: stop rendering on the sink.
    pub fn stop(&mut self) {
        self.sink.stop();
    }

    /// Delegate: route the render output (e.g. to a USB DAC's device name).
    pub fn route(&mut self, target: &str) -> Result<(), Error> {
        self.sink.route(target)
    }

    /// Delegate: negotiated render format.
    pub fn current_format(&self) -> FormatMeta {
        self.sink.format()
    }

    /// **RT-side** render callback (Linux: `pw_stream::process` with
    /// `PW_STREAM_FLAG_RT_PROCESS`): memcpy the (already decoded) PCM `block`
    /// into the preallocated buffer. Overflow is rejected with
    /// [`Error::BufferOverflow`]; no allocation, no locks, no blocking on this
    /// path (RT_CONTRACT.md Linux PipeWire row).
    pub fn push_render(&mut self, block: &[u8]) -> Result<(), Error> {
        if self.buf.len().saturating_add(block.len()) > self.capacity {
            return Err(Error::BufferOverflow);
        }
        self.buf.extend_from_slice(block);
        Ok(())
    }

    /// Drain the buffered bytes into the next seq-numbered [`Frame`] and
    /// deliver it to the `render` closure (which hands it to the [`RenderSink`]
    /// targeting the USB DAC). **Worker-side, never RT.**
    pub fn pull_block(&mut self) -> Result<Frame, Error> {
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
        (self.render)(frame.clone())?;
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
    fn bt_is_never_lossless_even_on_pro() {
        // The BT receive path is lossy-only in every tier: Pro's lossless is
        // Wi-Fi (FR-021), never Bluetooth (ADR-009).
        assert!(!bt_supports_lossless());

        let pro =
            LinuxReceiverApp::new(FakeRenderSink::new(stereo48()), "Pro", 1, 4096, |_| Ok(()));
        assert!(pro.lossless_allowed(), "Pro allows Wi-Fi lossless");
        assert_eq!(pro.bt_fidelity(), BtFidelity::LossyOnly);
        assert_eq!(
            pro.bt_fidelity().to_string(),
            "Bluetooth receive is lossy-only (A2DP SBC); lossless (FR-021) is Pro Wi-Fi only"
        );

        let free =
            LinuxReceiverApp::new(FakeRenderSink::new(stereo48()), "Free", 1, 4096, |_| Ok(()));
        assert!(!free.lossless_allowed(), "Free denies Wi-Fi lossless");
        assert_eq!(free.bt_fidelity(), BtFidelity::LossyOnly);
    }

    #[test]
    fn fake_render_sink_is_deterministic() {
        let mut a = FakeRenderSink::new(stereo48());
        let mut b = FakeRenderSink::new(stereo48());
        for _ in 0..4 {
            assert_eq!(
                a.next_block(),
                b.next_block(),
                "production must be deterministic"
            );
        }
        assert_eq!(a.next_block().len(), 960);
        assert_eq!(a.rendered_bytes(), 0);
    }

    #[test]
    fn app_increments_seq_and_delivers_to_render() {
        let mut sink = FakeRenderSink::new(stereo48());
        let blocks = [sink.next_block(), sink.next_block(), sink.next_block()];

        let expected: Vec<Vec<u8>> = blocks.to_vec();
        let mut seen: Vec<Frame> = Vec::new();
        let mut app = LinuxReceiverApp::new(sink, "Pro", 7, 8192, |f| {
            seen.push(f.clone());
            Ok(())
        });

        app.start().unwrap();
        for block in &blocks {
            // RT side: push into the preallocated render buffer.
            app.push_render(block).unwrap();
            // Worker side: pull a seq-numbered frame and deliver it.
            app.pull_block().unwrap();
        }

        assert_eq!(seen.len(), 3);
        for (i, f) in seen.iter().enumerate() {
            assert_eq!(f.stream_id, 7);
            assert_eq!(f.seq, i as u64);
            assert_eq!(&f.bytes, &expected[i]);
        }
    }

    #[test]
    fn push_render_overflow_never_panics() {
        let sink = FakeRenderSink::new(stereo48());
        let mut app = LinuxReceiverApp::new(sink, "Pro", 1, 16, |_| Ok(()));
        app.push_render(&[0u8; 16]).unwrap();
        assert_eq!(app.push_render(&[0u8; 1]), Err(Error::BufferOverflow));
    }

    #[test]
    fn route_selects_output_and_rejects_empty() {
        let mut sink = FakeRenderSink::new(stereo48());
        assert_eq!(sink.route_target(), None);
        sink.route("alsa_output.usb-0d8c_1234.analog-stereo")
            .unwrap();
        assert_eq!(
            sink.route_target(),
            Some("alsa_output.usb-0d8c_1234.analog-stereo")
        );
        assert_eq!(sink.route(""), Err(Error::EndpointNotFound(String::new())));
        assert_eq!(
            sink.route_target(),
            Some("alsa_output.usb-0d8c_1234.analog-stereo"),
            "failed route must not clobber the previous target"
        );

        // Route the app itself (delegation).
        let mut app =
            LinuxReceiverApp::new(FakeRenderSink::new(stereo48()), "Free", 2, 128, |_| Ok(()));
        app.route("alsa_output.usb-cm6206.analog-stereo").unwrap();
        assert_eq!(
            app.current_format(),
            stereo48(),
            "format is the negotiated render format"
        );
    }

    #[test]
    fn render_sink_lifecycle_is_idempotent() {
        let mut sink = FakeRenderSink::new(stereo48());
        sink.start().unwrap();
        assert!(sink.is_running());
        assert_eq!(sink.start(), Err(Error::AlreadyStarted));
        sink.stop();
        sink.stop(); // idempotent
        assert!(!sink.is_running());
        assert_eq!(sink.format(), stereo48());
    }

    #[test]
    fn route_change_matches_all_variants() {
        let cases = [
            RouteChange::SinkConnected("alsa_output.usb-0d8c_1234".into()),
            RouteChange::SinkDisconnected("alsa_output.usb-0d8c_1234".into()),
            RouteChange::A2DPProfileReady,
            RouteChange::RFCOMMPeerConnected {
                name: "WDR-Peer-7C2E".into(),
            },
            RouteChange::FormatChanged(stereo48()),
        ];
        match &cases[0] {
            RouteChange::SinkConnected(n) => assert_eq!(n, "alsa_output.usb-0d8c_1234"),
            _ => unreachable!(),
        }
        match &cases[1] {
            RouteChange::SinkDisconnected(n) => assert_eq!(n, "alsa_output.usb-0d8c_1234"),
            _ => unreachable!(),
        }
        match &cases[2] {
            RouteChange::A2DPProfileReady => {}
            _ => unreachable!(),
        }
        match &cases[3] {
            RouteChange::RFCOMMPeerConnected { name } => assert_eq!(name, "WDR-Peer-7C2E"),
            _ => unreachable!(),
        }
        match &cases[4] {
            RouteChange::FormatChanged(m) => assert_eq!(*m, stereo48()),
            _ => unreachable!(),
        }
    }

    #[test]
    fn a2dp_matrix_says_only_linux_is_a_full_standard_sink() {
        let rows = BluetoothMesh::rows();
        assert_eq!(rows.len(), 9);

        // The only full public standard-sink path is Linux (ADR-009).
        assert_eq!(
            BluetoothMesh::verdict(PlatformCell::LinuxStandardSink),
            CellVerdict::Supported
        );

        // Every other stock-OS standard sink is UnsupportedByPublicApi with the
        // one-action fallback.
        for cell in [
            PlatformCell::AndroidStandardSink,
            PlatformCell::IosStandardSink,
            PlatformCell::WindowsStandardSink,
            PlatformCell::MacOsStandardSink,
        ] {
            assert_eq!(
                BluetoothMesh::verdict(cell),
                CellVerdict::UnsupportedByPublicApi,
                "{cell:?} must be honest-unsupported"
            );
            assert_eq!(
                BluetoothMesh::fallback(cell),
                FallbackAction::FreeLossyWifi,
                "{cell:?} one-action fallback is free lossy Wi-Fi"
            );
        }

        // Product-peer classic sockets are supported-lossy on And/Win-RFCOMM/Linux.
        for cell in [
            PlatformCell::AndroidRfcommPeer,
            PlatformCell::WindowsRfcommPeer,
            PlatformCell::LinuxRfcommPeer,
        ] {
            assert_eq!(
                BluetoothMesh::verdict(cell),
                CellVerdict::SupportedLossy,
                "{cell:?} is the low-bitrate lossy fallback cell"
            );
        }

        // iOS MFi-scoped peer is not part of the general product.
        assert_eq!(
            BluetoothMesh::verdict(PlatformCell::IosRfcommPeer),
            CellVerdict::UnsupportedByPublicApi
        );
    }

    #[test]
    fn matrix_never_offers_a_headset_as_the_fallback() {
        // FR-034: the product-receiver definition is "device running THIS
        // product receives AND renders to its own output/DAC"; routing to an
        // ordinary BT headset does NOT count. The fallback must be one action
        // to free lossy Wi-Fi and nothing else.
        for row in BluetoothMesh::rows() {
            match row.fallback {
                FallbackAction::None => {
                    assert_ne!(
                        row.verdict,
                        CellVerdict::UnsupportedByPublicApi,
                        "{}",
                        row.cell.label()
                    );
                }
                FallbackAction::FreeLossyWifi => {
                    assert_eq!(
                        row.verdict,
                        CellVerdict::UnsupportedByPublicApi,
                        "{} must be unsupported to offer a fallback",
                        row.cell.label()
                    );
                    assert!(
                        !row.note.to_ascii_lowercase().contains("headset"),
                        "fallback note must never suggest routing to a headset: {}",
                        row.note
                    );
                }
            }
        }
    }

    #[test]
    fn receiver_roles_are_lossy_only() {
        for role in [
            ReceiverRole::StandardA2DPSink,
            ReceiverRole::RfcommProductPeer,
        ] {
            assert_eq!(role.fidelity(), BtFidelity::LossyOnly);
            assert!(!role.label().is_empty());
        }
    }
}
