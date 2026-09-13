//! `ref_receiver` — headless reference-sim receiver CLI (task t-B1-receiver).
//!
//! ```text
//! ref_receiver --role receiver --buffer low|balanced|resilient <addr>
//! ```
//!
//! Environment (matching the compose harness contract):
//! * `WDR_SIM_ROLE` — fallback for `--role` (must be `receiver`).
//! * `WDR_METRICS_DIR` — directory for `receiver-sim.json` (default `/tmp/metrics`).
//! * `WDR_NETEM_PROFILE` — records the impairment profile into the metrics
//!   JSON (informational; the receiver itself is impairment-agnostic).
//! * `WDR_PEER_WAIT_SECS` — optionally bounds the peer-arrival wait.
//!
//! The receiver listens on `<addr>` for a QUIC connection from the reference
//! emitter, reads the media plane (reliable stream for lossless, datagrams for
//! lossy), runs it through the shared pipeline in `wdr_refsim::receiver`, and
//! writes `receiver-sim.json` (role, status, `packets_recv`, loss, duplicate,
//! reorder, `late_discard`, `fatal_count`, hash, …).
//!
//! The lane machinery (sniff → `run_lane`) lives in
//! `wdr_refsim::receiver_server` — the **same code** `QuicRenderReceiver`
//! drives, so the reference sim and the shell-facing receiver share one path.
//!
//! Exit: `0` on a clean end-of-stream (hash + metrics flushed); non-zero on
//! error and on end-of-stream timeout (bounded poll, never sleep-and-assume).

use std::net::SocketAddr;
use std::process::ExitCode;

use wdr_refsim::receiver::BufferProfile;
use wdr_refsim::receiver_server;

struct Args {
    buffer: BufferProfile,
    addr: SocketAddr,
    /// Advertise `_wdr._tcp` (FR-003, ADR-006) for the QUIC port this
    /// receiver listens on, so an emitter can `--discover` it.
    advertise: bool,
}

fn usage() -> &'static str {
    "usage: ref_receiver --role receiver [--buffer low|balanced|resilient] [--advertise] [<listen-addr>]\n\
     env:  WDR_SIM_ROLE=receiver  WDR_METRICS_DIR=<dir>  WDR_NETEM_PROFILE=<profile>\n\
     \tWDR_RECEIVER_ADDR=<addr> (default 0.0.0.0:9000)"
}

fn parse_args() -> Result<Args, String> {
    let mut role = std::env::var("WDR_SIM_ROLE").ok();
    let mut buffer = BufferProfile::Balanced;
    let mut advertise = false;
    let mut positional = Vec::new();

    let mut it = std::env::args().skip(1).peekable();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--role" => role = Some(it.next().ok_or("--role needs a value")?),
            "--advertise" => advertise = true,
            "--buffer" => {
                buffer = BufferProfile::parse(&it.next().ok_or("--buffer needs a value")?)?
            }
            "--help" | "-h" => return Err(usage().to_string()),
            other if other.starts_with('-') => {
                return Err(format!("unknown option '{other}'; {}", usage()))
            }
            other => positional.push(other.to_string()),
        }
    }
    // The compose harness (`docker/sim-start.sh`) passes config via env only, so
    // the listen address may come from `WDR_RECEIVER_ADDR` (default :9000) as
    // well as the positional CLI argument.
    let env_addr = std::env::var("WDR_RECEIVER_ADDR").ok();
    if positional.len() > 1 {
        return Err(format!(
            "expected at most one <addr>; got {}; {}",
            positional.len(),
            usage()
        ));
    }
    let addr_src = positional
        .first()
        .cloned()
        .or(env_addr)
        .unwrap_or_else(|| "0.0.0.0:9000".into());
    let addr: SocketAddr = addr_src
        .parse()
        .map_err(|_| format!("invalid listen address '{addr_src}'"))?;
    if role.as_deref() != Some("receiver") {
        return Err(format!(
            "role must be 'receiver' (got {:?}); {}",
            role,
            usage()
        ));
    }
    Ok(Args {
        buffer,
        addr,
        advertise,
    })
}

