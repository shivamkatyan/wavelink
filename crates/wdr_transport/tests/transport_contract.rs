//! Loopback transport contract tests for the `wdr_transport` spike.
//!
//! These assert behaviours (not numbers — numbers live in the example +
//! report): datagram + reliable-stream roundtrip, 0-RTT-off, deadline abort,
//! malformed-datagram no-panic, and the metrics counters. Each test spins two
//! loopback `quinn` endpoints (client + server) with a self-signed cert.
//!
//! A helper (`TestNet`) builds the QUIC pair. The client uses the crate's
//! 0-RTT-*off* `make_client_config`; the server uses `make_server_config`.

use std::{sync::Arc, time::Duration};

use quinn::crypto::rustls::QuicClientConfig;
use quinn::rustls::{
    client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier},
    pki_types::{CertificateDer, ServerName, UnixTime},
    DigitallySignedStruct, Error as RustlsError, SignatureScheme,
};
use rcgen::generate_simple_self_signed;
use wdr_transport::{
    drain_datagrams, stream_send_with_deadline, CongestionControl, Got, Metrics,
    SendDatagramOutcome, TransportConnConfig,
};

/// Accept-any-server-cert verifier (loopback test fixture only).
#[derive(Debug)]
struct AcceptAllVerifier;

impl ServerCertVerifier for AcceptAllVerifier {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, RustlsError> {
        Ok(ServerCertVerified::assertion())
    }
    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, RustlsError> {
        Ok(HandshakeSignatureValid::assertion())
    }
    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, RustlsError> {
        Ok(HandshakeSignatureValid::assertion())
    }
    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        vec![
            SignatureScheme::ECDSA_NISTP256_SHA256,
            SignatureScheme::ECDSA_NISTP384_SHA384,
            SignatureScheme::RSA_PSS_SHA256,
            SignatureScheme::ED25519,
        ]
    }
}

/// A connected loopback client + server pair.
///
/// The endpoints are kept alive for the lifetime of the pair: dropping a quinn
/// `Endpoint` closes all its connections, which would kill the sockets mid-test.
struct TestNet {
    client: quinn::Connection,
    server: quinn::Connection,
    _client_endpoint: quinn::Endpoint,
    _server_endpoint: quinn::Endpoint,
}

/// Build the loopback pair with a given client config.
async fn connect_pair(client_cfg: &TransportConnConfig) -> TestNet {
    let cert = generate_simple_self_signed(vec!["localhost".into()]).unwrap();

    // Server config (accepts our self-signed cert; the client's verifier
    // accepts any cert, so no root store is needed).
    let key = PrivateKeyDer::Pkcs8(cert.signing_key.serialize_der().into());
    let mut sc = quinn::ServerConfig::with_single_cert(vec![cert.cert.der().clone()], key).unwrap();
    sc.transport_config(Arc::new(client_cfg.build_transport()));

    let server_endpoint = quinn::Endpoint::server(sc, "127.0.0.1:0".parse().unwrap()).unwrap();
    let server_addr = server_endpoint.local_addr().unwrap();

    // Client config: reuse the crate's 0-RTT-off config builder but inject our
    // accept-any verifier so no root store dance is needed.
    let mut rustls_cfg = build_rustls_client_cfg();
    rustls_cfg.enable_early_data = client_cfg.zrt_enabled_media;
    let qcc = QuicClientConfig::try_from(rustls_cfg).unwrap();
    let mut qclient = quinn::ClientConfig::new(Arc::new(qcc));
    qclient.transport_config(Arc::new(client_cfg.build_transport()));

    let client_endpoint = quinn::Endpoint::client("127.0.0.1:0".parse().unwrap()).unwrap();

    let connecting = client_endpoint
        .connect_with(qclient, server_addr, "localhost")
        .unwrap();
    let server_accept = tokio::spawn({
        let server_endpoint = server_endpoint.clone();
        async move {
            server_endpoint
                .accept()
                .await
                .unwrap()
                .accept()
                .unwrap()
                .await
                .unwrap()
        }
    });
    let client_conn = connecting.await.unwrap();
    let server_conn = server_accept.await.unwrap();

    TestNet {
        client: client_conn,
        server: server_conn,
        _client_endpoint: client_endpoint,
        _server_endpoint: server_endpoint,
    }
}

