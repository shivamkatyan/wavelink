//! Fake-adapter behaviour: underrun detection driven by an injected clock,
//! plus determinism of the captured stream.

use wdr_fakes::adapters::{
    CaptureSource, FakeCaptureSource, FakeClock, FakeRenderSink, FakeRenderSinkStats,
};
use wdr_fakes::hash::HashSinkState;
use wdr_fakes::source::SourceFormat;
use wdr_fakes::{CaptureStem, FakeCaptureSourceConfig};

mod common;

#[test]
fn fake_render_underrun_fires() {
    // A slow clock: each chunk of 480 samples at 48 kHz represents 10 ms, but
    // the caller only feeds one every 30 ms → the device is starved.
    let clock = FakeClock::at(0);
    let mut sink = FakeRenderSink::new(clock.clone(), SourceFormat::stereo_i16_48k());

    // First chunk at t=0.
    sink.render_chunk(&[0u8; 480 * 2], 480)
        .expect("first chunk ok");

    // Second chunk 30 ms later: 30 ms × 48 samples/ms = 1440 > 480 buffered.
    clock.set_now_ms(30);
    assert!(
        sink.render_chunk(&[0u8; 480 * 2], 480).is_err(),
        "30ms gap at 48kHz (10ms of audio per chunk) must underrun"
    );
}

#[test]
fn fake_render_underrun_reports_stats() {
    let clock = FakeClock::at(0);
    let mut sink = FakeRenderSink::new(clock.clone(), SourceFormat::stereo_i16_48k());

    sink.render_chunk(&[0u8; 480 * 2], 480).unwrap();
    clock.advance_ms(60); // 120ms of audio elapsed after 10ms of supply → starved
    let err = sink.render_chunk(&[0u8; 480 * 2], 480).unwrap_err();
    assert_eq!(err.at_ms, 60);
    assert_eq!(err.waited_ms, 60);
    assert_eq!(err.buffered_before, 480); // 480 held but 2880 needed
    let stats: &FakeRenderSinkStats = sink.stats();
    assert_eq!(stats.underruns, 1);
    assert!(stats.last_underrun.is_some());
}

#[test]
fn fake_capture_is_deterministic_and_hashable() {
    let cfg_a = FakeCaptureSourceConfig {
        format: SourceFormat::stereo_i16_48k(),
        stem: CaptureStem::Impulse { period: 64 },
        fail_on_capture: false,
        cancel_on_next: false,
    };
    let mut a = FakeCaptureSource::new(cfg_a);
    let mut b = FakeCaptureSource::new(cfg_a);
    a.open().unwrap();
    b.open().unwrap();

    let mut ha = HashSinkState::default();
    let mut hb = HashSinkState::default();
    for _ in 0..8 {
        let (bytes_a, _) = a.capture_chunk(480).unwrap();
        let (bytes_b, _) = b.capture_chunk(480).unwrap();
        ha.update_bytes(&bytes_a);
        hb.update_bytes(&bytes_b);
    }
    assert_eq!(ha.hex(), hb.hex());
    assert_eq!(a.produced(), b.produced());
}

#[test]
fn fake_capture_failure_and_cancel() {
    let cfg = FakeCaptureSourceConfig {
        format: SourceFormat::stereo_i16_48k(),
        stem: CaptureStem::Silence,
        fail_on_capture: true,
        cancel_on_next: false,
    };
    let mut s = FakeCaptureSource::new(cfg);
    assert!(s.open().is_err());

    let mut s = FakeCaptureSource::new(FakeCaptureSourceConfig {
        format: SourceFormat::stereo_i16_48k(),
        stem: CaptureStem::Silence,
        fail_on_capture: false,
        cancel_on_next: true,
    });
    s.open().unwrap();
    assert!(s.capture_chunk(480).is_err());
}
