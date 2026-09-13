# t-B0-codec — Codec Adapters + Frame-Size Calculator + TPDF Dither

- **task_id:** t-B0-codec
- **root_task_id:** R-B0-CODEC
- **owner_role:** Audio Systems
- **date:** 2026-09-06
- **status:** complete

## Summary

Implemented the standalone `wdr_codec` crate: the `CodecAdapter` seam
(ADR-001/ARCHITECTURE) with three back-ends — **Opus** (lossy, wraps vendored
libopus), **FLAC** (lossless, encode via libFLAC `flac-bound`, decode via pure-
Rust claxon), and **PCM** (raw lossless baseline) — plus the ADR-005
**frame-size calculator** (`max_frame_samples`) and deterministic **TPDF dither**
for the documented 24→16-bit lossy down-conversion.

`wdr_codec` is standalone (does not depend on `wdr_proto`); the per-frame CRC-32
for lossless integrity is implemented locally (`frame_crc32`) rather than
reusing `wdr_proto::crc32`, so the crate boundary stays decoupled at B0. The
root `Cargo.toml` already uses `members = ["crates/*"]`, so no root edit was
required.

### ADR-005 spot-check matrix (frame-size calculator preview)

`max_frame_samples(sample_rate, channels, bytes_per_sample, budget)` computes
the raw-byte ceiling `budget / (channels*bps)` — the binding upper bound for
both FLAC (incompressible ≈ raw + header) and PCM, and the raw ceiling for Opus
(which libopus additionally caps at ≤1275 B/packet). Measured on seeded
incompressible-noise fixtures (spot-check; full benchmark harness is a later
spike). Block used per cell: FLAC 240 samples, Opus 20 ms, PCM = the computed
max for that cell.

| rate | bit depth | codec | max_frame_samples | measured_frame_bytes | fits 4KiB |
|------|-----------|-------|-------------------|----------------------|-----------|
| 44100 | 16 | Flac | 1024 | 1057 | ✅ |
| 44100 | 16 | Pcm  | 1024 | 4096 | ✅ |
| 44100 | 16 | Opus | 1024 | 626  | ✅ |
| 44100 | 24 | Flac | 682  | 1440 (raw ceiling) | ✅ |
| 44100 | 24 | Pcm  | 682  | 2728 | ✅ |
| 44100 | 24 | Opus | 682  | 626  | ✅ |
| 48000 | 16 | Flac | 1024 | 1057 | ✅ |
| 48000 | 16 | Pcm  | 1024 | 4096 | ✅ |
| 48000 | 16 | Opus | 1024 | 587  | ✅ |
| 48000 | 24 | Flac | 682  | 1440 (raw ceiling) | ✅ |
| 48000 | 24 | Pcm  | 682  | 2728 | ✅ |
| 48000 | 24 | Opus | 682  | 587  | ✅ |

Notes:
- `max_frame_samples` is **rate-independent** (the raw bound is a byte-size
  ceiling); a 20/10 ms Opus frame still needs Opus legal frame sizes (the
  caller picks 960/480 or 882/441 per profile).
- FLAC incompressible single-frame sizes (measured): 240-sample stereo @48k =
  **1057 B**, 512 = **2144 B**, 1024 = **4192 B (>4 KiB, exceeds cap)**. The
  practical incompressible ceiling for 16-bit stereo @48k is therefore block
  sizes ≤512 (~10 ms); 240 (~5 ms, the ADR-005 default) fits comfortably. The
  CI unit `adr005_flac_noise_frame_fits_4k_block_sizes` asserts 240 and 512
  fit and that 1024 does not.

## Files changed

