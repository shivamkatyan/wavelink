//! Integration tests for the live transport seam — `QuicAudioSink` +
//! `AudioFrameSink` — against the real in-process receiver over a loopback
//! QUIC pair (the same plane the CLIs use). This is the software gate for
//! "first real stream", and the hash is checked against the canonical golden
//! exactly like the refsim loopback tests.
//!
//! The sink dials from its OWN tokio runtime, while the server accepts on a
//! separate thread+runtime — no cross-runtime quinn misuse, no nested-runtime
//! deadlock.

use std::sync::Arc;
use std::time::Duration;

use quinn::rustls::pki_types::PrivateKeyDer;
use rcgen::generate_simple_self_signed;
use wdr_entitlement::provider::Tier;
use wdr_fakes::source::{ChannelKind, Fixture, FixtureKind, SampleFormat};
use wdr_fakes::PcmSource;
use wdr_proto::{ChannelLayout, Codec, SampleRepr};
use wdr_refsim::emitter::BufferMeta;
use wdr_refsim::framing::{FrameWire, FramedItem};
use wdr_refsim::receiver::{BufferProfile, ClockHandle, Receiver, ReceiverOutcome};
use wdr_refsim::sink::{FrameSink, QuicAudioSink, SinkFormat};

/// The canonical golden for pseudo-random i16 48 kHz stereo (recorded in
/// `wdr_fakes/tests/golden.rs` / `docs/orchestration/reports/t-B0-fakes.md`).
const GOLDEN_PRNG_I16_48K_STEREO: &str =
    "b7a3c25c8ccaa05643f14dfb5bc0e223f4ace3154c11ecd25bed5975d839c223";

const RATE_HZ: u32 = 48_000;

fn sink_format() -> SinkFormat {
    SinkFormat {
        sample_rate: RATE_HZ,
        channels: 2,
        sample_repr: SampleRepr::I16,
        channel_layout: ChannelLayout::Stereo,
    }
}

/// Wire meta for a codec at a specific (possibly non-48k) rate: Opus uses 20 ms
/// frames (882 @44.1k, 960 @48k; odd low rates are normalized to 48k = 960 on
/// the wire by the sink's resampler), lossless uses 512-sample frames at any
/// delivered rate.
fn meta_for_at(codec: Codec, rate: u32) -> BufferMeta {
    BufferMeta {
        codec,
        sample_rate: rate,
        channels: 2,
        sample_repr: SampleRepr::I16,
        channel_layout: ChannelLayout::Stereo,
        frame_samples: match (codec, rate) {
            (Codec::Opus, 44_100) => 882,
            (Codec::Opus, _) => 960,
            _ => 512,
        },
    }
}

/// Spawn a loopback QUIC server on a background thread that accepts ONE
/// connection and runs the receiver pipeline on the SAME thread+runtime that
/// owns the connection driver (moving a `quinn::Connection` off its runtime
/// kills its driver — connections are Send but not usable cross-runtime). The
/// client (sink) dials from its own runtime; returns the bound address now and
/// the completed receiver `ReceiverOutcome` via the join handle.
fn spawn_receiver_server(
    codec: Codec,
    rate: u32,
) -> (
    std::net::SocketAddr,
    std::thread::JoinHandle<ReceiverOutcome>,
) {
    let (addr_tx, addr_rx) = std::sync::mpsc::channel();
    let handle = std::thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new().expect("server runtime");
        rt.block_on(async move {
            let cert = generate_simple_self_signed(vec!["localhost".into()]).unwrap();
            let key = PrivateKeyDer::Pkcs8(cert.signing_key.serialize_der().into());
            let mut sc =
                quinn::ServerConfig::with_single_cert(vec![cert.cert.der().clone()], key).unwrap();
            let tc = Arc::new(wdr_transport::TransportConnConfig::default().build_transport());
            sc.transport_config(tc);
            let endpoint =
                quinn::Endpoint::server(sc, "127.0.0.1:0".parse().unwrap()).expect("server bind");
            let _ = addr_tx.send(endpoint.local_addr().unwrap());
            let conn = endpoint
                .accept()
                .await
                .expect("incoming")
                .accept()
                .expect("connecting")
                .await
                .expect("handshake");
            let meta = meta_for_at(codec, rate);
            match codec {
                Codec::Opus => receive_datagram_lane(conn, meta, Duration::from_secs(15)).await,
                _ => receive_stream_lane(conn, meta, Duration::from_secs(15)).await,
            }
        })
    });
    let addr = addr_rx.recv().expect("server address");
    (addr, handle)
}