async fn write_metrics(dir: &str, body: &str) {
    if let Err(e) = tokio::fs::create_dir_all(dir).await {
        eprintln!("[ref_receiver] cannot mkdir metrics dir {dir}: {e}");
    }
    let path = std::path::Path::new(dir).join("receiver-sim.json");
    if let Err(e) = tokio::fs::write(&path, body).await {
        eprintln!("[ref_receiver] cannot write metrics {path:?}: {e}");
    }
}

#[tokio::main]
async fn main() -> ExitCode {
    let args = match parse_args() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("[ref_receiver] {e}");
            return ExitCode::FAILURE;
        }
    };

    let metrics_dir = std::env::var("WDR_METRICS_DIR").unwrap_or_else(|_| "/tmp/metrics".into());
    let profile = std::env::var("WDR_NETEM_PROFILE").unwrap_or_else(|_| "clean".into());

    let config = receiver_server::loopback_server_config();
    let endpoint = match quinn::Endpoint::server(config, args.addr) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("[ref_receiver] bind {}: {e}", args.addr);
            return ExitCode::FAILURE;
        }
    };
    println!(
        "[ref_receiver] listening on {} (buffer={:?}); WDR_PEER_WAIT_SECS bounds peer wait",
        args.addr, args.buffer
    );

    // `--advertise` (FR-003, ADR-006): publish `_wdr._tcp` for this port so an
    // emitter can `--discover` it instead of a hand-typed address. Kept alive
    // for the whole run.
    let _advertise = if args.advertise {
        match wdr_discovery::advertise_loopback(wdr_discovery::RECEIVER_INSTANCE, args.addr.port())
        {
            Ok(a) => {
                println!(
                    "[ref_receiver] advertising `_wdr._tcp` {}:{} (FR-003)",
                    wdr_discovery::RECEIVER_INSTANCE,
                    args.addr.port(),
                );
                Some(a)
            }
            Err(e) => {
                eprintln!("[ref_receiver] advertise failed (continuing): {e}");
                None
            }
        }
    } else {
        None
    };

    let result =
        receiver_server::run_listener(endpoint, args.buffer, receiver_server::null_render_sink())
            .await;
    match result {
        Ok(outcome) => {
            let json = serde_json::json!({
                "role": "receiver",
                "status": "complete",
                "profile": profile,
                "packets_recv": outcome.metrics.packets_recv,
                "bytes_recv": outcome.metrics.bytes_recv,
                "loss": { "packets": outcome.metrics.loss },
                "duplicate": { "packets": outcome.metrics.duplicate },
                "reorder": { "packets": outcome.metrics.reorder },
                "late": { "packets": outcome.metrics.late_discard },
                "late_discard": { "packets": outcome.metrics.late_discard },
                "fatal_count": outcome.metrics.fatal_count,
                "underruns": outcome.metrics.underruns,
                "hash": outcome.hash_hex(),
                "buffer": args.buffer.as_str(),
            });
            write_metrics(
                &metrics_dir,
                &serde_json::to_string_pretty(&json).expect("json"),
            )
            .await;
            println!(
                "[ref_receiver] complete: frames={} loss={} dup={} reorder={} late={} hash={}",
                outcome.metrics.packets_recv,
                outcome.metrics.loss,
                outcome.metrics.duplicate,
                outcome.metrics.reorder,
                outcome.metrics.late_discard,
                outcome.hash_hex()
            );
            ExitCode::SUCCESS
        }
        Err(e) => {
            let json = serde_json::json!({
                "role": "receiver",
                "status": "error",
                "profile": profile,
                "packets_recv": 0,
                "loss": { "packets": 0 },
                "duplicate": { "packets": 0 },
                "reorder": { "packets": 0 },
                "late": { "packets": 0 },
                "late_discard": { "packets": 0 },
                "fatal_count": 0,
                "hash": null,
            });
            write_metrics(&metrics_dir, &serde_json::to_string(&json).expect("json")).await;
            eprintln!("[ref_receiver] error: {e}");
            ExitCode::FAILURE
        }
    }
}
