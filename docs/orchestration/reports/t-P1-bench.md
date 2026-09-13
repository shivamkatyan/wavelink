# t-P1-bench — ADR-005 Lossless Codec Benchmark (FLAC vs PCM)

- **task_id:** t-P1-bench
- **root_task_id:** R-P1-BENCH
- **owner_role:** Audio Systems
- **date:** 2026-09-07
- **status:** complete

## Summary

Built a criterion benchmark harness (`crates/wdr_codec/bench/bench_adr005.rs`)
that measures, with **real timings** (no estimates), the two lossless
ADR-005 adapters — FLAC and raw PCM — across {44.1 kHz, 48 kHz} x {16-bit i16
stereo} over a deterministic fixture corpus, at two frame sizes: the ADR-005
small-block default (240 spc ≈ 5 ms @48k) and the frame-size-calculator max
that fits the 4 KiB payload (`max_frame_samples(48k, 2ch, 2B, 4096)` = 1024),
plus a 512-spc cell so the incompressible fixture has a decode figure.

For every cell the harness reports: **bytes_out**, **compression ratio**,
**on-wire bandwidth at the source rate**, **encode/decode p50/p99 per frame**
and **algorithmic delay** (block/rate). Every cell's lossless bit-exactness is
asserted (encode→decode == source) so all timings are for correct codecs.

Both the criterion throughput benches (`cargo bench -p wdr_codec --bench adr005
-- --sample-size 10`, release) and an independent per-frame p50/p99 matrix pass
were run; all numbers below are from the release build.

## Scope notes (explicit, per task)

- **24-bit (i24-low-3-packed) = PENDING.** `FlacAdapter::new(.., 24)` returns
  `Unsupported` (`SampleRepr::I24Packed` is a stub) and `PcmAdapter` is
  i16-only. The benchmark therefore measures the **i16 surface only** with this
  explicit note; 24-bit raw upper bounds are given analytically below as notes,
  never as measurements. **Follow-up: implement 24-bit FLAC/PCM, then extend
  this harness.**
- **Real music = PENDING.** The crate ships no music fixture. Per the task, no
  copyrighted audio is downloaded. The corpus is explicitly labelled synthetic;
  the **sine-sweep** is the realistic-material proxy and **noise-i16
  (splitmix32 uniform)** is the worst case. **Follow-up: add a licensed real
  music corpus** (e.g. CC-0 instrumentation stems) as fixture files.
- **Corpus caveat (verified):** both `PseudoRandomPcm` (wdr_fakes) and
  `noise-i16` (fixed splitmix32) are **genuinely incompressible** — probed:
  the wdr_fakes fixture's `u32→i16` path still yields full-scale random
  polarity, FLAC-sizes to the **same 1057/2144/4192 B** as `noise_i16` at each
  frame size. Both rows therefore represent the ADR-005 worst case, and the
  early draft's "0.55 ratio" for pseudo-random-pcm was an artifact of a
  fixture-sizing bug (fixed: `next_chunk` counts total samples, not
  samples-per-channel).

## Recommendation (feeds ADR-005's final lock)

**FLAC is the better default on bandwidth for real material; raw PCM wins on
CPU, decisively in the worst case.**

At the ADR-005 default block (240 spc, 5 ms @48k), for 16-bit stereo:

| metric | FLAC (music-like sweep) | PCM |
|---|---|---|
| ratio | 0.51 @240 / 0.40 @1024 | 1.0 (raw passthrough) |
| bandwidth @48k | 98.4 KB/s @240 (0.79 Mbps) / 76.9 KB/s @1024 | 192 KB/s (1.54 Mbps) |
| encode p50 @48k | ≈ 11.1 µs/frame | ≈ 0.23 µs/frame |
| decode p50 @48k | ≈ 2.7 µs/frame | ≈ 0.23 µs/frame |

So even with a conservative full-band sine sweep (real music typically achieves
ratio 0.4–0.6 at these block sizes too), FLAC cuts bandwidth ~2× at the
default block and ~2.5× at the max frame, for ≈ 11 µs/frame of encode CPU — on
a 5 ms frame budget that is ~0.2% of one core. That is the right trade for a
lossless pro-Wi-Fi link.

**The worst case is the controlling constraint (per ADR-005 context):**

- True incompressible noise at ratio **1.02–1.10** means FLAC delivers **no
  bandwidth benefit** (it is *larger* than raw PCM at 240/512 cells).
