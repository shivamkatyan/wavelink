//! Receiver pipeline (task t-B1-receiver) — mirrors `ARCHITECTURE.md`
//! §Audio data flow for the reference simulator.
//!
//! ```text
//! transport reader (datagram/lossy OR reliable-stream/lossless)
//!   → parse + guard (reject payload > MAX_FRAME_PAYLOAD before allocation;
//!       validate lossless CRC; unsupported sample repr → typed error; NEVER panic)
//!   → bounded in-order jitter buffer (sorted by u64 seq; drop duplicate;
//!       drop late beyond the reorder window with `late` counter;
//!       queue depth hard-capped so memory stays bounded)
//!   → wdr_codec decode
//!   → HashSink (wdr_fakes, canonical blake3) + NullRenderSink
//!       (injectable fake clock; counts underruns)
//! ```
//!
//! End-of-stream is an explicit `StreamEnd` marker in the sim plane
//! ([`crate::framing`]); on it the receiver flushes the hash + metrics and
//! completes with exact loss accounting.
//!
//! Constraint: this whole module is **panic-free on untrusted input** — every
//! parse/guard/decode failure is a typed error or a counter bump, never a
//! panic.

use std::collections::BTreeMap;
use std::fmt;

use wdr_codec::{CodecAdapter, CodecAdapter24, FlacAdapter, OpusAdapter, PcmAdapter};
use wdr_proto::{Codec, Frame, FrameIntegrity, Integrity, SampleRepr, FRAME_VERSION};

use crate::emitter::BufferMeta;
use crate::framing::{FrameWire, FramedItem, FramingError};
use crate::sink::{RenderSink, SinkFormat};

/// Hard bound on the in-order jitter queue (frames). Mirrors the
/// `BufferProfile` ballpark while keeping a generous, *bounded* ceiling so a
/// burst can never grow the queue without bound (PROTOCOL_SPEC §Numeric
/// bounds: bounded parsers/queues; SECURITY_SPEC §4).
pub const MAX_QUEUE_BOUND: usize = 512;

/// Default reorder window (frames) for the balanced profile.
pub const REORDER_WINDOW: u64 = 256;

/// Receiver buffer profile selection (`--buffer low|balanced|resilient`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BufferProfile {
    Low,
    Balanced,
    Resilient,
}

impl BufferProfile {
    /// Queue capacity for the profile (frames).
    pub const fn queue_cap(self) -> usize {
        match self {
            BufferProfile::Low => 128,
            BufferProfile::Balanced => MAX_QUEUE_BOUND,
            BufferProfile::Resilient => 1024,
        }
    }

    /// Reorder window for the profile (frames).
    pub const fn reorder_window(self) -> u64 {
        match self {
            BufferProfile::Low => 32,
            BufferProfile::Balanced => REORDER_WINDOW,
            BufferProfile::Resilient => 1024,
        }
    }

    pub fn parse(s: &str) -> Result<Self, String> {
        match s {
            "low" => Ok(BufferProfile::Low),
            "balanced" => Ok(BufferProfile::Balanced),
            "resilient" => Ok(BufferProfile::Resilient),
            other => Err(format!(
                "unknown buffer profile '{other}' (low|balanced|resilient)"
            )),
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            BufferProfile::Low => "low",
            BufferProfile::Balanced => "balanced",
            BufferProfile::Resilient => "resilient",
        }
    }
}

/// Receiver counters (the metrics contract the compose harness reads).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReceiverMetrics {
    pub packets_recv: u64,
    pub loss: u64,
    pub duplicate: u64,
    pub reorder: u64,
    pub late_discard: u64,
    pub malformed: u64,
    pub fatal_count: u64,
    pub underruns: u64,
    pub bytes_recv: u64,
}

/// Final outcome of a receiver run.
#[derive(Debug, Clone)]
pub struct ReceiverOutcome {
    pub metrics: ReceiverMetrics,
    /// blake3 hash of the decoded canonical bytes (lossless roundtrip target).
    pub hash: blake3::Hash,
    /// Frames actually rendered to the hash/render sink.
    pub frames_rendered: u64,
}

impl ReceiverOutcome {
    pub fn hash_hex(&self) -> String {
        self.hash.to_hex().to_string()
    }
}

