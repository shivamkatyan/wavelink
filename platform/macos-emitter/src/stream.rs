//! `--stream` driver: real capture → [`AudioFrameSink`] → QUIC receiver.
//!
//! macOS-only (SCK capture backend + POSIX signals). Runs on the main thread:
//! `QuicAudioSink` and `SystemCaptureHandle` are deliberately not `Send`, so
//! capture, encode, transport and the receiver all live on one thread. A
//! SIGINT/SIGTERM handler sets a stop flag that the drain loop polls; on stop
//! we flush `finish()` (end-of-stream marker) and exit 0, so the reference
//! receiver completes cleanly.
//!
//! Two sources:
//! * `--fixture <stem>` — deterministic `wdr_fakes` fixture (NO hardware, NO
//!   TCC). Default budget reproduces the canonical 4096-value golden; a
//!   `--duration N` budget streams N wall-clock seconds. This is the
//!   hash-perfect software gate.
//! * (no fixture) — real ScreenCaptureKit system-audio capture. Requires a
//!   logged-in GUI session + Screen Recording TCC (hardware gate); the
//!   permission probe runs first and fails fast with a typed `fatal` event.
//!
//! STATUS contract (newline-delimited JSON on stdout) is documented in the
//! crate README; the Swift GUI parses it.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use serde_json::json;
use wdr_entitlement::provider::Tier;
use wdr_fakes::source::{ChannelKind, Fixture, FixtureKind, SampleFormat};
use wdr_fakes::PcmSource;
use wdr_proto::{ChannelLayout, Codec, SampleRepr};
use wdr_refsim::emitter::policy_gate;
use wdr_refsim::sink::{AudioFrameSink, QuicAudioSink, SinkFormat};

use crate::backend::permission::{PermissionGate, ScShareableContentProbe};
use crate::backend::system_capture::SystemCaptureHandle;
use crate::FormatMeta;

/// Default receiver address (GUI default + ref_receiver loopback gate).
const DEFAULT_ADDR: &str = "127.0.0.1:9100";
/// Canonical golden budget (values): 4 frames of 512-sample FLAC × 2ch.
const CANONICAL_GOLDEN_VALUES: usize = 4096;
/// Real-capture drain poll cadence.
const POLL_MS: u64 = 2;
/// Real-capture preallocated buffer size (bytes) — 2 MiB headroom so a slow
/// drain flags an overflow instead of silently dropping audio.
const CAPTURE_CAPACITY: usize = 2 << 20;

/// Stop flag set by the SIGINT/SIGTERM handler; polled by the drain loops.
static STOP: AtomicBool = AtomicBool::new(false);

extern "C" fn on_signal(_sig: libc::c_int) {
    STOP.store(true, Ordering::SeqCst);
}

fn install_signal_handlers() {
    // POSIX signal handlers only write an atomic bool — async-signal-safe.
    // `libc::SignalHandler` is `usize` on macOS: cast the fn item through a
    // pointer to keep the cast warning-free (and correct).
    unsafe {
        libc::signal(libc::SIGINT, on_signal as *const () as libc::sighandler_t);
        libc::signal(libc::SIGTERM, on_signal as *const () as libc::sighandler_t);
    }
}

/// Print one newline-delimited status JSON event (GUI contract).
fn status(v: serde_json::Value) {
    println!("{v}");
}

fn fatal(message: &str) {
    status(json!({ "ev": "fatal", "status": "error", "message": message }));
}

fn parse_codec(s: &str) -> Result<Codec, String> {
    match s {
        "opus" => Ok(Codec::Opus),
        "flac" => Ok(Codec::Flac),
        "pcm" => Ok(Codec::Pcm),
        other => Err(format!("unknown --codec '{other}' (opus|flac|pcm)")),
    }
}

fn parse_tier(s: &str) -> Result<Tier, String> {
    match s.to_ascii_lowercase().as_str() {
        "free" => Ok(Tier::Free),
        "pro" => Ok(Tier::Pro),
        other => Err(format!("unknown --tier '{other}' (free|pro)")),
    }
}

/// Fixture stems mirroring `ref_emitter --source`.
fn parse_fixture(s: &str) -> Result<FixtureKind, String> {
    use wdr_fakes::source::Stereo;
    Ok(match s {
        "pseudo-random" | "pcm" => FixtureKind::PseudoRandomPcm,
        "silence" => FixtureKind::Silence,
        "impulse" => FixtureKind::ImpulseTrain { period: 64 },
        "full-scale" => FixtureKind::FullScaleEdge,
        "sine" => FixtureKind::SineSweep { f0: 20.0, f1: 20_000.0 },
        "channel-left" => FixtureKind::ChannelId { lane: Stereo::LeftPattern },
        "channel-right" => FixtureKind::ChannelId { lane: Stereo::RightPattern },
        other => {
            return Err(format!(
                "unknown --fixture '{other}' (pseudo-random|silence|impulse|full-scale|sine|channel-left|channel-right)"
            ))
        }
    })
}