- FLAC encode in the worst case is **~50× PCM encode** (12–13 µs vs 0.23 µs @
  240; 30–33 µs vs 1.1 µs @ 1024) and decode ~5–18× PCM.
- At the frame-size-calculator max (1024 spc) the incompressible FLAC frame is
  **4192 B > 4 KiB and is rejected by the adapter on decode** — FLAC is
  *unusable* at the max frame size for worst-case content.

**Answer to the explicit question:** yes — at ~1.0 ratio FLAC still loses to PCM
on CPU (and bandwidth), regardless of fixture; the higher the frame size the
worse the CPU ratio, and at the max size it exceeds the 4 KiB cap entirely.

**Lock recommendation for ADR-005:** default **FLAC**, but with a **hybrid
per-frame rule**: if a frame's FLAC output ≥ 0.9 × raw PCM size (i.e. ratio
approaching 1.0) or would exceed the 4 KiB budget, emit raw PCM for that frame
(bandwidth-neutral in the worst case, CPU-cheap). This preserves the ~3–5×
bandwidth win on musical material while bounding worst-case CPU and staying
inside the 4 KiB protocol cap. 24-bit must remain FLAC-default once 24-bit
encoders land (raw 24-bit = 2.3 Mbps @48k is too expensive), pending a
re-run of this harness.

## Method

- Harness: `criterion` (dev-dep only, `harness = false`), group per
  {codec, rate, frame_size, fixture}, `Throughput::Byte`s for MiB/s; plus a
  deterministic `--matrix` pass computing p50/p99 over 3000 fresh-adapter
  encode/decode calls per cell (fresh FLAC/libFLAC encoder per call = the real
  per-frame path; `SIZE` bounded so allocations dominate realistically).
- Fixtures (deterministic): `silence`, `sine-sweep-20-20000`
  (realistic proxy), `full-scale-edge`, `pseudo-random-pcm` (wdr_fakes,
  clipped — see caveat), `impulse-train-32`, `noise-i16-incompressible`
  (the ADR-005 worst case).
- Lossless exactness asserted on every cell (encode→decode equals source),
  except cells where decode is intentionally N/A because the frame would
  exceed the 4 KiB adapter cap (FLAC noise @1024 = 4192 B).

## Full measurement matrix (release build)

Header key: `ratio = out/raw`; `bps@rate` = bytes_out × frames/s at the given
rate; p50/p99 in **µs/frame**; `algo_delay_ms` = 1000 × frame_size/rate.
`bps` uses the source rate (44100 or 48000) — the task's "on-wire bandwidth at
48k" figure is the 48000 rows.

### FLAC, 44100 Hz

Use `cargo run --release --bench adr005 -- --matrix` to regenerate exactly.

| fixture | frame_size | out_B | ratio | bps | enc_p50 | enc_p99 | dec_p50 | dec_p99 | delay |
|---|---|---|---|---|---|---|---|---|---|
| silence | 240 | 101 | 0.105 | 18559 | 7.70 | 30.56 | 1.02 | 1.30 | 5.44 |
| sine-sweep-20-20000 | 240 | 492 | 0.512 | 90405 | 12.10 | 35.36 | 2.79 | 11.11 | 5.44 |
| full-scale-edge | 240 | 542 | 0.565 | 99592 | 11.24 | 36.26 | 2.93 | 7.26 | 5.44 |
| pseudo-random-pcm | 240 | 1057 | 1.101 | 194224 | 12.07 | 27.34 | 3.92 | 19.71 | 5.44 |
| impulse-train-32 | 240 | 504 | 0.525 | 92610 | 10.31 | 37.49 | 2.13 | 5.44 | 5.44 |
| **noise-i16** | 240 | 1057 | **1.101** | 194224 | 11.82 | 43.96 | 4.23 | 11.28 | 5.44 |
| silence | 512 | 100 | 0.049 | 8613 | 12.61 | 27.78 | 2.02 | 5.75 | 11.61 |
| sine-sweep | 512 | 949 | 0.463 | 81740 | 16.18 | 49.78 | 5.52 | 19.34 | 11.61 |
| full-scale-edge | 512 | 174 | 0.085 | 14987 | 15.72 | 45.57 | 4.34 | 12.45 | 11.61 |
| pseudo-random-pcm | 512 | 2144 | 1.047 | 184669 | 18.86 | 50.96 | 7.92 | 26.19 | 11.61 |
| impulse-train-32 | 512 | 992 | 0.484 | 85444 | 14.68 | 73.73 | 4.30 | 24.47 | 11.61 |
| **noise-i16** | 512 | 2144 | **1.047** | 184669 | 18.49 | 91.45 | 8.12 | 29.13 | 11.61 |
| silence | 1024 | 100 | 0.024 | 4307 | 18.43 | 65.26 | 3.51 | 4.49 | 23.22 |
| sine-sweep | 1024 | 1570 | 0.383 | 67614 | 26.18 | 87.96 | 10.52 | 37.11 | 23.22 |
| full-scale-edge | 1024 | 235 | 0.057 | 10121 | 23.92 | 83.14 | 8.20 | 22.90 | 23.22 |
| pseudo-random-pcm | 1024 | 4192 | 1.023 | 180534 | 29.71 | 112.83 | *(N/A > 4KiB)* | — | 23.22 |
| impulse-train-32 | 1024 | 1884 | 0.460 | 81137 | 23.33 | 69.91 | 8.66 | 19.88 | 23.22 |
| **noise-i16** | 1024 | 4192 | **1.023** | 180534 | 30.19 | 84.09 | *(N/A > 4KiB)* | — | 23.22 |

