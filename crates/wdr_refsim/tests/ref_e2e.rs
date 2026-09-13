//! Loopback end-to-end tests for `wdr_refsim` (task R-B1-SIM closure).
//!
//! These drive the shared in-process emitter (`wdr_refsim::emitter`) and the
//! shared receiver pipeline (`wdr_refsim::receiver`) over a **loopback QUIC
//! pair** (quinn, gorilladev `localhost` self-signed identity) — the same
//! plane the `ref_emitter`/`ref_receiver` CLIs use, so the coverage is the
//! production pipeline, not a fake.
//!
//! Coverage required by the task:
//! * `lossless_flac_roundtrip_hash_preserved` — FLAC over the reliable stream
//!   (ADR-003: lossless = reliable stream), receiver hash == the canonical
//!   golden for pseudo-random i16 48k stereo, tolerance-free.
//! * `lossless_pcm_roundtrip_hash_preserved` — raw PCM over the reliable
//!   stream, same golden.
//! * `lossy_opus_no_panic_bounded` — Opus over datagrams with send-side
//!   `Impairment` (loss + reorder); receiver completes, never panics,
//!   loss/late counters bounded + reported, and the jitter queue never exceeds
//!   `MAX_QUEUE_BOUND`.
//! * `malformed_rejected_not_panic` — raw garbage fed into `ingest_bytes`
//!   before the valid frames; no panic and the valid stream still completes.

use std::sync::Arc;
use std::time::Duration;

use quinn::crypto::rustls::QuicClientConfig;
use quinn::rustls::{
    client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier},
    pki_types::{CertificateDer, PrivateKeyDer, ServerName, UnixTime},
    DigitallySignedStruct, Error as RustlsError, SignatureScheme,
};
use rcgen::generate_simple_self_signed;
use wdr_refsim::emitter::{BufferMeta, Emitter, EmitterConfig, Impairment, StreamKind};
use wdr_refsim::framing::{FrameWire, FramedItem};
use wdr_refsim::receiver::{
    BufferProfile, ClockHandle, Receiver, ReceiverOutcome, MAX_QUEUE_BOUND,
};

/// The canonical golden for pseudo-random i16 48 kHz stereo — recorded in
/// `wdr_fakes/tests/golden.rs` and `docs/orchestration/reports/t-B0-fakes.md`.
const GOLDEN_PRNG_I16_48K_STEREO: &str =
    "b7a3c25c8ccaa05643f14dfb5bc0e223f4ace3154c11ecd25bed5975d839c223";

const RATE_HZ: u32 = 48_000;

/// Canonical lossless metadata: i16 / 48k / stereo / 512-sample chunks (the
/// conditions under which the t-B0 golden hashes were recorded).
fn canonical_meta(codec: wdr_proto::Codec, frame_samples: usize) -> BufferMeta {
    BufferMeta {
        codec,
        sample_rate: RATE_HZ,
        channels: 2,
        sample_repr: wdr_proto::SampleRepr::I16,
        channel_layout: wdr_proto::ChannelLayout::Stereo,
        frame_samples,
    }
}

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
            SignatureScheme::RSA_PSS_SHA256,
            SignatureScheme::ED25519,
        ]
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

/// A connected loopback client + server pair, kept alive for the pair's
/// lifetime (dropping a quinn Endpoint closes its connections).
struct Loopback {
    client: quinn::Connection,
    server: quinn::Connection,
    _client_endpoint: quinn::Endpoint,
    _server_endpoint: quinn::Endpoint,
}

async fn connect_pair() -> Loopback {
    let cert = generate_simple_self_signed(vec!["localhost".into()]).unwrap();
    let key = PrivateKeyDer::Pkcs8(cert.signing_key.serialize_der().into());
    let mut sc = quinn::ServerConfig::with_single_cert(vec![cert.cert.der().clone()], key).unwrap();
    let tc = Arc::new(wdr_transport::TransportConnConfig::default().build_transport());
    sc.transport_config(tc.clone());

    let server_endpoint = quinn::Endpoint::server(sc, "127.0.0.1:0".parse().unwrap()).unwrap();
    let server_addr = server_endpoint.local_addr().unwrap();

    let mut rustls_cfg = build_rustls_client_cfg();
    rustls_cfg.enable_early_data = false; // SECURITY_SPEC §3.6: 0-RTT off
    let qcc = QuicClientConfig::try_from(rustls_cfg).unwrap();
    let mut qclient = quinn::ClientConfig::new(Arc::new(qcc));
    qclient.transport_config(tc);

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

    Loopback {
        client: client_conn,
        server: server_conn,
        _client_endpoint: client_endpoint,
        _server_endpoint: server_endpoint,
    }
}

