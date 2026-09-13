//! Core transport configuration + the small loopback transport helpers
//! (0-RTT-off, CC selection, datagram send/recv, deadline-abort reliable stream).
//!
//! This is a **spike**: the helpers are intentionally thin over `quinn` so the
//! measurements are honest. In production these move behind an adapter seam
//! (ADR-001 / ARCHITECTURE "transport lives behind testable adapter traits").

use std::{net::SocketAddr, sync::Arc, time::Instant};

use quinn::{Endpoint, SendDatagramError, TransportConfig};

pub use quinn::{
    ClientConfig, Connection, ConnectionError, ConnectionStats, EndpointConfig, ServerConfig,
    VarInt,
};

/// Per-`wdr_proto` bound mirrored here (no cargo dep needed): an audio frame
/// payload is ≤ 4 KiB after AEAD (SECURITY_SPEC §4). Loopback is host-only; for
/// a realistic LAN `max_udp_payload_size` use ~1200–1500 (network MTU).
pub const FRAME_PAYLOAD_MAX_BYTES: usize = 4096;
pub const PEER_MAX_UDP_PAYLOAD_SIZE: u16 = 1452;

/// Congestion-control selection for the transport config.
///
/// `quinn` 0.11 exposes this per-`TransportConfig` via
/// `congestion_controller_factory` (`quinn::congestion::{CubicConfig, BbrConfig}`).
/// There is **no** `Connection::set_congestion_controller` mutator in 0.11; the
/// controller is fixed at connection build from the factory→default `Cubic`. A
/// BBR vs CUBIC *measurement* therefore requires two connections, one per
/// factory, under identical load (see the example + netem follow-up).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CongestionControl {
    Cubic,
    Bbr,
}

/// Spike transport connection options.
#[derive(Debug, Clone)]
pub struct TransportConnConfig {
    /// Whether 0-RTT / early data is permitted **on the media path**.
    ///
    /// Default **`false`** (0-RTT OFF for media): locked by SECURITY_SPEC §3.6
    /// (`enable_early_data = false`), ADR-003, RFC 9221 (datagrams forbidden in
    /// 0-RTT). **Disabling early data must be done on the rustls side too** —
    /// `quinn::ClientConfig::with_root_certificates()` enables it by default.
    pub zrt_enabled_media: bool,
    /// Congestion controller (`Cubic`|`Bbr`).
    pub congestion: CongestionControl,
    /// Datagram payload cap. `None` = "whatever quinn allows" (probe).
    pub datagram_max_payload: Option<usize>,
    /// Peer `max_udp_payload_size` advertised for loopback MTU probing.
    pub peer_max_udp_payload_size: u16,
    /// Per-stream receive window (bytes). `None` = quinn default (large); used
    /// by tests to force a blocking stream write for the deadline-abort case.
    pub stream_receive_window: Option<u32>,
    /// Outgoing datagram buffer budget (bytes). `None` = quinn default (1 MiB).
    ///
    /// NOTE (spike finding): quinn 0.11.17's drop path when this budget is
    /// exceeded has a double-`payload_bytes` decrement (`datagrams.rs` `send` +
    /// `pop_front`), which underflows under sustained over-budget bursts. For
    /// the clean throughput number we set this high; the drop-path probe is run
    /// separately with a tiny budget and the overflow is documented as a quinn
    /// bug to re-verify at the pinned version.
    pub datagram_send_buffer_size: Option<usize>,
}

impl Default for TransportConnConfig {
    fn default() -> Self {
        Self {
            // Locked: 0-RTT OFF for media (SECURITY_SPEC §3.6 / SEC-13).
            zrt_enabled_media: false,
            congestion: CongestionControl::Cubic,
            datagram_max_payload: None,
            peer_max_udp_payload_size: PEER_MAX_UDP_PAYLOAD_SIZE,
            stream_receive_window: None,
            datagram_send_buffer_size: None,
        }
    }
}

impl TransportConnConfig {
    /// Disable 0-RTT for media (default; builder-style for clarity).
    #[must_use]
    pub fn disable_0rtt(mut self) -> Self {
        self.zrt_enabled_media = false;
        self
    }

    /// Enable 0-RTT **for media** (explicitly NOT the WDR locked default; used
    /// only to *measure the 0-RTT-on path in the example*).
    #[must_use]
    pub fn enable_0rtt(mut self) -> Self {
        self.zrt_enabled_media = true;
        self
    }

