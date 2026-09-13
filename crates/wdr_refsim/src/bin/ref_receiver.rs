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
//!
//! The receiver listens on `<addr>` for a QUIC connection from the reference
//! emitter, reads the media plane (reliable stream for lossless, datagrams for
//! lossy), runs it through the shared pipeline in `wdr_refsim::receiver`, and
//! writes `receiver-sim.json` (role, status, `packets_recv`, loss, duplicate,
//! reorder, `late_discard`, `fatal_count`, hash, …).
//!
//! Exit: `0` on a clean end-of-stream (hash + metrics flushed); non-zero on
//! error and on end-of-stream timeout (bounded poll, never sleep-and-assume).

use std::net::SocketAddr;
use std::process::ExitCode;

use wdr_proto::ChannelLayout;
use wdr_refsim::emitter::BufferMeta;
use wdr_refsim::framing::{FrameWire, FramedItem, FramingError, MAX_FRAME_TOTAL};
use wdr_refsim::receiver::{BufferProfile, ClockHandle, Receiver, ReceiverError, ReceiverOutcome};

/// Bounded wait for the end-of-stream marker (ms) — never sleep-and-assume.
const END_POLL_MS: u64 = 30_000;

struct Args {
    buffer: BufferProfile,
    addr: SocketAddr,
}

fn usage() -> &'static str {
    "usage: ref_receiver --role receiver [--buffer low|balanced|resilient] [<listen-addr>]\n\
     env:  WDR_SIM_ROLE=receiver  WDR_METRICS_DIR=<dir>  WDR_NETEM_PROFILE=<profile>\n\
     \tWDR_RECEIVER_ADDR=<addr> (default 0.0.0.0:9000)"
}

fn parse_args() -> Result<Args, String> {
    let mut role = std::env::var("WDR_SIM_ROLE").ok();
    let mut buffer = BufferProfile::Balanced;
    let mut positional = Vec::new();

    let mut it = std::env::args().skip(1).peekable();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--role" => role = Some(it.next().ok_or("--role needs a value")?),
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
    Ok(Args { buffer, addr })
}

/// Loopback-only QUIC server config (self-signed `localhost` cert via rcgen).
/// The production receiver uses the platform trust store + fingerprint
/// pinning; this is the reference-sim identity.
fn server_config() -> quinn::ServerConfig {
    let cert = rcgen::generate_simple_self_signed(vec!["localhost".into()])
        .expect("loopback cert generation (reference sim)");
    let key = quinn::rustls::pki_types::PrivateKeyDer::Pkcs8(
        quinn::rustls::pki_types::PrivatePkcs8KeyDer::from(cert.signing_key.serialize_der()),
    );
    let mut sc = quinn::ServerConfig::with_single_cert(vec![cert.cert.der().clone()], key)
        .expect("quinn server config from loopback cert");
    let mut tc = quinn::TransportConfig::default();
    let mut mtud = quinn::MtuDiscoveryConfig::default();
    mtud.upper_bound(wdr_transport::wire::PEER_MAX_UDP_PAYLOAD_SIZE);
    tc.mtu_discovery_config(Some(mtud));
    sc.transport_config(std::sync::Arc::new(tc));
    sc
}

/// Build a `BufferMeta` from the first observed frame.
fn sniff_meta(frame: &wdr_proto::Frame) -> BufferMeta {
    BufferMeta {
        codec: frame.codec,
        sample_rate: frame.sample_rate,
        channels: match frame.channel_layout {
            ChannelLayout::Mono => 1,
            _ => 2,
        },
        sample_repr: frame.sample_repr,
        channel_layout: frame.channel_layout,
        frame_samples: frame.frame_sample_count as usize,
    }
}

/// Read one length-prefixed reliable-stream item.
async fn read_one_stream_item(recv: &mut quinn::RecvStream) -> Result<Vec<u8>, ReceiverError> {
    let mut len_buf = [0u8; 2];
    read_exact_timeout(recv, &mut len_buf).await?;
    let len = u16::from_le_bytes(len_buf) as usize;
    if len > MAX_FRAME_TOTAL + 13 {
        return Err(ReceiverError::Framing(FramingError::Bounds));
    }
    let mut body = vec![0u8; len];
    read_exact_timeout(recv, &mut body).await?;
    Ok(body)
}

async fn read_exact_timeout(
    recv: &mut quinn::RecvStream,
    buf: &mut [u8],
) -> Result<(), ReceiverError> {
    let mut off = 0;
    while off < buf.len() {
        let n = tokio::time::timeout(
            std::time::Duration::from_millis(5_000),
            recv.read(&mut buf[off..]),
        )
        .await
        .map_err(|_| ReceiverError::EndTimeout)?
        .map_err(|_| ReceiverError::EndTimeout)?;
        match n {
            Some(0) | None => return Err(ReceiverError::EndTimeout),
            Some(n) => off += n,
        }
    }
    Ok(())
}

/// Probe for the first unit of traffic: either a datagram or a reliable-stream
/// open carrying the first item. Returns `(use_stream, bytes)`.
async fn sniff_first_item(conn: &quinn::Connection) -> Result<(bool, Vec<u8>), ReceiverError> {
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_millis(5_000);
    loop {
        if tokio::time::Instant::now() >= deadline {
            return Err(ReceiverError::EndTimeout);
        }
        // Datagrams first (lossy path is datagram-primary).
        if let Ok(Ok(d)) =
            tokio::time::timeout(std::time::Duration::from_millis(50), conn.read_datagram()).await
        {
            return Ok((false, d.to_vec()));
        }
        // Stream: accept_bi with a timeout; the first inbound stream carries
        // the first item.
        match tokio::time::timeout(std::time::Duration::from_millis(250), conn.accept_bi()).await {
            Ok(Ok((_, mut recv))) => {
                let item = read_one_stream_item(&mut recv).await?;
                return Ok((true, item));
            }
            Ok(Err(_)) => return Err(ReceiverError::Internal("accept_bi failed".into())),
            Err(_) => continue,
        }
    }
}