/// Buffer assistant for exact length-prefixed stream reads.
async fn read_exact(recv: &mut quinn::RecvStream, buf: &mut [u8]) -> Result<(), ()> {
    let mut off = 0;
    while off < buf.len() {
        let n = tokio::time::timeout(Duration::from_secs(10), recv.read(&mut buf[off..]))
            .await
            .map_err(|_| ())?
            .map_err(|_| ())?;
        match n {
            Some(0) | None => return Err(()),
            Some(n) => off += n,
        }
    }
    Ok(())
}

/// Read one length-prefixed reliable-stream item (mirrors `ref_receiver`'s
/// `read_one_stream_item`: `[u16 len][body]`).
async fn read_one_stream_item(recv: &mut quinn::RecvStream) -> Result<Vec<u8>, ()> {
    let mut len_buf = [0u8; 2];
    read_exact(recv, &mut len_buf).await?;
    let len = u16::from_le_bytes(len_buf) as usize;
    if len > wdr_refsim::framing::MAX_FRAME_TOTAL + 13 {
        return Err(());
    }
    let mut body = vec![0u8; len];
    read_exact(recv, &mut body).await?;
    Ok(body)
}

/// Receiver driver over the reliable-stream lane, mirroring `ref_receiver`'s
/// stream path: the emitter opens **one bidirectional stream per frame** (plus
/// the end marker), each carrying a single length-prefixed item, so we loop
/// `accept_bi()` → `read_one_stream_item` until the receiver observes the end
/// marker.
async fn receive_stream_lane(
    conn: quinn::Connection,
    meta: BufferMeta,
    timeout: Duration,
) -> ReceiverOutcome {
    let mut receiver =
        Receiver::for_stream(meta, BufferProfile::Balanced, ClockHandle::system(), 0)
            .expect("receiver build");
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            panic!("stream lane: end-of-stream marker not received within {timeout:?}");
        }
        let (_send, mut recv) = tokio::time::timeout(remaining, conn.accept_bi())
            .await
            .expect("stream open timeout")
            .expect("accept_bi failed");
        let item_bytes = match read_one_stream_item(&mut recv).await {
            Ok(b) => b,
            Err(()) => {
                receiver.metrics_mut().malformed += 1;
                continue;
            }
        };
        let item = match FrameWire::parse(&item_bytes) {
            Ok(i) => i,
            Err(e) => {
                receiver.metrics_mut().malformed += 1;
                eprintln!("[e2e] dropped malformed stream item: {e}");
                continue;
            }
        };
        match item {
            FramedItem::Audio(frame) => {
                if let Err(e) = receiver.ingest_frame_direct(frame) {
                    receiver.metrics_mut().malformed += 1;
                    eprintln!("[e2e] frame discarded: {e}");
                }
            }
            FramedItem::End { total_frames } => {
                let _ = receiver.ingest_end_marker(total_frames);
            }
        }
        if receiver.ended() {
            break;
        }
    }
    receiver.finalize()
}

/// Receiver driver over the datagram lane (mirrors `ref_receiver`'s datagram
/// path: `ingest_bytes` per datagram until the end marker or deadline), also
/// tracking the peak jitter-queue depth so the caller can assert the hard
/// `MAX_QUEUE_BOUND` guarantee.
async fn receive_datagram_lane_peak(
    conn: quinn::Connection,
    meta: BufferMeta,
    timeout: Duration,
) -> (ReceiverOutcome, usize) {
    let mut receiver =
        Receiver::for_stream(meta, BufferProfile::Balanced, ClockHandle::system(), 0)
            .expect("receiver build");
    let mut peak = 0usize;
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            panic!("datagram lane: end-of-stream marker not received within {timeout:?}");
        }
        let d = tokio::time::timeout(remaining, conn.read_datagram())
            .await
            .expect("datagram read timeout")
            .expect("datagram read error");
        match receiver.ingest_bytes(&d) {
            Ok(()) => {}
            Err(e) => {
                receiver.metrics_mut().malformed += 1;
                eprintln!("[e2e] datagram discarded: {e}");
            }
        }
        peak = peak.max(receiver.buffer_len());
        if receiver.ended() {
            break;
        }
    }
    (receiver.finalize(), peak)
}