#[derive(Debug)]
struct StreamArgs {
    addr: String,
    tier: Tier,
    codec: Codec,
    fixture: Option<FixtureKind>,
    duration_secs: Option<u64>,
    /// Captured sample rate (fixture/real default 48k; `--rate 44100` exercises
    /// the rate-aware seam hardware-free).
    rate: u32,
}

const STREAM_USAGE: &str = "usage: macos-emitter --stream --addr <ip:port> [--tier free|pro]\n\
     \t[--codec opus|flac|pcm] [--fixture pseudo-random|silence|impulse|full-scale|sine|channel-left|channel-right]\n\
     \t[--duration <secs>] [--rate <hz>]\n\
     env:  WDR_STREAM_ADDR=<addr> (default 127.0.0.1:9100)  WDR_ENT_TIER=<free|pro>  WDR_STREAM_DURATION_SECS=N  WDR_CC=<cubic|bbr>";

fn parse_stream_args(argv: &[String]) -> Result<StreamArgs, String> {
    let env_addr = std::env::var("WDR_STREAM_ADDR").ok();
    let env_tier = std::env::var("WDR_ENT_TIER").ok();
    let env_dur = std::env::var("WDR_STREAM_DURATION_SECS").ok();

    let mut addr: Option<String> = None;
    let mut tier: Option<Tier> = None;
    let mut codec: Option<Codec> = None;
    let mut fixture: Option<FixtureKind> = None;
    let mut duration: Option<u64> = None;
    let mut rate: Option<u32> = None;

    let mut i = 0;
    let value = |args: &[String], i: &mut usize, name: &str| -> Result<String, String> {
        *i += 1;
        args.get(*i)
            .cloned()
            .ok_or_else(|| format!("{name} needs a value"))
    };
    while i < argv.len() {
        let a = argv[i].as_str();
        match a {
            "--addr" => addr = Some(value(argv, &mut i, "--addr")?),
            "--tier" => tier = Some(parse_tier(&value(argv, &mut i, "--tier")?)?),
            "--codec" => codec = Some(parse_codec(&value(argv, &mut i, "--codec")?)?),
            "--fixture" => fixture = Some(parse_fixture(&value(argv, &mut i, "--fixture")?)?),
            "--duration" => {
                let v = value(argv, &mut i, "--duration")?;
                duration = Some(
                    v.parse::<u64>()
                        .map_err(|_| format!("invalid --duration '{v}'"))?,
                );
            }
            "--rate" => {
                let v = value(argv, &mut i, "--rate")?;
                rate = Some(
                    v.parse::<u32>()
                        .map_err(|_| format!("invalid --rate '{v}'"))?,
                );
            }
            "--help" | "-h" => return Err(STREAM_USAGE.to_string()),
            other if other.starts_with('-') => {
                return Err(format!("unknown option '{other}'; {STREAM_USAGE}"))
            }
            other => return Err(format!("unexpected positional '{other}'; {STREAM_USAGE}")),
        }
        i += 1;
    }

    let addr = match (addr, env_addr) {
        (Some(a), _) => a,
        (None, Some(e)) => e,
        (None, None) => DEFAULT_ADDR.to_string(),
    };
    let tier = match (tier, env_tier) {
        (Some(t), _) => t,
        (None, Some(e)) => parse_tier(&e)?,
        (None, None) => Tier::Free,
    };
    let duration = match (duration, env_dur) {
        (Some(d), _) => Some(d),
        (None, Some(e)) => Some(
            e.parse::<u64>()
                .map_err(|_| "invalid WDR_STREAM_DURATION_SECS".to_string())?,
        ),
        (None, None) => None,
    };
    // Codec default: Free → Opus, Pro → FLAC.
    let codec = codec.unwrap_or(if tier == Tier::Pro {
        Codec::Flac
    } else {
        Codec::Opus
    });

    Ok(StreamArgs {
        addr,
        tier,
        codec,
        fixture,
        duration_secs: duration,
        rate: rate.unwrap_or(48_000),
    })
}