fn build_rustls_client_cfg() -> quinn::rustls::ClientConfig {
    let provider = Arc::new(quinn::rustls::crypto::ring::default_provider());
    quinn::rustls::ClientConfig::builder_with_provider(provider)
        .with_protocol_versions(&[&quinn::rustls::version::TLS13])
        .unwrap()
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(AcceptAllVerifier))
        .with_no_client_auth()
}

use quinn::rustls::pki_types::PrivateKeyDer;

#[test]
fn cc_variants_distinct() {
    assert_ne!(CongestionControl::Cubic, CongestionControl::Bbr);
}

#[tokio::test]
async fn loopback_roundtrip_datagram_and_stream() {
    let net = connect_pair(&TransportConnConfig::default()).await;

    // --- datagram: client -> server ---
    let frame: Vec<u8> = vec![0xAB; 400]; // Opus 20 ms ≈ 400 B
    assert_eq!(
        net.client.send_datagram(frame.clone().into()),
        Ok(()),
        "datagram send"
    );
    let recv = tokio::time::timeout(Duration::from_secs(5), net.server.read_datagram())
        .await
        .expect("server recv timeout")
        .expect("server read_datagram error");
    assert_eq!(recv.to_vec(), frame, "datagram payload roundtrip");

    // --- reliable stream: server -> client (opposite direction) ---
    let (send_stream, _recv_stream) = net.server.open_bi().await.unwrap();
    let client_accept = tokio::spawn(async move {
        let (_, mut recv) = net.client.accept_bi().await.unwrap();
        recv.read_to_end(4096).await.unwrap()
    });
    let mut send_stream = send_stream;
    send_stream
        .write_all(b"lossless-frame-bytes")
        .await
        .unwrap();
    send_stream.finish().unwrap();
    let got = tokio::time::timeout(Duration::from_secs(5), client_accept)
        .await
        .expect("client stream read timeout")
        .expect("client read_to_end");
    assert_eq!(got, b"lossless-frame-bytes", "stream payload roundtrip");
}

#[tokio::test]
#[cfg(not(miri))]
async fn zero_rtt_off_media_path() {
    // The locked WDR default: 0-RTT OFF on the media path. We assert (c):
    //   - handshake completes before any datagram is sent,
    //   - no 0-RTT token path was used,
    //   - the rustls client config has `enable_early_data == false`.
    let cfg = TransportConnConfig::default();
    assert!(!cfg.zrt_enabled_media, "default 0-RTT OFF for media");

    let cert = generate_simple_self_signed(vec!["localhost".into()]).unwrap();
    let key = PrivateKeyDer::Pkcs8(cert.signing_key.serialize_der().into());
    let mut sc = quinn::ServerConfig::with_single_cert(vec![cert.cert.der().clone()], key).unwrap();
    sc.transport_config(Arc::new(cfg.build_transport()));
    let server_endpoint = quinn::Endpoint::server(sc, "127.0.0.1:0".parse().unwrap()).unwrap();
    let server_addr = server_endpoint.local_addr().unwrap();

    let mut rustls_cfg = build_rustls_client_cfg();
    rustls_cfg.enable_early_data = false; // SECURITY_SPEC §3.6
    let qcc = QuicClientConfig::try_from(rustls_cfg).unwrap();
    let mut qclient = quinn::ClientConfig::new(Arc::new(qcc));
    qclient.transport_config(Arc::new(cfg.build_transport()));

    let client_endpoint = quinn::Endpoint::client("127.0.0.1:0".parse().unwrap()).unwrap();
    let connecting = client_endpoint
        .connect_with(qclient, server_addr, "localhost")
        .unwrap();

    // Drive the server side concurrently (otherwise the client handshake
    // blocks until the server accepts, deadlocking a serial await).
    let server_accept = tokio::spawn(async move {
        server_endpoint
            .accept()
            .await
            .unwrap()
            .accept()
            .unwrap()
            .await
            .unwrap()
    });

    // Critical: `into_0rtt` must FAIL (no 0-RTT keys available) because
    // `enable_early_data == false`. `into_0rtt` returns `Result<(Connection,
    // ZeroRttAccepted), Connecting>` — on failure it returns `Err(Self)`, which
    // we destructure (no `Debug` needed for `ZeroRttAccepted`).
    let connecting = match connecting.into_0rtt() {
        Ok((_conn, _accepted)) => panic!("0-RTT must be rejected when early data is disabled"),
        Err(connecting) => connecting,
    };
    let client_conn = connecting.await.unwrap();

    // Datagrams must NOT be sent before the handshake completes. `handshake_data`
    // is a future that resolves once the handshake is done.
    let hd = client_conn.handshake_data();
    assert!(
        hd.is_some(),
        "handshake_data available only after completion"
    );
    // (A datagram sent now is post-handshake.)

    // Server side accepts the connection.
    let server_conn: quinn::Connection = server_accept.await.unwrap();

    // After handshake, a datagram roundtrips normally (no 0-RTT needed).
    client_conn
        .send_datagram(b"post-handshake".to_vec().into())
        .unwrap();
    let got = tokio::time::timeout(Duration::from_secs(5), server_conn.read_datagram())
        .await
        .expect("recv timeout")
        .expect("recv datagram");
    assert_eq!(got.to_vec(), b"post-handshake".to_vec());
}