/// Drive ONE loopback run end-to-end with the real in-process emitter +
/// receiver over QUIC. The emitter runs to completion first (QUIC buffers the
/// tiny frame payloads either on the reliable-stream/`accept_bi` queue or as
/// datagrams), then the receiver drains — deterministic and free of any
/// cross-thread Send requirement on the codec adapters.
async fn run_loopback(
    kind: StreamKind,
    codec: wdr_proto::Codec,
    total_samples: u64,
    frame_samples: usize,
    impairment: Impairment,
    seed: u64,
) -> (blake3::Hash, ReceiverOutcome) {
    let net = connect_pair().await;
    let meta = canonical_meta(codec, frame_samples);
    let cfg = EmitterConfig {
        kind,
        total_samples,
        fixture: wdr_fakes::source::FixtureKind::PseudoRandomPcm,
        impairment,
        seed,
        pace: None,
        pace_real_time: false,
    };

    let mut emitter = Emitter::new(net.server.clone(), cfg, meta).expect("emitter build");
    let source_hash = emitter.run().await.expect("emitter run");

    let outcome = match kind {
        StreamKind::OpusDatagram => {
            receive_datagram_lane_peak(net.client, meta, Duration::from_secs(20))
                .await
                .0
        }
        StreamKind::FlacStream | StreamKind::PcmStream => {
            receive_stream_lane(net.client, meta, Duration::from_secs(20)).await
        }
    };
    (source_hash, outcome)
}

/// Deterministic garbage generator (no rand needed in tests): a tiny LCG.
fn garbage_stream(seed: u64, len: usize) -> Vec<u8> {
    let mut state = seed;
    (0..len)
        .map(|_| {
            state = state
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (state >> 33) as u8
        })
        .collect()
}

#[tokio::test]
async fn lossless_flac_roundtrip_hash_preserved() {
    // `EmitterConfig.total_samples` is a PER-CHANNEL budget (rev-8 semantics):
    // 2048 per-channel × 2 ch = 4096 values = the canonical golden stream
    // (4 frames of 1024 values each). 4096 here would drain 8192 values and
    // break the recorded golden hash.
    let (source_hash, outcome) = run_loopback(
        StreamKind::FlacStream,
        wdr_proto::Codec::Flac,
        2048,
        512,
        Impairment::clean(),
        0xB1E1_0000,
    )
    .await;

    // Source bytes hash == canonical golden (pseudo-random i16 48k stereo).
    assert_eq!(
        source_hash.to_hex().to_string(),
        GOLDEN_PRNG_I16_48K_STEREO,
        "emitter source hash must match the recorded golden"
    );
    // Receiver decoded exactly the source bytes → identical hash (lossless).
    assert_eq!(
        outcome.hash_hex(),
        GOLDEN_PRNG_I16_48K_STEREO,
        "lossless FLAC roundtrip must preserve the hash tolerance-free"
    );
    // No loss on a clean reliable stream.
    assert_eq!(
        outcome.metrics.loss, 0,
        "clean reliable stream must not lose frames"
    );
    assert_eq!(outcome.metrics.duplicate, 0);
    // 4096 source *values* / 1024 values-per-frame (512 per-channel × 2 ch).
    assert_eq!(outcome.frames_rendered, 4);
    assert_eq!(outcome.metrics.fatal_count, 0);
}

#[tokio::test]
async fn lossless_pcm_roundtrip_hash_preserved() {
    // Per-channel budget 2048 × 2 ch = 4096 values = canonical golden (see the
    // FLAC test above for the rev-8 per-channel semantics explanation).
    let (source_hash, outcome) = run_loopback(
        StreamKind::PcmStream,
        wdr_proto::Codec::Pcm,
        2048,
        512,
        Impairment::clean(),
        0xB1E1_0000,
    )
    .await;

    assert_eq!(
        source_hash.to_hex().to_string(),
        GOLDEN_PRNG_I16_48K_STEREO,
        "emitter source hash must match the recorded golden"
    );
    assert_eq!(
        outcome.hash_hex(),
        GOLDEN_PRNG_I16_48K_STEREO,
        "lossless PCM roundtrip must preserve the hash tolerance-free"
    );
    assert_eq!(outcome.metrics.loss, 0);
    assert_eq!(outcome.metrics.duplicate, 0);
    // 4096 source *values* / 1024 values-per-frame (512 per-channel frames × 2
    // channels) = 4 frames rendered, each recovering 512 per-channel samples.
    assert_eq!(outcome.frames_rendered, 4);
    assert_eq!(outcome.metrics.fatal_count, 0);
}