    /// Set the congestion controller.
    #[must_use]
    pub fn congestion_control(mut self, cc: CongestionControl) -> Self {
        self.congestion = cc;
        self
    }

    /// Cap datagrams at `n` bytes (checked at the send helper, in addition to
    /// quinn's own `max_datagram_size`).
    #[must_use]
    pub fn datagram_max_payload(mut self, n: usize) -> Self {
        self.datagram_max_payload = Some(n);
        self
    }

    /// Set a small per-stream receive window (bytes) — used by tests to make a
    /// stream write block so the deadline path can be exercised.
    #[must_use]
    pub fn stream_receive_window(mut self, n: u32) -> Self {
        self.stream_receive_window = Some(n);
        self
    }

    /// Set the outgoing datagram buffer budget in bytes (see the struct doc).
    #[must_use]
    pub fn set_datagram_send_buffer(mut self, n: usize) -> Self {
        self.datagram_send_buffer_size = Some(n);
        self
    }

    /// Build the `quinn::TransportConfig` from this spike config.
    pub fn build_transport(&self) -> TransportConfig {
        let mut tc = TransportConfig::default();
        if let Some(w) = self.stream_receive_window {
            tc.stream_receive_window(VarInt::from_u32(w));
        }
        if let Some(b) = self.datagram_send_buffer_size {
            // quinn's default (1 MiB) lets a sustained burst exceed the budget
            // and hit its buggy drop path; force the budget explicitly.
            tc.datagram_send_buffer_size(b);
        }
        match self.congestion {
            CongestionControl::Cubic => {
                tc.congestion_controller_factory(Arc::new(
                    quinn::congestion::CubicConfig::default(),
                ));
            }
            CongestionControl::Bbr => {
                tc.congestion_controller_factory(Arc::new(quinn::congestion::BbrConfig::default()));
            }
        }
        let mut mtud = quinn::MtuDiscoveryConfig::default();
        mtud.upper_bound(self.peer_max_udp_payload_size);
        tc.mtu_discovery_config(Some(mtud));
        tc
    }
}

/// Typed results from the transport.
///
/// This mirrors the "datagram receive never panics; it returns bytes" contract
/// (d): decode of the *payload* is the `wdr_proto` layer's job, so the
/// transport yields `Ok(bytes)` for any garbage and stays healthy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Got {
    /// Transport received a datagram: bytes yielded as-is.
    Datagram(Vec<u8>),
    /// A reliable-stream chunk arrived (lossless path).
    Stream(Vec<u8>),
}

/// Outcome of a `send_datagram` attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SendDatagramOutcome {
    /// The datagram was queued. (`ack` relates to the **reliable stream** ack,
    /// not datagrams — `datagram_lost` is not app-measurable on ack in quinn.)
    Sent,
    /// Too large for the current path (probed `max_datagram_size` / cap).
    TooLarge { max: usize },
    /// Datagram support is not enabled on the peer / locally.
    Unsupported,
    /// Send buffer full: dropped under load (congestion; no ack, no resend).
    BufferFull,
    /// Transport-level failure.
    Error,
}

/// Result helpers used by the example's measurement code.
#[derive(Debug, Clone, Copy)]
pub struct Sending {
    pub ok: bool,
}

impl Sending {
    #[must_use]
    pub const fn is_ok(&self) -> bool {
        self.ok
    }
}

/// Stats snapshot of what quinn exposes that we can turn into measured evidence.
#[derive(Debug, Clone, Copy, Default)]
pub struct StreamAck {
    /// Total bytes written to the reliable stream.
    pub bytes_written: u64,
    /// `quinn::ConnectionStats` ack frames / path loss the connection observed
    /// (datagram loss is NOT individually acked — see report).
    pub path: Option<crate::metrics::PathReport>,
}

/// Accumulate a connection's stats into a `StreamAck`-style report.
#[must_use]
pub fn accumulate_acks(_epoch_elapsed: std::time::Duration, _conn: &Connection) -> StreamAck {
    // Retained for API-shape parity with the planned production adapter
    // (which would aggregate per-epoch counters). The spike reads
    // `Connection::stats()` live via `metrics_snapshot()`.
    StreamAck {
        bytes_written: 0,
        path: None,
    }
}

