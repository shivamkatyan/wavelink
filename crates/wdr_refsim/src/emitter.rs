//! In-process emitter for the reference-sim loopback e2e tests (t-B1-receiver).
//!
//! The *production* emitter binary (`ref_emitter`) is owned by a sibling
//! worker. The tests here do not depend on it: they spawn this deterministic,
//! in-process emitter over `wdr_transport`/quinn loopback so the receiver
//! pipeline is exercised end-to-end without any external binary.
//!
//! The emitter:
//! * builds `wdr_proto::Frame`s from a `wdr_fakes` [`PcmSource`] fixture,
//! * encodes each chunk with a `wdr_codec` adapter (FLAC/PCM lossless on a
//!   reliable stream, Opus lossy on datagrams),
//! * imposes **deterministic** send-side impairment (seeded PRNG loss drop,
//!   duplicate, reorder-injection) — the loopback analogue of
//!   `WDR_IMPAIRED`-style sender loss,
//! * finishes with an explicit end-of-stream marker so the receiver terminates
//!   with bounded state (no sleep-and-assume).
//!
//! Everything is error-typed; nothing here panics on data.

use rand::{rngs::StdRng, RngExt, SeedableRng as _};
use wdr_codec::{CodecAdapter, CodecAdapter24, CodecError, FlacAdapter, OpusAdapter, PcmAdapter};
use wdr_fakes::source::{ChannelKind, Fixture, FixtureKind, PcmSource, SampleFormat};
use wdr_proto::{ChannelLayout, Codec, Frame, FrameFlags, SampleRepr};

use crate::framing::FrameWire;

/// The adapter an emitter drives, keyed to its sample representation. i16
/// streams drive the [`CodecAdapter`] surface; 24-bit streams drive
/// [`CodecAdapter24`] (the optional Adapter is built only for I24Packed).
/// Keeping the choice explicit means a sample-repr mismatch is a typed error,
/// never a silent 24→16 down-convert (FR-026).
enum EmitterAdapter {
    I16(Box<dyn CodecAdapter>),
    /// The 24-bit adapter is constructed (proving I24Packed config builds and
    /// is metadata-correct) but the emitter's i16 frame seam refuses to drive
    /// it today — `encode_i16_frame` returns a typed error instead of a silent
    /// downgrade. The payload is dereferenced by the unit tests and by the
    /// future 24-bit frame path, hence the allow(dead_code) for lib-only builds.
    #[allow(dead_code)]
    I24(Box<dyn CodecAdapter24>),
}

/// Which lane the emitter drives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamKind {
    /// Lossy Opus over QUIC datagrams (one datagram = one packed frame).
    OpusDatagram,
    /// Lossless FLAC over the reliable stream (length-prefixed items).
    FlacStream,
    /// Lossless raw PCM over the reliable stream.
    PcmStream,
}

/// Deterministic send-side impairment (loopback analogue of `netem` /
/// `WDR_IMPAIRED`-style loss). Seeded so runs are reproducible.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Impairment {
    pub loss_pct: u32,
    pub duplicate_pct: u32,
    pub reorder_pct: u32,
}

impl Default for Impairment {
    fn default() -> Self {
        Self::clean()
    }
}

impl Impairment {
    pub const fn clean() -> Self {
        Self {
            loss_pct: 0,
            duplicate_pct: 0,
            reorder_pct: 0,
        }
    }
}

/// Static frame metadata an emitter + receiver agree on for one run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BufferMeta {
    pub codec: Codec,
    pub sample_rate: u32,
    pub channels: u16,
    pub sample_repr: SampleRepr,
    pub channel_layout: ChannelLayout,
    /// Codec frame size in samples per channel (e.g. 512 for the FLAC/PCM
    /// canonical goldens; 960 for Opus 20 ms @48k).
    pub frame_samples: usize,
}

impl BufferMeta {
    /// The canonical lossless metadata: i16 / 48k / stereo / 512-sample chunks
    /// — exactly the conditions under which the t-B0 golden hashes are recorded.
    pub const fn canonical_lossless(codec: Codec) -> Self {
        Self {
            codec,
            sample_rate: 48_000,
            channels: 2,
            sample_repr: SampleRepr::I16,
            channel_layout: ChannelLayout::Stereo,
            frame_samples: 512,
        }
    }

