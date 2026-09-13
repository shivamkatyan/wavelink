//! Fake adapters mirroring the ARCHITECTURE adapter names, plus `FakeClock`,
//! `FakeStorage` and `FakeEntitlement`. All are deterministic and injectable.
//!
//! The traits here are **local** (this crate owns them so the fakes can be
//! adapted later without depending on a product crate); the codec/jitter/
//! session workers will bind their own trait to these concrete fakes in a
//! future integration pass.

use std::collections::HashMap;

use crate::hash::HashSinkState;
use crate::source::{PcmSource, SourceFormat};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RenderUnderrun {
    /// Samples that were still buffered when the device starved.
    pub buffered_before: u64,
    /// How long (ms) the caller waited before supplying the late chunk.
    pub waited_ms: u64,
    pub at_ms: u64,
}

// ---------------------------------------------------------------------------
// Clock
// ---------------------------------------------------------------------------

/// An injectable, settable monotonic millisecond clock (deterministic tests).
///
/// The clock is cheaply clonable and **shares** its current value (an
/// `Rc<Cell<u64>>` internally), so a test can advance one handle while a sink
/// holding another handle observes the jump — this models `sleep/wake` and
/// lets an underrun be driven deterministically.
#[derive(Debug, Clone)]
pub struct FakeClock {
    now_ms: std::rc::Rc<std::cell::Cell<u64>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FakeClockError;

impl FakeClock {
    pub fn new(now_ms: u64) -> Self {
        Self {
            now_ms: std::rc::Rc::new(std::cell::Cell::new(now_ms)),
        }
    }
    pub fn at(now_ms: u64) -> Self {
        Self::new(now_ms)
    }
    /// Current time in ms.
    pub fn now_ms(&self) -> u64 {
        self.now_ms.get()
    }
    /// Set the clock to exactly `ms` (used to model a jump / sleep/wake).
    pub fn set_now_ms(&self, ms: u64) {
        self.now_ms.set(ms);
    }
    /// Advance by `ms` (deterministic).
    pub fn advance_ms(&self, ms: u64) {
        self.now_ms.set(self.now_ms.get().saturating_add(ms));
    }
}

/// The local `Clock` adapter seam (ARCHITECTURE names).
pub trait Clock {
    fn now_ms(&self) -> u64;
}

impl Clock for FakeClock {
    fn now_ms(&self) -> u64 {
        self.now_ms()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdapterError {
    /// Injected failure (open/capture/browse/permission denial).
    Failed,
    /// Cancellation was injected/triggered.
    Cancelled,
}

impl core::fmt::Display for AdapterError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            AdapterError::Failed => f.write_str("fake adapter failed"),
            AdapterError::Cancelled => f.write_str("fake adapter cancelled"),
        }
    }
}
impl std::error::Error for AdapterError {}

impl From<AdapterError> for () {
    fn from(_: AdapterError) {}
}
impl From<()> for AdapterError {
    fn from(_: ()) -> Self {
        AdapterError::Failed
    }
}

// ---------------------------------------------------------------------------
// Capture source
// ---------------------------------------------------------------------------

/// Config for [`FakeCaptureSource`].
#[derive(Debug, Clone, Copy)]
pub struct FakeCaptureSourceConfig {
    pub format: SourceFormat,
    /// Emit stem (fixture tag) — see [`crate::source::FixtureKind`] tags.
    pub stem: CaptureStem,
    /// Fail the next open/capture when true (failure injection).
    pub fail_on_capture: bool,
    /// Cancel the stream at the first call when true (cancellation sim).
    pub cancel_on_next: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CaptureStem {
    Silence,
    Impulse { period: u32 },
    SineSweep { f0: f64, f1: f64 },
    FullScaleEdge,
    PseudoRandomPcm,
    ChannelIdLeft,
    ChannelIdRight,
}

impl CaptureStem {
    pub fn sample_total(&self, format: SourceFormat) -> u64 {
        // Unlimited by default; callers may bound via config.
        let _ = format;
        0
    }
}

/// Fake `CaptureSource` — produces a deterministic stem, simulates format
/// metadata + cancellation, and records the rendered format.
#[derive(Debug, Clone)]
pub struct FakeCaptureSource {
    cfg: FakeCaptureSourceConfig,
    hash: HashSinkState,
    samples_produced: u64,
    opened: bool,
    // stems store normalized fixtures; recreated on open.
    fixture_len: u64,
}

impl FakeCaptureSource {
    pub fn new(cfg: FakeCaptureSourceConfig) -> Self {
        Self {
            cfg,
            hash: HashSinkState::default(),
            samples_produced: 0,
            opened: false,
            fixture_len: cfg.stem.sample_total(cfg.format),
        }
    }

