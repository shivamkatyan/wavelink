# t-B0-wire — Codec lossless-equality tests wired to the shared wdr_fakes canonical goldens

- **task_id:** t-B0-wire
- **root_task_id:** R-B0-WIRE
- **hypothesis:** H-WIRE-1 — `crates/wdr_codec`'s lossless-equality tests can consume the shared
  canonical fixtures from `crates/wdr_fakes` so lossless integrity is proven as
  `hash(decoded) == hash(source)` against the recorded goldens (ADR-005 / TEST_PLAN
  §Golden audio), complementary to (and overlapping with) the pre-existing
  self-contained golden tests.
- **owner_role:** Audio Systems (implementation worker)
- **date:** 2026-09-06
- **status:** complete (workspace-wide)

## Summary

The codec worker (t-B0-codec) asserted lossless equality with *locally-built* fixtures and
FNV-1a hashes; the fakes worker (t-B0-fakes) recorded **canonical blake3 goldens** for the
shared fixtures. This task wires the two: a new test file
`crates/wdr_codec/tests/golden_lossless_vs_fakes.rs` drives the `wdr_fakes` `Fixture` enum
through `PcmAdapter` and `FlacAdapter` (`encode → decode`) using the **same instrumentation
the goldens were recorded with** (chunk = 512 samples, total = 4096 samples, i16 LE
interleaved L–R–L–R @48k stereo), hashing via the `wdr_fakes` `HashSink`, and asserts
tolerance-free `hash(decoded) == hash(source) == canonical golden`.

Channel ORDER is asserted separately for the channel-id fixtures: after decode, the left lane
stays pattern A and the right lane stays pattern B (distinct DC levels ±564 / ±904).

24-bit is the honest, non-silent skip: the `wdr_codec` adapters are i16-only at B0
(`FlacAdapter::new(...,24)` → `Unsupported`; `SampleRepr::I24Packed`/`F32`/`I32` →
`Unsupported` stub, ADR-005 focuses 16/24-bit, 16-bit implemented). The i24 test asserts the
typed `Unsupported` contract and `eprintln!`s the skip reason so it can never be mistaken for
a passing coverage.

## What changed

| Path | Note |
|------|------|
| `crates/wdr_codec/Cargo.toml` | Added dev-dependency `wdr_fakes = { path = "../wdr_fakes" }` (dev-deps only; no library code touched). |
| `crates/wdr_codec/tests/golden_lossless_vs_fakes.rs` | NEW: 5 integration tests wired to the shared fixtures/goldens (below). |
| `docs/orchestration/reports/t-B0-wire.md` | This report. |

No library code or source adapters were modified (`src/*` untouched). Existing
`tests/golden.rs` / `roundtrip.rs` / `properties.rs` unchanged — they may overlap with the
new file; that is intended.

## Coverage matrix (exactly what is covered vs skipped)

| Adapter | Format | Rate / channels | Status | Proof |
|---------|--------|-----------------|--------|-------|
| `PcmAdapter` | i16 | 48k / stereo | ✅ covered | hash(decoded)==hash(source)==golden for all 9 fixtures; channel order preserved |
| `FlacAdapter` | i16 | 48k / stereo | ✅ covered | hash(decoded)==hash(source)==golden for all 9 fixtures; channel order preserved |
| i24 any adapter | — | — | ⛔ skipped (typed) | adapters are i16-only at B0 — `Unsupported` asserted + `eprintln!` reason |

The 9 fixtures (all as `wdr_fakes` `FixtureKind`, i16 @48k stereo, canonical goldens from
`t-B0-fakes.md`):
`silence`, `impulse-train-64`, `impulse-train-256`, `sine-sweep-20-20000`,
`sine-sweep-200-20000`, `full-scale-edge`, `pseudo-random-pcm`, `channel-id-left`,
`channel-id-right`.

## Decisions

1. **Instrument with the golden's exact chunk parameters.** The recorded goldens are blake3
   of the canonical bytes with chunk = 512 samples / total = 4096 samples. The test replays
   the fixture in the same chunk sizes and hashes source and decode with the same `HashSink`,
   so both `hash(decoded)==hash(source)` AND `hash(source)==recorded golden` hold. FLAC decode
   also enforces the 4 KiB payload cap; 512-sample chunks keep each encoded chunk ~1 KB
   (well under the cap) even for incompressible noise.
2. **Convert at the canonical-byte boundary, never reinvent goldens.** The adapters take
   `&[i16]`; the fixtures stream canonical i16-le bytes. The test converts canonical bytes →
   `i16` for `encode()` and back `i16` → canonical bytes for the decode hash, so both sides
   hash the identical canonical stream (the only ordering `wdr_fakes` owns is the L–R–L–R
   interleave).
3. **Channel id covers both adapters + both pattern lanes.** `channel-id-left` (left=+564 /
   right=−564) and `channel-id-right` (left=+904 / right=−904) verified per-frame after
   decode for PCM and FLAC, across the full 4096-sample stream.
