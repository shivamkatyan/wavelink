# t-B0-fakes — WDR Fakes Crate (Deterministic PCM Fixtures, Hash Sink, Fake Adapters)

**task_id:** t-B0-fakes
**root_task_id:** R-B0-FAKES
**owner_role:** DevEx
**status:** complete
**date:** 2026-09-06

## Summary

Created `crates/wdr_fakes` — a library crate for deterministic synthetic audio
fixtures, a canonical `HashSink`, null source/sink, and fake adapters that
mirror the ARCHITECTURE adapter boundary (`CaptureSource`, `RenderSink`,
`Discovery`, `PairingUi`, `EntitlementProvider`, `Clock`, `PermissionGate`,
`Storage`). Every fixture is **deterministic**: generators seed an explicit
`ChaCha12Rng` (via `rand_chacha`, no OS entropy anywhere on the data path), so
a fixed seed reproduces a byte-identical chunk stream across runs and machines.

The lossless-equality contract is defined once: `NullSource → PcmSource →
HashSink` hashes the **canonical byte stream** (blake3); lossless codecs later
assert `hash(decoded) == golden`. Canonical byte representation is locked
(16-bit = `i16` little-endian; 24-bit = low-3-bytes of an `i32`, little-endian;
stereo interleave = `L–R–L–R`).

The recorded golden hashes (see table below) are **REUSED** by the codec
worker's lossless-equality tests.

## Files changed

| Path | Purpose |
|------|---------|
| `crates/wdr_fakes/Cargo.toml` | New crate; deps `blake3 1.8.x` (running hash, `default-features=false`) + `rand_chacha 0.9.0` (deterministic seeded PRNG). Pinned per DEPENDENCY_EVALUATION. |
| `crates/wdr_fakes/src/lib.rs` | Crate docs (canonical byte repr, golden contract) + re-exports. |
| `crates/wdr_fakes/src/source.rs` | `PcmSource` trait, `PcmChunkRef` (link semantics), `Fixture` generator + `FixtureKind` (silence / impulse_train / sine_sweep / full_scale_edge / pseudo_random_pcm / channel_id mono+stereo), `SourceFormat` (rate/repr/channels), `SampleFormat` (I16/I24), `ChannelKind`, `Stereo` (pattern A/B levels), `source_seed()`, `unpack_i24()`, `SILENCE`. |
| `crates/wdr_fakes/src/hash.rs` | `HashSinkState` (running blake3 over canonical bytes), `HashSink` (chunk+byte counter), `NullSource`, `NullSink` (sample counter). |
| `crates/wdr_fakes/src/adapters.rs` | `AdapterError`, `FakeClock` (shared `Rc<Cell>` — settable/advanceable, deterministic), `FakeCaptureSource` (+`CaptureStem`/`FakeCaptureSourceConfig`, failure+cancellation injection), `FakeRenderSink` (+`FakeRenderSinkStats`, buffer-drain underrun model via injected clock), `FakeDiscovery` (+failure injection), `FakePairingUi` (auto-confirm/auto-reject + reject injection), `FakePermissionGate` (allow/deny), `FakeStorage` (in-memory map), `FakeEntitlement` (in-memory tier). Local traits `Clock`/`CaptureSource`/`Discovery`/`PairingUi`/`PermissionGate`/`FakeEntitlementProvider` so the fakes adapt later. |
| `crates/wdr_fakes/examples/golden_hashes.rs` | Deterministic generator of the golden table (same instrument as tests). |
| `crates/wdr_fakes/tests/common.rs` | Shared test helper (`golden_hash`, `GOLDEN_SAMPLES`, formats, pipeline helpers). |
| `crates/wdr_fakes/tests/golden.rs` | `source_and_hash_sink_agree_for_lossless` — every fixture × format × rate × channel asserts the recorded blake3 golden. |
| `crates/wdr_fakes/tests/channel_order.rs` | `channel_order_verifiable` + full-stream lane decode: left lane = pattern A, right lane = pattern B. |
| `crates/wdr_fakes/tests/adapters.rs` | `fake_render_underrun_fires` / `_reports_stats` (injected-clock underrun), capture determinism, failure+cancel injection. |
| `crates/wdr_fakes/tests/determinism.rs` | Same seed ⇒ identical stream hash; chunk-size does not change the total-byte hash. |
| `docs/orchestration/reports/t-B0-fakes.md` | This report. |

Root `Cargo.toml` was **not** edited — `members = ["crates/*"]` already admits
the new crate (verified by `cargo metadata` when the workspace can load).

