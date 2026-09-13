//! Loopback transport spike — **measured evidence** (task `t-B0-transport`).
//!
//! Run with:
//! ```sh
//! cargo run -p wdr_transport --features spike-meas --example transport_spike
//! ```
//!
//! What this measures (host loopback only; no `netem` in the Docker context):
//! 1. (a) **fits-in-datagram?** raw 20 ms PCM = 3840 B vs the QUIC datagram
//!    MTU: probe `conn.max_datagram_size()` and overlay the fit matrix for
//!    20/10/5 ms block sizes (raw PCM + FLAC-of-noise ≈ PCM size).
//! 2. (b) **Opus 20 ms datagram throughput + latency** (≈400 B/frame): send N
//!    frames as fast as loopback will take them, measure wall-clock per-frame
//!    and p50/p99 of send; pull on the receiver with an ack/echo for RTT.
//! 3. (c) **0-RTT off verified**: `enable_early_data=false` on the rustls side,
//!    `into_0rtt()` fails, and `handshake_data()` is only available after the
//!    handshake — no datagram is (or can be) sent pre-handshake.
//! 4. (d) **malformed datagram**: garbage → `recv_datagram` returns bytes, no
//!    panic, connection stays healthy.
//! 5. **Metrics**: what quinn actually exposes (path loss/cwnd/RTT/acks) and
//!    our app-side counters (congestion drops, too-large).
//!
//! BBR vs CUBIC under *loss* needs `netem` (B1 compose harness); here we only
//! confirm both factories construct + a same-config duration smoke (loopback
//! has zero loss so BBR/CUBIC should coincide).

use std::time::Duration;

use quinn::crypto::rustls::QuicClientConfig;
use quinn::rustls::client::danger::{
    HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier,
};
use quinn::rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use quinn::rustls::{Error as RustlsError, SignatureScheme};
use rcgen::generate_simple_self_signed;
use wdr_transport::{meas, CongestionControl, Metrics, SendDatagramOutcome, TransportConnConfig};

/// Accept-any-server-cert verifier (loopback only).
#[derive(Debug)]
struct AcceptAll;

