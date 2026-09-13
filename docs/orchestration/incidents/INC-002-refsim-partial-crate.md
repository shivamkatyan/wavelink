# INC-002 — Partial non-compiling wdr_refsim + void returns

- **Date:** 2026-09-06
- **Root task:** R-B1-SIM (crates/wdr_refsim emitter+receiver sim)
- **Owner:** Protocol/Audio (subagent)
- **Attempts:** 1 (void), 2a emitter (void), 2b receiver (voit + auth failure). Pattern: workers handle the task but return an EMPTY final message, leaving the crate partially written and uncommitted.
- **Classification:** Contract mismatch (return-contract violation) surfaced as a product-code defect: the written crate does not compile (E0119 conflicting Debug impls in receiver.rs; E0631 type mismatch in emitter.rs; E0277 dyn CodecAdapter not Debug).
- **State at restart:** rev-3 (6678682) committed; wdr_refsim untracked+partial on disk; Cargo.lock modified (workspace no longer green). Supervisor restored by keeping the partial crate isolated and redispatching a repair.
- **Action:** record here; dispatch a repair worker (attempt 3, H-B1-SIM-2) to fix+complete+test the crate with the full return contract; on success, re-integrate into workspace and commit rev-4.
- **Status:** open → repair in progress

## Resolution (2026-09-07)
- Refsim closure completed via final worker (H-B1-SIM-3, precise contract): created `src/bin/ref_emitter.rs`, `tests/ref_e2e.rs` (4 e2e), fixed residuals (ChannelLayout import, clippy lint x6), repaired 3 latent bugs (Opus values-per-frame, reliable-stream end-marker length-prefix, whole-frames-only tail), added `EmitterError::Policy`.
- Supervisor independently verified: workspace builds, 212 tests pass / 0 fail, e2e explicit run green, clippy/fmt clean, both `ref_emitter`/`ref_receiver` bins present.
- Status: **closed** (rev-4 tracks the integration). Lesson: broad contracts on this harness tend to void; precise single-closure contracts with a final-YAML mandate and a scope guard succeed.