    pub fn config(&self) -> &FakeCaptureSourceConfig {
        &self.cfg
    }

    /// Whether the pump's `open` step has run.
    pub fn is_opened(&self) -> bool {
        self.opened
    }

    fn make_fixture(&self) -> crate::source::Fixture {
        let kind = match self.cfg.stem {
            CaptureStem::Silence => crate::source::FixtureKind::Silence,
            CaptureStem::Impulse { period } => crate::source::FixtureKind::ImpulseTrain { period },
            CaptureStem::SineSweep { f0, f1 } => crate::source::FixtureKind::SineSweep { f0, f1 },
            CaptureStem::FullScaleEdge => crate::source::FixtureKind::FullScaleEdge,
            CaptureStem::PseudoRandomPcm => crate::source::FixtureKind::PseudoRandomPcm,
            CaptureStem::ChannelIdLeft => crate::source::FixtureKind::ChannelId {
                lane: crate::source::Stereo::LeftPattern,
            },
            CaptureStem::ChannelIdRight => crate::source::FixtureKind::ChannelId {
                lane: crate::source::Stereo::RightPattern,
            },
        };
        crate::source::Fixture::new(
            kind,
            self.cfg.format.format,
            self.cfg.format.rate_hz,
            self.cfg.format.channels,
            self.fixture_len,
        )
    }
}

/// The local `CaptureSource` seam (ARCHITECTURE names). The fake implements
/// the client surface the adapter workers need: open, produce a chunk,
/// metadata, and canal-log of what was emitted.
pub trait CaptureSource {
    fn open(&mut self) -> Result<(), AdapterError>;
    fn format(&self) -> SourceFormat;
    /// Produce `samples` samples; returns (slice bytes, sample_count) or an
    /// error when cancellation/injection is active.
    fn capture_chunk(&mut self, samples: u32) -> Result<(Vec<u8>, usize), AdapterError>;
    fn cancel(&mut self);
    fn produced(&self) -> u64;
}

impl CaptureSource for FakeCaptureSource {
    fn open(&mut self) -> Result<(), AdapterError> {
        if self.cfg.fail_on_capture {
            return Err(AdapterError::Failed);
        }
        self.opened = true;
        if self.cfg.cancel_on_next {
            self.cancel();
        }
        Ok(())
    }
    fn format(&self) -> SourceFormat {
        self.cfg.format
    }
    fn capture_chunk(&mut self, samples: u32) -> Result<(Vec<u8>, usize), AdapterError> {
        if self.cfg.cancel_on_next {
            return Err(AdapterError::Cancelled);
        }
        let mut fx = self.make_fixture();
        let chunk = fx.next_chunk(samples);
        let bytes = chunk.bytes.to_vec();
        let count = chunk.len;
        self.samples_produced = self.samples_produced.saturating_add(count as u64);
        self.hash.update_bytes(&bytes);
        Ok((bytes, count))
    }
    fn cancel(&mut self) {
        self.opened = false;
        self.cfg.cancel_on_next = true;
    }
    fn produced(&self) -> u64 {
        self.samples_produced
    }
}

// ---------------------------------------------------------------------------
// Render sink
// ---------------------------------------------------------------------------

/// Stats a [`FakeRenderSink`] records about the stream it rendered.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FakeRenderSinkStats {
    pub samples_rendered: u64,
    pub chunks: u64,
    pub underruns: u64,
    pub last_underrun: Option<RenderUnderrun>,
    pub last_rendered_at_ms: Option<u64>,
}

/// Fake `RenderSink` that records a running blake3 hash of everything it was
/// given, and detects **underrun** whenever, at the moment the next chunk is
/// offered, the accumulated supplied samples can no longer keep the buffer
/// ahead of the render position implied by the injected clock.
#[derive(Debug, Clone)]
pub struct FakeRenderSink {
    clock: FakeClock,
    format: SourceFormat,
    hash: HashSinkState,
    stats: FakeRenderSinkStats,
    /// Simulated device buffer capacity in samples.
    capacity_samples: u64,
    /// Simulated device buffer occupancy (samples waiting to be played).
    buffered: u64,
    /// Previous render timestamp (for drain accounting).
    last_chunk_supplied: Option<u64>,
}