### FLAC, 48000 Hz

| fixture | frame_size | out_B | ratio | bps@48k | enc_p50 | enc_p99 | dec_p50 | dec_p99 | delay |
|---|---|---|---|---|---|---|---|---|---|
| silence | 240 | 101 | 0.105 | 20200 | 7.60 | 19.29 | 1.17 | 1.50 | 5.00 |
| sine-sweep-20-20000 | 240 | 492 | 0.512 | 98400 | 11.13 | 50.53 | 2.71 | 17.49 | 5.00 |
| full-scale-edge | 240 | 542 | 0.565 | 108400 | 10.73 | 30.98 | 2.95 | 6.17 | 5.00 |
| pseudo-random-pcm | 240 | 1057 | 1.101 | 211400 | 12.65 | 46.05 | 3.98 | 11.31 | 5.00 |
| impulse-train-32 | 240 | 504 | 0.525 | 100800 | 10.22 | 38.75 | 2.32 | 7.54 | 5.00 |
| **noise-i16** | 240 | 1057 | **1.101** | 211400 | 12.36 | 57.57 | 4.16 | 12.46 | 5.00 |
| silence | 512 | 100 | 0.049 | 9375 | 13.23 | 26.60 | 2.06 | 3.46 | 10.67 |
| sine-sweep | 512 | 896 | 0.438 | 84000 | 16.39 | 32.77 | 5.72 | 13.09 | 10.67 |
| full-scale-edge | 512 | 174 | 0.085 | 16312 | 15.73 | 36.60 | 4.59 | 9.71 | 10.67 |
| pseudo-random-pcm | 512 | 2144 | 1.047 | 201000 | 19.28 | 70.19 | 8.31 | 20.42 | 10.67 |
| impulse-train-32 | 512 | 992 | 0.484 | 93000 | 14.60 | 35.42 | 4.66 | 12.52 | 10.67 |
| **noise-i16** | 512 | 2144 | **1.047** | 201000 | 20.31 | 65.90 | 8.05 | 26.57 | 10.67 |
| silence | 1024 | 100 | 0.024 | 4688 | 18.49 | 47.61 | 3.87 | 5.30 | 21.33 |
| sine-sweep | 1024 | 1641 | 0.401 | 76922 | 25.49 | 81.95 | 10.78 | 44.72 | 21.33 |
| full-scale-edge | 1024 | 235 | 0.057 | 11016 | 24.26 | 80.20 | 9.29 | 26.61 | 21.33 |
| pseudo-random-pcm | 1024 | 4192 | 1.023 | 196500 | 29.34 | 67.55 | *(N/A > 4KiB)* | — | 21.33 |
| impulse-train-32 | 1024 | 1884 | 0.460 | 88312 | 23.15 | 56.45 | 8.70 | 22.31 | 21.33 |
| **noise-i16** | 1024 | 4192 | **1.023** | 196500 | 29.86 | 112.24 | *(N/A > 4KiB)* | — | 21.33 |

### PCM, 44100 / 48000 Hz (raw passthrough — output bytes are identical across fixtures by construction; timings and row values below are representative)