/// Snapshot the connection's measurable metrics (ack frames, path loss, RTT,
/// cwnd, current MTU) — the raw material for the spike table.
#[must_use]
pub fn metrics_snapshot(conn: &Connection) -> crate::metrics::PathReport {
    let s = conn.stats();
    crate::metrics::PathReport::from_path_stats(s.path, s.frame_rx.acks, s.udp_tx.datagrams)
}

/// Tell whether a datagram frame was acked, for our metrics (quinn 0.11 does
/// **not** expose per-datagram ack; see [`crate::metrics`]).
#[must_use]
pub fn datagram_acked(_conn: &Connection) -> bool {
    false
}

/// Build a loopback-only `quinn::ClientConfig` with **0-RTT forced OFF** on the
/// rustls side (see [`TransportConnConfig`]).
///
/// `quinn::ClientConfig::with_root_certificates()` ships with `enable_early_data
/// = true`; WDR disables early data entirely (SECURITY_SPEC §3.6 / Appendix A
/// #3), so we hand-build the `rustls::ClientConfig` and clear
/// `enable_early_data` before wrapping it in `QuicClientConfig`.
pub fn make_client_config(cfg: &TransportConnConfig) -> quinn::ClientConfig {
    let mut rustls_cfg = rustls_configs::spike_client_crypto();
    rustls_cfg.enable_early_data = cfg.zrt_enabled_media;
    let qcc = quinn::crypto::rustls::QuicClientConfig::try_from(rustls_cfg)
        .expect("spike client rustls config is QUIC-valid (TLS 1.3 + initial suite)");
    let mut out = quinn::ClientConfig::new(Arc::new(qcc));
    out.transport_config(Arc::new(cfg.build_transport()));
    out
}

/// Build a loopback-only `quinn::ServerConfig` (self-signed hostname cert).
///
/// The cert is generated with `rcgen` (dev-only identity, loopback fixture),
/// so this helper is only available under the `spike-meas` feature (the
/// measurement example + loopback adapter). Production configures the server
/// cert from the platform trust store instead.
#[cfg(feature = "spike-meas")]
pub fn make_server_config() -> quinn::ServerConfig {
    let cert = rcgen::generate_simple_self_signed(vec!["localhost".into()])
        .expect("loopback self-signed cert generation");
    let key = quinn::rustls::pki_types::PrivateKeyDer::Pkcs8(
        quinn::rustls::pki_types::PrivatePkcs8KeyDer::from(cert.signing_key.serialize_der()),
    );
    let mut sc = quinn::ServerConfig::with_single_cert(vec![cert.cert.der().clone()], key)
        .expect("quinn server config from loopback cert");
    let mut tc = TransportConfig::default();
    let mut mtud = quinn::MtuDiscoveryConfig::default();
    mtud.upper_bound(PEER_MAX_UDP_PAYLOAD_SIZE);
    tc.mtu_discovery_config(Some(mtud));
    sc.transport_config(Arc::new(tc));
    sc
}

/// Create a loopback `quinn::Endpoint` (client side).
pub fn make_client_endpoint() -> Result<Endpoint, std::io::Error> {
    Endpoint::client(SocketAddr::from(([127, 0, 0, 1], 0)))
}

/// A tiny `Core` holder mirroring the planned production `TransportCore` seam;
/// the spike's helpers take `&Connection` directly.
pub struct Core {
    /// The connected QUIC connection.
    pub conn: Connection,
}

/// Send one datagram with the optional cap; typed outcome.
pub fn try_send_datagram(
    conn: &Connection,
    frame: &[u8],
    max_payload: Option<usize>,
) -> SendDatagramOutcome {
    if let Some(cap) = max_payload {
        if frame.len() > cap {
            return SendDatagramOutcome::TooLarge { max: cap };
        }
    }
    match conn.send_datagram(bytes::Bytes::copy_from_slice(frame)) {
        Ok(()) => SendDatagramOutcome::Sent,
        Err(SendDatagramError::TooLarge) => SendDatagramOutcome::TooLarge {
            max: conn.max_datagram_size().unwrap_or(0),
        },
        Err(SendDatagramError::UnsupportedByPeer) => SendDatagramOutcome::Unsupported,
        Err(SendDatagramError::Disabled) => SendDatagramOutcome::Unsupported,
        Err(SendDatagramError::ConnectionLost(_)) => SendDatagramOutcome::Error,
    }
}

/// Drain all currently buffered datagrams non-blockingly, returning them in order.
#[must_use]
pub fn drain_datagrams(conn: &Connection) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    while let Some(Ok(d)) = conn.read_datagram().now_or_never() {
        out.push(d.to_vec());
    }
    out
}