/// Canonical fixture chunk: 4096 values = 4 frames of 512 samples/channel.
fn canonical_chunk() -> (Vec<u8>, usize) {
    let mut fixture = Fixture::new(
        FixtureKind::PseudoRandomPcm,
        SampleFormat::I16,
        RATE_HZ,
        ChannelKind::Stereo,
        4096,
    );
    let chunk = fixture.next_chunk(4096);
    (chunk.bytes.to_vec(), chunk.len)
}

/// Feed `frames` whole-fixture frames at `rate` into `sink`, hashing the raw
/// source bytes; returns (source_hash_hex, packets_sent).
fn feed_fixture_hash(
    sink: &mut QuicAudioSink,
    kind: FixtureKind,
    rate: u32,
    frames: usize,
) -> (String, u64) {
    let whole = sink.values_per_frame();
    let mut fixture = Fixture::new(
        kind,
        SampleFormat::I16,
        rate,
        ChannelKind::Stereo,
        (frames * whole) as u64,
    );
    let mut h = blake3::Hasher::new();
    for _ in 0..frames {
        let chunk = fixture.next_chunk(whole as u32);
        assert_eq!(chunk.len, whole, "whole frames only");
        h.update(chunk.bytes);
        sink.on_block(chunk.bytes).expect("block accepted");
    }
    (h.finalize().to_hex().to_string(), sink.packets_sent())
}

// --- receiver lane drivers (mirror ref_e2e helpers) ----------------------------

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
                eprintln!("[sink-test] dropped malformed stream item: {e}");
                continue;
            }
        };
        match item {
            FramedItem::Audio(frame) => {
                if let Err(e) = receiver.ingest_frame_direct(frame) {
                    receiver.metrics_mut().malformed += 1;
                    eprintln!("[sink-test] frame discarded: {e}");
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

async fn receive_datagram_lane(
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
                eprintln!("[sink-test] datagram discarded: {e}");
            }
        }
        if receiver.ended() {
            break;
        }
    }
    receiver.finalize()
}

// --- tests ---------------------------------------------------------------------

#[test]
fn sink_lossless_flac_roundtrip_hash_preserved() {
    let (addr, handle) = spawn_receiver_server(Codec::Flac, RATE_HZ);

    let mut sink = QuicAudioSink::connect(&addr.to_string(), Tier::Pro, Codec::Flac)
        .expect("pro/flac sink connects + policy-approved");
    assert_eq!(
        sink.values_per_frame(),
        512 * 2,
        "pro FLAC = 512-sample x stereo"
    );
    sink.on_format(sink_format()).expect("format accepted");

    // Canonical fixture: 4096 values = 4 whole frames; hash equals the golden.
    let (bytes, len) = canonical_chunk();
    assert_eq!(len, 4096);
    sink.on_block(&bytes).expect("block accepted");
    sink.finish().expect("end marker sent");
    let packets = sink.packets_sent();
    drop(sink);

    let outcome = handle.join().expect("server join");
    assert_eq!(
        outcome.metrics.packets_recv, packets,
        "all frames delivered"
    );
    assert_eq!(packets, 4, "4 whole frames sent");
    assert_eq!(
        outcome.hash_hex(),
        GOLDEN_PRNG_I16_48K_STEREO,
        "lossless hash preserved"
    );
    assert_eq!(outcome.metrics.fatal_count, 0);
    assert_eq!(outcome.metrics.underruns, 0);
    assert_eq!(outcome.metrics.loss, 0);
}

#[test]
fn sink_opus_datagram_bounded_no_panic() {
    let (addr, handle) = spawn_receiver_server(Codec::Opus, RATE_HZ);

    let mut sink = QuicAudioSink::connect(&addr.to_string(), Tier::Free, Codec::Opus)
        .expect("free/opus sink connects + policy-approved");
    assert_eq!(
        sink.values_per_frame(),
        960 * 2,
        "opus = 960-sample x stereo"
    );
    sink.on_format(sink_format()).expect("format accepted");

    // Three whole 960-sample frames plus a partial tail (dropped on finish —
    // hash-perfect streams must supply whole frames; lossy just must not panic).
    let mut fixture = Fixture::new(
        FixtureKind::PseudoRandomPcm,
        SampleFormat::I16,
        RATE_HZ,
        ChannelKind::Stereo,
        960 * 2 * 3 + 7,
    );
    let chunk = fixture.next_chunk(960 * 2 * 3 + 7);
    sink.on_block(chunk.bytes).expect("block accepted");
    sink.finish().expect("end marker sent");
    let packets = sink.packets_sent();
    assert_eq!(packets, 3, "only whole frames hit the wire");
    drop(sink);

    let outcome = handle.join().expect("server join");
    assert!(
        outcome.metrics.packets_recv >= packets,
        "audio frames delivered"
    );
    assert_eq!(
        outcome.metrics.fatal_count, 0,
        "no fatal decode/transport errors"
    );
}