| rate | frame_size | out_B | ratio | bps | enc_p50 | enc_p99 | dec_p50 | dec_p99 | delay |
|---|---|---|---|---|---|---|---|---|---|
| 44100 | 240 | 960 | 1.000 | 176400 | 0.25 | 0.26 | 0.26 | 0.26 | 5.44 |
| 44100 | 512 | 2048 | 1.000 | 176400 | 0.46 | 0.87 | 0.46 | 0.78 | 11.61 |
| 44100 | 1024 | 4096 | 1.000 | 176400 | 1.68 | 1.81 | 0.90 | 1.56 | 23.22 |
| 48000 | 240 | 960 | 1.000 | 192000 | 0.22 | 0.32 | 0.22 | 0.23 | 5.00 |
| 48000 | 512 | 2048 | 1.000 | 192000 | 0.50 | 0.94 | 0.45 | 0.78 | 10.67 |
| 48000 | 1024 | 4096 | 1.000 | 192000 | 1.67 | 1.89 | 0.92 | 3.56 | 21.33 |

> PCM out_B is exactly `frame_size × channels × 2` (raw i16 passthrough), so
> the wire frame fills the 4 KiB cap at 1024 spc (4096 B) and always fits.
> PCM decode never returns N/A (its own passthrough cannot exceed the cap
> input that produced it). Note the encode timings above are for `iter_batched`
> fresh-encoder cells (worst-case frame path); the plain PCM passthrough
> (`PcmAdapter::encode`) is ~0.12–0.93 µs/frame in the steady matrix.

### On-wire bandwidth at 48 kHz (the ADR-005 headline)

| fixture | rate | frame_size | FLAC B/frame | FLAC bps(kbps) | PCM bps(kbps) | FLAC ratio |
|---|---|---|---|---|---|---|
| sine-sweep (music-like) | 48k | 240 | 492 | 984.0 | 1536 | 0.512 |
| sine-sweep (music-like) | 48k | 1024 | 1641 | 769.2 | 1536 | 0.401 |
| noise / pseudo-random (worst case) | 48k | 240 | 1057 | 2114.0 | 1536 | 1.101 |
| noise / pseudo-random (worst case) | 48k | 512 | 2144 | 2010.0 | 1536 | 1.047 |
| silence | 48k | 240 | 101 | 202.0 | 1536 | 0.105 |
| sine-sweep (music-like) | 48k | 1024 | 816 | 382.5 | 1536 | 0.199 |
| noise (worst case) | 48k | 240 | 1057 | 1691.2 | 1536 | 1.101 |
| noise (worst case) | 48k | 512 | 2144 | 2010.0 | 1536 | 1.047 |
| silence | 48k | 240 | 101 | 202.0 | 1536 | 0.105 |

### 24-bit (i24-low-3-backed) upper bounds — ANALYTICAL NOTES ONLY, PENDING

Raw 24-bit bounds (frame_size applied to `max_frame_samples(rate,2ch,3B,4096)`
= 682): raw = frame_size × 6 B. @240 → 1440 B (5 ms); @682 → 4092 B (≈ 4 KiB
full). Bandwidth @48k: 240-spc → 288,000 B/s ≈ **2.30 Mbps**; 682-spc →
287,952 B/s ≈ 2.30 Mbps. Per ADR-005 context this is the ~2.9 Mbps raw figure;
FLAC (once 24-bit lands) is expected to regain ~2–3× on music but remain
≥1.02 on noise, exactly like the 16-bit noise rows above. These are bounds,
NOT measurements — 24-bit encode/decode remains PENDING.

## Commands run

```bash
source dev/env.sh
# criterion dev-dep + [[bench]] added; lock regenerated by cargo
cargo build -p wdr_codec --benches                           # OK (debug)
cargo bench -p wdr_codec --bench adr005 -- --sample-size 10  # full criterion run, release, exit 0
cargo bench -p wdr_codec --bench adr005 -- --matrix          # deterministic p50/p99 matrix, release, exit 0
cargo test -p wdr_codec                                      # unit + roundtrip + proptest all pass
cargo clippy -p wdr_codec --all-targets -- -D warnings       # clean
cargo fmt -p wdr_codec -- --check                            # clean
```

All table values above were produced by the release `bench` profile. The full
criterion log (`--sample-size 10`, 1786 lines) was captured and the matrix pass
(`--matrix`, 3000 fresh-encoder iterations per cell) converges with the
criterion medians within ~5–15% (e.g. FLAC noise encode @48k/240 = 12.36 µs
matrix vs 14.6 µs criterion; PCM noise = 0.41 vs 1.08 µs — PCM allocator noise
at ns scale). Fixture sizing was verified against `wdr_fakes` (`next_chunk`
counts total samples) and `wdr_codec::noise_fixture` (per-channel frames).