impl FakeRenderSink {
    pub fn new(clock: FakeClock, format: SourceFormat) -> Self {
        Self::with_capacity(clock, format, 2048)
    }

    /// Construct with an explicit simulated device buffer capacity.
    pub fn with_capacity(clock: FakeClock, format: SourceFormat, capacity_samples: u64) -> Self {
        Self {
            clock,
            format,
            hash: HashSinkState::default(),
            stats: FakeRenderSinkStats::default(),
            capacity_samples,
            buffered: 0,
            last_chunk_supplied: None,
        }
    }

    pub fn format(&self) -> SourceFormat {
        self.format
    }
    pub fn stats(&self) -> &FakeRenderSinkStats {
        &self.stats
    }
    pub fn hash_state(&self) -> &HashSinkState {
        &self.hash
    }

    /// Supplied bytes for a chunk of `count` samples (canonical). Returns
    /// `Ok(())` normally, `Err(underrun)` when the device would have starved
    /// given the injected clock.
    ///
    /// Model: the device drains `rate_hz * elapsed_ms / 1000` samples between
    /// supply callbacks. If the drain would empty the buffer *before* this
    /// chunk arrives (`previous_buffered < drained`), that's an underrun; the
    /// buffer resets and the chunk refills from zero.
    pub fn render_chunk(&mut self, bytes: &[u8], count: u64) -> Result<(), RenderUnderrun> {
        let now = self.clock.now_ms();
        self.stats.chunks += 1;
        self.stats.last_rendered_at_ms = Some(now);

        let mut underrun: Option<RenderUnderrun> = None;
        if let Some(prev) = self.last_chunk_supplied {
            let elapsed_ms = now.saturating_sub(prev);
            let drained = self.samples_per_ms(elapsed_ms);
            if drained >= self.buffered {
                // Buffer would have emptied (or been negative) before this
                // chunk — starvation.
                underrun = Some(RenderUnderrun {
                    buffered_before: self.buffered,
                    waited_ms: elapsed_ms,
                    at_ms: now,
                });
                self.buffered = 0;
            } else {
                self.buffered -= drained;
            }
        }
        self.last_chunk_supplied = Some(now);

        self.hash.update_bytes(bytes);
        self.buffered = self.buffered.saturating_add(count);
        self.stats.samples_rendered += count;

        if let Some(u) = underrun {
            self.stats.underruns += 1;
            self.stats.last_underrun = Some(u);
            return Err(u);
        }
        Ok(())
    }

    fn samples_per_ms(&self, ms: u64) -> u64 {
        let rate = self.format.rate_hz as u64;
        (rate.saturating_mul(ms)) / 1000
    }
}

impl FakeRenderSink {
    /// Capacity (in samples) of the simulated device buffer.
    pub fn capacity(&self) -> u64 {
        self.capacity_samples
    }
}

// ---------------------------------------------------------------------------
// Discovery
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FakeDiscoveryPeer {
    pub id: String,
    pub name: String,
}

/// Fake `Discovery`: returns a fixed peer list, optionally with a failure
/// injection (a poisoned browse).
#[derive(Debug, Clone)]
pub struct FakeDiscovery {
    peers: Vec<FakeDiscoveryPeer>,
    fail: bool,
    at_ms: u64,
}

impl FakeDiscovery {
    pub fn new(peers: Vec<FakeDiscoveryPeer>) -> Self {
        Self {
            peers,
            fail: false,
            at_ms: 0,
        }
    }
    pub fn with_failure(mut self, fail: bool) -> Self {
        self.fail = fail;
        self
    }
    pub fn set_failure(&mut self, fail: bool) {
        self.fail = fail;
    }
}

/// The local `Discovery` seam.
pub trait Discovery {
    fn browse(&mut self) -> Result<Vec<FakeDiscoveryPeer>, AdapterError>;
}

impl Discovery for FakeDiscovery {
    fn browse(&mut self) -> Result<Vec<FakeDiscoveryPeer>, AdapterError> {
        if self.fail {
            self.at_ms = 1;
            return Err(AdapterError::Failed);
        }
        Ok(self.peers.clone())
    }
}

// ---------------------------------------------------------------------------
// Pairing UI
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FakePairingUiOutcome {
    Confirmed,
    Rejected,
}

/// Fake `PairingUi`: auto-confirm or auto-reject SAS, with failure injection
/// (a policy-based reject on demand).
#[derive(Debug, Clone)]
pub struct FakePairingUi {
    mode: FakePairingUiOutcome,
    reject_sas: bool,
}

