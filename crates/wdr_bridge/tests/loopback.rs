//! Bridge host proof (WS-G, BRIDGE_PLAN step 1): the uniffi-exported control
//! surface drives the real `QuicAudioSink` engine to a live `QuicRenderReceiver`
//! and the decoded stream must equal the canonical golden hash. This calls the
//! `#[uniffi::export]` functions directly (Rust) — no NDK, no JVM needed; the
//! android `.so` at real ABIs is the `android-ci`/device gate.

use std::time::Duration;

use wdr_bridge::{EngineHandle, EngineStatus};
use wdr_fakes::source::{ChannelKind, Fixture, FixtureKind, PcmSource, SampleFormat};
use wdr_refsim::receiver::BufferProfile;
use wdr_refsim::receiver_server;
use wdr_refsim::sink::QuicRenderReceiver;

const GOLDEN_PRNG_I16_48K_STEREO: &str =
    "b7a3c25c8ccaa05643f14dfb5bc0e223f4ace3154c11ecd25bed5975d839c223";

#[test]
fn bridge_engine_streams_lossless_golden_over_loopback() {
    let addr: std::net::SocketAddr = "127.0.0.1:0".parse().expect("loopback addr");
    let mut receiver = QuicRenderReceiver::listen(
        addr,
        BufferProfile::Balanced,
        receiver_server::null_render_sink(),
    )
    .expect("listen");

    let engine = EngineHandle::new();
    assert_eq!(engine.status(), EngineStatus::Idle);
    engine
        .start(receiver.local_addr().to_string(), 1, 0, None)
        .expect("start (pro/flac)");
    assert_eq!(engine.status(), EngineStatus::Streaming);

    engine
        .on_format(48_000, 2, 0)
        .expect("on_format (i16/stereo)");

    // Canonical 4096-value fixture in one block; the engine accumulates to
    // whole 512-sample frames (4 frames here).
    let mut fixture = Fixture::new(
        FixtureKind::PseudoRandomPcm,
        SampleFormat::I16,
        48_000,
        ChannelKind::Stereo,
        4096,
    );
    let chunk = fixture.next_chunk(4096);
    engine.on_block(chunk.bytes.to_vec()).expect("on_block");

    let report = engine.finish().expect("finish");
    assert_eq!(report.packets_sent, 4, "4 whole FLAC frames");
    assert!(report.bytes_sent > 0, "bytes actually sent");
    assert_eq!(engine.status(), EngineStatus::Finished);

    let outcome = receiver
        .wait(Duration::from_secs(15))
        .expect("receiver outcome");
    assert_eq!(
        outcome.hash_hex(),
        GOLDEN_PRNG_I16_48K_STEREO,
        "the FFI-driven engine must hash to the canonical golden"
    );
    assert_eq!(outcome.metrics.packets_recv, 4);
    assert!(
        !outcome.metrics.secured,
        "plain lane (no fingerprint) stays unsecured"
    );
    assert_eq!(outcome.metrics.underruns, 0);
}

#[test]
fn bridge_engine_errors_are_typed_not_panics() {
    let engine = EngineHandle::new();
    // Before start, an on_block is a typed error, never a panic/crash.
    let err = engine.on_block(vec![0u8; 16]);
    assert!(err.is_err(), "on_block before start must error");
    // A bad format code is a typed error.
    engine.on_format(48_000, 2, 99).unwrap_err();
}