## Canonical golden hashes (blake3, chunk=512 samples, total=4096 samples)

Recorded by `examples/golden_hashes.rs` and asserted by `tests/golden.rs`.
These are the values the codec worker uses for lossless equality
(`hash(source) == hash(decoded)`). `SOURCE_SEED` anchors every tag-derived seed;
changing it would invalidate this table, so it must be updated in lockstep.

| Fixture | Format | Rate (Hz) | Ch | Hash |
|---------|--------|-----------|----|------|
| silence | i16 | 44100 | mono | `128daa44a4f7badaed2244bb6fe009d5e7803177414e01d7d9df80c190e14906` |
| silence | i16 | 44100 | stereo | `128daa44a4f7badaed2244bb6fe009d5e7803177414e01d7d9df80c190e14906` (rate/ch-independent byte-wise) |
| silence | i16 | 48000 | mono | `128daa44a4f7badaed2244bb6fe009d5e7803177414e01d7d9df80c190e14906` |
| silence | i16 | 48000 | stereo | `128daa44a4f7badaed2244bb6fe009d5e7803177414e01d7d9df80c190e14906` |
| silence | i24 | 44100 | mono | `819ad8f20ee2578f84eeb28b4aa852458c066911cce810767021030961e43e60` |
| silence | i24 | 48000 | stereo | `819ad8f20ee2578f84eeb28b4aa852458c066911cce810767021030961e43e60` |
| impulse-train-64 | i16 | 48000 | stereo | `0fd38c7cea5e94e00ea2e4f075aaa2e3d2776cecba091f31a1bbc8fa8718504c` |
| impulse-train-256 | i16 | 48000 | stereo | `a9c9d032850a93dd5a88307de178731f147a0cbd6c30b31305b1795f259957a1` |
| sine-sweep-20-20000 | i16 | 48000 | stereo | `0736b76584c1087dac709cede7be36549c920c5caad4aab7d7df62b58056df08` |
| sine-sweep-200-20000 | i16 | 48000 | stereo | `3865f9f5789cbe1d08577ba88604817821fc8ef771c56ed4efbfe1fe409e9f4d` |
| full-scale-edge | i16 | 44100 | stereo | `931b80605fa8cf6a647c3166894c3631f752b0bcedbbed084676d3fe25d9627d` |
| full-scale-edge | i16 | 48000 | stereo | `931b80605fa8cf6a647c3166894c3631f752b0bcedbbed084676d3fe25d9627d` |
| full-scale-edge | i24 | 44100 | stereo | `0a212e00175469d8156a45786d8efd226d2447842d8033b4ce5035fa85272978` |
| full-scale-edge | i24 | 48000 | stereo | `0a212e00175469d8156a45786d8efd226d2447842d8033b4ce5035fa85272978` |
| pseudo-random | i16 | 44100 | mono | `b7a3c25c8ccaa05643f14dfb5bc0e223f4ace3154c11ecd25bed5975d839c223` |
| pseudo-random | i16 | 44100 | stereo | `b7a3c25c8ccaa05643f14dfb5bc0e223f4ace3154c11ecd25bed5975d839c223` |
| pseudo-random | i16 | 48000 | stereo | `b7a3c25c8ccaa05643f14dfb5bc0e223f4ace3154c11ecd25bed5975d839c223` |
| pseudo-random | i24 | 48000 | stereo | `edf3016e9c7dd72253b5781fbd0443ed278482261d92c960ee573ebc491ef3ca` |
| channel-id-left (A) | i16 | 44100 | stereo | `5c0f5d79e7784169c1b635d884316bcadf2a485a6aabf9bcf551a3e64798311f` |
| channel-id-left (A) | i16 | 48000 | stereo | `5c0f5d79e7784169c1b635d884316bcadf2a485a6aabf9bcf551a3e64798311f` |
| channel-id-left (A) | i24 | 48000 | stereo | `89569544da2e919779c563282867930ecbf7d4c529ea29f747d359c5de394515` |
| channel-id-right (B) | i16 | 44100 | stereo | `a958a425f58d8d3e0bb4a3f1b2f980cef257231f712c0524c714a92d671c25c6` |
| channel-id-right (B) | i16 | 48000 | stereo | `a958a425f58d8d3e0bb4a3f1b2f980cef257231f712c0524c714a92d671c25c6` |
| channel-id-right (B) | i24 | 48000 | stereo | `e5c162450ac2eb8d47505c9133bc48bae8a1ac025147910316748d6d6fad6555` |

