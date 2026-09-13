//! ADR-005 lossless codec benchmark (task t-P1-bench) — real measurements,
//! not estimates.
//!
//! Measures, for {Flac, Pcm} x {44100, 48000} x {16-bit i16 stereo} over the
//! deterministic `wdr_fakes` synthetic corpus:
//!   * encode throughput (MB/s) + single-frame p50/p99 encode time
//!   * decode throughput + single-frame p50/p99 decode time
//!   * output size / compression ratio / on-wire bandwidth
//!
//! Corpus (all synthetic — see the 24-bit / real-music notes below):
//! `silence`, `sine-sweep-20..20000` (realistic material), `full-scale-edge`,
//! `pseudo-random-pcm` (worst-case incompressible), `impulse-train-32`.
//!
//! Frame sizes: the ADR-005 small-block default (240 spc ~5 ms @48k) and the
//! frame-size-calculator-derived max that fits the 4 KiB payload
//! (`max_frame_samples(48k, 2ch, 2B, 4096)` = 1024). A 512-spc cell is added
//! for the incompressible fixture because FLAC noise @1024 = 4192 B > 4 KiB,
//! which the codec adapter rejects on decode (`PayloadTooLarge`); 512 is the
//! true protocol-capable incompressible ceiling (t-B0-codec matrix).
//!
//! # 24-bit status (explicit, ADR-005 "16/24-bit first")
//!
//! The FLAC adapter implements 16-bit only today (`FlacAdapter::new(.., 24)`
//! returns `Unsupported`; `SampleRepr::I24Packed` is a stub) and `PcmAdapter`
//! is i16-only. The 24-bit (i24-low-3-packed) cells are therefore reported as
//! **PENDING** and this benchmark measures the i16 surface only, with an
//! explicit note. 24-bit raw upper bounds are given analytically as notes, not
//! measurements.
//!
//! # Real-music corpus (explicit)
//!
//! The crate ships no music fixture. Per the task, no copyrighted audio is
//! downloaded; the corpus is labelled synthetic and "add a licensed
//! real-music corpus" is a PENDING follow-up. The sine-sweep is used as the
//! realistic-material proxy; the pseudo-random-PCM fixture is the worst case.

use std::hint::black_box;
use std::time::{Duration, Instant};

use criterion::{criterion_group, Criterion, Throughput};

use wdr_codec::adapters::{CodecAdapter, FlacAdapter, PcmAdapter};
use wdr_codec::size::MAX_FRAME_PAYLOAD;
use wdr_codec::CodecKind;
use wdr_fakes::source::{ChannelKind, Fixture, FixtureKind, PcmSource, SampleFormat};

const RATES: &[u32] = &[44_100, 48_000];
const CHANNELS: u16 = 2;
const BITS: u16 = 16;
/// ADR-005 small-block default (flac.rs `FLAC_BLOCK_DEFAULT`): ~5 ms @48k.
const BLOCK_DEFAULT: usize = 240;
/// Protocol-capable incompressible FLAC ceiling (t-B0-codec: noise@512 = 2144 B).
const BLOCK_512: usize = 512;
/// Frame-size-calculator max for 16-bit stereo @4 KiB (size.rs: 4096/(2*2)).
const BLOCK_MAX: usize = 1024;
/// Manual-matrix iterations per cell-direction.
const MATRIX_ITERS: u32 = 3000;

const FIXTURES: &[(FixtureKind, &str)] = &[
    (FixtureKind::Silence, "silence"),
    (
        FixtureKind::SineSweep {
            f0: 20.0,
            f1: 20_000.0,
        },
        "sine-sweep-20-20000",
    ),
    (FixtureKind::FullScaleEdge, "full-scale-edge"),
    (FixtureKind::PseudoRandomPcm, "pseudo-random-pcm"),
    (FixtureKind::ImpulseTrain { period: 32 }, "impulse-train-32"),
];

/// A benchmark cell input: either a `wdr_fakes` fixture or the true
/// uniform-i16 incompressible fixture.
enum Cell<'a> {
    WdrFakes(&'a FixtureKind, &'a str),
    Noise(&'a str),
}

impl Cell<'_> {
    fn label(&self) -> String {
        match self {
            Cell::WdrFakes(_, l) => (*l).to_string(),
            Cell::Noise(l) => (*l).to_string(),
        }
    }
}

const NOISE_LABEL: &str = "noise-i16-incompressible";

/// All cells: the deterministic wdr_fakes corpus + the ADR-005 worst case.
fn cells() -> Vec<Cell<'static>> {
    let mut v: Vec<Cell<'static>> = FIXTURES.iter().map(|(k, l)| Cell::WdrFakes(k, l)).collect();
    v.push(Cell::Noise(NOISE_LABEL));
    v
}

