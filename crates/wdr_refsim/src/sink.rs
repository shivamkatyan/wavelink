//! Live capture→transport sink — the `FrameSink`/`AudioFrameSink` seam
//! (README status row "In-app transport wiring").
//!
//! MACOS-FIRST: the macOS emitter is the first real consumer; the Android/iOS
//! shells keep their null placeholders until their wiring pass.
//!
//! Data path (all off the RT callback): PCM block → accumulate whole frames →
//! encode (per tier) → `Frame` with per-frame CRC on lossless → pack → quinn
//! (datagram for Opus, reliable stream for FLAC/PCM). Mirrors the in-process
//! [`Emitter`] so refsim and shells share identical encode/wire code — the same
//! path the golden loopback tests prove.
//!
//! NOTE: no AEAD on this milestone's data path — that is the pairing/Noise
//! follow-up (`wdr_crypto` is ready but not wired; loopback security is the
//! quinn TLS 1.3 identity, 0-RTT off).

use std::net::SocketAddr;
use std::time::Duration;

use wdr_entitlement::provider::Tier;
use wdr_fakes::source::unpack_i24;
use wdr_proto::{ChannelLayout, Codec, SampleRepr};

use crate::emitter::{lane_and_meta, policy_gate, wire_meta_repr, Emitter, EmitterError};
use crate::receiver::{BufferProfile, ReceiverError, ReceiverOutcome};

/// Stable capture/format metadata delivered once (the Android
/// `FrameSink.onFormat` analogue). Declared by the shell once the format is
/// stable, before the first block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SinkFormat {
    pub sample_rate: u32,
    pub channels: u16,
    pub sample_repr: SampleRepr,
    pub channel_layout: ChannelLayout,
}

/// The canonical capture→transport seam. Synchronous so any shell drives it
/// from a plain worker thread; encoding/QUIC run inside the concrete sink on
/// its own runtime (off the RT callback).
///
/// NOT `Send`: the codec adapters wrap single-threaded C encoders (opcode/flac),
/// and a sink owns its quinn runtime — create it **on the worker thread** that
/// drives it and it never needs to cross threads (same discipline as the
/// platform capture handles).
pub trait AudioFrameSink {
    /// Called once, before the first block, when the capture format is stable.
    fn on_format(&mut self, fmt: SinkFormat) -> Result<(), SinkError>;
    /// One captured PCM block (canonical LE bytes: 2 bytes/sample for i16,
    /// low-3-bytes for i24 — any size). The sink accumulates to whole codec
    /// frames (`frame_samples × channels` values).
    fn on_block(&mut self, bytes: &[u8]) -> Result<(), SinkError>;
    /// Flush accumulation and send the end-of-stream marker; no calls after.
    /// A partial tail (< one whole frame) is dropped and reported through the
    /// marker — hash-perfect streams must supply whole frames (fixtures do).
    fn finish(&mut self) -> Result<(), SinkError>;
}
/// Both names used across the docs/shells (`FrameSink` / `AudioFrameSink`)
/// resolve to the same seam.
pub use AudioFrameSink as FrameSink;

/// The receiver-side render seam — the decoded-canonical-bytes consumer whose
/// mirror of [`AudioFrameSink`] makes the receive half of the pipeline
/// injectable the same way the capture half is.
///
/// A render device receives the **decoded canonical PCM** stream: the
/// reference sim's null device hashes it (blake3) + counts underruns, while a
/// real desktop/mobile shell's output sink (AVAudioEngine / HAL / AudioTrack)
/// plays it. Same golden/fixture machinery drives both, so the receiver
/// plumbing is testable end-to-end without hardware.
///
/// Synchronous, NOT `Send` (like the emitter seam): created and driven from
/// the worker thread that owns the receiver pipeline.
pub trait RenderSink {
    /// Called once, before the first block, when the decoded output format is
    /// stable (the receiver sniffs it from the first frame).
    fn on_format(&mut self, fmt: SinkFormat) -> Result<(), SinkError>;
    /// One block of decoded canonical PCM bytes (interleaved LE, any size).
    fn on_block(&mut self, bytes: &[u8]) -> Result<(), SinkError>;
    /// Flush + finalize the render device; no calls after.
    fn finish(&mut self) -> Result<(), SinkError>;