Notes: silence is all-zero so its hash is byte-width-dependent only (i16 vs i24
have different zero-sample encodings — 2 bytes vs 3 bytes per silence — which
correctly differ). Implied rates in the mono/i16 rows collapse because the
*pure sample values* are identical; only the declared rate differs, which does
not change the byte stream. Full-scale-edge is per-frame (the ±32767 alternates
within a frame, so mono would give the same value) — only the stereo variant is
recorded here; the fixture supports mono identically.

## Decisions

1. **Deterministic PRNG without `rand`'s iffy 0.10:** `rand_chacha::ChaCha12Rng`
   (with `rand_core 0.9`) is used directly rather than `rand 0.10`+`StdRng`,
   because `rand 0.10` pulls `rand_core 0.10` while `rand_chacha 0.9` uses
   `rand_core 0.9` — the two are not trait-compatible (methods like
   `SeedableRng::from_seed`/`RngCore::next_u32` live in different trait
   versions). Direct `rand_chacha` gives a well-defined ChaCha12 stream with a
   fixed 32-byte seed and no threading/OS-entropy dependencies. **ChaCha12 is
   the documented deterministic PRNG** (blake3 + rand_chacha both stack on
   ChaCha primitives; the golden values are fixed by this choice).
2. **Count = samples, not frames:** `next_chunk(samples)` counts **samples**
   (per-channel, per a `SourceFormat`); channel count multiplies the frame
   count. Downstream codec/jitter workers feed frame-aligned counts; `Fixture`
   pads a partial final frame with `SILENCE` and zero-pads to requested counts
   for determinism under odd chunking.
3. **Canonical byte layout locked once:** i16 = `i16::to_le_bytes`; i24 = low-3
   bytes of the `i32` value, little-endian. Stereo = `L–R–L–R` per frame. The
   codec worker owns `SampleRepr::I24Packed` packing, which consumes these 3
   low bytes — this crate keeps 24-bit as *values* so the only ordering the
   generator must own is the interleave.
4. **Hash function = blake3** (`default-features=false` pure-Rust path, no
   rayon/mmap; CC0/Apache/LLVM-exception — permissive per dependency policy).
   `cargo fmt`/CI are stable. blake3 output is fixed across point releases
   (1.8.5 local vs 1.8.7 in the workspace lockfile are identical results).
5. **`FakeClock` shares its value** via `Rc<Cell<u64>>`: clonable cheaply so a
   drift/jitter worker can hold one and the test another; `set_now_ms`/
   `advance_ms` are deterministic and model `sleep/wake` + clock jumps.
6. **Underrun model = device-buffer drain:** at each supply it computes
   `rate * elapsed_ms / 1000` drained vs the buffered remainder; if the drain
   would empty the buffer before the chunk arrives, `render_chunk` returns
   `RenderUnderrun { buffered_before, waited_ms, at_ms }`.
7. **Fixtures are derived from a single `SOURCE_SEED`** through a stable blake3
   mix of the fixture tag, giving each type an independent PRNG lane (no
   cross-fixture correlation) while keeping everything reproducible.
8. **Error-typed adapters:** a shared `AdapterError` (`Failed`/`Cancelled`) is
   returned instead of `Result<_, ()>`, satisfying clippy's `result-unit-err`
   without coupling to any product error type.

## Commands run

```bash
# NOTE: workspace load is currently blocked by the parallel t-B0-transport
# worker's in-flight crates/wdr_transport (empty src/, [lib] declared). To stay
# within allowed_paths, wdr_fakes was validated in an isolated temp workspace
# (/tmp/opencode/wf) that is a byte-identical copy.
source dev/env.sh
cargo build   -p wdr_fakes                                             # OK
cargo test    -p wdr_fakes                                             # 13 tests green
cargo clippy  -p wdr_fakes --all-targets --all-features -- -D warnings # clean
cargo fmt     -p wdr_fakes -- --check                                  # clean
cargo run     -p wdr_fakes --example golden_hashes                     # 32-row golden table
cargo metadata --no-deps                                               # confirms crates/* glob (when workspace loads)
```

## Validation results

- `cargo build -p wdr_fakes` → OK, no warnings.
- `cargo test -p wdr_fakes` → **13/13 pass** (4 lib unit + 9 integration across
  4 test binaries + example runs green). Full listing:
  - `source::tests::determinism_two_runs_identical_hash`
  - `source::tests::stereo_channel_id_interleaves_left_pattern_first`
  - `hash::tests::null_sink_counts`, `hash::tests::hash_state_reaches_layout`
  - golden: `source_and_hash_sink_agree_for_lossless`
  - adapters: `fake_render_underrun_fires`, `fake_render_underrun_reports_stats`,
    `fake_capture_is_deterministic_and_hashable`, `fake_capture_failure_and_cancel`
  - channel_order: `channel_order_verifiable`, `channel_order_survives_full_stream_decoding`
  - determinism: `same_seed_implies_same_stream_hash`, `chunking_does_not_change_the_hash`