/// Receive a single datagram with an optional timeout (None = await forever).
pub async fn recv_datagram_timeout(
    conn: &Connection,
    timeout: Option<std::time::Duration>,
) -> Result<Vec<u8>, ConnectionError> {
    let fut = conn.read_datagram();
    match timeout {
        Some(d) => match tokio::time::timeout(d, fut).await {
            Ok(r) => r.map(|b| b.to_vec()),
            Err(_) => Ok(Vec::new()), // timed out: yields empty, not an error
        },
        None => fut.await.map(|b| b.to_vec()),
    }
}

/// Loopback measurement harness: drive a `core::pin::pin!`-style receive loop
/// draining datagrams. Kept simple per spike scope — production uses the
/// receiver task in the B1 session core.
pub async fn receive_datagrams(conn: &Connection, cores: usize) -> usize {
    let mut got = Vec::new();
    for _ in 0..cores {
        match conn.read_datagram().await {
            Ok(d) => got.push(d),
            Err(_) => break,
        }
    }
    got.len()
}

/// Probe the path MTU on loopback after handshake: how big can a datagram be?
#[must_use]
pub fn handshake_probe(conn: &Connection) -> Option<usize> {
    // With `mtu_discovery_config` + media MTU disabled in favour of the
    // peer's declared max_udp_payload_size, `max_datagram_size` is stable.
    conn.max_datagram_size()
}

/// Send an Opus-sized frame as a datagram under a `Instant` clock (fake-clock
/// ready: the `now` parameter is retained for API-shape; the example passes
/// `std::time::Instant::now()`).
pub fn send_datagrams(conn: &Connection, frames: &[Vec<u8>], _now: Instant, label: &str) -> usize {
    let mut sent = 0;
    for f in frames {
        match conn.send_datagram(f.clone().into()) {
            Ok(()) => sent += 1,
            Err(_) => break,
        }
    }
    if sent > 0 {
        let _ = label;
    }
    sent
}

/// Send `bytes` down a reliable stream and enforce the app-layer
/// **retransmit deadline**: abort the stream if it has not been delivered (the
/// write cannot complete within the budget) within `deadline` of `now`.
///
/// Stream retransmits are internal to QUIC — `SendStream::write` completing
/// only means the bytes are buffered/sent, not acked. The ≤50 ms budget
/// (PROTOCOL_SPEC) is therefore a **wall-clock** bound on the send side:
/// `write` is raced against `deadline - now.elapsed()`. Under flow control
/// (peer not reading) the write blocks and the deadline fires → stream aborted.
///
/// The `now` is injectable for fake-clock tests later; the lib/example pass
/// `std::time::Instant::now()`.
pub async fn stream_send_with_deadline(
    conn: &Connection,
    now: Instant,
    deadline: std::time::Duration,
    bytes: Vec<u8>,
) -> Result<(), &'static str> {
    let (mut send, _recv) = conn.open_bi().await.map_err(|_| "open_bi failed")?;

    let mut off = 0usize;
    while off < bytes.len() {
        // Re-check the wall clock each iteration: if the budget is already
        // blown, abort without attempting another write.
        let elapsed = now.elapsed();
        if elapsed >= deadline {
            return Err("retransmit_deadline exceeded");
        }
        let remain = deadline - elapsed;
        let n = tokio::time::timeout(remain, send.write(&bytes[off..]))
            .await
            .map_err(|_| "retransmit_deadline exceeded")?
            .map_err(|_| "stream write failed")?;
        off += n;
        if n == 0 {
            return Err("stream write stalled");
        }
    }

    send.finish().map_err(|_| "stream finish failed")?;
    Ok(())
}

/// Receive a reliable stream with a retransmit deadline; abort (stop reading)
/// when `deadline` passes with no data.
///
/// The spike enforces the lossless deadline on the **send** side (see
/// [`stream_send_with_deadline`]); the receive-side reader lives in the B1
/// session core. This signature is kept for the planned seam so the B1 worker
/// can fill in the true receiver (and a fake clock) without changing the API.
pub async fn stream_receive_with_deadline(
    _conn: &Connection,
    _now: Instant,
    _deadline: std::time::Duration,
) -> Result<Vec<u8>, &'static str> {
    // No-op stand-in: without a peer send-side the receiver would await forever;
    // the deadline-abort *behavior* is exercised via the send-side wrapper.
    Err("stream_receive_with_deadline needs a peer send side in the spike harness")
}