    /// The canonical lossy metadata: i16 / 48k / stereo / Opus 20 ms (960).
    pub const fn canonical_lossy() -> Self {
        Self {
            codec: Codec::Opus,
            sample_rate: 48_000,
            channels: 2,
            sample_repr: SampleRepr::I16,
            channel_layout: ChannelLayout::Stereo,
            frame_samples: 960,
        }
    }

    pub const fn stream_id(&self) -> u32 {
        7
    }
}

/// Emitter configuration for an e2e run.
#[derive(Debug, Clone)]
pub struct EmitterConfig {
    /// The lane (also selects the codec). Redundant with `meta.codec` but kept
    /// explicit so the send path is a plain match.
    pub kind: StreamKind,
    /// Total source samples per channel to emit.
    pub total_samples: u64,
    /// Source fixture driving the PCM.
    pub fixture: FixtureKind,
    /// Send-side impairment percentages.
    pub impairment: Impairment,
    /// Seed for the deterministic impairment PRNG.
    pub seed: u64,
    /// Optional steady-state pacing: sleep `pace` between frames before
    /// emitting. `None` = burst (the loopback default, all frames back-to-back).
    ///
    /// Set by the t-P1-cc spike (`WDR_EMIT_PACE_MS=20` → a real 20 ms Opus
    /// cadence) so the datagram lane exercises the congestion controller under
    /// steady-state load rather than a single burst-then-close — a burst that
    /// exits before QUIC drains under 30 ms jitter measures "drop on close",
    /// not CC behaviour.
    pub pace: Option<std::time::Duration>,
    /// Pace by the frame's own audio duration (frame_samples/sample_rate) so a
    /// run reaches wall-clock real-time regardless of codec frame size. Used
    /// by the soak; ignored if `pace` is set.
    pub pace_real_time: bool,
}

impl EmitterConfig {
    /// A clean, canonical lossless run over the recorded fakes fixture
    /// (chunk 512 / total 4096 / i16 / 48k stereo — the t-B0 golden).
    pub fn lossless_canonical(kind: StreamKind) -> Self {
        Self {
            kind,
            total_samples: 4096,
            fixture: FixtureKind::PseudoRandomPcm,
            impairment: Impairment::clean(),
            seed: 0xB1E1_0000,
            pace: None,
            pace_real_time: false,
        }
    }
}

/// A running emitter over an open quinn connection. Owns the connection handle
/// (quinn `Connection` is internally reference-counted, so this is cheap) —
/// that lets the live [`crate::sink::AudioFrameSink`] own an `Emitter` with no
/// self-referential borrow.
pub struct Emitter {
    conn: quinn::Connection,
    kind: StreamKind,
    meta: BufferMeta,
    cfg: EmitterConfig,
    adapter: EmitterAdapter,
    fixture: Fixture,
    seq: u64,
    rng: StdRng,
    held: Vec<Vec<u8>>,
    packets_sent: u64,
    bytes_sent: u64,
}

/// Select the codec adapter for a `BufferMeta`, keyed to its sample
/// representation. i16 drives the [`CodecAdapter`] surface; I24Packed drives
/// the 24-bit [`CodecAdapter24`] surface (ADR-005 follow-up). F32/I32 remain
/// declared-but-unimplemented stubs, and Opus is i16-only — both are typed
/// errors, never a silent re-formatter.
fn build_adapter(meta: &BufferMeta) -> Result<EmitterAdapter, EmitterError> {
    match meta.sample_repr {
        SampleRepr::I16 => {
            let boxed: Box<dyn CodecAdapter> = match meta.codec {
                Codec::Flac => Box::new(FlacAdapter::new(meta.sample_rate, meta.channels, 16)?),
                Codec::Pcm => Box::new(PcmAdapter::new(meta.channels)?),
                Codec::Opus => Box::new(OpusAdapter::new(
                    meta.sample_rate,
                    meta.channels,
                    wdr_codec::FrameProfile::Ms20,
                )?),
            };
            Ok(EmitterAdapter::I16(boxed))
        }
        SampleRepr::I24Packed => {
            let boxed: Box<dyn CodecAdapter24> = match meta.codec {
                Codec::Flac => Box::new(FlacAdapter::new(meta.sample_rate, meta.channels, 24)?),
                Codec::Pcm => Box::new(PcmAdapter::new24(meta.channels)?),
                Codec::Opus => {
                    return Err(EmitterError::Format(
                        "Opus is i16-only (ADR-004); 24-bit sample repr not supported for Opus"
                            .into(),
                    ));
                }
            };
            Ok(EmitterAdapter::I24(boxed))
        }
        // F32/I32 remain declared-but-unimplemented stubs (ADR-005).
        other => Err(EmitterError::Format(format!(
            "sample representation {other:?} is not built by the emitter (ADR-005: i16/i24)"
        ))),
    }
}

