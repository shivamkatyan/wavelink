//! `ref_emitter` — headless reference-sim emitter CLI (task R-B1-SIM).
//!
//! ```text
//! ref_emitter --role emitter --lossy|--lossless [--codec opus|flac|pcm]
//!             [--source <fixture>] [--seconds N] [--buffer low|balanced|resilient]
//!             <receiver-addr>
//! ```
//!
//! Environment (matching the compose harness contract):
//! * `WDR_SIM_ROLE` — fallback for `--role` (must be `emitter`).
//! * `WDR_METRICS_DIR` — directory for `emitter-sim.json` (default `/tmp/metrics`).
//! * `WDR_NETEM_PROFILE` — records the impairment profile into the metrics
//!   JSON (informational; the emitter itself is impairment-agnostic).
//! * `WDR_ENT_TIER` — entitlement tier override (default `free`). A `--lossless`
//!   lane under the **effective Free policy** is rejected with a typed policy
//!   error **before** any packet is sent (FR-042/FR-043, ADR-003: lossless
//!   rides a reliable stream; Free never silently sends lossless).
//! * `WDR_CC` — congestion controller selection for the t-P1-cc spike:
//!   `cubic` (default, ADR-003 default-compatible) or `bbr`
//!   (`wdr_transport::CongestionControl::Bbr`, quinn `BbrConfig`). Read at
//!   dial time and recorded in `emitter-sim.json` so the receiver-side
//!   measurements can be attributed to the controller that was active.
//!
//! The emitter is the QUIC client: it dials the reference receiver
//! (`ref_receiver`, the listening side) over the loopback/QUIC plane, drives
//! the shared in-process [`Emitter`] on the selected lane (Opus datagrams for
//! lossy, FLAC/PCM reliable stream for lossless — ADR-003), then writes
//! `emitter-sim.json` using the metrics key contract from `docker/sim-start.sh`
//! and `docker/assert.sh` (role, status, [`hash`], `packets_sent`, `bytes_sent`,
//! [`errors`], [`latency_us`], `fatal_count`).
//!
//! Exit: `0` on a clean run (metrics flushed); non-zero on error. The metrics
//! file is flushed on normal completion **and** on `SIGTERM`, so the harness
//! collector never reads a stale/partial file.

use std::process::ExitCode;

use wdr_refsim::emitter::{
    lane_and_meta, policy_gate, tier_from_env, BufferMeta, Emitter, EmitterConfig, EmitterError,
    Impairment, StreamKind,
};
use wdr_refsim::receiver::BufferProfile;
use wdr_refsim::sink::{cc_from_env, dial_loopback};
use wdr_transport::CongestionControl;

/// Canonical sim metadata: i16 / 48 kHz / stereo (ADR-005 i16-only adapters).
const SIM_RATE_HZ: u32 = 48_000;

/// Seconds → total samples per channel at the canonical rate (default 2 s).
fn seconds_to_samples(seconds: f64) -> u64 {
    (seconds.max(0.0) * f64::from(SIM_RATE_HZ)).round() as u64
}

struct Args {
    lossless: bool,
    codec: wdr_proto::Codec,
    fixture: wdr_fakes::source::FixtureKind,
    total_samples: u64,
    buffer: BufferProfile,
    /// Host:port of the receiver. The compose harness resolves the receiver
    /// service by DNS (`receiver-sim`), so this is kept as a string and
    /// resolved at dial time.
    addr: String,
}

fn usage() -> &'static str {
    "usage: ref_emitter --role emitter --lossy|--lossless [--codec opus|flac|pcm]\n\
     \t[--source pseudo-random|silence|impulse|full-scale|sine|channel-left|channel-right]\n\
     \t[--seconds N] [--buffer low|balanced|resilient] [<receiver-addr>]\n\
     env:  WDR_SIM_ROLE=emitter  WDR_METRICS_DIR=<dir>  WDR_NETEM_PROFILE=<profile>\n\
     \tWDR_ENT_TIER=free|pro (default free; --lossless under free is refused)\n\
     \tWDR_CC=cubic|bbr (default cubic; congestion controller for the t-P1-cc spike)\n\
     \tWDR_EMITTER_ADDR=<addr> (default 127.0.0.1:9000)"
}