/// Typed receiver errors (no panics on data).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReceiverError {
    Framing(FramingError),
    /// Lossless path: the per-frame CRC32 did not match the payload.
    CrcMismatch {
        seq: u64,
        got: u32,
        expected: u32,
    },
    /// The frame's codec/sample representation is not supported by this
    /// reference receiver (all `wdr_proto::Codec` variants are supported, but
    /// sample representations beyond `I16` are ADR-005 stubs → typed error).
    UnsupportedCodec {
        codec: String,
    },
    /// A decode failure surfaced by the codec adapter.
    Decode {
        codec: &'static str,
        what: String,
    },
    /// End-of-stream marker not received within the bounded poll deadline.
    EndTimeout,
    /// A protocol-invariant violation (e.g. version/header mismatch).
    Internal(String),
}

impl fmt::Display for ReceiverError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ReceiverError::Framing(e) => write!(f, "receiver: {e}"),
            ReceiverError::CrcMismatch { seq, got, expected } => write!(
                f,
                "receiver: lossless CRC mismatch on seq {seq} (got {got:08x}, expected {expected:08x})"
            ),
            ReceiverError::UnsupportedCodec { codec } => {
                write!(f, "receiver: unsupported codec/sample repr '{codec}'")
            }
            ReceiverError::Decode { codec, what } => {
                write!(f, "receiver: decode ({codec}): {what}")
            }
            ReceiverError::EndTimeout => write!(f, "receiver: end-of-stream marker timeout"),
            ReceiverError::Internal(w) => write!(f, "receiver: internal: {w}"),
        }
    }
}
impl std::error::Error for ReceiverError {}

impl From<FramingError> for ReceiverError {
    fn from(e: FramingError) -> Self {
        ReceiverError::Framing(e)
    }
}

/// Parse + guard one packed frame. An oversized total or payload is rejected
/// **before** any payload-sized allocation (`Frame::unpack` enforces
/// `MAX_FRAME_PAYLOAD` inside its deserializer), and the lossless per-frame
/// CRC is validated here (before decode).
pub fn parse_and_guard(bytes: &[u8]) -> Result<Frame, ReceiverError> {
    if bytes.len() > crate::framing::MAX_FRAME_TOTAL {
        return Err(ReceiverError::Framing(FramingError::Bounds));
    }
    let frame = Frame::unpack(bytes).map_err(|e| {
        ReceiverError::Framing(match e {
            wdr_proto::DecodeError::PayloadOverflow => FramingError::PayloadOverflow,
            _ => FramingError::Malformed,
        })
    })?;
    guard_frame(&frame)?;
    Ok(frame)
}

/// Guards applied to a decoded frame before it enters the pipeline.
pub fn guard_frame(frame: &Frame) -> Result<(), ReceiverError> {
    if frame.version != FRAME_VERSION {
        return Err(ReceiverError::Internal(format!(
            "frame version {} unsupported (floor {FRAME_VERSION})",
            frame.version
        )));
    }
    if !supported_repr(frame.sample_repr) {
        return Err(ReceiverError::UnsupportedCodec {
            codec: format!("sample repr {:?}", frame.sample_repr),
        });
    }
    let lossless = matches!(frame.codec, Codec::Flac | Codec::Pcm);
    if lossless {
        if frame.integrity != Integrity::Crc32 {
            return Err(ReceiverError::Internal(format!(
                "lossless frame missing Crc32 integrity (got {:?})",
                frame.integrity
            )));
        }
        let expected = match frame.frame_integrity {
            FrameIntegrity::Crc32(c) => c,
            other => {
                return Err(ReceiverError::Internal(format!(
                    "lossless frame missing CRC value (got {other:?})"
                )));
            }
        };
        let got = wdr_proto::crc32(&frame.payload);
        if got != expected {
            return Err(ReceiverError::CrcMismatch {
                seq: frame.seq,
                got,
                expected,
            });
        }
    }
    Ok(())
}

const fn supported_repr(r: SampleRepr) -> bool {
    matches!(r, SampleRepr::I16 | SampleRepr::I24Packed)
}