/// Encode one whole PCM i16 frame with the emitter's adapter. A 24-bit
/// adapter driven through the i16 frame seam is a typed error — the 24-bit
/// codec surface is proven (`CodecAdapter24`), but the emitter's frame seam
/// carries i16 frames today, so anything else would be a silent downgrade
/// (FR-026).
fn encode_i16_frame(adapter: &mut EmitterAdapter, pcm: &[i16]) -> Result<Box<[u8]>, EmitterError> {
    match adapter {
        EmitterAdapter::I16(a) => Ok(a.encode(pcm)?),
        EmitterAdapter::I24(_) => Err(EmitterError::Format(
            "24-bit emitter frame path is not wired through the i16 frame seam yet \
             (ADR-005 follow-up; codec-level 24-bit lossless is proven in wdr_codec)"
                .into(),
        )),
    }
}

impl Emitter {
    /// Build an emitter over the emitter-side connection. Constructing the
    /// codec adapter fails only for unsupported configs (typed `EmitterError`).
    pub fn new(
        conn: quinn::Connection,
        cfg: EmitterConfig,
        meta: BufferMeta,
    ) -> Result<Self, EmitterError> {
        let adapter = build_adapter(&meta)?;
        let format = match meta.sample_repr {
            SampleRepr::I16 => SampleFormat::I16,
            SampleRepr::I24Packed => SampleFormat::I24,
            // Unreachable: build_adapter already errored for F32/I32. Kept
            // exhaustive for the match.
            _ => return Err(EmitterError::Format("unsupported sample repr".into())),
        };
        let channels = match meta.channel_layout {
            ChannelLayout::Stereo => ChannelKind::Stereo,
            _ => ChannelKind::Stereo,
        };
        // The fixture's `total_samples` budget is counted in *values*
        // (channel-multiplied), whereas `EmitterConfig.total_samples` is a
        // per-channel budget; scale so a `--seconds N` run covers N wall-clock
        // seconds of stereo audio (512-frame FLAC at 10 s ⇒ 480k per-channel
        // ⇒ 960k values), not half the intended duration.
        let fixture_values = cfg.total_samples.saturating_mul(u64::from(meta.channels));
        let fixture = Fixture::new(
            cfg.fixture.clone(),
            format,
            meta.sample_rate,
            channels,
            fixture_values,
        );
        let rng = StdRng::seed_from_u64(cfg.seed);
        Ok(Self {
            conn,
            kind: cfg.kind,
            meta,
            cfg,
            adapter,
            fixture,
            seq: 0,
            rng,
            held: Vec::new(),
            packets_sent: 0,
            bytes_sent: 0,
        })
    }

    /// Build a *live* emitter (no fixture, no impairment) over an
    /// already-dialed connection — the transport-seam entry for real capture.
    /// `total_samples = 0` makes `run()` a no-op, so the only way to push audio
    /// through this emitter is [`Emitter::emit_pcm_frame`].
    pub fn new_live(
        conn: quinn::Connection,
        kind: StreamKind,
        meta: BufferMeta,
    ) -> Result<Self, EmitterError> {
        let cfg = EmitterConfig {
            kind,
            total_samples: 0,
            fixture: FixtureKind::Silence,
            impairment: Impairment::clean(),
            seed: 0,
            pace: None,
            pace_real_time: false,
        };
        Self::new(conn, cfg, meta)
    }

    /// A clone of the underlying connection handle (quinn `Connection` is
    /// internally reference-counted) so a live sink can re-skin the emitter
    /// with a new wire meta without a new handshake (the rate-aware
    /// `on_format` rebuild).
    pub fn conn_handle(&self) -> quinn::Connection {
        self.conn.clone()
    }