fn parse_source(s: &str) -> Result<wdr_fakes::source::FixtureKind, String> {
    use wdr_fakes::source::{FixtureKind, Stereo};
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
                "unknown --source '{other}' (pseudo-random|silence|impulse|full-scale|sine|channel-left|channel-right)"
            ))
        }
    })
}

fn parse_codec(s: &str) -> Result<wdr_proto::Codec, String> {
    Ok(match s {
        "opus" => wdr_proto::Codec::Opus,
        "flac" => wdr_proto::Codec::Flac,
        "pcm" => wdr_proto::Codec::Pcm,
        other => return Err(format!("unknown --codec '{other}' (opus|flac|pcm)")),
    })
}

fn parse_args() -> Result<Args, String> {
    let mut role = std::env::var("WDR_SIM_ROLE").ok();
    let mut lossless = false;
    let mut codec: Option<wdr_proto::Codec> = None;
    let mut fixture = wdr_fakes::source::FixtureKind::PseudoRandomPcm;
    let mut total_samples = seconds_to_samples(2.0);
    let mut _buffer = BufferProfile::Balanced;
    let mut positional = Vec::new();

    let mut it = std::env::args().skip(1);

    while let Some(a) = it.next() {
        match a.as_str() {
            "--role" => role = Some(it.next().ok_or("--role needs a value")?),
            "--lossy" => lossless = false,
            "--lossless" => lossless = true,
            "--codec" => codec = Some(parse_codec(&it.next().ok_or("--codec needs a value")?)?),
            "--source" => fixture = parse_source(&it.next().ok_or("--source needs a value")?)?,
            "--seconds" => {
                let v = it.next().ok_or("--seconds needs a value")?;
                total_samples = seconds_to_samples(
                    v.parse::<f64>()
                        .map_err(|_| format!("invalid --seconds '{v}'"))?,
                );
            }
            "--buffer" => {
                let v = it.next().ok_or("--buffer needs a value")?;
                _buffer = BufferProfile::parse(&v)?;
            }
            "--help" | "-h" => return Err(usage().to_string()),
            other if other.starts_with('-') => {
                return Err(format!("unknown option '{other}'; {}", usage()));
            }
            other => positional.push(other.to_string()),
        }
    }

    if positional.len() > 1 {
        return Err(format!(
            "expected at most one <addr>; got {}; {}",
            positional.len(),
            usage()
        ));
    }
    if role.as_deref() != Some("emitter") {
        return Err(format!(
            "role must be 'emitter' (got {:?}); {}",
            role,
            usage()
        ));
    }
    // The compose harness (`docker/sim-start.sh`) passes config via env only,
    // so the receiver address may come from `WDR_EMITTER_ADDR` (default
    // 127.0.0.1:9000 — the harness's receiver resolves via DNS/netns) as well
    // as the positional CLI argument.
    let env_addr = std::env::var("WDR_EMITTER_ADDR").ok();
    let addr = positional
        .first()
        .cloned()
        .or(env_addr)
        .unwrap_or_else(|| "127.0.0.1:9000".into());

    // Default codec from the lane when not given (lossless defaults to FLAC).
    let codec = codec.unwrap_or(if lossless {
        wdr_proto::Codec::Flac
    } else {
        wdr_proto::Codec::Opus
    });

    Ok(Args {
        lossless,
        codec,
        fixture,
        total_samples,
        buffer: _buffer,
        addr,
    })
}