/// The codec adapter the receiver decodes with, keyed to the stream's sample
/// representation. i16 drives the [`CodecAdapter`] surface; I24Packed drives
/// the 24-bit [`CodecAdapter24`] surface (decode → i32, low-3-byte canonical).
/// F32/I32 remain unbuilt at the receiver (a guard in `decoder_for`).
enum Decoder {
    I16(Box<dyn CodecAdapter>),
    I24(Box<dyn CodecAdapter24>),
}

/// The decoded PCM payload, before canonicalization into wire bytes.
enum DecodedPcm {
    I16(Box<[i16]>),
    I24(Box<[i32]>),
}

impl DecodedPcm {
    /// Canonical-LE byte count the decoded samples will occupy on the render
    /// seam (2 bytes/sample for i16, 3 for i24 — never conflated).
    fn canonical_len(&self) -> usize {
        match self {
            DecodedPcm::I16(s) => s.len() * 2,
            DecodedPcm::I24(s) => s.len() * 3,
        }
    }
}

/// Map a `wdr_proto::Codec` + metadata to a codec adapter instance, or a typed
/// error when the combination is not supported by this reference receiver.
pub fn codec_adapter_for(meta: BufferMeta) -> Result<Box<dyn CodecAdapter>, ReceiverError> {
    let adapter: Box<dyn CodecAdapter> = match meta.codec {
        Codec::Flac => Box::new(
            FlacAdapter::new(meta.sample_rate, meta.channels, 16).map_err(|e| {
                ReceiverError::Decode {
                    codec: "Flac",
                    what: e.to_string(),
                }
            })?,
        ),
        Codec::Pcm => {
            Box::new(
                PcmAdapter::new(meta.channels).map_err(|e| ReceiverError::Decode {
                    codec: "Pcm",
                    what: e.to_string(),
                })?,
            )
        }
        Codec::Opus => Box::new(
            OpusAdapter::new(
                meta.sample_rate,
                meta.channels,
                wdr_codec::FrameProfile::Ms20,
            )
            .map_err(|e| ReceiverError::Decode {
                codec: "Opus",
                what: e.to_string(),
            })?,
        ),
    };
    Ok(adapter)
}

/// Map metadata to the receiver's [`Decoder`], keyed to the sample repr. The
/// i16 arm reuses [`codec_adapter_for`]; I24Packed builds the 24-bit adapter
/// (FLAC/PCM only — Opus is i16-only, a typed error). F32/I32 are refused.
fn decoder_for(meta: BufferMeta) -> Result<Decoder, ReceiverError> {
    match meta.sample_repr {
        SampleRepr::I16 => Ok(Decoder::I16(codec_adapter_for(meta)?)),
        SampleRepr::I24Packed => {
            let adapter: Box<dyn CodecAdapter24> = match meta.codec {
                Codec::Flac => Box::new(
                    FlacAdapter::new(meta.sample_rate, meta.channels, 24).map_err(|e| {
                        ReceiverError::Decode {
                            codec: "Flac24",
                            what: e.to_string(),
                        }
                    })?,
                ),
                Codec::Pcm => Box::new(PcmAdapter::new24(meta.channels).map_err(|e| {
                    ReceiverError::Decode {
                        codec: "Pcm24",
                        what: e.to_string(),
                    }
                })?),
                Codec::Opus => {
                    return Err(ReceiverError::UnsupportedCodec {
                        codec: "Opus on a 24-bit lane (Opus is i16-only, ADR-004)".into(),
                    });
                }
            };
            Ok(Decoder::I24(adapter))
        }
        other => Err(ReceiverError::UnsupportedCodec {
            codec: format!("sample repr {other:?}"),
        }),
    }
}

