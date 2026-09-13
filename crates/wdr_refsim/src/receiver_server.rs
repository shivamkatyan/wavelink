//! Receiver server: accept **one** emitter connection over quinn and run its
//! media plane through the [`Receiver`] pipeline into a caller-supplied
//! [`RenderSink`] (WS3 — receiver render seam).
//!
//! This is the shared machinery behind both `ref_receiver` (the reference-sim
//! CLI, which injects the null render device) and the shell-facing
//! [`crate::sink::QuicRenderReceiver`] (a desktop/mobile receiver plugs its
//! real output sink in here). Extracted so the two never drift apart — the
//! same golden tests guard both.

use std::net::SocketAddr;

use wdr_proto::{ChannelLayout, Frame};

use crate::framing::{FrameWire, FramedItem, FramingError, MAX_FRAME_TOTAL};
use crate::receiver::{
    guard_frame, BufferProfile, ClockHandle, NullRenderSink, Receiver, ReceiverError,
    ReceiverOutcome,
};
use crate::sink::RenderSink;

/// Bounded wait for the end-of-stream marker (ms) — never sleep-and-assume.
pub const END_POLL_MS: u64 = 30_000;

/// Loopback-only QUIC server config (self-signed `localhost` cert via rcgen).
/// The production receiver uses the platform trust store + fingerprint
/// pinning; this is the reference/live-shell identity.
pub fn loopback_server_config() -> quinn::ServerConfig {
    let cert = rcgen::generate_simple_self_signed(vec!["localhost".into()]).expect("loopback cert");
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

/// Build a `BufferMeta` from the first observed frame (the receiver sniffs
/// wire metadata from the first frame rather than out-of-band).
pub fn sniff_meta(frame: &Frame) -> crate::emitter::BufferMeta {
    crate::emitter::BufferMeta {
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
pub async fn sniff_first_item(conn: &quinn::Connection) -> Result<(bool, Vec<u8>), ReceiverError> {
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_millis(5_000);
    loop {
        if tokio::time::Instant::now() >= deadline {
            return Err(ReceiverError::EndTimeout);
        }
        if let Ok(Ok(d)) =
            tokio::time::timeout(std::time::Duration::from_millis(50), conn.read_datagram()).await
        {
            return Ok((false, d.to_vec()));
        }
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
                eprintln!("[receiver-server] frame discarded: {e}");
            }
        }
        FramedItem::End { total_frames } => {
            let _ = receiver.ingest_end_marker(total_frames);
        }
    }
}

/// Run the receive loop over the detected lane until the end marker, driving
/// the injected render sink. Used by both `ref_receiver` (null device) and
/// [`crate::sink::QuicRenderReceiver`] (a real shell sink).
pub async fn run_lane(
    conn: &quinn::Connection,
    use_stream: bool,
    first_bytes: Vec<u8>,
    profile: BufferProfile,
    sink: Box<dyn RenderSink + Send>,
) -> Result<ReceiverOutcome, ReceiverError> {
    let first_item = FrameWire::parse(&first_bytes)?;
    let frame = match first_item {
        FramedItem::Audio(f) => f,
        FramedItem::End { .. } => {
            return Err(ReceiverError::Internal("first frame cannot be End".into()))
        }
    };
    guard_frame(&frame)?;
    let meta = sniff_meta(&frame);
    let mut receiver = Receiver::for_stream(meta, profile, ClockHandle::system(), 0)?;
    receiver.set_render_sink(sink);

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
                            eprintln!("[receiver-server] dropped malformed stream item: {e}");
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
        // Datagram lane: the first sniffed datagram is replayed through the
        // same ingest path used by the rest of the stream.
        match receiver.ingest_bytes(&first_bytes) {
            Ok(()) => {}
            Err(e) => eprintln!("[receiver-server] frame discarded: {e}"),
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
                    eprintln!("[receiver-server] frame discarded: {e}");
                }
            }
            if receiver.ended() {
                return Ok(receiver.finalize());
            }
        }
    }
}

/// Accept **one** emitter connection on `endpoint` and run the whole receive
/// loop, delivering into `sink`. Peer-arrival wait is unbounded by default;
/// `WDR_PEER_WAIT_SECS` optionally bounds it for harness/CI use.
pub async fn run_listener(
    endpoint: quinn::Endpoint,
    profile: BufferProfile,
    sink: Box<dyn RenderSink + Send>,
) -> Result<ReceiverOutcome, ReceiverError> {
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

    let (use_stream, first_bytes) = sniff_first_item(&conn).await?;
    run_lane(&conn, use_stream, first_bytes, profile, sink).await
}

/// Convenience for callers that own no endpoint (binds `addr` first).
pub async fn listen_and_run(
    addr: SocketAddr,
    profile: BufferProfile,
    sink: Box<dyn RenderSink + Send>,
) -> Result<ReceiverOutcome, ReceiverError> {
    let config = loopback_server_config();
    let endpoint = quinn::Endpoint::server(config, addr)
        .map_err(|e| ReceiverError::Internal(format!("bind {addr}: {e}")))?;
    let outcome = run_listener(endpoint, profile, sink).await;
    outcome
}

/// The reference-sim null render device, boxed for the shared lane helpers.
pub fn null_render_sink() -> Box<dyn RenderSink + Send> {
    Box::new(NullRenderSink::new(ClockHandle::system(), 0))
}
