# INC-003 — ref_e2e lossless golden desync after rev-8 (per-channel budget)

- **Date:** 2026-09-10 (discovered on macOS intake verification)
- **Root task:** R-B1-SIM closure (crates/wdr_refsim/tests/ref_e2e.rs)
- **Owner:** supervisor (baseline verification) — repaired on intake, no worker dispatch needed.
- **Classification:** Product-code test defect caused by a commit that changed emitter
  semantics without updating the dependent e2e assertions (contract drift within one crate's tests).

## Reproduction (deterministic, OS-independent — NOT a macOS artifact)
- `cargo test -p wdr_refsim --test ref_e2e` → `lossless_pcm_roundtrip_hash_preserved` and
  `lossless_flac_roundtrip_hash_preserved` FAIL:
  `assertion left == right failed: emitter source hash must match the recorded golden`
  - left  (computed on host): `9441997640c597df51d696ee522c3eeec8b133d32e46954a5372d48ac7cbae8c`
  - right (recorded golden): `b7a3c25c8ccaa05643f14dfb5bc0e223f4ace3154c11ecd25bed5975d839c223`

## Root cause
- Commit `4975960` (rev-8, "fixture sample budget is values not per-channel") redefined
  `EmitterConfig.total_samples` as a **per-channel** budget and scaled `fixture_values` by
  `channels` — but `ref_e2e.rs` (last touched at `9c89e77`) still passed `total_samples: 4096`
  and asserted the canonical golden, which was recorded by `wdr_fakes` against a **4096-value**
  drain (chunk 512 × 8).
- After rev-8, the emitter drains `total_samples(ch)*channels = 4096*2 = 8192` values
  (8 frames × 1024 values); the golden covers 4096 values. blake3 hashes of different-length
  streams can never match → deterministic failure on every OS.
- Empirical confirmation on this host (throwaway probe, since deleted):
  `drain_hash(4096,512)  = b7a3c25c…` (matches golden) ·
  `drain_hash(8192,1024) = 9441997640…` (matches computed).
- The "184 passed / 0 failed" recorded at `746befe` was therefore stale w.r.t. ref_e2e.

## Repair (evidence-producing, minimal)
- Set the two lossless call sites' `total_samples` to **2048** (per-channel), restoring the
  intended canonical drain: `2048 × 2 = 4096` values over 4 frames of 1024 values → source hash
  equals the recorded golden `b7a3c25c…` and `frames_rendered == 4` (the test's own documented intent).
- Also reconciled `rustfmt` style-edition drift in pre-existing `crates/wdr_rt/src/spsc.rs`
  (formatting-only; pinned-current-toolchain canonical format).
- Verified: `cargo build --workspace` PASS, `cargo test --workspace` **215 passed / 0 failed**,
  `cargo clippy --workspace --all-targets --all-features -- -D warnings` PASS,
  `cargo fmt --all -- --check` PASS.
- Status: **closed** (baseline rev; integrated at the macOS-host intake rev).

## Lesson
- Changing a crate's parameter semantics (values ↔ per-channel) must update its own dependent
  tests in the same commit, or the known-green record is stale. Supervisor intake now re-runs
  the full workspace suite before planning (governing contract §19 step 2).