/// The bounded in-order jitter buffer.
///
/// Frames are held in a `BTreeMap<u64, Frame>` and emitted in ascending `seq`
/// once the next-expected watermark is reached. Rules:
/// * duplicate `seq` → `duplicate` counter, dropped,
/// * `seq` strictly behind the next-expected watermark beyond the reorder
///   window → `late` counter, dropped,
/// * a gap deeper than the reorder window advances the next-expected
///   watermark (the missing run is counted as late; a far-ahead frame jumps
///   the anchor),
/// * `reorder` counts arrivals whose `seq` is lower than the highest seen so
///   far (out-of-order observation),
/// * the queue is hard-capped at `cap`; when full the oldest in-window frame
///   is emitted early so memory stays bounded.
/// * `_hb` (high watermark observed) is tracked with the queue.
#[derive(Debug, Clone)]
pub struct JitterBuffer {
    queue: BTreeMap<u64, Frame>,
    next_out: Option<u64>,
    high_seen: Option<u64>,
    reorder_window: u64,
    cap: usize,
    pub duplicate: u64,
    pub late: u64,
    pub reorder: u64,
}

impl JitterBuffer {
    pub const fn new(reorder_window: u64, cap: usize) -> Self {
        Self {
            queue: BTreeMap::new(),
            next_out: None,
            high_seen: None,
            reorder_window,
            cap,
            duplicate: 0,
            late: 0,
            reorder: 0,
        }
    }

    pub fn len(&self) -> usize {
        self.queue.len()
    }

    pub fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }

    /// Hard bound on the queue (bounded-growth guarantee).
    pub const fn bound(&self) -> usize {
        self.cap
    }

    /// Push a frame; returns the frames that became ready to emit (in-order,
    /// ascending). A duplicate / out-of-window / capped-early frame yields an
    /// empty vector.
    pub fn push(&mut self, frame: Frame) -> Vec<Frame> {
        let seq = frame.seq;
        if self.high_seen.is_some_and(|h| seq < h) {
            self.reorder += 1;
        }
        if self.high_seen.is_none_or(|h| seq > h) {
            self.high_seen = Some(seq);
        }

        match self.next_out {
            None => {
                self.next_out = Some(seq);
                self.queue.insert(seq, frame);
                self.emit_contiguous()
            }
            Some(no) => {
                if seq < no {
                    // Behind the expected watermark.
                    if no.saturating_sub(seq) > self.reorder_window {
                        self.late += 1;
                    } else if self.queue.contains_key(&seq) {
                        self.duplicate += 1;
                    } else {
                        // Within the window but behind the emitted point: a
                        // duplicate (already emitted) or a straggler.
                        self.duplicate += 1;
                    }
                    return Vec::new();
                }
                // seq >= no
                if seq.saturating_sub(no) > self.reorder_window {
                    // The gap no..seq is deeper than the window: the missing
                    // run is abandoned as late; drop in-window holdovers of
                    // the abandoned run and jump the watermark.
                    let abandoned: Vec<u64> = self.queue.range(..seq).map(|(k, _)| *k).collect();
                    for k in abandoned {
                        self.queue.remove(&k);
                        self.late += 1;
                    }
                    self.next_out = Some(seq);
                }
                if self.queue.contains_key(&seq) {
                    self.duplicate += 1;
                    return Vec::new();
                }
                // Bounded capacity: emit the oldest buffered early.
                if self.queue.len() >= self.cap {
                    if let Some(&oldest) = self.queue.keys().next() {
                        self.declare_emitted(oldest);
                    }
                }
                self.queue.insert(seq, frame);
                self.emit_contiguous()
            }
        }
    }

    /// Advance `next_out` past `seq` (used for capacity eviction / gap jump),
    /// counting any frames we stop waiting for as late.
    fn declare_emitted(&mut self, seq: u64) {
        if let Some(no) = self.next_out {
            if seq >= no {
                // Everything in [no, seq] we no longer wait for.
                let mut k = no;
                while k <= seq {
                    if self.queue.remove(&k).is_some() {
                        self.late += 1;
                    }
                    k += 1;
                }
                self.next_out = Some(seq.saturating_add(1));
            }
        }
    }

    /// Pop consecutive frames starting at `next_out`; advance the watermark.
    fn emit_contiguous(&mut self) -> Vec<Frame> {
        let mut out = Vec::new();
        while let Some(no) = self.next_out {
            match self.queue.remove(&no) {
                Some(f) => {
                    self.next_out = Some(no + 1);
                    out.push(f);
                }
                None => break,
            }
        }
        out
    }

    /// Pop every buffered frame in ascending order (end-of-stream flush).
    pub fn drain_all(&mut self) -> Vec<Frame> {
        let mut out = Vec::new();
        for (_, f) in std::mem::take(&mut self.queue) {
            out.push(f);
        }
        out
    }
}