    /// Number of audio frames (packets) actually sent (incl. duplicates).
    pub fn packets_sent(&self) -> u64 {
        self.packets_sent
    }

    /// Total payload bytes sent (audio + end marker), not counting the
    /// reliable-stream length prefix.
    pub fn bytes_sent(&self) -> u64 {
        self.bytes_sent
    }

    /// The lane this emitter drives.
    pub fn kind(&self) -> StreamKind {
        self.kind
    }

    fn should_drop(&mut self) -> bool {
        self.cfg.impairment.loss_pct > 0
            && self.rng.random_range(0u32..100) < self.cfg.impairment.loss_pct
    }

    fn should_duplicate(&mut self) -> bool {
        self.cfg.impairment.duplicate_pct > 0
            && self.rng.random_range(0u32..100) < self.cfg.impairment.duplicate_pct
    }

    fn should_reorder(&mut self) -> bool {
        self.cfg.impairment.reorder_pct > 0
            && self.rng.random_range(0u32..100) < self.cfg.impairment.reorder_pct
    }

    /// Emit the whole fixture; returns the blake3 hash of the *source bytes*
    /// the fixture produced (the reference the receiver's decoded hash must
    /// equal on the lossless path).
    pub async fn run(&mut self) -> Result<blake3::Hash, EmitterError> {
        let mut source_hash = blake3::Hasher::new();
        // `PcmSource::next_chunk(samples)` produces `samples` *values*; for a
        // stereo/surround layout that is channels × per-channel frames, so the
        // requested value count for one `frame_samples`-per-channel frame is
        // `frame_samples * channels`.
        let values_per_frame =
            (self.meta.frame_samples as u32).saturating_mul(u32::from(self.meta.channels));
        loop {
            // Honor `--seconds N` (EmitterConfig.total_samples is per-channel):
            // stop once the configured per-channel sample budget has been
            // carried, so a long run (e.g. 3600 s soak) is driven by the limit
            // and not by the bounded fixture length.
            let carried = self.seq * self.meta.frame_samples as u64;
            if carried >= self.cfg.total_samples {
                break;
            }
            let got = self.fixture.next_chunk(values_per_frame);
            if got.len == 0 {
                break;
            }
            // Only whole frames can be encoded (the Opus adapter enforces an
            // exact frame length); a partial tail is a truncated source and is
            // dropped rather than emitted as a malformed short frame.
            if got.len < values_per_frame as usize {
                break;
            }
            source_hash.update(got.bytes);
            let pcm: Vec<i16> = got
                .bytes
                .as_chunks::<2>()
                .0
                .iter()
                .map(|b| i16::from_le_bytes(*b))
                .collect();
            if !self.should_drop() {
                // Shared encode/wire path: `frame_payload` uses (but does not
                // advance) `self.seq`, so a duplicated frame keeps the same seq.
                let packed = self.frame_payload(&pcm)?;
                self.emit_one(&packed).await?;
                if self.should_duplicate() {
                    let dup = packed.clone();
                    self.emit_one(&dup).await?;
                }
            }
            self.seq += 1;
            // Optional steady-state pacing. Default: pace by the frame's own
            // audio duration (frame_samples/sample_rate) so ANY codec runs at
            // true real-time cadence (Opus 20ms, FLAC 512@48k = ~10.7ms).
            // `WDR_EMIT_PACE_MS` overrides to a fixed per-frame sleep.
            if let Some(step) = self.cfg.pace {
                tokio::time::sleep(step).await;
            } else if self.cfg.pace_real_time {
                let period = std::time::Duration::from_secs_f64(
                    self.meta.frame_samples as f64 / self.meta.sample_rate as f64,
                );
                tokio::time::sleep(period).await;
            }
        }
        self.emit_end().await?;
        Ok(source_hash.finalize())
    }

    /// Encode one whole PCM frame and pack it (header + payload). Uses but does
    /// **not** advance `self.seq` — the caller decides whether to re-send the
    /// same packed bytes (duplication) or advance a new frame. Shared by
    /// `run()` and the live [`AudioFrameSink`](crate::sink::AudioFrameSink).
    fn frame_payload(&mut self, pcm: &[i16]) -> Result<Vec<u8>, EmitterError> {
        let payload = encode_i16_frame(&mut self.adapter, pcm)?;
        let frame = self.make_frame(payload.to_vec());
        frame
            .pack()
            .map_err(|e| EmitterError::Send(format!("frame pack: {e:?}")))
    }