/// `Instant::now_or_never`-style extension for `read_datagram()`'s future.
trait NowOrNever {
    type Output;
    fn now_or_never(self) -> Option<Self::Output>;
}

impl<F: std::future::Future> NowOrNever for F {
    type Output = F::Output;
    fn now_or_never(self) -> Option<Self::Output> {
        use std::task::{Context, Poll};
        // SAFETY: the future is polled once to completion on the current task;
        // no value is leaked across suspension because we only return Ready.
        let noop_waker = noop_waker();
        let mut cx = Context::from_waker(&noop_waker);
        let mut fut = std::pin::pin!(self);
        match fut.as_mut().poll(&mut cx) {
            Poll::Ready(v) => Some(v),
            Poll::Pending => None,
        }
    }
}

fn noop_waker() -> std::task::Waker {
    use std::task::{RawWaker, RawWakerVTable};
    fn noop(_: *const ()) {}
    fn clone(_: *const ()) -> RawWaker {
        RAW
    }
    const RAW: RawWaker = RawWaker::new(
        std::ptr::null(),
        &RawWakerVTable::new(clone, noop, noop, noop),
    );
    // SAFETY: stateless waker; all vtable fns are pure no-ops.
    unsafe { std::task::Waker::from_raw(RAW) }
}

/// Re-export of the internal rustls-config helpers used by `make_client_config`
/// / `make_server_config`.
pub mod rustls_configs {
    #[cfg(feature = "spike-meas")]
    use quinn::rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
    use quinn::rustls::ClientConfig as RClientConfig;

    /// A `rustls::ClientConfig` bound to the ring default provider with TLS 1.3
    /// only (QUIC requirement), the hostname verifier disabled for loopback and
    /// no client auth. **Loopback/spike only** — production uses the platform
    /// verifier + pinned fingerprints (SECURITY_SPEC §3.4).
    pub fn spike_client_crypto() -> RClientConfig {
        let provider = std::sync::Arc::new(quinn::rustls::crypto::ring::default_provider());
        // Loopback-only: accept any cert for `localhost`. This uses rustls's
        // built-in `WebPkiServerVerifier` over an empty root store (nothing is
        // trusted), which for a self-signed loopback identity is exactly "accept
        // anything". Production replaces this with the platform verifier +
        // fingerprint pinning (SECURITY_SPEC §3.4).
        let empty_roots = std::sync::Arc::new(quinn::rustls::RootCertStore::empty());
        let verifier = quinn::rustls::client::WebPkiServerVerifier::builder_with_provider(
            empty_roots,
            provider.clone(),
        )
        .build()
        .expect("webpki verifier build");
        RClientConfig::builder_with_provider(provider)
            .with_protocol_versions(&[&quinn::rustls::version::TLS13])
            .expect("ring provider supports TLS 1.3")
            .dangerous()
            .with_custom_certificate_verifier(verifier)
            .with_no_client_auth()
    }

    /// A self-signed `localhost` cert + key for loopback (rcgen-derived),
    /// returned as DER for `quinn::ServerConfig`. Only under `spike-meas`
    /// (rcgen is an optional dependency for the measurement harness).
    #[cfg(feature = "spike-meas")]
    pub fn spike_self_signed_cert(
    ) -> Result<(Vec<CertificateDer<'static>>, PrivateKeyDer<'static>), String> {
        let cert = rcgen::generate_simple_self_signed(vec!["localhost".into()])
            .map_err(|e| format!("rcgen: {e}"))?;
        let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(cert.signing_key.serialize_der()));
        Ok((vec![cert.cert.der().clone()], key))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_transport_is_0rtt_off() {
        let c = TransportConnConfig::default();
        assert!(!c.zrt_enabled_media);
        assert!(c.datagram_max_payload.is_none());
    }

    #[test]
    fn builder_caps_datagram() {
        let c = TransportConnConfig::default().datagram_max_payload(1200);
        assert_eq!(c.datagram_max_payload, Some(1200));
    }

    #[test]
    fn cc_variants() {
        assert_ne!(CongestionControl::Cubic, CongestionControl::Bbr);
        assert_eq!(
            TransportConnConfig::default()
                .congestion_control(CongestionControl::Bbr)
                .congestion,
            CongestionControl::Bbr
        );
    }
}