/// Injectable millisecond clock (deterministic, shareable across clones;
/// `Arc<AtomicU64>` keeps `NullRenderSink` `Send` so the live receiver can run
/// its quinn server on a dedicated thread).
#[derive(Debug, Clone)]
pub enum ClockHandle {
    System,
    Injectable(std::sync::Arc<std::sync::atomic::AtomicU64>),
}

impl ClockHandle {
    pub fn system() -> Self {
        ClockHandle::System
    }

    pub fn injectable(now_ms: u64) -> Self {
        ClockHandle::Injectable(std::sync::Arc::new(std::sync::atomic::AtomicU64::new(
            now_ms,
        )))
    }

    pub fn now_ms(&self) -> u64 {
        match self {
            ClockHandle::System => std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0),
            ClockHandle::Injectable(c) => c.load(std::sync::atomic::Ordering::Relaxed),
        }
    }

    pub fn advance_ms(&self, ms: u64) {
        if let ClockHandle::Injectable(c) = self {
            let _ = c.fetch_update(
                std::sync::atomic::Ordering::Relaxed,
                std::sync::atomic::Ordering::Relaxed,
                |v| Some(v.saturating_add(ms)),
            );
        }
    }
}

/// A light render sink fed by the receiver: hashes (via the canonical
/// `wdr_fakes` `HashSink`) and counts underruns against an injectable clock.
/// This is the reference sim's "null render device".
#[derive(Debug)]
pub struct NullRenderSink {
    hash: wdr_fakes::hash::HashSink,
    clock: ClockHandle,
    last_at_ms: Option<u64>,
    pub underruns: u64,
    /// Underrun threshold (ms). `0` disables underrun accounting.
    max_gap_ms: u64,
}

impl Default for NullRenderSink {
    fn default() -> Self {
        Self::new(ClockHandle::system(), 0)
    }
}

impl NullRenderSink {
    pub fn new(clock: ClockHandle, max_gap_ms: u64) -> Self {
        Self {
            hash: wdr_fakes::hash::HashSink::new(),
            clock,
            last_at_ms: None,
            underruns: 0,
            max_gap_ms,
        }
    }

    pub fn new_injectable(now_ms: u64, max_gap_ms: u64) -> Self {
        Self::new(ClockHandle::injectable(now_ms), max_gap_ms)
    }

    pub fn clock(&self) -> &ClockHandle {
        &self.clock
    }

    /// Feed canonical decoded bytes to the hash/render sink. If `max_gap_ms`
    /// is non-zero and the gap since the last chunk exceeds it, the simulated
    /// device would have starved → underrun counter.
    pub fn render(&mut self, bytes: &[u8], now_ms: u64) {
        if self.max_gap_ms > 0 {
            if let Some(prev) = self.last_at_ms {
                if now_ms.saturating_sub(prev) > self.max_gap_ms {
                    self.underruns += 1;
                }
            }
        }
        self.last_at_ms = Some(now_ms);
        self.hash.push(bytes);
    }

    pub fn hash(&self) -> blake3::Hash {
        self.hash.final_state().finish()
    }

    pub fn hash_hex(&self) -> String {
        self.hash.hex()
    }

    pub fn bytes_rendered(&self) -> u64 {
        self.hash.samples_pushed()
    }
}

/// The null render device is the reference sim's `RenderSink`: it hashes
/// (canonical blake3) and counts underruns against its injectable clock. Its
/// `on_block` feeds the same `render` body, so the render seam and the hash
/// verification are one path.
impl RenderSink for NullRenderSink {
    fn on_format(&mut self, _fmt: SinkFormat) -> Result<(), crate::sink::SinkError> {
        // The null device hashes bytes only — format is immaterial.
        Ok(())
    }

    fn on_block(&mut self, bytes: &[u8]) -> Result<(), crate::sink::SinkError> {
        let now_ms = self.clock.now_ms();
        self.render(bytes, now_ms);
        Ok(())
    }

    fn finish(&mut self) -> Result<(), crate::sink::SinkError> {
        Ok(())
    }