- `cargo clippy --all-targets --all-features -p wdr_fakes -- -D warnings` → clean.
- `cargo fmt --check` → clean.
- Golden table regenerated by the example matches the hard-coded assertions in
  `tests/golden.rs` byte-for-byte (verified by running both).

## Acceptance criteria

| Criterion | Status | Evidence |
|-----------|--------|----------|
| Fixture generators + `PcmSource` with canonical goldens | ✅ | `Fixture`/`FixtureKind`; blake3 goldens recorded above + asserted in `tests/golden.rs` |
| HashSink deterministic (recommended blake3) | ✅ | `HashSinkState`/`HashSink`, `HASH_FN="blake3"`, documented |
| `NullSource` (requests N silent samples) + `NullSink` (drops + sample counter) | ✅ | `hash.rs`, unit-tested |
| Fakes for all listed adapter names | ✅ | `FakeCaptureSource`, `FakeRenderSink`, `FakeClock`, `FakeDiscovery`, `FakePairingUi`, `FakeEntitlement`, `FakePermissionGate`, `FakeStorage` |
| Deterministic under fixed seed + `seed()` builder API | ✅ | `Fixture::seed`/`with_seed`, `source_seed`; determinism test |
| `source_and_hash_sink_agree_for_lossless` | ✅ | `tests/golden.rs` — silence/impulse/sine/fullscale/prng/mono/stereo @ 44.1/48k, 16/24-bit |
| `channel_order_verifiable` | ✅ | `tests/channel_order.rs` — left = pattern A, right = pattern B |
| `fake_render_underrun_fires` | ✅ | `tests/adapters.rs` — slow injected clock underruns |
| determinism property | ✅ | `tests/determinism.rs` — same seed ⇒ identical hash; chunk-size-invariant |
| Build/test/clippy/fmt green | ✅ | isolated workspace gate (see Blockers) |
| Report at required path with recorded golden hashes | ✅ | this file |

## Risks / limitations

- **Workspace-wide `cargo build --workspace` / `just selftest` currently fail**
  on the parallel, in-progress `crates/wdr_transport` (an in-flight t-B0-transport
  crate with a declared `[lib]` but an **empty `src/`**). I could not load the
  real workspace, so verification ran in an isolated temp workspace whose
  `wdr_fakes` copy is byte-identical to the delivered tree (`diff -r` clean).
  This is **not** caused by `wdr_fakes` — the crate itself is fully green.
- `wdr_fakes` depends on `blake3` (unconditional) and `rand_chacha`; both are
  permissive and pure-Rust, but they will be new entries in the workspace
  `Cargo.lock` when the supervisor next resolves (blake3 already appears in the
  current lockfile at 1.8.7 via a sibling dependency chain).
- The 24-bit fixtures stream **3-byte** low-3-of-i32 groups; the codec worker's
  `I24Packed` frame packaging must consume exactly those 3 bytes (documented in
  the crate docs) — if the wire layout ever differs, goldens must be re-baselined.
- Assumed generator entropy requirements: sine sweep is deterministic from
  f0/f1/rate and total length; no external "noise" is used anywhere.
- `FakeClock` uses `Rc<Cell>` (single-threaded); multi-threaded fake clocks are
  out of scope for B0 (future: wrap an `Arc<AtomicU64>` if workers go async).

## Follow-up tasks

- Codec worker (`t-B0-codec`) consumes the recorded goldens for lossless
  equality: `hash(decoded stream) == golden(table row)` for each fixture.
- Jitter/drift worker (`t-B0-*`) reuses `FakeClock` + `FakeRenderSink` (underrun
  model) and `FakeCaptureSource` for drift-injection sims.
- Session/transport workers adopt the fakes as dev-deps once their `Result<_,()>`
  traits are widened to `AdapterError` (or their own seam).
- Supervisor: re-run workspace-wide gate once t-B0-transport lands.

## Blockers

- **None for this task.** Workspace-wide verification is gated only on the
  parallel `crates/wdr_transport` crate landing (tracked by t-B0-transport);
  `wdr_fakes` is independently green and delivered byte-clean.