/// Derive the (lane, wire metadata, in-process emitter config) for a run.
fn build_emitter(cfg: &Args) -> (StreamKind, BufferMeta, EmitterConfig) {
    // Lane + canonical wire metadata shared with the live transport sink
    // (wdr_refsim::emitter::lane_and_meta).
    let (kind, meta) = lane_and_meta(cfg.lossless, cfg.codec);
    let emit_cfg = EmitterConfig {
        kind,
        total_samples: cfg.total_samples,
        fixture: cfg.fixture.clone(),
        impairment: Impairment::clean(),
        seed: 0xB1E1_0000,
        // t-P1-cc spike pacing: `WDR_EMIT_PACE_MS` runs the datagram lane at a
        // steady frame cadence (20 ms = real Opus 20 ms cadence). Default
        // None preserves the loopback burst behaviour. `WDR_EMIT_REAL_TIME=1`
        // paces by the frame's own audio duration so a run reaches true
        // wall-clock real-time for any codec (used by the soak).
        pace: std::env::var("WDR_EMIT_PACE_MS")
            .ok()
            .and_then(|v| v.trim().parse::<u64>().ok())
            .map(std::time::Duration::from_millis),
        pace_real_time: std::env::var("WDR_EMIT_REAL_TIME").as_deref() == Ok("1"),
    };
    (kind, meta, emit_cfg)
}

/// Race the emit future against a quieted SIGTERM. `None` == the run was
/// cut by the harness; the caller flushes partial counters and exits 0.
async fn run_until_signal<F>(run: &mut F) -> Result<Option<blake3::Hash>, EmitterError>
where
    F: core::future::Future<Output = Result<blake3::Hash, EmitterError>> + Unpin,
{
    let mut sig = std::pin::pin!(async {
        let mut s = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("SIGTERM handler");
        s.recv().await;
    });
    tokio::select! {
        r = run => r.map(Some),
        _ = &mut sig => Ok(None),
    }
}

fn metrics_json(
    status: &str,
    hash: Option<&blake3::Hash>,
    packets_sent: u64,
    bytes_sent: u64,
    errors: u64,
    latency_us: Option<u64>,
    path: Option<&wdr_transport::metrics::PathReport>,
) -> String {
    let profile = std::env::var("WDR_NETEM_PROFILE").unwrap_or_else(|_| "clean".into());
    let cc = match cc_from_env() {
        Ok(c) => match c {
            CongestionControl::Cubic => "cubic",
            CongestionControl::Bbr => "bbr",
        },
        Err(_) => "unknown",
    };
    let path_json = path.map(|p| {
        serde_json::json!({
            "rtt_us": p.rtt_micros,
            "cwnd": p.cwnd,
            "lost_packets": p.lost_packets,
            "lost_bytes": p.lost_bytes,
            "congestion_events": p.congestion_events,
            "ack_frames": p.ack_frames,
            "current_mtu": p.current_mtu,
        })
    });
    let json = serde_json::json!({
        "role": "emitter",
        "status": status,
        "profile": profile,
        "cc": cc,
        "hash": hash.map(|h| h.to_hex().to_string()),
        "packets_sent": packets_sent,
        "bytes_sent": bytes_sent,
        "errors": errors,
        "latency_us": latency_us,
        "path": path_json,
        "loss": { "packets": 0 },
        "duplicate": { "packets": 0 },
        "reorder": { "packets": 0 },
        "late": { "packets": 0 },
        "fatal_count": 0,
    });
    serde_json::to_string_pretty(&json).expect("emitter metrics json")
}

fn write_metrics(dir: &str, body: &str) {
    if let Err(e) = std::fs::create_dir_all(dir) {
        eprintln!("[ref_emitter] cannot mkdir metrics dir {dir}: {e}");
    }
    let path = std::path::Path::new(dir).join("emitter-sim.json");
    if let Err(e) = std::fs::write(&path, body) {
        eprintln!("[ref_emitter] cannot write metrics {path:?}: {e}");
    }
}