    fn bytes_rendered(&self) -> u64 {
        self.hash.samples_pushed()
    }

    fn underruns(&self) -> u64 {
        self.underruns
    }

    fn integrity_hash(&self) -> Option<blake3::Hash> {
        Some(self.hash())
    }
}

/// Receiver pipeline shared between the CLI and the loopback tests so both
/// drive the exact same code path. Transport I/O is injected: the caller
/// supplies items (datagrams and/or length-prefixed reliable-stream items) via
/// [`Receiver::ingest_bytes`]; the receiver owns jitter/decode/render and
/// end-of-stream accounting.
// NOTE: `adapter` is a `dyn CodecAdapter` that is not `Debug`; the manual
// `Debug` impl below skips it, so no `Debug` derive is possible on this struct.
pub struct Receiver {
    buffer: JitterBuffer,
    profile: BufferProfile,
    // NOTE: `adapter` is a `Decoder` around a `dyn` codec that is not `Debug`;
    // the manual `Debug` impl below skips it, so no derive is possible.
    adapter: Option<Decoder>,
    /// The decoded-canonical-bytes consumer (null render device in the sim; a
    /// real desktop/mobile shell plugs its output sink in here). Boxed as the
    /// `RenderSink` seam so the receive half is injectable like the capture
    /// half — the same golden machinery verifies both.
    render: Box<dyn RenderSink>,
    meta: BufferMeta,
    format_sent: bool,
    metrics: ReceiverMetrics,
    expected_total: Option<u64>,
    ended: bool,
    stream_id: Option<u32>,
}

// (manual `Debug` impl below skips the non-`Debug` codec adapter)

impl fmt::Debug for Receiver {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Receiver")
            .field("profile", &self.profile)
            .field("metrics", &self.metrics)
            .field("queued", &self.buffer.len())
            .field("ended", &self.ended)
            .finish_non_exhaustive()
    }
}

impl Receiver {
    /// Build a receiver for a single stream with the given metadata.
    pub fn for_stream(
        meta: BufferMeta,
        profile: BufferProfile,
        clock: ClockHandle,
        max_gap_ms: u64,
    ) -> Result<Self, ReceiverError> {
        let adapter = decoder_for(meta)?;
        Ok(Self {
            buffer: JitterBuffer::new(profile.reorder_window(), profile.queue_cap()),
            profile,
            adapter: Some(adapter),
            render: Box::new(NullRenderSink::new(clock, max_gap_ms)),
            meta,
            format_sent: false,
            metrics: ReceiverMetrics::default(),
            expected_total: None,
            ended: false,
            stream_id: None,
        })
    }

    /// Replace the render sink (shells inject their output device here) and
    /// force a fresh `on_format` on the next rendered block.
    pub fn set_render_sink(&mut self, sink: Box<dyn RenderSink>) {
        self.render = sink;
        self.format_sent = false;
    }

    pub fn metrics(&self) -> &ReceiverMetrics {
        &self.metrics
    }

    pub fn metrics_mut(&mut self) -> &mut ReceiverMetrics {
        &mut self.metrics
    }

    pub fn buffer_len(&self) -> usize {
        self.buffer.len()
    }

    pub fn ended(&self) -> bool {
        self.ended
    }

    pub fn expected_total(&self) -> Option<u64> {
        self.expected_total
    }

    pub fn profile(&self) -> BufferProfile {
        self.profile
    }

    /// Ingest one raw item (a datagram or a single length-prefixed reliable
    /// stream item). Never panics on garbage.
    pub fn ingest_bytes(&mut self, bytes: &[u8]) -> Result<(), ReceiverError> {
        if FrameWire::is_end(bytes) {
            return self.handle_end_marker(bytes);
        }
        let framed = FrameWire::parse(bytes).inspect_err(|_| self.metrics.malformed += 1)?;
        match framed {
            FramedItem::Audio(frame) => {
                if self.ended {
                    self.metrics.duplicate += 1;
                    return Ok(());
                }
                self.ingest_frame(frame)
            }
            FramedItem::End { total_frames } => {
                self.finish_after_end(total_frames);
                Ok(())
            }
        }
    }