/// True uniform-i16 incompressible fixture: `wdr_codec::noise_fixture`
/// (splitmix32 high bits), the same seed the ADR-005 spot-check matrix
/// (t-B0-codec) uses. This is the worst case FLAC was sized for: 240spc =
/// 1057 B, 512spc = 2144 B, 1024spc = 4192 B (>4 KiB, decode rejected).
fn noise_i16(frame_size: usize) -> Vec<i16> {
    wdr_codec::noise_fixture(frame_size, usize::from(CHANNELS), 42)
}

fn cell_frame(cell: &Cell<'_>, rate: u32, frame_size: usize) -> Vec<i16> {
    match cell {
        Cell::WdrFakes(kind, _) => {
            // `next_chunk` counts *total* samples; request frames*channels so
            // the returned interleaved i16 buffer is exactly `frame_size`
            // samples per channel.
            let total = (frame_size * usize::from(CHANNELS)) as u64;
            let mut fx = Fixture::new(
                (*kind).clone(),
                SampleFormat::I16,
                rate,
                ChannelKind::Stereo,
                total,
            );
            let got = fx.next_chunk((frame_size * usize::from(CHANNELS)) as u32);
            assert_eq!(
                got.len,
                frame_size * usize::from(CHANNELS),
                "fixture total sample count"
            );
            got.bytes
                .as_chunks::<2>()
                .0
                .iter()
                .map(|b| i16::from_le_bytes(*b))
                .collect()
        }
        Cell::Noise(_) => noise_i16(frame_size),
    }
}

fn make_encoder(codec: CodecKind, rate: u32, block: usize) -> Box<dyn CodecAdapter> {
    match codec {
        CodecKind::Flac => Box::new(
            FlacAdapter::new(rate, CHANNELS, BITS)
                .expect("FLAC 16-bit adapter")
                .with_block(block.max(1)),
        ),
        CodecKind::Pcm => Box::new(PcmAdapter::new(CHANNELS).expect("PCM adapter")),
        CodecKind::Opus => unreachable!("Opus is out of scope for this benchmark"),
    }
}

fn make_decoder(codec: CodecKind, rate: u32) -> Box<dyn CodecAdapter> {
    match codec {
        CodecKind::Flac => {
            Box::new(FlacAdapter::new(rate, CHANNELS, BITS).expect("FLAC 16-bit adapter"))
        }
        CodecKind::Pcm => Box::new(PcmAdapter::new(CHANNELS).expect("PCM adapter")),
        CodecKind::Opus => unreachable!("Opus is out of scope for this benchmark"),
    }
}

fn codec_tag(codec: CodecKind) -> &'static str {
    match codec {
        CodecKind::Flac => "Flac",
        CodecKind::Pcm => "Pcm",
        CodecKind::Opus => "Opus",
    }
}

fn p50(p: &mut [f64]) -> f64 {
    p.sort_by(|a, b| a.partial_cmp(b).unwrap());
    p[p.len() / 2]
}
fn p99(p: &mut [f64]) -> f64 {
    p.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let idx = ((p.len() as f64) * 0.99).ceil() as usize - 1;
    p[idx.min(p.len() - 1)]
}

/// Criterion encode/decode throughput benches (compliance + numbers).
fn codec_benches(c: &mut Criterion) {
    for codec in [CodecKind::Flac, CodecKind::Pcm] {
        for &rate in RATES {
            for frame_size in [BLOCK_DEFAULT, BLOCK_512, BLOCK_MAX] {
                for cell in cells() {
                    let pcm = cell_frame(&cell, rate, frame_size);
                    let n = usize::from(CHANNELS) * frame_size;
                    let raw_bytes = n * 2;
                    let mut enc = make_encoder(codec, rate, frame_size);
                    let encoded = enc.encode(&pcm).expect("encode");
                    let out_bytes = encoded.len();
                    let in_bytes = raw_bytes as u64;
                    let out_u64 = out_bytes as u64;
                    let tag = format!(
                        "{}/{}/{}bx/{}",
                        codec_tag(codec),
                        rate,
                        frame_size,
                        cell.label()
                    );

                    let mut g = c.benchmark_group(tag);
                    g.throughput(Throughput::Bytes(in_bytes));
                    g.bench_function("encode", |b| {
                        b.iter_batched(
                            || make_encoder(codec, rate, frame_size),
                            |mut a| {
                                black_box(a.encode(&pcm).expect("encode"));
                            },
                            criterion::BatchSize::SmallInput,
                        );
                    });
                    g.finish();

                    // Decode only where the frame fits the 4 KiB adapter cap.
                    if out_bytes <= MAX_FRAME_PAYLOAD {
                        let mut g2 = c.benchmark_group(format!(
                            "{}/{}/{}bx/{}",
                            codec_tag(codec),
                            rate,
                            frame_size,
                            cell.label()
                        ));
                        g2.throughput(Throughput::Bytes(out_u64));
                        g2.bench_function("decode", |b| {
                            b.iter_batched(
                                || make_decoder(codec, rate),
                                |mut a| {
                                    black_box(a.decode(&encoded).expect("decode"));
                                },
                                criterion::BatchSize::SmallInput,
                            );
                        });
                        g2.finish();
                    }
                }
            }
        }
    }
}