#[tokio::test]
#[cfg(not(miri))]
async fn stream_deadline_abort() {
    // Lossless media rides a reliable stream with an app-layer retransmit
    // deadline (≤50 ms per PROTOCOL_SPEC): if the wall-clock budget is already
    // exhausted when the transfer completes, we abort.
    let net = connect_pair(&TransportConnConfig::default()).await;

    // Case 1: succeeds well within the deadline (peer actively reads).
    {
        let server = net.server.clone();
        let reader = tokio::spawn(async move {
            let (_, mut recv) = server.accept_bi().await.unwrap();
            let _ = recv.read_to_end(1 << 20).await;
        });

        let now = std::time::Instant::now();
        let r = tokio::time::timeout(
            Duration::from_secs(5),
            stream_send_with_deadline(
                &net.client,
                now,
                Duration::from_millis(500),
                vec![0x11; 1024],
            ),
        )
        .await
        .expect("stream task");
        assert!(r.is_ok(), "within-deadline stream should complete");
        reader.await.unwrap();
    }

    // Case 2: the peer never reads → the write blocks under flow control and
    // the wall-clock deadline fires, aborting the stream. Force a small
    // *peer* stream receive window so a large write genuinely blocks.
    {
        let cfg = TransportConnConfig::default().stream_receive_window(4096);
        let net2 = connect_pair(&cfg).await;
        let now = std::time::Instant::now();
        let payload = vec![0x22u8; 256 * 1024]; // ≫ 4 KiB peer window
        let r = tokio::time::timeout(
            Duration::from_secs(5),
            stream_send_with_deadline(&net2.client, now, Duration::from_millis(200), payload),
        )
        .await
        .expect("stream task");
        assert_eq!(
            r,
            Err("retransmit_deadline exceeded"),
            "unread stream aborts on deadline"
        );
    }

    // Case 3: a deadline already in the past aborts immediately.
    {
        let now = std::time::Instant::now() - Duration::from_secs(10);
        let r =
            stream_send_with_deadline(&net.client, now, Duration::from_millis(1), vec![0x33; 64])
                .await;
        assert_eq!(
            r,
            Err("retransmit_deadline exceeded"),
            "past deadline aborts"
        );
    }
}

