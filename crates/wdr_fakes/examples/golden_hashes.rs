//! Prints the canonical golden hashes for every fixture × format × rate ×
//! channel combination (the values recorded in `t-B0-fakes.md` and asserted by
//! `tests/golden.rs`). Deterministic: same instrument, same integers.
//!
//! ```sh
//! cargo run -p wdr_fakes --example golden_hashes
//! ```

use wdr_fakes::source::{ChannelKind, Fixture, FixtureKind, PcmSource, SampleFormat, Stereo};

const CHUNK: u32 = 512;
const SAMPLES: u64 = 4096;

fn h(kind: FixtureKind, format: SampleFormat, rate_hz: u32, channels: ChannelKind) -> String {
    let mut fx = Fixture::new(kind, format, rate_hz, channels, SAMPLES);
    fx.drain_hash(CHUNK).hex()
}

fn main() {
    let rates = [44_100u32, 48_000];
    let formats = [SampleFormat::I16, SampleFormat::I24];
    let formats_str = ["i16", "i24"];

    let mut rows: Vec<(String, String)> = Vec::new();
    for (fi, format) in formats.iter().copied().enumerate() {
        for &rate in &rates {
            for (ch, ch_str) in [(ChannelKind::Mono, "mono"), (ChannelKind::Stereo, "stereo")] {
                let tag = format!("silence-{}-{rate}-{ch_str}", formats_str[fi]);
                rows.push((tag, h(FixtureKind::Silence, format, rate, ch)));
            }
        }
    }
    for period in [64u32, 256] {
        let tag = format!("impulse-train-{period}");
        rows.push((
            tag,
            h(
                FixtureKind::ImpulseTrain { period },
                SampleFormat::I16,
                48_000,
                ChannelKind::Stereo,
            ),
        ));
    }
    for (f0, f1) in [(20.0, 20_000.0), (200.0, 20_000.0)] {
        let tag = format!("sine-sweep-{f0:.0}-{f1:.0}");
        rows.push((
            tag,
            h(
                FixtureKind::SineSweep { f0, f1 },
                SampleFormat::I16,
                48_000,
                ChannelKind::Stereo,
            ),
        ));
    }
    for (fi, format) in formats.iter().copied().enumerate() {
        for &rate in &rates {
            let tag = format!("full-scale-edge-{}-{rate}-stereo", formats_str[fi]);
            rows.push((
                tag,
                h(
                    FixtureKind::FullScaleEdge,
                    format,
                    rate,
                    ChannelKind::Stereo,
                ),
            ));
        }
    }
    for (fi, format) in formats.iter().copied().enumerate() {
        for &rate in &rates {
            for (ch, ch_str) in [(ChannelKind::Mono, "mono"), (ChannelKind::Stereo, "stereo")] {
                let tag = format!("pseudo-random-{}-{rate}-{ch_str}", formats_str[fi]);
                rows.push((tag, h(FixtureKind::PseudoRandomPcm, format, rate, ch)));
            }
        }
    }
    for (fi, format) in formats.iter().copied().enumerate() {
        for &rate in &rates {
            let tag = format!("channel-id-left-{}-{rate}-stereo", formats_str[fi]);
            rows.push((
                tag,
                h(
                    FixtureKind::ChannelId {
                        lane: Stereo::LeftPattern,
                    },
                    format,
                    rate,
                    ChannelKind::Stereo,
                ),
            ));
            let tag = format!("channel-id-right-{}-{rate}-stereo", formats_str[fi]);
            rows.push((
                tag,
                h(
                    FixtureKind::ChannelId {
                        lane: Stereo::RightPattern,
                    },
                    format,
                    rate,
                    ChannelKind::Stereo,
                ),
            ));
        }
    }

    // Deterministic sort so the output is stable regardless of iteration order.
    rows.sort_unstable_by(|a, b| a.0.cmp(&b.0));

    println!("golden-hashes (blake3, chunk={CHUNK}, samples={SAMPLES})");
    for (tag, hex) in &rows {
        println!("{tag}\t{hex}");
    }
}