/// The `--stream` entry point (called from `main`). Returns a process exit
/// code: 0 = clean end-of-stream flushed, 1 = typed fatal error.
pub fn run_stream(argv: &[String]) -> i32 {
    let args = match parse_stream_args(argv) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("{e}");
            return 2;
        }
    };

    let lossless = args.codec != Codec::Opus;
    // Tier policy gate BEFORE any byte is sent (FR-042/43).
    if let Err(e) = policy_gate(lossless, args.tier) {
        fatal(&e.to_string());
        return 1;
    }

    // Real capture (no fixture) first checks Screen Recording TCC; fail fast
    // with a typed fatal event so the GUI can show a fixable message.
    if args.fixture.is_none() {
        let mut gate = PermissionGate::new(ScShareableContentProbe);
        let state = gate.refresh();
        if !gate.capture_allowed() {
            fatal(&format!(
                "Screen Recording not granted (state={state:?}) — enable it in \
                 System Settings → Privacy & Security → Screen Recording, then retry."
            ));
            return 1;
        }
    }

    // Connect (dial + handshake inside the sink's runtime) before streaming.
    let mut sink = match QuicAudioSink::connect(&args.addr, args.tier, args.codec) {
        Ok(s) => s,
        Err(e) => {
            fatal(&format!("connect {}: {e}", args.addr));
            return 1;
        }
    };

    install_signal_handlers();
    STOP.store(false, Ordering::SeqCst);

    status(json!({
        "ev": "start",
        "tier": format!("{:?}", args.tier).to_ascii_lowercase(),
        "codec": format!("{:?}", args.codec).to_ascii_lowercase(),
        "rate": args.rate,
        "channels": 2,
        "lane": if args.codec == Codec::Opus { "datagram" } else { "stream" },
        "fixture": args.fixture.is_some(),
        "addr": args.addr,
    }));

    // Stable capture format for the seam (i16 / stereo; the delivered rate —
    // `--rate` for fixtures, the SCK-delivered rate for real capture — makes
    // the transport rate-aware: lossless at the true rate, Opus native 44.1k
    // or worker-resampled to 48k).
    if let Err(e) = sink.on_format(SinkFormat {
        sample_rate: args.rate,
        channels: 2,
        sample_repr: SampleRepr::I16,
        channel_layout: ChannelLayout::Stereo,
    }) {
        fatal(&e.to_string());
        return 1;
    }
    if args.rate != 48_000 || args.fixture.is_some() {
        status(json!({
            "ev": "format",
            "rate": args.rate,
            "channels": 2,
            "codec": format!("{:?}", args.codec).to_ascii_lowercase(),
            "resampled": sink.resampled(),
            "wire_rate": sink.wire_sample_rate(),
        }));
    }

    let result = match args.fixture {
        Some(kind) => fixture_stream(&mut sink, kind, args.duration_secs, args.rate),
        None => real_stream(&mut sink, args.duration_secs),
    };
    let sink_ref = &sink;
    match result {
        Ok(()) => {
            status(json!({
                "ev": "end", "status": "ok",
                "packets_sent": sink_ref.packets_sent(),
                "bytes_sent": sink_ref.bytes_sent(),
            }));
            0
        }
        Err(e) => {
            fatal(&e.to_string());
            1
        }
    }
}

type StreamResult = Result<(), String>;

/// Periodic status event at most once per second. `send_ms` is the worker-side
/// encode+send time for the last drained block (an honest send-side latency
/// number — NOT end-to-end, which needs receiver timestamps; see
/// LATENCY_MEASUREMENT.md).
fn maybe_stats(sink: &QuicAudioSink, overflowed: bool, last: &mut Instant, send_ms: u64) {
    if last.elapsed() >= Duration::from_secs(1) {
        status(json!({
            "ev": "stats",
            "seq": sink.packets_sent(),
            "packets_sent": sink.packets_sent(),
            "bytes_sent": sink.bytes_sent(),
            "overflowed": overflowed,
            "send_ms": send_ms,
            "period_ms": 1000,
        }));
        *last = Instant::now();
    }
}

/// Fixture path (NO hardware): drive `wdr_fakes` buckets through the same
/// seam as real capture. Default budget reproduces the canonical 4096-value
/// golden; `--duration N` streams N wall-clock seconds; `--rate` exercises
/// non-48k capture (lossless true-rate / Opus native 44.1k or resampled).
fn fixture_stream(
    sink: &mut QuicAudioSink,
    kind: FixtureKind,
    duration: Option<u64>,
    rate: u32,
) -> StreamResult {
    let whole = sink.values_per_frame(); // frame_samples × channels
    let budget = match duration {
        Some(secs) => rate as usize * 2 * (secs.max(1) as usize), // stereo wall-clock
        None => CANONICAL_GOLDEN_VALUES,
    };
    let total_values = (budget / whole) * whole; // whole frames only

    let mut fixture = Fixture::new(
        kind,
        SampleFormat::I16,
        rate,
        ChannelKind::Stereo,
        total_values as u64,
    );
    let overflowed = false;
    let mut last_stats = Instant::now();
    loop {
        if STOP.load(Ordering::SeqCst) {
            break;
        }
        let chunk = fixture.next_chunk(whole as u32);
        if chunk.len == 0 {
            break;
        }
        sink.on_block(chunk.bytes).map_err(|e| e.to_string())?;
        maybe_stats(sink, overflowed, &mut last_stats, 0);
    }
    sink.finish().map_err(|e| e.to_string())?;
    Ok(())
}