    /// Bytes accepted by the sink (render telemetry readout). Defaults to 0;
    /// the null render device reports its real count.
    fn bytes_rendered(&self) -> u64 {
        0
    }

    /// Simulated-device underruns (null render device only by default).
    fn underruns(&self) -> u64 {
        0
    }

    /// Lossless-integrity hash of everything rendered, if the sink verifies
    /// (the null render device returns its blake3 hash; a real playback sink
    /// returns `None`).
    fn integrity_hash(&self) -> Option<blake3::Hash> {
        None
    }
}

/// Typed errors from the transport seam. No panics on data.
#[derive(Debug)]
pub enum SinkError {
    /// The tier refused the lane before any byte is sent (FR-042/043).
    Policy(String),
    Encode(wdr_codec::CodecError),
    Send(String),
    /// Capture format not supported by the wired adapters (i16/48k/stereo at
    /// B0; ADR-005). Surfaced early, never silently mis-decoded.
    Format(String),
}

impl From<EmitterError> for SinkError {
    fn from(e: EmitterError) -> Self {
        match e {
            EmitterError::Encode(ce) => SinkError::Encode(ce),
            EmitterError::Send(s) => SinkError::Send(s),
            EmitterError::Format(m) => SinkError::Format(m),
            EmitterError::Policy(m) => SinkError::Policy(m),
        }
    }
}

impl From<wdr_codec::CodecError> for SinkError {
    fn from(e: wdr_codec::CodecError) -> Self {
        SinkError::Encode(e)
    }
}

impl core::fmt::Display for SinkError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            SinkError::Policy(m) => write!(f, "sink policy: {m}"),
            SinkError::Encode(e) => write!(f, "sink encode: {e}"),
            SinkError::Send(s) => write!(f, "sink send: {s}"),
            SinkError::Format(m) => write!(f, "sink format: {m}"),
        }
    }
}
impl std::error::Error for SinkError {}

/// Listen for **one** emitter connection and render its decoded stream into a
/// caller-supplied [`RenderSink`]. This is the receiver-side mirror of
/// [`QuicAudioSink`]: it owns the quinn server and drives the shared
/// `receiver_server` lane machinery on a dedicated thread, so a desktop/mobile
/// shell supplies only a **`Send`** [`RenderSink`] and blocks on
/// [`QuicRenderReceiver::wait`] — no tokio/quinn dependencies required in the
/// shell (WS3 receiver render seam).
///
/// Why `Send`: the quinn server must poll its endpoint driver continuously for
/// the handshake to progress while a remote emitter dials, so the receive loop
/// runs on its own OS thread. `NullRenderSink` (and any shell sink composed of
/// plain data + a channel to the platform audio thread) is `Send`; a sink that
/// must stay on one thread can drive
/// [`crate::receiver_server::run_listener`] itself from its own worker instead.
pub struct QuicRenderReceiver {
    rx: std::sync::mpsc::Receiver<Result<ReceiverOutcome, ReceiverError>>,
    local_addr: SocketAddr,
}