    /// Encode + transport one whole PCM frame on this emitter's lane (datagram
    /// for Opus, reliable stream for FLAC/PCM). **No impairment and no
    /// pacing** — real capture cadence is the caller's (the live sink runs off
    /// the RT callback on a worker thread). Whole-frame invariant: `pcm` must
    /// hold exactly `frame_samples × channels` values, which is what the
    /// [`crate::sink::QuicAudioSink`] accumulation guarantees.
    ///
    /// Shares the exact encode/wire path with `run()`, so the golden loopback
    /// tests prove the same code the shells drive.
    pub async fn emit_pcm_frame(&mut self, pcm: &[i16]) -> Result<(), EmitterError> {
        let packed = self.frame_payload(pcm)?;
        self.emit_one(&packed).await?;
        self.seq += 1;
        Ok(())
    }

    /// Send the end-of-stream marker on this lane (flushes any held
    /// datagrams first). The receiver terminates with bounded state on seeing
    /// it — the live sink's `finish()`.
    pub async fn emit_end_marker(&mut self) -> Result<(), EmitterError> {
        self.emit_end().await
    }

    async fn emit_one(&mut self, packed: &[u8]) -> Result<(), EmitterError> {
        match self.kind {
            StreamKind::OpusDatagram => {
                if self.should_reorder() && self.held.len() < 8 {
                    self.held.push(packed.to_vec());
                    return Ok(());
                }
                self.send_datagram(packed)?;
                self.packets_sent += 1;
                self.bytes_sent += packed.len() as u64;
                self.flush_held().await?;
            }
            StreamKind::FlacStream | StreamKind::PcmStream => {
                let wrapped = FrameWire::wrap_item(packed);
                self.write_stream_all(wrapped).await?;
                self.packets_sent += 1;
                self.bytes_sent += packed.len() as u64;
            }
        }
        Ok(())
    }

    async fn flush_held(&mut self) -> Result<(), EmitterError> {
        let held: Vec<Vec<u8>> = std::mem::take(&mut self.held);
        for b in held {
            self.send_datagram(&b)?;
            self.packets_sent += 1;
            self.bytes_sent += b.len() as u64;
        }
        Ok(())
    }

    fn send_datagram(&self, bytes: &[u8]) -> Result<(), EmitterError> {
        self.conn
            .send_datagram(bytes.to_vec().into())
            .map_err(|e| EmitterError::Send(format!("datagram: {e:?}")))
    }

    async fn emit_end(&mut self) -> Result<(), EmitterError> {
        let end = FrameWire::pack_known_end(self.seq);
        match self.kind {
            StreamKind::OpusDatagram => {
                self.flush_held().await?;
                self.send_datagram(&end)?;
            }
            StreamKind::FlacStream | StreamKind::PcmStream => {
                // The reliable-stream lane is length-prefixed (one item per
                // stream, `[u16 len][body]`, as produced by `wrap_item` for the
                // audio frames). The end marker must carry that prefix too,
                // otherwise the receiver's `read_one_stream_item` misreads the
                // marker magic as the length and drops it as malformed.
                self.write_stream_all(FrameWire::wrap_item(&end)).await?;
            }
        }
        Ok(())
    }

    async fn write_stream_all(&self, bytes: Vec<u8>) -> Result<(), EmitterError> {
        let (mut send, _recv) = self
            .conn
            .open_bi()
            .await
            .map_err(|e| EmitterError::Send(format!("open_bi: {e:?}")))?;
        send.write_all(&bytes)
            .await
            .map_err(|e| EmitterError::Send(format!("stream write: {e:?}")))?;
        send.finish()
            .map_err(|e| EmitterError::Send(format!("stream finish: {e:?}")))?;
        Ok(())
    }