impl FakePairingUi {
    pub fn new(mode: FakePairingUiOutcome) -> Self {
        Self {
            mode,
            reject_sas: false,
        }
    }
    pub fn auto_confirm() -> Self {
        Self::new(FakePairingUiOutcome::Confirmed)
    }
    pub fn auto_reject() -> Self {
        Self::new(FakePairingUiOutcome::Rejected)
    }
    pub fn with_reject_injection(mut self, reject_sas: bool) -> Self {
        self.reject_sas = reject_sas;
        self
    }
}

/// The local `PairingUi` seam.
pub trait PairingUi {
    /// Ask the human to confirm `sas`; auto modes return without a human.
    fn confirm_sas(&self, sas: &str) -> Result<FakePairingUiOutcome, AdapterError>;
}

impl PairingUi for FakePairingUi {
    fn confirm_sas(&self, _sas: &str) -> Result<FakePairingUiOutcome, AdapterError> {
        if self.reject_sas {
            return Ok(FakePairingUiOutcome::Rejected);
        }
        Ok(self.mode)
    }
}

// ---------------------------------------------------------------------------
// Permission gate
// ---------------------------------------------------------------------------

/// Fake `PermissionGate`: allow/deny injected, plus a deny-after-allow switch.
#[derive(Debug, Clone)]
pub struct FakePermissionGate {
    allow: bool,
    deny_after_allow: bool,
    granted: bool,
}

impl FakePermissionGate {
    pub fn new(allow: bool) -> Self {
        Self {
            allow,
            deny_after_allow: false,
            granted: false,
        }
    }
    pub fn allow() -> Self {
        Self::new(true)
    }
    pub fn deny() -> Self {
        Self::new(false)
    }
    pub fn set_allow(&mut self, allow: bool) {
        self.allow = allow;
    }
    pub fn set_deny_after_allow(&mut self, on: bool) {
        self.deny_after_allow = on;
    }
}

/// The local `PermissionGate` seam.
pub trait PermissionGate {
    fn request(&mut self) -> Result<bool, AdapterError>;
    fn is_granted(&self) -> bool;
}

impl PermissionGate for FakePermissionGate {
    fn request(&mut self) -> Result<bool, AdapterError> {
        if self.allow {
            self.granted = true;
            if self.deny_after_allow {
                self.allow = false;
            }
            Ok(true)
        } else {
            Ok(false)
        }
    }
    fn is_granted(&self) -> bool {
        self.granted
    }
}

// ---------------------------------------------------------------------------
// Storage / entitlement
// ---------------------------------------------------------------------------

/// Fake `Storage`: in-memory secure-kv stand-in with typed get/set.
#[derive(Debug, Clone, Default)]
pub struct FakeStorage {
    map: HashMap<String, Vec<u8>>,
}

impl FakeStorage {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn get(&self, key: &str) -> Option<&[u8]> {
        self.map.get(key).map(|v| v.as_slice())
    }
    pub fn set(&mut self, key: &str, value: Vec<u8>) {
        self.map.insert(key.to_string(), value);
    }
    pub fn remove(&mut self, key: &str) {
        self.map.remove(key);
    }
    pub fn contains(&self, key: &str) -> bool {
        self.map.contains_key(key)
    }
}

/// Fake `EntitlementProvider` (the `wdr_entitlement` crate's trait is the real
/// seam; this is an independent in-memory stand-in with tier injection).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FakeTier {
    Free,
    Pro,
}

#[derive(Debug, Clone)]
pub struct FakeEntitlement {
    pub tier: FakeTier,
    pub lossless_allowed: bool,
}

impl FakeEntitlement {
    pub fn new(tier: FakeTier) -> Self {
        Self {
            tier,
            lossless_allowed: matches!(tier, FakeTier::Pro),
        }
    }
    pub fn free() -> Self {
        Self::new(FakeTier::Free)
    }
    pub fn pro() -> Self {
        Self::new(FakeTier::Pro)
    }
    pub fn set_tier(&mut self, tier: FakeTier) {
        self.tier = tier;
        self.lossless_allowed = matches!(tier, FakeTier::Pro);
    }
}

/// Local `EntitlementProvider` stand-in surface.
pub trait FakeEntitlementProvider {
    fn tier(&self) -> FakeTier;
    fn lossless_allowed(&self) -> bool;
}

impl FakeEntitlementProvider for FakeEntitlement {
    fn tier(&self) -> FakeTier {
        self.tier
    }
    fn lossless_allowed(&self) -> bool {
        self.lossless_allowed
    }
}