#[tokio::main]
async fn main() -> ExitCode {
    let args = match parse_args() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("[ref_emitter] {e}");
            return ExitCode::FAILURE;
        }
    };

    let metrics_dir = std::env::var("WDR_METRICS_DIR").unwrap_or_else(|_| "/tmp/metrics".into());

    // CC selection gate FIRST — refuse an unknown `WDR_CC` before any packet is
    // sent, so the t-P1-cc spike can never attribute measurements to a
    // controller that was not actually active.
    let cc = match cc_from_env() {
        Ok(c) => c,
        Err(e) => {
            write_metrics(
                &metrics_dir,
                &metrics_json("error", None, 0, 0, 1, None, None),
            );
            eprintln!("[ref_emitter] {e}");
            return ExitCode::FAILURE;
        }
    };

    // Policy gate FIRST — refuse an ungranted lane before dialing/sending.
    let tier = match tier_from_env() {
        Ok(t) => t,
        Err(e) => {
            write_metrics(
                &metrics_dir,
                &metrics_json("error", None, 0, 0, 1, None, None),
            );
            eprintln!("[ref_emitter] {e}");
            return ExitCode::FAILURE;
        }
    };
    if let Err(e) = policy_gate(args.lossless, tier) {
        write_metrics(
            &metrics_dir,
            &metrics_json("error", None, 0, 0, 1, None, None),
        );
        eprintln!("[ref_emitter] {e}");
        return ExitCode::FAILURE;
    }

    let (kind, meta, emit_cfg) = build_emitter(&args);
    eprintln!(
        "[ref_emitter] connecting to {} (lane={kind:?}, codec={:?}, source={:?}, samples={}, buffer={}, cc={cc:?})",
        args.addr,
        meta.codec,
        args.fixture,
        args.total_samples,
        args.buffer.as_str()
    );

    let conn = match dial_loopback(&args.addr).await {
        Ok(c) => c,
        Err(e) => {
            write_metrics(
                &metrics_dir,
                &metrics_json("error", None, 0, 0, 1, None, None),
            );
            eprintln!("[ref_emitter] {e}");
            return ExitCode::FAILURE;
        }
    };

    let started = std::time::Instant::now();
    // `conn` is kept alive for the path metrics below; quinn Connection is a
    // cheap clone handle, so the emitter may own one too.
    let mut emitter = match Emitter::new(conn.clone(), emit_cfg, meta) {
        Ok(e) => e,
        Err(e) => {
            write_metrics(
                &metrics_dir,
                &metrics_json("error", None, 0, 0, 1, None, None),
            );
            eprintln!("[ref_emitter] {e}");
            return ExitCode::FAILURE;
        }
    };

    // Run to completion or until SIGTERM. The sender counters are snapshotted
    // after `emitter` is no longer mutably borrowed by the emit future.
    let outcome = {
        let mut run = Box::pin(emitter.run());
        run_until_signal(&mut run).await
    };
    let packets = emitter.packets_sent();
    let bytes = emitter.bytes_sent();
    let latency_us = started.elapsed().as_micros() as u64;
    // Path metrics at the end of the run: RTT/cwnd/lost/congestion events are
    // what quinn exposes to distinguish CC behaviour under loss (t-P1-cc).
    let path = wdr_transport::metrics_snapshot(&conn);

    match outcome {
        Ok(Some(h)) => {
            write_metrics(
                &metrics_dir,
                &metrics_json(
                    "ok",
                    Some(&h),
                    packets,
                    bytes,
                    0,
                    Some(latency_us),
                    Some(&path),
                ),
            );
            println!(
                "[ref_emitter] complete: packets={packets} bytes={bytes} latency_us={latency_us} hash={} path={path:?}",
                h.to_hex()
            );
            ExitCode::SUCCESS
        }
        Ok(None) => {
            // SIGTERM cut the run: flush partial counters (hash unknown),
            // status ok so the harness "no crash" floor still passes.
            write_metrics(
                &metrics_dir,
                &metrics_json("ok", None, packets, bytes, 0, Some(latency_us), Some(&path)),
            );
            eprintln!("[ref_emitter] SIGTERM: flushed metrics (packets={packets} bytes={bytes})");
            ExitCode::SUCCESS
        }
        Err(e) => {
            write_metrics(
                &metrics_dir,
                &metrics_json("error", None, 0, 0, 1, None, None),
            );
            eprintln!("[ref_emitter] error: {e}");
            ExitCode::FAILURE
        }
    }
}