| Path | Note |
|---|---|
| `crates/wdr_codec/Cargo.toml` | New crate manifest; pinned runtime deps per DEPENDENCY_EVALUATION.md: opus 0.4.0 (vendored libopus via opusic-sys bundled/default + cmake), flac-bound 0.5.0 (libflac-noogg → vendored BSD-3 libFLAC via cmake), claxon 0.4.3, hound 3.5.1, rubato 5.0.0, rand 0.10.2; dev: proptest 1.11.0. |
| `crates/wdr_codec/src/lib.rs` | Crate root + re-exports. |
| `crates/wdr_codec/src/error.rs` | `CodecError` (typed, Display+Error): `Unsupported`, `RateUnsupported{rate,codec}`, `InvalidPcmLen`, `InvalidFrameSize`, `Backend`, `MalformedFrame`, `PayloadTooLarge` (checked before allocation), `Format`, `OddSampleBuffer`. |
| `crates/wdr_codec/src/adapters/mod.rs` | `CodecAdapter` trait, `CodecKind` (Opus/Flac/Pcm), `SampleRepr` (I16 implemented; F32/I24Packed/I32 future stubs returning `Unsupported` with doc marks), `frame_crc32`. |
| `crates/wdr_codec/src/adapters/opus.rs` | `OpusAdapter` + `FrameProfile` (Ms20/Ms10). 44.1/48 kHz mono/stereo, 20/10 ms, VBR ~128–256 kbps. 44.1 kHz internally resampled 44.1↔48 (rubato Fft, FixedSync::Input both ways) because libopus/RFC 6716 accepts only 8/12/16/24/48 kHz. Typed `RateUnsupported` for >48 kHz. |
| `crates/wdr_codec/src/adapters/flac.rs` | `FlacAdapter`: libFLAC encode (native non-OGG, fixed blocks, compression 5, verify-off) + claxon decode; per-frame CRC-32 exposed over raw frame bytes; default block 240. |
| `crates/wdr_codec/src/adapters/pcm.rs` | `PcmAdapter`: i16 interleaved little-endian passthrough (bit-perfect), per-frame CRC-32. |
| `crates/wdr_codec/src/size.rs` | `max_frame_samples`, `MAX_FRAME_PAYLOAD` (4096), `DEFAULT_BUDGET`, `MAX_FRAME_SAMPLES_FALLBACK`, `noise_fixture` (deterministic splitmix32), `MatrixRow`. |
| `crates/wdr_codec/src/dither.rs` | `tpdf_dither_24_to_16` (injectable seeded RNG via `DitherRng`, `TwentyFourBitSamples` Packed24/I32), mean-preserving, deterministic, clamp to i16. |
| `crates/wdr_codec/tests/golden.rs` | Lossless golden equality (`hash(decoded)==hash(source)`, FNV-1a + std hasher) across fixtures: silence, impulse trains, sine sweeps, full-scale edges, seeded pseudo-random PCM, channel-ID patterns, DC; 44.1/48 kHz × 16-bit; exact equality, tolerance-free. Also a hound WAV fixture roundtrip. |
| `crates/wdr_codec/tests/roundtrip.rs` | Roundtrip integrity; FLAC noise-frame 4 KiB assertions (actual sizes 1057/2144/4192 B); ADR-005 matrix (table above); `max_frame_samples` consistency; unsupported 24-bit FLAC; `RateUnsupported` for >48 kHz Opus; oversized payload rejected before decode; malformed/truncated frames error not panic. |
| `crates/wdr_codec/tests/properties.rs` | proptest (4×256): PCM roundtrip exactness (any length), arbitrary-bytes-never-panic (FLAC + Opus decoders), frame-size calculator budget-floor invariant. |
| `crates/wdr_codec/src/*` | (see above) |
| `docs/orchestration/reports/t-B0-codec.md` | This report. |

Root `Cargo.lock` gained the new dependency closure (opus→opusic-sys [vendored
libopus], flac-bound→libflac-sys [vendored libFLAC], claxon, hound, rubato→
audioadapter-buffers/realfft/rustfft, rand). It is regenerated by `cargo build`;
as with sibling reports, nothing was committed.

## Decisions

1. **Error-typed `Result<_, CodecError>`** rather than the bare `Result` in the
   sketch trait — matches the error-typed, panic-free contract of the sibling
   crates and PROTOCOL_SPEC §Error taxonomy; malformed/oversized input returns
   `Err`, never panics.
2. **44.1 kHz Opus = internal resample to/from 48 kHz.** Empirical finding:
   vendored libopus 1.6.1 rejects `44100` (`opus_encoder_create` →
   `OPUS_BAD_ARG`); RFC 6716 §2.1.1 only defines 8/12/16/24/48 kHz. To honour
   ADR-004's "44.1/48 kHz" lossy profile, `OpusAdapter` transparently resamples
   via rubato `Fft` (44.1k→48k encode, 48k→44.1k decode, `FixedSync::Input`
   both ways: 882→960 / 960→882). The resampler inserts a fixed
   `downsampler_delay()` (~147 frames @44.1k) before steady-state output, which
   a stream/jitter buffer absorbs; documented, not a silent sample-slip. The
   `CodecAdapter` API stays pure i16 both sides.
3. **>48 kHz through lossy returns `CodecError::RateUnsupported`** both at
   `OpusAdapter::new` and defensively at `encode` — never silently re-routed
   (FR-014 / ADR-004). Verified by tests at 88.2k and 96k.
4. **24-bit FLAC is a stub for now** (ADR-005 focuses 16/24-bit *first*, 16-bit
   is implemented). `FlacAdapter::new(…, 24)` returns `Unsupported`; the
   24-bit *source* path is expressed through `SampleRepr::I24Packed` (stub) and
   is the domain of the TPDF dither for the *lossy* down-conversion (implemented
   and deterministic). 24-bit FLAC encode/decode is a follow-up.
5. **Per-frame CRC lives in the codec crate** (`frame_crc32`, IEEE-802.3,
   deterministic) rather than importing `wdr_proto::crc32`, keeping `wdr_codec`
   standalone; both implement the identical polynomial, so wire values agree.
6. **Determinism contract for Opus:** libopus is stateless w.r.t. external
   seeds, so "determinism given same instance seed" is realized as *fresh,
   identically-configured instances produce byte-identical encoded payloads and
   identical decoded output*. Golden hash-equality is asserted for the
   **lossless** path only (FLAC/PCM) as required; Opus golden tests assert byte-
   and-decode determinism plus bounded steady-state RMS (not sample equality).