/// Real ScreenCaptureKit path (hardware/TCC gate): drain the preallocated
/// buffer into the seam off the RT callback, honouring the stop flag.
fn real_stream(sink: &mut QuicAudioSink, duration: Option<u64>) -> StreamResult {
    let deadline = duration.map(|s| Instant::now() + Duration::from_secs(s));
    // Caller-owned preallocated buffer, freed after stop (same thread, so the
    // unsafe with_buffer contract is trivially satisfied). 2 MiB gives a
    // comfortable headroom so a slower drain flags an overflow instead of
    // dropping audio silently.
    let mut memory = vec![0u8; CAPTURE_CAPACITY];
    let ptr = memory.as_mut_ptr();
    // SAFETY: `memory` outlives the handle (both live in this scope) and is a
    // valid writable allocation of CAPTURE_CAPACITY bytes.
    let mut handle = unsafe {
        SystemCaptureHandle::with_buffer(
            ptr,
            CAPTURE_CAPACITY,
            FormatMeta {
                rate: 48_000,
                bits: 16,
                channels: 2,
            },
        )
    };

    handle
        .start()
        .map_err(|e| format!("ScreenCaptureKit start: {e} (hardware/TCC gate)"))?;

    // Discover the DELIVERED rate from the first audio sample (SCK's returned
    // rate is authoritative; the requested 48k is just a preference) and point
    // the seam at the truth — the rate-aware sink streams the true rate for
    // lossless or resamples odd rates to 48k for Opus.
    let delivered = wait_delivered_format(&handle);
    match delivered {
        Some(fmt) => {
            sink.on_format(SinkFormat {
                sample_rate: fmt.rate,
                channels: fmt.channels as u16,
                sample_repr: SampleRepr::I16,
                channel_layout: ChannelLayout::Stereo,
            })
            .map_err(|e| {
                format!(
                    "delivered format {}/{}-ch not supported by the seam: {e}",
                    fmt.rate, fmt.channels
                )
            })?;
            status(json!({
                "ev": "format",
                "rate": fmt.rate,
                "channels": fmt.channels,
                "codec": format!("{:?}", sink.codec()).to_ascii_lowercase(),
                "resampled": sink.resampled(),
                "wire_rate": sink.wire_sample_rate(),
            }));
        }
        None => {
            status(json!({
                "ev": "warning",
                "kind": "format-unknown",
                "message": "delivered audio format not reported within 1.5s; streaming at the requested 48 kHz",
            }));
        }
    }

    let mut overflowed = false;
    let mut last_stats = Instant::now();
    let mut last_send = 0u64;
    loop {
        if STOP.load(Ordering::SeqCst) {
            break;
        }
        if let Some(dl) = deadline {
            if Instant::now() >= dl {
                break;
            }
        }
        let b = handle.buffered();
        if !b.is_empty() {
            let t0 = Instant::now();
            sink.on_block(b).map_err(|e| e.to_string())?;
            last_send = t0.elapsed().as_millis() as u64;
            handle.reset_buffered();
        }
        if handle.overflowed() && !overflowed {
            // First-time overflow is an actionable warning, not a silent flag.
            overflowed = true;
            status(json!({
                "ev": "warning",
                "kind": "buffer-overflow",
                "message": "capture fell behind and dropped audio — reduce load or raise the buffer",
            }));
        }
        maybe_stats(sink, overflowed, &mut last_stats, last_send);
        std::thread::sleep(Duration::from_millis(POLL_MS));
    }
    handle.stop();
    // Free the preallocated buffer only after the handler is stopped.
    drop(handle);
    drop(memory);
    sink.finish().map_err(|e| e.to_string())?;
    Ok(())
}

/// Bounded wait (≤1.5s) for the first audio sample to report the delivered
/// format; `None` if capture never delivers audio (e.g. silence / TCC) or the
/// format description is absent.
fn wait_delivered_format(handle: &SystemCaptureHandle) -> Option<crate::FormatMeta> {
    let deadline = Instant::now() + Duration::from_millis(1500);
    while Instant::now() < deadline {
        if STOP.load(Ordering::SeqCst) {
            return None;
        }
        if let Some(f) = handle.delivered_format() {
            return Some(f);
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    None
}
