//! Host reference-loopback latency probe (WS-H measurements).
//!
//! Times `QuicAudioSink` → `QuicRenderReceiver` per frame over loopback for a
//! `--profile × --runs` matrix, printing first-frame latency + p50/p95/p99 of
//! the per-frame latency distribution. This is the **simulated / upper-bound**
//! reference-loopback measurement (LATENCY_MEASUREMENT.md discipline); the §11
//! in-code T0..T3 hooks are a separate follow-up, and physical-device SLO
//! probes (Balanced ≤150 ms / Low ≤80 ms over a real DAC) remain device-gated.
//!
//! Usage: `cargo run -p wdr_refsim --release --example latprobe -- \
//!   --profile balanced --runs 5 --frames 256`

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use wdr_entitlement::provider::Tier;
use wdr_fakes::source::{ChannelKind, Fixture, FixtureKind, PcmSource, SampleFormat};
use wdr_proto::Codec;
use wdr_refsim::receiver::BufferProfile;
use wdr_refsim::sink::{
    AudioFrameSink, QuicAudioSink, QuicRenderReceiver, RenderSink, SinkError, SinkFormat,
};

/// A [`RenderSink`] that records the wall-clock instant of every decoded
/// `on_block` (the render presentation time, T3 proxy). `Send` (Arc<Mutex>)
/// so the readout lives on the main thread while the sink is on the receive
/// thread.
struct TimestampSink {
    stamps: Arc<Mutex<Vec<Instant>>>,
}

impl RenderSink for TimestampSink {
    fn on_format(&mut self, _fmt: SinkFormat) -> Result<(), SinkError> {
        Ok(())
    }
    fn on_block(&mut self, _bytes: &[u8]) -> Result<(), SinkError> {
        self.stamps.lock().unwrap().push(Instant::now());
        Ok(())
    }
    fn finish(&mut self) -> Result<(), SinkError> {
        Ok(())
    }
}

fn percentile(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let idx = ((sorted.len() as f64) * p).ceil() as usize - 1;
    sorted[idx.min(sorted.len() - 1)]
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mut profile = String::from("balanced");
    let mut runs = 5usize;
    let mut frames = 256usize;
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--profile" => {
                profile = args.get(i + 1).expect("profile value").clone();
                i += 2;
            }
            "--runs" => {
                runs = args
                    .get(i + 1)
                    .expect("runs value")
                    .parse()
                    .expect("runs usize");
                i += 2;
            }
            "--frames" => {
                frames = args
                    .get(i + 1)
                    .expect("frames value")
                    .parse()
                    .expect("frames usize");
                i += 2;
            }
            other => {
                eprintln!("unknown arg {other}");
                std::process::exit(2);
            }
        }
    }
    let bp = match profile.as_str() {
        "low" => BufferProfile::Low,
        "balanced" => BufferProfile::Balanced,
        "resilient" => BufferProfile::Resilient,
        other => {
            eprintln!("unknown buffer profile '{other}' (low|balanced|resilient)");
            std::process::exit(2);
        }
    };

    let mut all_first_ms: Vec<f64> = Vec::new();
    let mut all_lat: Vec<f64> = Vec::new();

    for _ in 0..runs {
        // A FRESH fixture per run (a Fixture exhausts after its total budget;
        // reuse would send zero frames on the next run → end marker first).
        let mut fixture = Fixture::new(
            FixtureKind::PseudoRandomPcm,
            SampleFormat::I16,
            48_000,
            ChannelKind::Stereo,
            (frames * 512 * 2) as u64,
        );
        let addr: std::net::SocketAddr = "127.0.0.1:0".parse().expect("loopback addr");
        let stamps = Arc::new(Mutex::new(Vec::new()));
        let mut receiver = QuicRenderReceiver::listen(
            addr,
            bp,
            Box::new(TimestampSink {
                stamps: stamps.clone(),
            }),
        )
        .expect("listen");

        let mut sink =
            QuicAudioSink::connect(&receiver.local_addr().to_string(), Tier::Pro, Codec::Flac)
                .expect("sink connect");
        sink.on_format(SinkFormat::canonical()).expect("format");

        // One whole 512-sample frame (1024 values) per on_block, timestamped at
        // publish (T0-side); the render sink stamps each decoded frame (T3-side).
        let mut send_stamps: Vec<Instant> = Vec::with_capacity(frames);
        for _ in 0..frames {
            let chunk = fixture.next_chunk(1024);
            debug_assert_eq!(chunk.len, 1024, "one whole stereo frame");
            send_stamps.push(Instant::now());
            sink.on_block(chunk.bytes).expect("block");
        }
        sink.finish().expect("finish");

        let outcome = match receiver.wait(Duration::from_secs(20)) {
            Ok(o) => o,
            Err(e) => {
                eprintln!("latprobe wait error: {e}");
                std::process::exit(3);
            }
        };
        assert_eq!(
            outcome.metrics.packets_recv as usize, frames,
            "all frames rendered"
        );

        let recv_stamps = stamps.lock().unwrap();
        assert_eq!(recv_stamps.len(), frames, "one stamp per rendered frame");
        let first_ms = recv_stamps[0]
            .saturating_duration_since(send_stamps[0])
            .as_secs_f64()
            * 1e3;
        all_first_ms.push(first_ms);
        for (s, r) in send_stamps.iter().zip(recv_stamps.iter()) {
            all_lat.push(r.saturating_duration_since(*s).as_secs_f64() * 1e3);
        }
    }

    let mut sorted = all_lat.clone();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let mut first_sorted = all_first_ms.clone();
    first_sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
    println!("latprobe profile={profile} runs={runs} frames={frames}");
    println!(
        "  first_frame_to_render_ms: p50={:.3} p95={:.3} p99={:.3} ({} runs)",
        percentile(&first_sorted, 0.50),
        percentile(&first_sorted, 0.95),
        percentile(&first_sorted, 0.99),
        runs
    );
    println!(
        "  per_frame_latency_ms:    p50={:.3} p95={:.3} p99={:.3} (n={})",
        percentile(&sorted, 0.50),
        percentile(&sorted, 0.95),
        percentile(&sorted, 0.99),
        sorted.len()
    );
}