7. **Frame-size calculator** returns `usize` (not `Option`): `0` for
   degenerate/too-small inputs (fewer samples than a full frame / zero channels
   or bytes-per-sample), capped at `MAX_FRAME_SAMPLES_FALLBACK` (8192). It is
   the raw-byte ceiling (rate-independent), the conservative superset bound
   both FLAC and PCM must respect; the Opus row additionally benefits from
   libopus's hard 1275-B packet cap.

## Commands run

```bash
source dev/env.sh
cargo build -p wdr_codec                             # OK (vendored libopus + libFLAC via cmake)
cargo test -p wdr_codec                              # 38 unit + 10 golden + 11 roundtrip + 4 proptest: all pass
cargo test -p wdr_codec -- --nocapture               # prints ADR-005 matrix table
cargo clippy -p wdr_codec --all-targets -- -D warnings   # clean
cargo fmt --all -- --check                           # clean
cargo build --workspace                              # wdr_codec ok; pre-existing wdr_transport errors (sibling, not mine)
```

## Acceptance criteria — validation

| Criterion | Status | Evidence |
|---|---|---|
| Adapters compile and roundtrip | ✅ | Opus 44.1k/48k × stereo/mono × 20/10 ms; FLAC + PCM exact roundtrips in unit + integration tests |
| Golden lossless equality green | ✅ | `tests/golden.rs`: hash(decoded)==hash(source) exact across all lossless fixtures (silence, impulse, sine sweep, full-scale, seeded pseudo-random, channel-ID, DC) at 44.1/48 kHz |
| Frame-size calculator reports ADR-005 matrix table | ✅ | `adr005_matrix_frame_size_calculator` prints the {44.1k,48k}×{16,24b}×{Flac,Pcm,Opus} table; `adr005_flac_noise_frame_fits_4k_block_sizes` asserts actual FLAC sizes (1057/2144/4192 B) |
| Dither deterministic | ✅ | `tpdf_dither_24_to_16` with seeded `StdRng`; unit test: same seed → same output, mean preserved within 0.5 LSB, ±1 LSB of quantised baseline, packed-24 ≡ i32 forms, edge clamp to ±32767/±32768 |
| Clippy/fmt clean | ✅ | `-D warnings` clean; `cargo fmt --check` clean |

Additional: malformed/truncated/oversized decode returns `Err` never panics
(unit + proptest with arbitrary bytes); unsupported formats return
`Unsupported`; >48 kHz lossy returns typed `RateUnsupported` (construction and
encode-guard); oversized payloads rejected before allocation.

## Risks / limitations

- **44.1k Opus transparency:** requires the internal 44.1↔48 resampler; the
  double resample adds ~147 frames (≈3 ms @44.1k) fixed latency and the
  roundtrip is lossy (as Opus always is). Tested for steady-state fidelity, not
  sample-exactness. If B0-bench later prefers an alternative (e.g. always run
  Opus at 48k and let the sample-clock block handle 44.1k), the adapter seam
  isolates the change.
- **24-bit FLAC not yet implemented** — ADR-005's 16/24-bit "first" is met at
  16-bit; 24-bit FLAC encode/decode is the natural follow-up once the benchmark
  selects it. The 24-bit→16-bit lossy dither path (the documented conversion)
  *is* implemented and deterministic.
- **libopus vendored build** requires a C toolchain + cmake
  (`opusic-sys`/`libflac-sys` build scripts). Present on this host; tracked as
  existing RISK_R08 (FFI cross-build) per DEPENDENCY_EVALUATION.
- **FLAC incompressible ceiling:** at 16-bit stereo @48k, noise frames exceed
  4 KiB at block sizes ≥1024; the protocol cap therefore forces ≤512-sample
  blocks for worst-case content. ADR-005's ~240-sample default is safely under.
- **Rate-unsupported is construction-time for Opus** (and encode-time
  defensive); FLAC/PCM accept any non-zero rate. A 88.2k/96k lossy route is a
  typed error, never a silent downgrade, as required.

## Follow-up tasks

- 24-bit FLAC encode/decode (ADR-005 "24-bit first" completion) once benchmark
  selects FLAC default per format; exercise through hound 24-bit WAV fixtures.
- Full ADR-005 benchmark harness (spike): real music + worst-case
  incompressible fixtures, encode/decode CPU, on-wire bandwidth, frame
  duration, algorithmic delay; uses the `max_frame_samples` calculator + this
  matrix as the preview.
- Drift-correction adapter behind rubato (`Slip`/`Async`) as a proper
  `resample` module (ADR-007); the dependency is already pinned in this crate.
- Wire `frame_crc32` and the `CodecAdapter` output into the frame container
  (`wdr_proto`/B1 session) for the AEAD-tagged lossy / CRC-protected lossless
  frames.

## Blockers

None. `wdr_codec` is standalone. (Note: `wdr_transport` — a sibling crate — has
a pre-existing compile error in its own code (`SendDatagramOutcome`) unrelated
to this crate; `cargo build -p wdr_codec` and all wdr_codec gates are green.)