4. **24-bit is a typed skip, not a silent pass.** `wdr_fakes` records i24 goldens
   (full-scale-edge i24, channel-id i24 rows) but no B0 adapter implements an i24 code path.
   Covering them at this stage would be a lie, so the i24 test asserts the `Unsupported`
   contract and prints the skip reason. Recorded as a follow-up, not a loss.
5. **Prefer straight-line test functions over harness plumbing.** Each supported adapter has
   one integration test iterating the 9 goldens (with `#[track_caller]` helpers naming the
   fixture on failure); the `#[ignore]`-style note is realized via the eprintln-quoting
   i24 contract test rather than silently returning success.

## Commands run

```bash
source dev/env.sh
cargo build   -p wdr_codec                                   # OK
cargo test    -p wdr_codec                                   # 38 unit + 10 golden + 5 new + 4 proptest + 11 roundtrip: all pass
cargo clippy  -p wdr_codec --all-targets --all-features -- -D warnings   # clean
cargo fmt -p wdr_codec -- --check                            # clean
cargo build   --workspace                                    # OK
```

## Validation results

| What | Result | Evidence |
|------|--------|----------|
| New test file compiles + runs | ✅ | `tests/golden_lossless_vs_fakes.rs`: 5/5 pass (FLAC all goldens, PCM all goldens, PCM channel order, FLAC channel order, i24 typed-unsupported) |
| FLAC lossless vs shared goldens | ✅ | `flac_i16_48k_stereo_all_canonical_fakes_goldens … ok` — decoded hash equals each recorded canonical, and equals source hash |
| PCM lossless vs shared goldens | ✅ | `pcm_i16_48k_stereo_all_canonical_fakes_goldens … ok` |
| Channel order both adapters | ✅ | `*_preserves_channel_id_left_equals_pattern_a_right_equals_pattern_b … ok` |
| i24 handled honestly | ✅ | `i24_is_typed_unsupported_not_silently_passed … ok` + skip reason printed via `eprintln!` |
| Existing codec suites unchanged/green | ✅ | golden 10/10, roundtrip 11/11, properties 4/4, unit 38/38 |
| Clippy `-D warnings` | ✅ | clean |
| `cargo fmt -- --check` | ✅ | clean |
| `cargo build --workspace` | ✅ | finished without errors (workspace now loads; sibling `wdr_telemetry` landed) |

## Acceptance criteria

| Criterion | Status | Evidence |
|-----------|--------|----------|
| New test file green: tolerance-free hash equality against `wdr_fakes` canonical goldens for every supported (adapter, format) combo actually implemented | ✅ | i16 @48k stereo: PCM + FLAC, all 9 recorded goldens (decoded hash == source hash == canonical golden) |
| Channel order verified for channel-id fixtures | ✅ | left=pattern A / right=pattern B asserted per-frame post-decode, both adapters, both lanes |
| Existing codec golden tests unchanged | ✅ | `tests/golden.rs` untouched; 10/10 pass |
| Unsupported cells (i24) are a typed, documented skip — never a silent pass | ✅ | `i24_is_typed_unsupported_not_silently_passed` asserts `Unsupported`; `eprintln!` records why |
| Gate: test + clippy + fmt + workspace build | ✅ | all green (above) |
| Report lists exactly which combos are covered vs skipped | ✅ | coverage matrix above: {PCM, FLAC} × i16/48k/stereo covered; i24 skipped with reason |

## Risks / limitations

- **i24 lossless roundtrip not wired** — no B0 adapter implements an i24 code path
  (`SampleRepr::I24Packed` is a stub). `wdr_fakes` i24 goldens (full-scale-edge i24,
  channel-id i24 rows) await the ADR-005 i24 codec follow-up; this is the explicit,
  eprintln-documented skip.
- **Rate/channel matrix deliberately narrow.** The recorded golden rows the codec can
  actually drive are i16/48k/stereo; 44.1k mono i16 *does* have recorded goldens
  (silence/i16/44100/mono, full-scale-edge/i16/44100, pseudo-random/i16/44100,
  channel-id i16/44100 rows) but the codec adapters accept any non-zero rate and any
  1–2 channels, so extending the loop to those cells is a one-line follow-up. 48k/stereo
  covers every fixture type once, which is what the golden contract requires.
- **Overlap with existing `tests/golden.rs` is deliberate** (they use local fixtures/FNV);
  the new file proves the same losslessness against the SHARED fixtures with the canonical
  blake3 hashes.

## Follow-up tasks

- Wire i24 lossless roundtrip once an i24 adapter code path exists (ADR-005 24-bit first
  completion / t-B0-codec follow-up); then consume the recorded i24 goldens
  (full-scale-edge i24, channel-id i24 rows).
- Optionally extend the fixture loop to 44.1k mono i16 cells (goldens already recorded).

## Blockers

None. Workspace-wide `cargo build --workspace` is green (the earlier `wdr_telemetry`
empty-src transient is gone — that crate now has a real `src/lib.rs`).