#[test]
fn free_tier_refuses_lossless_before_send() {
    // Policy gate refuses the lossless lane under Free BEFORE any byte leaves.
    assert!(
        QuicAudioSink::connect("127.0.0.1:1", Tier::Free, Codec::Flac).is_err(),
        "Free tier must refuse FLAC (lossless) — FR-042/043"
    );
    assert!(
        QuicAudioSink::connect("127.0.0.1:1", Tier::Free, Codec::Pcm).is_err(),
        "Free tier must refuse PCM (lossless) — FR-042/043"
    );
}

#[test]
fn sink_lossless_flac_44_1k_roundtrip_hash_preserved() {
    // A 44.1 kHz capture (very common USB DAC / system default). Lossless must
    // carry the TRUE rate on the wire (bit-exact — never resampled), and the
    // receiver (which sniffs the meta from the first frame) decodes to the
    // same bytes.
    let rate = 44_100;
    let (addr, handle) = spawn_receiver_server(Codec::Flac, rate);
    let mut sink = QuicAudioSink::connect_with_format(
        &addr.to_string(),
        Tier::Pro,
        Codec::Flac,
        SinkFormat {
            sample_rate: rate,
            channels: 2,
            sample_repr: SampleRepr::I16,
            channel_layout: ChannelLayout::Stereo,
        },
    )
    .expect("pro/flac @44.1k sink");
    assert_eq!(sink.captured_sample_rate(), rate);
    assert!(!sink.resampled(), "lossless is never resampled");
    assert_eq!(sink.wire_sample_rate(), rate, "true rate on the wire");

    let (src_hash, packets) = feed_fixture_hash(&mut sink, FixtureKind::PseudoRandomPcm, rate, 8);
    sink.finish().expect("end marker");

    let outcome = handle.join().expect("server join");
    assert_eq!(
        outcome.metrics.packets_recv, packets,
        "all 44.1k frames delivered"
    );
    assert_eq!(
        outcome.hash_hex(),
        src_hash,
        "lossless 44.1k roundtrip is hash-exact"
    );
    assert_eq!(outcome.metrics.fatal_count, 0);
    assert_eq!(outcome.metrics.underruns, 0);
}

#[test]
fn sink_opus_44_1k_native_adapter_no_resample() {
    // OpusAdapter handles 44.1k natively (882-sample 20 ms frames, internally
    // upsampled to the 48k encoder) — no worker resampler needed, wire = 44.1k.
    let rate = 44_100;
    let (addr, handle) = spawn_receiver_server(Codec::Opus, rate);
    let mut sink =
        QuicAudioSink::connect(&addr.to_string(), Tier::Free, Codec::Opus).expect("free/opus sink");
    sink.on_format(SinkFormat {
        sample_rate: rate,
        channels: 2,
        sample_repr: SampleRepr::I16,
        channel_layout: ChannelLayout::Stereo,
    })
    .expect("44.1k opus accepted");
    assert!(!sink.resampled());
    assert_eq!(sink.wire_sample_rate(), rate);
    assert_eq!(sink.frame_samples(), 882, "20 ms @44.1k");

    feed_fixture_hash(&mut sink, FixtureKind::PseudoRandomPcm, rate, 6);
    sink.finish().expect("end marker");
    let packets = sink.packets_sent();

    let outcome = handle.join().expect("server join");
    assert!(outcome.metrics.packets_recv >= packets);
    assert_eq!(
        outcome.metrics.fatal_count, 0,
        "44.1k opus decoded at the receiver"
    );
}

#[test]
fn sink_opus_odd_rate_is_resampled_to_48k() {
    // SCK can deliver 24 kHz; Opus takes 44.1/48k natively, so the worker
    // resampler normalizes the odd rate to 48k and the lane runs at 48k.
    let rate = 24_000;
    let (addr, handle) = spawn_receiver_server(Codec::Opus, 48_000);
    let mut sink =
        QuicAudioSink::connect(&addr.to_string(), Tier::Free, Codec::Opus).expect("free/opus sink");
    sink.on_format(SinkFormat {
        sample_rate: rate,
        channels: 2,
        sample_repr: SampleRepr::I16,
        channel_layout: ChannelLayout::Stereo,
    })
    .expect("24k accepted via the worker resampler");
    assert!(sink.resampled(), "odd rate engages the resampler");
    assert_eq!(sink.wire_sample_rate(), 48_000);

    feed_fixture_hash(&mut sink, FixtureKind::PseudoRandomPcm, rate, 6);
    sink.finish().expect("end marker");
    let packets = sink.packets_sent();
    assert!(packets >= 6, "resampling never drops whole frames");

    let outcome = handle.join().expect("server join");
    assert!(outcome.metrics.packets_recv >= packets);
    assert_eq!(
        outcome.metrics.fatal_count, 0,
        "48k opus decoded at the receiver"
    );
}