    fn handle_end_marker(&mut self, bytes: &[u8]) -> Result<(), ReceiverError> {
        let item = FrameWire::parse(bytes).inspect_err(|_| self.metrics.malformed += 1)?;
        if let FramedItem::End { total_frames } = item {
            self.finish_after_end(total_frames);
        } else {
            self.metrics.malformed += 1;
        }
        Ok(())
    }

    fn finish_after_end(&mut self, total: u64) {
        self.ended = true;
        self.expected_total = Some(total);
        let frames = self.buffer.drain_all();
        self.render_frames(frames);
        // End-of-stream: let the render sink flush/finalize its device (the
        // null device is a no-op; a real output sink commits its tail).
        let _ = self.render.finish();
        let rendered = self.metrics.packets_recv;
        self.metrics.loss = total.saturating_sub(rendered);
    }

    fn ingest_frame(&mut self, frame: Frame) -> Result<(), ReceiverError> {
        if let Some(sid) = self.stream_id {
            if sid != frame.stream_id {
                self.metrics.late_discard += 1;
                return Ok(());
            }
        } else {
            self.stream_id = Some(frame.stream_id);
        }

        guard_frame(&frame)?;

        let ready = self.buffer.push(frame);
        self.render_frames(ready);
        // Propagate the buffer counters into the metrics.
        self.metrics.duplicate = self.buffer.duplicate;
        self.metrics.late_discard = self.buffer.late;
        self.metrics.reorder = self.buffer.reorder;
        Ok(())
    }

    /// Directly ingest an already-parsed audio frame (used by the CLI's
    /// reliable-stream lane after `FrameWire::take_stream_items`).
    pub fn ingest_frame_direct(&mut self, frame: Frame) -> Result<(), ReceiverError> {
        self.ingest_frame(frame)
    }

    /// Ingest an explicit end-of-stream marker (used by the CLI's reliable
    /// stream lane). Returns true if this ended the stream.
    pub fn ingest_end_marker(&mut self, total_frames: u64) -> bool {
        if self.ended {
            return true;
        }
        self.finish_after_end(total_frames);
        true
    }

    fn render_frames(&mut self, frames: Vec<Frame>) {
        if frames.is_empty() {
            return;
        }
        let Some(mut adapter) = self.adapter.take() else {
            // Double-finalize guard: no adapter -> already flushed.
            self.metrics.duplicate = self.metrics.duplicate.saturating_add(frames.len() as u64);
            return;
        };
        // Announce the decoded output format once, from the stream metadata.
        if !self.format_sent {
            let fmt = SinkFormat {
                sample_rate: self.meta.sample_rate,
                channels: self.meta.channels,
                sample_repr: self.meta.sample_repr,
                channel_layout: self.meta.channel_layout,
            };
            self.format_sent = true;
            if self.render.on_format(fmt).is_err() {
                self.metrics.malformed += 1;
            }
        }
        for frame in frames {
            let decoded: DecodedPcm = match &mut adapter {
                Decoder::I16(a) => match a.decode(&frame.payload) {
                    Ok(d) => DecodedPcm::I16(d),
                    Err(_e) => {
                        // Decode failure on a valid CRC'd frame is not a crash;
                        // it is counted as malformed and the frame is skipped.
                        self.metrics.malformed += 1;
                        continue;
                    }
                },
                Decoder::I24(a) => match a.decode_24(&frame.payload) {
                    Ok(d) => DecodedPcm::I24(d),
                    Err(_e) => {
                        self.metrics.malformed += 1;
                        continue;
                    }
                },
            };
            let mut canonical = Vec::with_capacity(decoded.canonical_len());
            match &decoded {
                // Canonical i16 = 2 LE bytes/sample.
                DecodedPcm::I16(s) => {
                    for &v in s.iter() {
                        canonical.extend_from_slice(&v.to_le_bytes());
                    }
                }
                // Canonical i24 = low-3 LE bytes/sample, sign-extended decode
                // already applied — byte-identical to the fixture packing.
                DecodedPcm::I24(s) => {
                    for &v in s.iter() {
                        canonical.extend_from_slice(&v.to_le_bytes()[..3]);
                    }
                }
            }
            // Render through the seam: the null device hashes + counts
            // underruns internally; a real sink plays the bytes.
            if self.render.on_block(&canonical).is_err() {
                self.metrics.malformed += 1;
                continue;
            }
            self.metrics.packets_recv += 1;
            self.metrics.bytes_recv = self.render.bytes_rendered();
        }
        self.adapter = Some(adapter);
        self.metrics.underruns = self.render.underruns();
    }