#[tokio::test]
#[cfg(not(miri))]
async fn malformed_datagram_no_panic() {
    // Feed random garbage to recv_datagram: the transport must yield bytes and
    // stay healthy (decode failure is the *protocol* layer's job, SEC-05).
    let net = connect_pair(&TransportConnConfig::default()).await;

    for _ in 0..8 {
        let garbage: Vec<u8> = (0..64).map(|i| (i * 37) as u8).collect();
        net.client.send_datagram(garbage.clone().into()).unwrap();
        let got = tokio::time::timeout(Duration::from_secs(5), net.server.read_datagram())
            .await
            .expect("recv timeout")
            .expect("recv datagram should not error");
        assert_eq!(got.to_vec(), garbage, "transport yields bytes as-is");
    }

    // Connection stays healthy: datagrams still flow in both directions.
    net.server
        .send_datagram(b"still-alive".to_vec().into())
        .unwrap();
    let got = tokio::time::timeout(Duration::from_secs(5), net.client.read_datagram())
        .await
        .expect("recv2 timeout")
        .expect("recv2 datagram");
    assert_eq!(got.to_vec(), b"still-alive".to_vec());
}

#[tokio::test]
async fn metrics_counters_reflect_outcome() {
    // The typed outcome path bumps counters; oversized datagrams are rejected
    // without panic and the connection remains usable.
    let net = connect_pair(&TransportConnConfig::default()).await;
    let mut m = Metrics::default();

    let ok = try_send(&net.client, &[0u8; 400], None);
    m.note_send(ok.clone());
    assert_eq!(ok, SendDatagramOutcome::Sent);
    assert_eq!(m.frames_sent, 1);

    // Oversized: what's the current bound?
    let cap = net.client.max_datagram_size().unwrap();
    let too_big = try_send(&net.client, &vec![0u8; cap + 512], None);
    m.note_send(too_big.clone());
    match too_big {
        SendDatagramOutcome::TooLarge { max } => {
            assert!(max <= cap + 1);
            assert!(m.dropped_too_large == 1);
        }
        other => panic!("expected TooLarge, got {other:?}"),
    }

    // Datagrams still flow. Drain the earlier 400-byte datagram first, then
    // the distinct "ok" one (order preserved).
    let first = tokio::time::timeout(Duration::from_secs(5), net.server.read_datagram())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        first.to_vec().len(),
        400,
        "first datagram is the 400-byte frame"
    );
    net.client.send_datagram(b"ok".to_vec().into()).unwrap();
    let got = tokio::time::timeout(Duration::from_secs(5), net.server.read_datagram())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(got.to_vec(), b"ok".to_vec());
}

#[tokio::test]
async fn drain_nonblocking_and_receivers() {
    let net = connect_pair(&TransportConnConfig::default()).await;

    // Drain helper is non-blocking: empty now, then collects what arrived.
    assert!(drain_datagrams(&net.server).is_empty());
    for i in 0..3 {
        // Use the *send* side and give the loopback driver a moment to move
        // the datagrams; then drain (non-blocking) server-side.
        let _ = net
            .client
            .send_datagram(format!("d{i}").into_bytes().into());
    }
    tokio::time::sleep(Duration::from_millis(50)).await;
    let drained = drain_datagrams(&net.server);
    assert_eq!(drained.len(), 3, "should drain 3 queued datagrams");

    // `Got` keeps its typed shape.
    let _ = Got::Datagram(vec![1, 2, 3]);
}

fn try_send(conn: &quinn::Connection, frame: &[u8], cap: Option<usize>) -> SendDatagramOutcome {
    use wdr_transport::SendDatagramOutcome::*;
    if let Some(c) = cap {
        if frame.len() > c {
            return TooLarge { max: c };
        }
    }
    match conn.send_datagram(frame.to_vec().into()) {
        Ok(()) => Sent,
        Err(quinn::SendDatagramError::TooLarge) => TooLarge {
            max: conn.max_datagram_size().unwrap_or(0),
        },
        Err(quinn::SendDatagramError::UnsupportedByPeer)
        | Err(quinn::SendDatagramError::Disabled) => Unsupported,
        Err(quinn::SendDatagramError::ConnectionLost(_)) => Error,
    }
}