/// Deterministic per-frame p50/p99 matrix (a second, independent measurement
/// of the same cells, cheap and stable). Emits TSV rows to stdout.
fn print_matrix() {
    println!("### ADR-005 benchmark matrix (i16 stereo; synthetic corpus)");
    println!(
        "codec\trate_hz\tbits\tframe_size\tfixture\tout_bytes\tratio\tbps@rate\t\
         enc_p50_us\tenc_p99_us\tdec_p50_us\tdec_p99_us\talgo_delay_ms\tnote"
    );
    for codec in [CodecKind::Flac, CodecKind::Pcm] {
        for &rate in RATES {
            for frame_size in [BLOCK_DEFAULT, BLOCK_512, BLOCK_MAX] {
                let delay_ms = frame_size as f64 / f64::from(rate) * 1e3;
                for cell in cells() {
                    let pcm = cell_frame(&cell, rate, frame_size);
                    let fname = cell.label();
                    let n = usize::from(CHANNELS) * frame_size;
                    let raw_bytes = n * 2;

                    let mut enc = make_encoder(codec, rate, frame_size);
                    let encoded = enc.encode(&pcm).expect("encode");
                    let out_bytes = encoded.len();
                    let ratio = out_bytes as f64 / raw_bytes as f64;
                    let rate_f = f64::from(rate);
                    let frames_per_sec = rate_f / frame_size as f64;
                    let bps = out_bytes as f64 * frames_per_sec;

                    // Encode timing (fresh adapter per iteration = real path).
                    let mut t_enc: Vec<f64> = Vec::with_capacity(MATRIX_ITERS as usize);
                    for _ in 0..MATRIX_ITERS {
                        let mut a = make_encoder(codec, rate, frame_size);
                        let t0 = Instant::now();
                        let out = a.encode(&pcm).expect("encode");
                        t_enc.push(t0.elapsed().as_secs_f64() * 1e6);
                        black_box(out);
                    }
                    let enc_p50 = p50(&mut t_enc);
                    let enc_p99 = p99(&mut t_enc);

                    let dec_feasible = out_bytes <= MAX_FRAME_PAYLOAD;
                    let (dec_p50, dec_p99, note) = if dec_feasible {
                        let mut t_dec: Vec<f64> = Vec::with_capacity(MATRIX_ITERS as usize);
                        for _ in 0..MATRIX_ITERS {
                            let mut a = make_decoder(codec, rate);
                            let t0 = Instant::now();
                            let d = a.decode(&encoded).expect("decode");
                            t_dec.push(t0.elapsed().as_secs_f64() * 1e6);
                            black_box(d);
                        }
                        (p50(&mut t_dec), p99(&mut t_dec), "-")
                    } else {
                        (0.0, 0.0, "decode-N/A: out_bytes>4096 adapter cap")
                    };

                    let mut row = format!(
                        "{}\t{}\t{}\t{}\t{}\t{}\t{:.3}\t{:.0}\t{:.2}\t{:.2}\t{:.2}\t{:.2}\t{:.2}\t{}",
                        codec_tag(codec),
                        rate,
                        BITS,
                        frame_size,
                        fname,
                        out_bytes,
                        ratio,
                        bps,
                        enc_p50,
                        enc_p99,
                        dec_p50,
                        dec_p99,
                        delay_ms,
                        note,
                    );
                    let _ = &mut row;
                    println!("{row}");
                }
            }
        }
    }
    println!(
        "notes: corpus=synthetic-only (no licensed real-music fixture; PENDING follow-up). \
         24-bit=PENDING (FlacAdapter/PcmAdapter are i16-only; I24Packed stub). \
         noise-i16-incompressible = wdr_codec::noise_fixture (splitmix32, ADR-005 spot-check seed). \
         pseudo-random-pcm = wdr_fakes ChaCha12 fixture, clipped to i16 (compresses more than true noise)."
    );
}

criterion_group! {
    name = adr005;
    config = Criterion::default()
        .sample_size(10)
        .warm_up_time(Duration::from_millis(200))
        .measurement_time(Duration::from_millis(500))
        .nresamples(1000);
    targets = codec_benches
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--matrix") {
        print_matrix();
        return;
    }
    let mut c: Criterion = Criterion::default()
        .sample_size(10)
        .warm_up_time(Duration::from_millis(200))
        .measurement_time(Duration::from_millis(500))
        .nresamples(1000);
    codec_benches(&mut c);
    c.final_summary();
}