    /// Finalize and return the outcome (idempotent; no mutation).
    pub fn finalize(&self) -> ReceiverOutcome {
        ReceiverOutcome {
            metrics: self.metrics.clone(),
            // A verifying sink (the null render device) reports its hash; a
            // non-verifying sink reports the empty-input hash (hash of nothing
            // rendered — the same value an un-fed HashSink finalizes to).
            hash: self
                .render
                .integrity_hash()
                .unwrap_or_else(|| blake3::hash(&[])),
            frames_rendered: self.metrics.packets_recv,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wdr_proto::{ChannelLayout, FrameFlags};

    fn frame(seq: u64) -> Frame {
        Frame::new_lossy(
            7,
            seq,
            seq * 512,
            Codec::Opus,
            48_000,
            SampleRepr::I16,
            ChannelLayout::Stereo,
            512,
            FrameFlags::default(),
            vec![0xABu8; 16],
        )
    }

    #[test]
    fn in_order_emits_immediately() {
        let mut jb = JitterBuffer::new(256, 512);
        for s in 0..100u64 {
            let ready = jb.push(frame(s));
            assert_eq!(ready.len(), 1, "in-order seq {s} emits immediately");
            assert_eq!(ready[0].seq, s);
        }
        assert!(jb.is_empty());
        assert_eq!(jb.duplicate, 0);
        assert_eq!(jb.late, 0);
    }

    #[test]
    fn duplicates_counted_and_dropped() {
        let mut jb = JitterBuffer::new(256, 512);
        let _ = jb.push(frame(0));
        let dup = jb.push(frame(0));
        assert!(dup.is_empty());
        assert_eq!(jb.duplicate, 1);
    }

    #[test]
    fn late_beyond_window_dropped() {
        let mut jb = JitterBuffer::new(8, 512);
        let _ = jb.push(frame(100));
        let _ = jb.push(frame(101));
        // seq 0 is way behind next_out=102+ ... beyond window -> late.
        let late = jb.push(frame(0));
        assert!(late.is_empty());
        assert_eq!(jb.late, 1);
    }

    #[test]
    fn reorder_buffered_and_emitted_in_order() {
        // A reordered frame that arrives *ahead* of the anchor is buffered and
        // delivered in ascending seq once the contiguous missing seq lands;
        // the reorder counter reflects the out-of-order observations.
        let mut jb = JitterBuffer::new(256, 512);
        // seq 5 arrives first and anchors the watermark at next_out=6.
        let a = jb.push(frame(5));
        assert_eq!(a.len(), 1);
        assert_eq!(a[0].seq, 5);
        // seq 8 arrives out of order (ahead): buffered while waiting for 6, 7.
        let b = jb.push(frame(8));
        assert!(b.is_empty(), "seq 8 is buffered awaiting 6..8");
        assert_eq!(jb.len(), 1);
        // seq 7 arrives: still waiting for 6, so it is buffered too.
        let c = jb.push(frame(7));
        assert!(c.is_empty(), "seq 7 is buffered awaiting 6");
        assert_eq!(jb.len(), 2);
        // The contiguous missing seq arrives last -> 6, 7, 8 flush in order.
        let d = jb.push(frame(6));
        assert_eq!(d.len(), 3, "contiguous run 6..8 emits together");
        assert_eq!(d.iter().map(|f| f.seq).collect::<Vec<_>>(), vec![6, 7, 8]);
        assert_eq!(jb.reorder, 2, "7 and 6 both arrived out of order");
        assert!(jb.is_empty());
        assert!(jb.drain_all().is_empty());
    }

    #[test]
    fn cap_is_bounded() {
        let mut jb = JitterBuffer::new(256, 4);
        let _ = jb.push(frame(0));
        for s in 5..100u64 {
            let _ = jb.push(frame(s));
            assert!(jb.len() <= 4, "queue bounded at cap");
        }
    }
}