fn push_item(receiver: &mut Receiver, item: FramedItem) {
    match item {
        FramedItem::Audio(frame) => {
            if let Err(e) = receiver.ingest_frame_direct(frame) {
                eprintln!("[ref_receiver] frame discarded: {e}");
            }
        }
        FramedItem::End { total_frames } => {
            let _ = receiver.ingest_end_marker(total_frames);
        }
    }
}

/// Run the receive loop over the detected lane until the end marker.
async fn run_lane(
    conn: &quinn::Connection,
    use_stream: bool,
    first_bytes: Vec<u8>,
    profile: BufferProfile,
) -> Result<ReceiverOutcome, ReceiverError> {
    let first_item = FrameWire::parse(&first_bytes)?;
    let frame = match first_item {
        FramedItem::Audio(f) => f,
        FramedItem::End { .. } => {
            return Err(ReceiverError::Internal("first frame cannot be End".into()))
        }
    };
    wdr_refsim::receiver::guard_frame(&frame)?;
    let meta = sniff_meta(&frame);
    let mut receiver = Receiver::for_stream(meta, profile, ClockHandle::system(), 0)?;

    if use_stream {
        push_item(&mut receiver, FramedItem::Audio(frame));
        if receiver.ended() {
            return Ok(receiver.finalize());
        }
        loop {
            match tokio::time::timeout(
                std::time::Duration::from_millis(END_POLL_MS),
                conn.accept_bi(),
            )
            .await
            {
                Ok(Ok((_, mut recv))) => {
                    let mut buf = Vec::new();
                    let mut chunk = [0u8; 4096];
                    loop {
                        let n = tokio::time::timeout(
                            std::time::Duration::from_millis(END_POLL_MS),
                            recv.read(&mut chunk),
                        )
                        .await
                        .map_err(|_| ReceiverError::EndTimeout)?
                        .map_err(|_f| ReceiverError::EndTimeout)?;
                        match n {
                            Some(0) | None => break,
                            Some(n) => buf.extend_from_slice(&chunk[..n]),
                        }
                        let (items, err) = FrameWire::take_stream_items(&mut buf);
                        if let Some(e) = err {
                            receiver.metrics_mut().malformed += 1;
                            eprintln!("[ref_receiver] dropped malformed stream item: {e}");
                        }
                        for item in items {
                            push_item(&mut receiver, item);
                        }
                        if receiver.ended() {
                            return Ok(receiver.finalize());
                        }
                    }
                }
                Ok(Err(_)) => return Err(ReceiverError::Internal("accept_bi failed".into())),
                Err(_) => return Err(ReceiverError::EndTimeout),
            }
        }
    } else {
        // Datagram lane: the first sniffed datagram is replayed through the same
        // ingest path used by the rest of the stream.
        match receiver.ingest_bytes(&first_bytes) {
            Ok(()) => {}
            Err(e) => eprintln!("[ref_receiver] frame discarded: {e}"),
        }
        if receiver.ended() {
            return Ok(receiver.finalize());
        }
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_millis(END_POLL_MS);
        loop {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                return Err(ReceiverError::EndTimeout);
            }
            let d = tokio::time::timeout(remaining, conn.read_datagram())
                .await
                .map_err(|_| ReceiverError::EndTimeout)?
                .map_err(|_f| ReceiverError::EndTimeout)?;
            match receiver.ingest_bytes(&d) {
                Ok(()) => {}
                Err(e) => {
                    eprintln!("[ref_receiver] frame discarded: {e}");
                }
            }
            if receiver.ended() {
                return Ok(receiver.finalize());
            }
        }
    }
}

async fn run_listener(
    addr: SocketAddr,
    profile: BufferProfile,
) -> Result<ReceiverOutcome, ReceiverError> {
    let config = server_config();
    let endpoint = quinn::Endpoint::server(config, addr)
        .map_err(|e| ReceiverError::Internal(format!("bind {addr}: {e}")))?;
    eprintln!("[ref_receiver] listening on {addr} (buffer={:?})", profile);

    // Peer-arrival wait is *unbounded by default* (a receiver must wait for a
    // peer that may connect later). `WDR_PEER_WAIT_SECS` optionally bounds it
    // for harness/CI use; the active-session end-marker wait stays bounded.
    let peer_wait = std::env::var("WDR_PEER_WAIT_SECS")
        .ok()
        .and_then(|s| s.trim().parse::<u64>().ok())
        .unwrap_or(0);
    let incoming = match peer_wait {
        0 => endpoint.accept().await,
        secs => tokio::time::timeout(std::time::Duration::from_secs(secs), endpoint.accept())
            .await
            .map_err(|_| ReceiverError::EndTimeout)?,
    }
    .ok_or_else(|| ReceiverError::Internal("accept returned None".into()))?;
    let conn = incoming
        .accept()
        .map_err(|_| ReceiverError::Internal("incoming accept failed".into()))?
        .await
        .map_err(|_| ReceiverError::Internal("handshake failed".into()))?;
    eprintln!("[ref_receiver] connected; reading media plane");

    let (use_stream, first_bytes) = sniff_first_item(&conn).await?;
    run_lane(&conn, use_stream, first_bytes, profile).await
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

    let result = run_listener(args.addr, args.buffer).await;
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