    fn make_frame(&self, payload: Vec<u8>) -> Frame {
        let flags = FrameFlags::default();
        let ts = self.seq * self.meta.frame_samples as u64;
        match self.meta.codec {
            Codec::Flac | Codec::Pcm => Frame::new_lossless(
                self.meta.stream_id(),
                self.seq,
                ts,
                self.meta.codec,
                self.meta.sample_rate,
                self.meta.sample_repr,
                self.meta.channel_layout,
                self.meta.frame_samples as u32,
                flags,
                payload,
            ),
            Codec::Opus => Frame::new_lossy(
                self.meta.stream_id(),
                self.seq,
                ts,
                self.meta.codec,
                self.meta.sample_rate,
                self.meta.sample_repr,
                self.meta.channel_layout,
                self.meta.frame_samples as u32,
                flags,
                payload,
            ),
        }
    }
}

/// Typed errors from the emitter.
#[derive(Debug)]
pub enum EmitterError {
    Encode(CodecError),
    Send(String),
    /// Format/configuration not supported by this emitter (e.g. an F32/I32
    /// sample repr, or a 24-bit path the i16 frame seam cannot drive).
    /// Raised **before** any bytes are sent — never a silent reformat.
    Format(String),
    /// The requested transport lane is not granted by the effective
    /// entitlement policy (e.g. lossless under the Free tier, FR-042/FR-043).
    /// Raised **before** any bytes are sent — never a silent premium.
    Policy(String),
}

impl From<CodecError> for EmitterError {
    fn from(e: CodecError) -> Self {
        EmitterError::Encode(e)
    }
}

impl core::fmt::Display for EmitterError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            EmitterError::Encode(e) => write!(f, "emitter encode: {e}"),
            EmitterError::Send(s) => write!(f, "emitter send: {s}"),
            EmitterError::Format(msg) => write!(f, "emitter format: {msg}"),
            EmitterError::Policy(msg) => write!(f, "emitter policy: {msg}"),
        }
    }
}
impl std::error::Error for EmitterError {}

/// The entitlement policy gate, evaluated **before** any byte is sent on a
/// lane (FR-042/FR-043; ADR-003 lossless rides a reliable stream). Free tier
/// never grants lossless. Shared by `ref_emitter` and the live
/// [`crate::sink::QuicAudioSink`].
pub fn policy_gate(
    lossless: bool,
    tier: wdr_entitlement::provider::Tier,
) -> Result<(), EmitterError> {
    use wdr_entitlement::provider::{DevToggleEntitlementProvider, EntitlementProvider};
    use wdr_entitlement::Feature;
    let provider = DevToggleEntitlementProvider::new(tier);
    if lossless && !provider.feature_enabled(Feature::LosslessWifi) {
        return Err(EmitterError::Policy(format!(
            "lossless (FLAC/PCM) is not granted by the effective tier {tier:?} \
             (Free never sends lossless; use tier Pro for the lossless lane)"
        )));
    }
    Ok(())
}

/// Resolve the entitlement tier from `WDR_ENT_TIER` (default `free`), matching
/// `ref_emitter`'s contract / the compose harness.
pub fn tier_from_env() -> Result<wdr_entitlement::provider::Tier, EmitterError> {
    use wdr_entitlement::provider::Tier;
    match std::env::var("WDR_ENT_TIER")
        .unwrap_or_else(|_| "free".into())
        .to_ascii_lowercase()
        .as_str()
    {
        "free" => Ok(Tier::Free),
        "pro" => Ok(Tier::Pro),
        other => Err(EmitterError::Policy(format!(
            "unknown WDR_ENT_TIER '{other}' (free|pro)"
        ))),
    }
}

/// Canonical sim wire metadata shared by `ref_emitter` and the live sink:
/// i16 / 48 kHz / stereo (ADR-005 i16-only adapters); 512-sample FLAC/PCM
/// frames, 960-sample (20 ms) Opus.
pub const SIM_RATE_HZ: u32 = 48_000;
pub const SIM_CHANNELS: u16 = 2;

/// Map (lossless, codec) to the transport lane + canonical wire metadata.
/// Shared by `ref_emitter` (`build_emitter`) and
/// [`crate::sink::QuicAudioSink::connect`].
pub fn lane_and_meta(lossless: bool, codec: Codec) -> (StreamKind, BufferMeta) {
    let kind = match (lossless, codec) {
        (false, _) => StreamKind::OpusDatagram,
        (true, Codec::Flac) => StreamKind::FlacStream,
        (true, Codec::Pcm) => StreamKind::PcmStream,
        // The policy gate refuses --lossless + Opus before this point.
        (true, Codec::Opus) => StreamKind::OpusDatagram,
    };
    let meta = BufferMeta {
        codec,
        sample_rate: SIM_RATE_HZ,
        channels: SIM_CHANNELS,
        sample_repr: SampleRepr::I16,
        channel_layout: ChannelLayout::Stereo,
        frame_samples: if codec == Codec::Opus { 960 } else { 512 },
    };
    (kind, meta)
}