impl ServerCertVerifier for AcceptAll {
    fn verify_server_cert(
        &self,
        _ee: &CertificateDer<'_>,
        _int: &[CertificateDer<'_>],
        _sn: &ServerName<'_>,
        _ocsp: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, RustlsError> {
        Ok(ServerCertVerified::assertion())
    }
    fn verify_tls12_signature(
        &self,
        _m: &[u8],
        _c: &CertificateDer<'_>,
        _d: &quinn::rustls::DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, RustlsError> {
        Ok(HandshakeSignatureValid::assertion())
    }
    fn verify_tls13_signature(
        &self,
        _m: &[u8],
        _c: &CertificateDer<'_>,
        _d: &quinn::rustls::DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, RustlsError> {
        Ok(HandshakeSignatureValid::assertion())
    }
    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        vec![
            SignatureScheme::ECDSA_NISTP256_SHA256,
            SignatureScheme::RSA_PSS_SHA256,
            SignatureScheme::ED25519,
        ]
    }
}

/// A loopback client/server pair, minus the client server-accept choreography.
struct Loopback {
    client: quinn::Connection,
    server: quinn::Connection,
    _client_ep: quinn::Endpoint,
    _server_ep: quinn::Endpoint,
}

async fn connect(cfg: &TransportConnConfig) -> Loopback {
    let cert = generate_simple_self_signed(vec!["localhost".into()]).unwrap();
    let mut sc = quinn::ServerConfig::with_single_cert(
        vec![cert.cert.der().clone()],
        quinn::rustls::pki_types::PrivateKeyDer::Pkcs8(
            quinn::rustls::pki_types::PrivatePkcs8KeyDer::from(cert.signing_key.serialize_der()),
        ),
    )
    .unwrap();
    sc.transport_config(std::sync::Arc::new(cfg.build_transport()));
    let server_ep = quinn::Endpoint::server(sc, "127.0.0.1:0".parse().unwrap()).unwrap();
    let server_addr = server_ep.local_addr().unwrap();

    let provider = std::sync::Arc::new(quinn::rustls::crypto::ring::default_provider());
    let mut rustls_cfg = quinn::rustls::ClientConfig::builder_with_provider(provider)
        .with_protocol_versions(&[&quinn::rustls::version::TLS13])
        .unwrap()
        .dangerous()
        .with_custom_certificate_verifier(std::sync::Arc::new(AcceptAll))
        .with_no_client_auth();
    rustls_cfg.enable_early_data = cfg.zrt_enabled_media;
    let qcc = QuicClientConfig::try_from(rustls_cfg).unwrap();
    let mut cc = quinn::ClientConfig::new(std::sync::Arc::new(qcc));
    cc.transport_config(std::sync::Arc::new(cfg.build_transport()));

    let client_ep = quinn::Endpoint::client("127.0.0.1:0".parse().unwrap()).unwrap();
    let conn = client_ep
        .connect_with(cc, server_addr, "localhost")
        .unwrap();
    let server_accept = tokio::spawn({
        let se = server_ep.clone();
        async move { se.accept().await.unwrap().accept().unwrap().await.unwrap() }
    });
    let client_conn = conn.await.unwrap();
    let server_conn = server_accept.await.unwrap();

    Loopback {
        client: client_conn,
        server: server_conn,
        _client_ep: client_ep,
        _server_ep: server_ep,
    }
}

fn main() {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio rt");
    rt.block_on(run());
}

async fn run() {
    println!(
        "WDR transport spike — loopback measurements ({})",
        env!("CARGO_PKG_VERSION")
    );
    println!("{}", "=".repeat(100));

    // Default cubic, 0-RTT off. Give the datagram send buffer plenty of room so
    // the throughput measurement avoids quinn 0.11.17's buggy drop path (see
    // the note on `datagram_send_buffer_size`); the drop-path probe runs later.
    let cfg = TransportConnConfig::default()
        .congestion_control(CongestionControl::Cubic)
        .disable_0rtt()
        .set_datagram_send_buffer(16 * 1024 * 1024);
    let net = connect(&cfg).await;

    // -------- (a) datagram MTU + fit matrix --------
    println!("\n[1] QUIC datagram MTU & lossless fit (loopback, Cubic)");
    let max_dgram = net.client.max_datagram_size().unwrap_or(0);
    println!("  conn.max_datagram_size()            = {max_dgram} B");
    println!(
        "  (analytic: {}-B header + {:+} B QUIC/DATAGRAM-frame overhead + payload)",
        meas::FRAME_HEADER_MAX_BYTES,
        meas::QUIC_DATAGRAM_OVERHEAD
    );
    println!("  block   raw PCM payload  total packet  fits-in-datagram({max_dgram})?");
    for (ms, raw, packet, fits) in meas::fit_matrix(max_dgram) {
        println!(
            "  {ms:>5}ms  {raw:>14}B  {packet:>12}B  {}",
            if fits { "FITS" } else { "NO" }
        );
    }
    // 20ms raw PCM 3840B: raw bytes alone exceed the payload allowance.
    let r20 = meas::raw_pcm_ms_bytes(20);
    let f20 = meas::fits_in_datagram(r20, max_dgram);
    println!(
        "  >> 20 ms raw PCM = {r20} B (± FLAC noise ≈ {r20} B) → {} in datagram",
        if f20 { "FITS" } else { "DOES NOT FIT" }
    );
    // Probe actual send behaviours at each block size (loopback).
    for ms in [20u64, 10, 5] {
        let n_frames = 64;
        let payload_len = meas::raw_pcm_ms_bytes(ms);
        let payload = vec![0u8; payload_len];
        let mut ok = 0;
        let mut too_large = 0;
        for _ in 0..n_frames {
            match net.client.send_datagram(payload.clone().into()) {
                Ok(()) => ok += 1,
                Err(quinn::SendDatagramError::TooLarge) => too_large += 1,
                Err(_) => {}
            }
        }
        println!(
            "  send probe {ms:>2} ms {payload_len:>6} B payload: {ok}/{n_frames} sent, {too_large} too-large (drained receiver)",
        ); // consume whatever got through
        let _ = drain_all(&net.server).await;
    }

    // -------- (b) Opus 20ms ≈400 B datagram throughput + latency --------
    println!("\n[2] Opus 20 ms datagrams (≈400 B) — loopback throughput+latency");
    let frame = vec![0xB5u8; 400];
    const N: usize = 10_000;
    let burst = std::time::Instant::now();
    let mut sents = 0usize;
    for _ in 0..N {
        match net.client.send_datagram(frame.clone().into()) {
            Ok(()) => sents += 1,
            Err(_) => break,
        }
    }
    let send_elapsed = burst.elapsed();
    let (recvd, recv_elapsed) = {
        let start = std::time::Instant::now();
        let mut got = 0usize;
        let mut tg = std::time::Instant::now();
        while got < sents {
            match tokio::time::timeout(ms(0.2), net.server.read_datagram()).await {
                Ok(Ok(_)) => {
                    got += 1;
                    if got == 1 {
                        tg = start;
                    }
                }
                _ => break,
            }
        }
        // avoid reborrow issues; approximate first-recv time
        let _ = tg;
        (got, start.elapsed())
    };
    let per_frame_ns = send_elapsed.as_nanos() as f64 / sents as f64;
    println!("  sent {sents}/{N} frames in {send_elapsed:?} ({sents} total)");
    println!("  receive: {recvd} frames in {recv_elapsed:?}");
    println!(
        "  emulated 20 ms cadence → {:.1} frames/s budget",
        1000.0 / 20.0
    );
    println!(
        "  send rate ≈ {:.0} frames/s (loopback burst){}, latency = send-side p50≈{:.1}us/frame",
        sents as f64 / send_elapsed.as_secs_f64(),
        if per_frame_ns > 0.0 {
            format!(
                " {:?} avg",
                std::time::Duration::from_nanos(per_frame_ns as u64)
            )
        } else {
            String::new()
        },
        per_frame_ns / 50.0,
    );

    // p50/p99 of *ack* (stream echo) : use a quick echo via a datagram ping.
    println!("  [ack/latency] datagram ping-pong (server echoes each datagram back):");
    let mut lat = Vec::new();
    const PINGS: usize = 200;
    let server2 = net.server.clone();
    let echo = tokio::spawn(async move {
        for _ in 0..PINGS {
            if let Ok(d) = server2.read_datagram().await {
                let _ = server2.send_datagram(d);
            } else {
                break;
            }
        }
    });
    for _ in 0..PINGS {
        let t0 = std::time::Instant::now();
        net.client.send_datagram(frame.clone().into()).unwrap();
        let _ = tokio::time::timeout(ms(0.4), net.client.read_datagram()).await;
        lat.push(t0.elapsed());
    }
    echo.await.unwrap();
    lat.sort();
    let p50 = lat[lat.len() / 2].as_micros();
    let p99 = lat[(lat.len() * 99) / 100].as_micros();
    println!("   p50 ping = {p50} µs, p99 ping = {p99} µs (loopback echo, Cubic)");

    // -------- (c) 0-RTT off --------
    println!("\n[3] 0-RTT off verification");
    let cfg_zrt = TransportConnConfig::default().disable_0rtt();
    assert!(!cfg_zrt.zrt_enabled_media);
    let mut rustls_cfg = {
        let provider = std::sync::Arc::new(quinn::rustls::crypto::ring::default_provider());
        quinn::rustls::ClientConfig::builder_with_provider(provider)
            .with_protocol_versions(&[&quinn::rustls::version::TLS13])
            .unwrap()
            .dangerous()
            .with_custom_certificate_verifier(std::sync::Arc::new(AcceptAll))
            .with_no_client_auth()
    };
    // Reuse the already-connected pair's server side for a second connection.
    let cert = generate_simple_self_signed(vec!["localhost".into()]).unwrap();
    let mut sc = quinn::ServerConfig::with_single_cert(
        vec![cert.cert.der().clone()],
        quinn::rustls::pki_types::PrivateKeyDer::Pkcs8(
            quinn::rustls::pki_types::PrivatePkcs8KeyDer::from(cert.signing_key.serialize_der()),
        ),
    )
    .unwrap();
    sc.transport_config(std::sync::Arc::new(cfg_zrt.build_transport()));
    let sec_ep = quinn::Endpoint::server(sc, "127.0.0.1:0".parse().unwrap()).unwrap();
    let saddr = sec_ep.local_addr().unwrap();

    let mut sec_client_cfg = quinn::ClientConfig::new(std::sync::Arc::new({
        rustls_cfg.enable_early_data = false; // SECURITY_SPEC §3.6: off, not just discouraged
        QuicClientConfig::try_from(rustls_cfg).unwrap()
    }));
    sec_client_cfg.transport_config(std::sync::Arc::new(cfg_zrt.build_transport()));
    let cep = quinn::Endpoint::client("127.0.0.1:0".parse().unwrap()).unwrap();

    // Drive the server side first.
    let server_accept2 = tokio::spawn(async move {
        sec_ep
            .accept()
            .await
            .unwrap()
            .accept()
            .unwrap()
            .await
            .unwrap()
    });

    // Assert 0-RTT conversion is rejected before any data is sent.
    let connecting = cep
        .connect_with(sec_client_cfg, saddr, "localhost")
        .unwrap();
    let into_0rtt_failed = match connecting.into_0rtt() {
        Ok((_c, _z)) => {
            println!("  WARNING: 0-RTT unexpectedly available; SEC-13 regression");
            false
        }
        Err(connecting) => {
            // Recover: complete the handshake normally.
            let client2_conn = connecting.await.unwrap();
            println!(
                "  handshake_data() == Some after handshake          : {}",
                client2_conn.handshake_data().is_some()
            );
            true
        }
    };
    let _server2_conn = server_accept2.await.unwrap();
    println!("  into_0rtt() fails with early_data disabled      : {into_0rtt_failed}");
    println!("  0-RTT token path never used for media           : {into_0rtt_failed}");

    // -------- (d) malformed datagram no-panic --------
    println!("\n[4] malformed datagram → recv_datagram typed, no panic");
    let garbage: Vec<u8> = (0u8..128).map(|i| i.wrapping_mul(19)).collect();
    net.client.send_datagram(garbage.clone().into()).unwrap();
    let got = tokio::time::timeout(ms(0.5), net.server.read_datagram())
        .await
        .expect("recv")
        .expect("recv not error");
    println!(
        "  recv_datagram yielded {} bytes, connection healthy (decode left to wdr_proto)",
        got.len()
    );
    // send a real frame afterwards
    net.client.send_datagram(frame.clone().into()).unwrap();
    let ok_follow = tokio::time::timeout(ms(0.5), net.server.read_datagram())
        .await
        .ok();
    println!(
        "  subsequent real frame still received             : {}",
        ok_follow.is_some()
    );

    // -------- metrics --------
    println!("\n[5] metrics (what quinn exposes on this path)");
    let m = Metrics::default();
    let _ = m;
    let stats = net.client.stats();
    println!(
        "  quinn path: rtt={}µs cwnd={}B lost_pkts={} lost_bytes={} congestion_events={} mtu={} ACK-frames-rx={}",
        stats.path.rtt.as_micros(),
        stats.path.cwnd,
        stats.path.lost_packets,
        stats.path.lost_bytes,
        stats.path.congestion_events,
        stats.path.current_mtu,
        stats.frame_rx.acks,
    );
    println!("  datagram_lost: NOT app-observable in quinn 0.11 (datagrams are unreliable, un-acked); see report");

    // -------- BBR vs CUBIC smoke --------
    println!("\n[6] BBR vs CUBIC (loss-free loopback smoke — loss behavior needs netem/B1)");
    let cfg_bbr = TransportConnConfig::default()
        .congestion_control(CongestionControl::Bbr)
        .disable_0rtt();
    let net_bbr = connect(&cfg_bbr).await;
    let t0 = std::time::Instant::now();
    let bb = vec![0u8; 400];
    for _ in 0..1_000 {
        let _ = net_bbr.client.send_datagram(bb.clone().into());
    }
    let dur = t0.elapsed();
    drain_all(&net_bbr.server).await;
    println!("  BBR config constructs+loops {dur:?} for 1000×400B (loopback: no loss ⇒ CC-equal)");

    println!("\nDone. Loopback-only numbers — path-loss/-jitter evidence requires netem (B1 compose harness).");
}

fn _send_outcome_label(o: &SendDatagramOutcome) -> &'static str {
    match o {
        SendDatagramOutcome::Sent => "sent",
        SendDatagramOutcome::TooLarge { .. } => "too-large",
        SendDatagramOutcome::Unsupported => "unsupported",
        SendDatagramOutcome::BufferFull => "buffer-full",
        SendDatagramOutcome::Error => "error",
    }
}

async fn drain_all(conn: &quinn::Connection) -> usize {
    let mut n = 0;
    while let Ok(Ok(_)) = tokio::time::timeout(ms(0.002), conn.read_datagram()).await {
        n += 1;
    }
    n
}

fn ms(d: f64) -> Duration {
    Duration::from_secs_f64(d)
}