impl QuicRenderReceiver {
    /// Start listening on `addr` for one emitter connection on a dedicated
    /// thread; every decoded block is delivered into `sink`. Returns once the
    /// endpoint is bound; call [`wait`](Self::wait) to block to completion.
    pub fn listen(
        addr: SocketAddr,
        profile: BufferProfile,
        sink: Box<dyn RenderSink + Send>,
    ) -> Result<Self, SinkError> {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| SinkError::Send(format!("tokio runtime: {e}")))?;
        let config = crate::receiver_server::loopback_server_config();
        let endpoint = rt
            .block_on(async move { quinn::Endpoint::server(config, addr) })
            .map_err(|e| SinkError::Send(format!("quinn bind {addr}: {e}")))?;
        let local_addr = endpoint
            .local_addr()
            .map_err(|e| SinkError::Send(format!("endpoint local_addr: {e}")))?;
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::Builder::new()
            .name("wdr-receive".to_string())
            .spawn(move || {
                let outcome = rt.block_on(crate::receiver_server::run_listener(
                    endpoint, profile, sink,
                ));
                let _ = tx.send(outcome);
            })
            .map_err(|e| SinkError::Send(format!("spawn receive thread: {e}")))?;
        Ok(Self { rx, local_addr })
    }

    /// The bound local address (the emitter dials this).
    pub fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }

    /// Block the current (worker) thread until the receive loop completes with
    /// an outcome, or `timeout` elapses. Can be awaited at most once.
    pub fn wait(&mut self, timeout: Duration) -> Result<ReceiverOutcome, SinkError> {
        self.rx
            .recv_timeout(timeout)
            .map_err(|_| SinkError::Send("receive loop timed out".into()))?
            .map_err(|e| SinkError::Send(format!("receive: {e}")))
    }
}

/// A QUIC audio sink that **owns its own tokio runtime** plus the quinn
/// endpoint/connection (dialed here, accept-any loopback identity, 0-RTT off).
/// The synchronous [`AudioFrameSink`] methods `block_on` that runtime, so a
/// shell needs **no tokio/quinn/codec dependencies** — it just drives this
/// from a worker thread.
///
/// **Rate-aware (works for everyone):** [``AudioFrameSink::on_format`] accepts
/// the *delivered* capture rate and either (lossless) streams the true rate on
/// the wire — the receiver sniffs it — or (Opus) passes 44.1k through natively,
/// or resamples odd rates (8/16/24/32k) to 48 kHz on the worker via
/// [`wdr_codec::ResamplerI16`]. Lossless is NEVER resampled (bit-exact).
pub struct QuicAudioSink {
    rt: tokio::runtime::Runtime,
    emitter: Emitter,
    codec: Codec,
    /// Current wire sample representation (i16 by default; I24Packed on the
    /// 24-bit live lane). Determines the canonical byte stride on `on_block`.
    sample_repr: SampleRepr,
    /// Current wire sample rate (48k for Opus-resampled; delivered for the
    /// 44.1/48k Opus and all lossless lanes).
    wire_rate: u32,
    /// The true delivered (captured) sample rate.
    captured_rate: u32,
    /// Worker-side resampler engaged for the Opus lane when the captured rate
    /// is an odd value (not 44.1/48k) — lossy-only by construction.
    resampler: Option<wdr_codec::ResamplerI16>,
    /// Codec frame size per channel (512 FLAC/PCM, 960 Opus @48k, 882 @44.1k).
    frame_samples: usize,
    channels: u16,
    /// Whole-frame value count (`frame_samples × channels`).
    values_per_frame: usize,
    /// Worker-side accumulation between `on_block` calls (allocation is fine,
    /// this is off the RT callback).
    acc: Vec<u8>,
}

impl SinkFormat {
    /// The canonical i16 / 48 kHz / stereo capture format (the fixture +
    /// default SCK request). `connect` uses this.
    pub const fn canonical() -> Self {
        SinkFormat {
            sample_rate: crate::emitter::SIM_RATE_HZ,
            channels: crate::emitter::SIM_CHANNELS,
            sample_repr: SampleRepr::I16,
            channel_layout: ChannelLayout::Stereo,
        }
    }

    /// The canonical 24-bit capture format: I24Packed / 48 kHz / stereo — the
    /// WS-A 24-bit live lane (3 canonical bytes/sample on the wire).
    pub const fn canonical_24() -> Self {
        SinkFormat {
            sample_rate: crate::emitter::SIM_RATE_HZ,
            channels: crate::emitter::SIM_CHANNELS,
            sample_repr: SampleRepr::I24Packed,
            channel_layout: ChannelLayout::Stereo,
        }
    }
}