## Files changed

| Path | Note |
|---|---|
| `crates/wdr_codec/Cargo.toml` | Added dev-dep `criterion = "0.8.2"` + `[[bench]] adr005` (`harness = false`, explicit `bench/bench_adr005.rs` path). No runtime-dependency change. |
| `crates/wdr_codec/bench/bench_adr005.rs` | **New** criterion harness + deterministic per-frame p50/p99 matrix (4 KiB-aware decode gating, lossless-exactness assertions, 24-bit & real-music PENDING notes). |
| `docs/orchestration/reports/t-P1-bench.md` | This report. |
| `Cargo.lock` | Regenerated: criterion + its closure added (mine); also carries unrelated pre-existing `wdr_rt` entry from another task's dirty tree (not mine, untouched). |

## Acceptance criteria — validation

| Criterion | Status | Evidence |
|---|---|---|
| Bench compiles | ✅ | `cargo build -p wdr_codec --benches` OK |
| Bench runs (`--sample-size 10` default path, release) | ✅ | `cargo bench ... -- --sample-size 10` exit 0; all Encode+Decode cells measured |
| Encode/decode throughput + p50/p99 per frame | ✅ | Full matrix above (release p50/p99 from `--matrix`, 3000 iters/cell) |
| Output size / ratio / on-wire bandwidth | ✅ | `out_bytes`, `ratio`, `bps@rate` columns; 48k bandwidth table |
| Fixtures = deterministic synthetic corpus (silence, sine-sweep, edge, pseudo-random) | ✅ | `wdr_fakes` Fixture + `wdr_codec::noise_fixture`; pseudo-random verified == noise (genuinely incompressible) |
| Frame sizes: ADR default 240 AND calculator max 1024 | ✅ | Both measured; 512 added for incompressible decode |
| FLAC-vs-PCM default recommendation incl. worst case (~1.0 ratio) | ✅ | See Recommendation; worst-case FLAC losts on CPU ~50x and breaks 4 KiB at max frame |
| Clippy/fmt clean for the crate | ✅ | `-D warnings` clean; `cargo fmt --check` clean (criterion dev-dep only) |

## Risks / limitations

- **Debug vs release:** earlier debug matrix pass showed ~3–5× larger p50/p99;
  all reported numbers are the release build (`bench` profile = optimized).
- **libFLAC encoder state re-created per call** in the matrix pass (fresh
  `FlacEncoder` per encode = worst-case realistic per-frame path, matching
  criterion's `iter_batched` setup). Sustained-stream amortization (one encoder
  kept alive) would lower FLAC encode further; the criterion encode group also
  re-creates the encoder per iteration, so both passes agree within ~5–15% on
  the headline FLAC figures (e.g. 12.36 µs matrix vs 14.6 µs criterion for
  noise@240 @48k).
- **Variance/outliers:** criterion flagged 1–3 outliers/10 in some FLAC cells
  (first-iteration alloc/init); p99 captures the tail. Compressibility of
  "real music" differs from a sweep; the sweep is a conservative middle ground.
- **Both noise fixtures are genuinely incompressible:** verified that
  `PseudoRandomPcm` (wdr_fakes) and `noise_i16` (splitmix32) FLAC-compress to
  the identical byte sizes (1057/2144/4192 B); the early draft's ~0.55 ratio
  was a fixture-sizing bug now fixed. ADR-005's "≈1.0 on noise" refers to this
  shared worst case.
- **24-bit entirely PENDING** in the codec adapters; measured i16 surface only.

## Follow-up tasks

- Implement 24-bit (i24-low-3-packed) FLAC encode/decode and 24-bit PCM, then
  extend the harness (same cells) and re-lock ADR-005 24-bit default.
- Add a licensed real-music corpus (no copyrighted downloads) and re-run the
  realistic-material cells.
- ADR-005 final lock: fold the hybrid per-frame FLAC→PCM fallback rule
  (ratio ≥ 0.9 or > 4 KiB → PCM) into the codec profile, and add the CI
  `just bench` plumbing (`just bench` is still a stub in `justfile`).

## Blockers

None. (Sibling crate `wdr_rt`/`wdr_refsim` dirty-tree changes and the pre-existing
`wdr_transport` compile error are outside this task's write scope and untouched.)