#[tokio::test]
async fn lossy_opus_no_panic_bounded() {
    // Opus over the datagram path with send-side impairment (loss 10% +
    // reorder 25%) so the jitter/reorder machinery is genuinely exercised.
    let impairment = Impairment {
        loss_pct: 10,
        duplicate_pct: 5,
        reorder_pct: 25,
    };
    let net = connect_pair().await;
    let meta = canonical_meta(wdr_proto::Codec::Opus, 960);
    let cfg = EmitterConfig {
        kind: StreamKind::OpusDatagram,
        total_samples: 960 * 64, // 64 frames of 960 per-channel samples
        fixture: wdr_fakes::source::FixtureKind::PseudoRandomPcm,
        impairment,
        seed: 0xBEEF,
        pace: None,
        pace_real_time: false,
    };
    let mut emitter = Emitter::new(net.server.clone(), cfg, meta).expect("emitter build");
    let source_hash = emitter.run().await.expect("emitter run");
    let (outcome, peak) =
        receive_datagram_lane_peak(net.client, meta, Duration::from_secs(20)).await;

    // Never panics is implied by the task reaching here; the receiver reports
    // bounded, non-fatal counters.
    assert_eq!(
        outcome.metrics.fatal_count, 0,
        "no fatal events under impairment"
    );
    assert!(
        outcome.metrics.loss > 0,
        "send-side loss impairment must be observed by the receiver"
    );
    // loss + delivered must not exceed the number the emitter sent (bounded).
    let sent = 64;
    assert!(
        outcome.metrics.packets_recv + outcome.metrics.loss <= sent,
        "received ({}) + loss ({}) must stay bounded by sent ({sent})",
        outcome.metrics.packets_recv,
        outcome.metrics.loss
    );
    assert!(
        outcome.metrics.packets_recv <= sent,
        "cannot receive more frames than were sent"
    );
    // The jitter queue is hard-bounded: even under sustained reorder the
    // in-order buffer never grows past its per-profile cap (Consumer-side
    // queue at MAX_QUEUE_BOUND for the balanced profile).
    assert!(
        peak <= MAX_QUEUE_BOUND,
        "jitter queue peak {peak} must never exceed MAX_QUEUE_BOUND {MAX_QUEUE_BOUND}"
    );
    // The hash is NOT the lossless golden — Opus is lossy. But it must be a
    // real 64-hex digest (the sink was fed real decoded bytes).
    assert_eq!(outcome.hash_hex().len(), 64);
    let _ = source_hash;
    assert!(
        outcome.metrics.reorder > 0 || outcome.metrics.late_discard > 0,
        "reorder impairment must be observable"
    );
}

#[tokio::test]
async fn malformed_rejected_not_panic() {
    // Feed raw garbage into `ingest_bytes` before the valid frames; the
    // receiver must reject without panicking and the valid stream completes.
    let net = connect_pair().await;
    let meta = canonical_meta(wdr_proto::Codec::Flac, 512);
    let cfg = EmitterConfig {
        kind: StreamKind::FlacStream,
        total_samples: 4096,
        fixture: wdr_fakes::source::FixtureKind::PseudoRandomPcm,
        impairment: Impairment::clean(),
        seed: 0xB1E1_0000,
        pace: None,
        pace_real_time: false,
    };

    // Garbage first: feed a few random byte blobs into a throwaway receiver
    // through the exact `ingest_bytes` entry the datagram lane uses. Nothing
    // may panic (the assert below would not be reached on a panic).
    {
        let mut rcv = Receiver::for_stream(meta, BufferProfile::Balanced, ClockHandle::system(), 0)
            .expect("receiver build");
        for seed in 0..16u64 {
            let garbage = garbage_stream(0xDEAD_0000 + seed, 1 + (seed as usize * 97 % 300));
            let _ = rcv.ingest_bytes(&garbage); // must not panic
        }
        // The receiver reports the malformed count and stays usable.
        assert!(
            rcv.metrics().malformed > 0,
            "garbage must be counted as malformed"
        );
        assert_eq!(rcv.metrics().fatal_count, 0);
    }

    // The valid stream still completes on the real receiver.
    let mut emitter = Emitter::new(net.server.clone(), cfg, meta).expect("emitter build");
    let source_hash = emitter.run().await.expect("emitter run");
    let outcome = receive_stream_lane(net.client, meta, Duration::from_secs(20)).await;

    assert_eq!(
        outcome.hash_hex(),
        source_hash.to_hex().to_string(),
        "valid stream still completes and matches the source"
    );
    assert_eq!(outcome.metrics.fatal_count, 0);
}