/// The canonical wire byte width of a sample repr (bytes/sample on the
/// `on_block` seam). Only the reprs the sink builds are valid (validated in
/// [`validate_basic_format`]); the other reprs map to a typed error upstream.
const fn wire_bytes_per_sample(repr: SampleRepr) -> usize {
    match repr {
        SampleRepr::I16 => 2,
        SampleRepr::I24Packed => 3,
        // F32/I32 are not buildable by this sink (validate_basic_format refuses
        // them), so this branch is unreachable from a driven sink.
        SampleRepr::F32 | SampleRepr::I32 => 0,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct WirePlan {
    wire_rate: u32,
    frame_samples: usize,
    resampled: bool,
}

/// Map a delivered (captured) sample rate to the wire plan for a codec.
///
/// * **Lossless (FLAC/PCM)**: ride the true delivered rate (bit-exact,
///   512-sample frames) — the receiver sniffs it from the first frame.
/// * **Opus**: 44.1k and 48k are native to `OpusAdapter` (882 / 960-sample
///   20 ms frames, wire == delivered). An *odd* lower rate (SCK can deliver
///   8/16/24/32k) is resampled to 48 kHz on the worker. **>48 kHz is refused**
///   for the lossy lane (ADR-004: never silently re-route hi-res through lossy).
fn wire_plan(codec: Codec, rate: u32) -> Result<WirePlan, SinkError> {
    if codec == Codec::Opus {
        match rate {
            48_000 => Ok(WirePlan {
                wire_rate: 48_000,
                frame_samples: 960,
                resampled: false,
            }),
            44_100 => Ok(WirePlan {
                wire_rate: 44_100,
                frame_samples: 882,
                resampled: false,
            }),
            r if r > 48_000 => Err(SinkError::Format(format!(
                "lossy (Opus) refuses a >48 kHz source ({r} Hz) — ADR-004, never a silent re-route"
            ))),
            _r => Ok(WirePlan {
                wire_rate: 48_000,
                frame_samples: 960,
                resampled: true,
            }),
        }
    } else if !(8_000..=192_000).contains(&rate) {
        Err(SinkError::Format(format!(
            "unsupported lossless rate {rate} Hz (8000..=192000)"
        )))
    } else {
        Ok(WirePlan {
            wire_rate: rate,
            frame_samples: 512,
            resampled: false,
        })
    }
}

impl QuicAudioSink {
    /// Dial `addr` with the reference client contract and build the lane +
    /// meta for the canonical i16 / 48k / stereo format (see
    /// [`QuicAudioSink::connect_with_format`] for other rates).
    pub fn connect(addr: &str, tier: Tier, codec: Codec) -> Result<Self, SinkError> {
        Self::connect_with_format(addr, tier, codec, SinkFormat::canonical())
    }

    /// Like [`QuicAudioSink::connect`] but for a caller that knows the capture
    /// format up front (fixture → canonical; a future adapter that reports its
    /// format pre-dial → its real rate). The Free/Pro policy gate still runs
    /// **before** any byte is dialed/sent (FR-042/43).
    pub fn connect_with_format(
        addr: &str,
        tier: Tier,
        codec: Codec,
        fmt: SinkFormat,
    ) -> Result<Self, SinkError> {
        validate_basic_format(&fmt)?;
        let plan = wire_plan(codec, fmt.sample_rate)?;
        policy_gate(codec != Codec::Opus, tier)?;
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| SinkError::Send(format!("tokio runtime: {e}")))?;
        let conn = rt.block_on(dial_loopback(addr))?;
        let (kind, _) = lane_and_meta(codec != Codec::Opus, codec);
        let meta = wire_meta_repr(codec, plan.wire_rate, plan.frame_samples, fmt.sample_repr);
        let emitter = Emitter::new_live(conn, kind, meta)?;
        let resampler = if plan.resampled {
            Some(wdr_codec::ResamplerI16::new(
                fmt.sample_rate,
                48_000,
                fmt.channels,
            )?)
        } else {
            None
        };
        Ok(Self {
            rt,
            emitter,
            codec,
            sample_repr: fmt.sample_repr,
            wire_rate: plan.wire_rate,
            captured_rate: fmt.sample_rate,
            resampler,
            frame_samples: plan.frame_samples,
            channels: fmt.channels,
            values_per_frame: plan.frame_samples * fmt.channels as usize,
            acc: Vec::new(),
        })
    }

    /// Whole-frame value count this sink accumulates to (`frame_samples ×
    /// channels`) — lets the shell drive a fixture at exactly frame granularity.
    pub fn values_per_frame(&self) -> usize {
        self.values_per_frame
    }

    /// Codec frame size per channel at the current wire rate.
    pub fn frame_samples(&self) -> usize {
        self.frame_samples
    }

    pub fn channels(&self) -> u16 {
        self.channels
    }

    /// The codec this sink transports.
    pub fn codec(&self) -> Codec {
        self.codec
    }

    /// Whether the worker is resampling the captured rate to the wire rate.
    pub fn resampled(&self) -> bool {
        self.resampler.is_some()
    }

    /// The wire sample rate the lane actually runs at (what the receiver
    /// sniffs): delivered for lossless & native Opus; 48k when resampled.
    pub fn wire_sample_rate(&self) -> u32 {
        self.wire_rate
    }

    /// The true delivered (captured) sample rate the source produced.
    pub fn captured_sample_rate(&self) -> u32 {
        self.captured_rate
    }

    /// Total audio frames actually transported so far.
    pub fn packets_sent(&self) -> u64 {
        self.emitter.packets_sent()
    }

    /// Total payload bytes transported (audio + end marker), excluding wire
    /// length prefixes.
    pub fn bytes_sent(&self) -> u64 {
        self.emitter.bytes_sent()
    }
}

/// Shared structural validation (stereo; i16 or I24Packed). Rate policy is
/// handled by [`wire_plan`]; the 24-bit lane rides only lossless codecs (Opus
/// is i16-only, refused by the emitter's `build_adapter`).
fn validate_basic_format(fmt: &SinkFormat) -> Result<(), SinkError> {
    if fmt.channels != 2 {
        return Err(SinkError::Format(format!(
            "unsupported channel count {} (stereo-only adapters)",
            fmt.channels
        )));
    }
    if !matches!(fmt.sample_repr, SampleRepr::I16 | SampleRepr::I24Packed) {
        return Err(SinkError::Format(format!(
            "unsupported sample repr {:?} (i16/i24 at B0, ADR-005)",
            fmt.sample_repr
        )));
    }
    if fmt.channel_layout != ChannelLayout::Stereo {
        return Err(SinkError::Format(format!(
            "unsupported layout {:?} (stereo-only this slice)",
            fmt.channel_layout
        )));
    }
    Ok(())
}

impl AudioFrameSink for QuicAudioSink {
    fn on_format(&mut self, fmt: SinkFormat) -> Result<(), SinkError> {
        validate_basic_format(&fmt)?;
        let plan = wire_plan(self.codec, fmt.sample_rate)?;
        // No-op when the plan (and hence the emission path) is unchanged.
        if plan.wire_rate == self.wire_rate
            && plan.frame_samples == self.frame_samples
            && plan.resampled == self.resampler.is_some()
            && fmt.channels == self.channels
            && fmt.sample_repr == self.sample_repr
        {
            return Ok(());
        }

        // Rebuild the emitter over the SAME connection with the new wire meta
        // (receiver sniffs it from the first frame — no receiver change), and
        // (re)create the worker resampler when the Opus lane now normalizes.
        let conn = self.emitter.conn_handle();
        let (kind, _) = lane_and_meta(self.codec != Codec::Opus, self.codec);
        let meta = wire_meta_repr(
            self.codec,
            plan.wire_rate,
            plan.frame_samples,
            fmt.sample_repr,
        );
        self.emitter = Emitter::new_live(conn, kind, meta)?;
        self.wire_rate = plan.wire_rate;
        self.sample_repr = fmt.sample_repr;
        self.captured_rate = fmt.sample_rate;
        self.frame_samples = plan.frame_samples;
        self.values_per_frame = plan.frame_samples * fmt.channels as usize;
        self.acc.clear();
        self.resampler = if plan.resampled {
            Some(wdr_codec::ResamplerI16::new(
                fmt.sample_rate,
                48_000,
                fmt.channels,
            )?)
        } else {
            None
        };
        Ok(())
    }

    fn on_block(&mut self, bytes: &[u8]) -> Result<(), SinkError> {
        if let Some(rs) = &mut self.resampler {
            // Odd captured rate → push the resampled 48 kHz stream into the
            // codec accumulator (all off-RT; the resampler is stateful).
            // The worker resampler is i16-only (lossy lane), so this branch
            // never runs for the 24-bit lane.
            let input: Vec<i16> = bytes
                .as_chunks::<2>()
                .0
                .iter()
                .map(|b| i16::from_le_bytes(*b))
                .collect();
            let mut out = Vec::new();
            rs.process(&input, &mut out);
            for v in out {
                self.acc.extend_from_slice(&v.to_le_bytes());
            }
        } else {
            self.acc.extend_from_slice(bytes);
        }

        // Whole-frame boundary depends on the repr's canonical wire width:
        // i16 = 2 bytes/sample, I24Packed = 3 bytes/sample (never conflated).
        let spb = wire_bytes_per_sample(self.sample_repr);
        let whole = self.values_per_frame * spb;
        while self.acc.len() >= whole {
            let rest = self.acc.split_off(whole);
            match self.sample_repr {
                SampleRepr::I16 => {
                    let pcm: Vec<i16> = self
                        .acc
                        .as_chunks::<2>()
                        .0
                        .iter()
                        .map(|b| i16::from_le_bytes(*b))
                        .collect();
                    self.acc = rest;
                    self.rt.block_on(self.emitter.emit_pcm_frame(&pcm))?;
                }
                SampleRepr::I24Packed => {
                    // Canonical i24 block → i32 values (low-3-bytes,
                    // sign-extended) exactly as the fixture packs them.
                    let pcm: Vec<i32> = self
                        .acc
                        .as_chunks::<3>()
                        .0
                        .iter()
                        .map(|b| unpack_i24(b))
                        .collect();
                    self.acc = rest;
                    self.rt.block_on(self.emitter.emit_pcm_frame_24(&pcm))?;
                }
                SampleRepr::F32 | SampleRepr::I32 => {
                    // Unreachable from a driven sink: validate_basic_format
                    // refuses these reprs. Typed error, never a silent guess.
                    return Err(SinkError::Format(format!(
                        "sample repr {:?} is not built by the sink (i16/i24)",
                        self.sample_repr
                    )));
                }
            }
        }
        Ok(())
    }

    fn finish(&mut self) -> Result<(), SinkError> {
        // Any partial tail (< one whole frame) is dropped here — the end marker
        // carries the deliverable frame count; a hash-perfect stream always
        // supplies whole frames (the fixture path does).
        self.rt.block_on(self.emitter.emit_end_marker())?;
        // Loopback flush grace: quinn datagram sends are async fire-and-forget.
        // Let this runtime's socket driver write any queued datagrams (incl.
        // the end marker) before the sink is dropped and its runtime shuts
        // down (also on the shell's SIGTERM→finish→exit path), so a receiver
        // deterministically gets a clean end-of-stream.
        self.rt.block_on(async {
            // Construct inside the runtime context (a bare `tokio::time::sleep`
            // at the call site would panic: "no reactor running").
            tokio::time::sleep(std::time::Duration::from_millis(50)).await
        });
        Ok(())
    }
}

/// The congestion controller for the dial, from `WDR_CC` (default `cubic`).
/// Shared with `ref_emitter`; unknown values are a typed error so a run can
/// never silently attribute measurements to a controller that wasn't active.
pub fn cc_from_env() -> Result<wdr_transport::CongestionControl, String> {
    match std::env::var("WDR_CC")
        .unwrap_or_else(|_| "cubic".into())
        .to_ascii_lowercase()
        .as_str()
    {
        "cubic" => Ok(wdr_transport::CongestionControl::Cubic),
        "bbr" => Ok(wdr_transport::CongestionControl::Bbr),
        other => Err(format!(
            "unknown WDR_CC '{other}' (cubic|bbr) — refusing to run an undocumented controller"
        )),
    }
}

/// Accept-any-server-cert verifier for the loopback reference sim (the
/// reference receiver uses a self-signed `localhost` identity via rcgen).
/// Loopback/sim-only: production pairing pins fingerprints (pairing follow-up).
#[derive(Debug)]
pub struct AcceptAll;

impl quinn::rustls::client::danger::ServerCertVerifier for AcceptAll {
    fn verify_server_cert(
        &self,
        _ee: &quinn::rustls::pki_types::CertificateDer<'_>,
        _int: &[quinn::rustls::pki_types::CertificateDer<'_>],
        _sn: &quinn::rustls::pki_types::ServerName<'_>,
        _ocsp: &[u8],
        _now: quinn::rustls::pki_types::UnixTime,
    ) -> Result<quinn::rustls::client::danger::ServerCertVerified, quinn::rustls::Error> {
        Ok(quinn::rustls::client::danger::ServerCertVerified::assertion())
    }
    fn verify_tls12_signature(
        &self,
        _m: &[u8],
        _c: &quinn::rustls::pki_types::CertificateDer<'_>,
        _d: &quinn::rustls::DigitallySignedStruct,
    ) -> Result<quinn::rustls::client::danger::HandshakeSignatureValid, quinn::rustls::Error> {
        Ok(quinn::rustls::client::danger::HandshakeSignatureValid::assertion())
    }
    fn verify_tls13_signature(
        &self,
        _m: &[u8],
        _c: &quinn::rustls::pki_types::CertificateDer<'_>,
        _d: &quinn::rustls::DigitallySignedStruct,
    ) -> Result<quinn::rustls::client::danger::HandshakeSignatureValid, quinn::rustls::Error> {
        Ok(quinn::rustls::client::danger::HandshakeSignatureValid::assertion())
    }
    fn supported_verify_schemes(&self) -> Vec<quinn::rustls::SignatureScheme> {
        vec![
            quinn::rustls::SignatureScheme::ECDSA_NISTP256_SHA256,
            quinn::rustls::SignatureScheme::RSA_PSS_SHA256,
            quinn::rustls::SignatureScheme::ED25519,
        ]
    }
}

/// Dial the receiver with the loopback-only client contract: accept-any cert
/// for the self-signed reference-sim identity, 0-RTT OFF on the media path
/// (SECURITY_SPEC §3.6), CC from `WDR_CC` (default Cubic). Bind `0.0.0.0:0` so
/// QUIC source-address validation can reach a remote/loopback receiver.
pub async fn dial_loopback(host_port: &str) -> Result<quinn::Connection, EmitterError> {
    let cc = cc_from_env().map_err(EmitterError::Send)?;
    let provider = std::sync::Arc::new(quinn::rustls::crypto::ring::default_provider());
    let mut rustls_cfg = quinn::rustls::ClientConfig::builder_with_provider(provider)
        .with_protocol_versions(&[&quinn::rustls::version::TLS13])
        .expect("ring provider supports TLS 1.3")
        .dangerous()
        .with_custom_certificate_verifier(std::sync::Arc::new(AcceptAll))
        .with_no_client_auth();
    rustls_cfg.enable_early_data = false; // 0-RTT OFF for media (SECURITY_SPEC)
    let qcc = quinn::crypto::rustls::QuicClientConfig::try_from(rustls_cfg)
        .map_err(|e| EmitterError::Send(format!("client crypto: {e:?}")))?;
    let transport_cfg = std::sync::Arc::new(
        wdr_transport::TransportConnConfig::default()
            .congestion_control(cc)
            .build_transport(),
    );
    let mut client_cfg = quinn::ClientConfig::new(std::sync::Arc::new(qcc));
    client_cfg.transport_config(transport_cfg);

    // Bind 0.0.0.0 so source-address validation can reach the receiver; the
    // compose harness needs this across netns/bridges (loopback is fine too).
    let bind_addr = std::env::var("WDR_EMITTER_BIND")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "0.0.0.0:0".to_string());
    let endpoint = quinn::Endpoint::client(bind_addr.parse().expect("bind addr"))
        .map_err(|e| EmitterError::Send(format!("endpoint: {e}")))?;
    let addr = tokio::net::lookup_host(host_port)
        .await
        .map_err(|e| EmitterError::Send(format!("resolve {host_port}: {e}")))?
        .next()
        .ok_or_else(|| EmitterError::Send(format!("no address for {host_port}")))?;
    let conn = endpoint
        .connect_with(client_cfg, addr, "localhost")
        .map_err(|e| EmitterError::Send(format!("connect {host_port}: {e}")))?;
    // Bounded handshake so a shell/GUI can fail fast on an unreachable
    // receiver instead of hanging.
    tokio::time::timeout(std::time::Duration::from_secs(5), conn)
        .await
        .map_err(|_| EmitterError::Send(format!("handshake with {host_port} timed out (5s)")))?
        .map_err(|e| EmitterError::Send(format!("handshake {:?}: {e}", addr)))
}

#[cfg(test)]
mod tests {
    use super::{wire_plan, SinkError};
    use wdr_proto::Codec;

    #[test]
    fn lossy_refuses_hi_res_sources() {
        // ADR-004: >48 kHz never silently re-routes through a lossy codec.
        assert!(matches!(
            wire_plan(Codec::Opus, 88_200),
            Err(SinkError::Format(_))
        ));
        assert!(matches!(
            wire_plan(Codec::Opus, 96_000),
            Err(SinkError::Format(_))
        ));
        // Lossless accepts hi-res at the true rate (bit-exact).
        assert!(wire_plan(Codec::Flac, 88_200).is_ok());
        assert!(wire_plan(Codec::Pcm, 192_000).is_ok());
    }

    #[test]
    fn rate_plan_matrix() {
        // Opus native 48k / 44.1k; odd low rate → worker-resampled to 48k.
        let p = wire_plan(Codec::Opus, 48_000).unwrap();
        assert_eq!(
            (p.wire_rate, p.frame_samples, p.resampled),
            (48_000, 960, false)
        );
        let p = wire_plan(Codec::Opus, 44_100).unwrap();
        assert_eq!(
            (p.wire_rate, p.frame_samples, p.resampled),
            (44_100, 882, false)
        );
        let p = wire_plan(Codec::Opus, 24_000).unwrap();
        assert_eq!(
            (p.wire_rate, p.frame_samples, p.resampled),
            (48_000, 960, true)
        );
        // Lossless true-rate, never resampled.
        let p = wire_plan(Codec::Flac, 44_100).unwrap();
        assert_eq!(
            (p.wire_rate, p.frame_samples, p.resampled),
            (44_100, 512, false)
        );
        let p = wire_plan(Codec::Pcm, 96_000).unwrap();
        assert_eq!(
            (p.wire_rate, p.frame_samples, p.resampled),
            (96_000, 512, false)
        );
        // Out-of-range lossless refused.
        assert!(wire_plan(Codec::Flac, 1_000).is_err());
    }
}