/// Canonical wire metadata for a negotiated capture rate (rate-aware seam). The
/// receiver sniffs `BufferMeta` from the first frame, so FLAC/PCM may ride any
/// delivered rate (bit-exact); Opus uses 882 (44.1k) or 960 (48k) 20 ms frames.
pub fn wire_meta(codec: Codec, wire_rate: u32, frame_samples: usize) -> BufferMeta {
    BufferMeta {
        codec,
        sample_rate: wire_rate,
        channels: SIM_CHANNELS,
        sample_repr: SampleRepr::I16,
        channel_layout: ChannelLayout::Stereo,
        frame_samples,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn i24_meta(codec: Codec) -> BufferMeta {
        BufferMeta {
            codec,
            sample_rate: SIM_RATE_HZ,
            channels: SIM_CHANNELS,
            sample_repr: SampleRepr::I24Packed,
            channel_layout: ChannelLayout::Stereo,
            frame_samples: 512,
        }
    }

    #[test]
    fn i24_meta_builds_24bit_flac_adapter_and_roundtrips() {
        let mut adapter = build_adapter(&i24_meta(Codec::Flac)).unwrap();
        let EmitterAdapter::I24(a) = &mut adapter else {
            panic!("I24Packed + Flac must build a 24-bit adapter");
        };
        let pcm: Vec<i32> = vec![0x007F_FFFF, -0x007F_FFFF, 0x0012_3456, -0x0012_3456];
        let enc = a.encode_24(&pcm).expect("encode_24");
        let dec = a.decode_24(&enc).expect("decode_24");
        assert_eq!(dec, pcm.into_boxed_slice(), "24-bit lossless roundtrip");
    }

    #[test]
    fn i24_meta_builds_24bit_pcm_adapter_and_roundtrips() {
        let mut adapter = build_adapter(&i24_meta(Codec::Pcm)).unwrap();
        let EmitterAdapter::I24(a) = &mut adapter else {
            panic!("I24Packed + Pcm must build a 24-bit adapter");
        };
        let pcm: Vec<i32> = vec![0, 0x007F_FFFF, -0x007F_FFFF, 1234, -99, 1];
        let enc = a.encode_24(&pcm).expect("encode_24");
        assert_eq!(enc.len(), pcm.len() * 3, "3 bytes/sample");
        let dec = a.decode_24(&enc).expect("decode_24");
        assert_eq!(dec, pcm.into_boxed_slice());
    }

    #[test]
    fn i24_meta_is_a_typed_error_for_opus_and_f32_i32_stubs() {
        assert!(matches!(
            build_adapter(&i24_meta(Codec::Opus)),
            Err(EmitterError::Format(_))
        ));
        let mut f32 = i24_meta(Codec::Pcm);
        f32.sample_repr = SampleRepr::F32;
        assert!(matches!(build_adapter(&f32), Err(EmitterError::Format(_))));
        let mut i32r = i24_meta(Codec::Pcm);
        i32r.sample_repr = SampleRepr::I32;
        assert!(matches!(build_adapter(&i32r), Err(EmitterError::Format(_))));
    }

    #[test]
    fn i16_frame_seam_refuses_24bit_adapter_never_silent_downgrade() {
        let mut adapter = build_adapter(&i24_meta(Codec::Flac)).unwrap();
        // Driving a 24-bit adapter through the i16 frame seam is a typed
        // error — never a silent 24→16 down-convert (FR-026).
        assert!(matches!(
            encode_i16_frame(&mut adapter, &[0i16, 1, 2, 3]),
            Err(EmitterError::Format(_))
        ));
        // The i16 seam still works for the i16 adapter.
        let mut i16_adapter = build_adapter(&BufferMeta::canonical_lossless(Codec::Flac)).unwrap();
        let out = encode_i16_frame(&mut i16_adapter, &[0i16, 1, 2, 3]).unwrap();
        assert!(!out.is_empty());
    }
}
