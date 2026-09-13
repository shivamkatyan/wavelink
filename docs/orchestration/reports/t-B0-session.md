# t-B0-session — Session State Machine Crate

- **task_id:** t-B0-session
- **root_task_id:** R-B0-SESSION
- **owner_role:** Session/State Machine
- **date:** 2026-09-06
- **status:** complete

## Summary

Implemented the standalone `wdr_session` Rust crate: a deterministic,
clock-injected session state machine (`Session::advance(&mut self, Event,
now_ms) -> Vec<Effect>`). Zero external dependencies (verified via `cargo tree`:
only the crate itself), so the crate compiles in isolation and cannot be broken
by sibling crates. No `std::time` anywhere in the code (`grep` confirms only
doc-comment mentions).

State path per `docs/planning/PROTOCOL_SPEC.md` §"State machine":
`Idle → Discover → Connect → Pairing → Negotiating → Streaming → Paused →
Renegotiating → Recovering → Terminated | Error`.

Locked numeric bounds honoured: reconnect backoff ×1.5 max 30 s, session idle
expiry 60 s, control response timeout 5 s, pairing window 60 s.

## Files changed

| Path | Note |
|---|---|
| `crates/wdr_session/Cargo.toml` | New crate manifest; **no dependencies** (empty `[dependencies]`). |
| `crates/wdr_session/src/lib.rs` | All logic + tests in this one file: `SessionState`, `Event`, `Effect`, `Session` with `advance`, lock-safe `next_backoff`, constants, and a `#[cfg(test)] mod tests` with 7 tests. |
| `docs/orchestration/reports/t-B0-session.md` | This report. |

## Decisions

1. **No dependencies at all** (not even `serde`/`postcard`) — the task's hard
   constraint; verified with `cargo tree`.
2. **Pure injected clock**: `advance` takes `now_ms: u64`; the machine stores
   private `last_activity_ms`/`attempt`/`last_reconnect_ms` bookkeeping. Idle
   expiry, control timeout, and backoff are all derived from the injected clock,
   never from `std::time`.
3. **Mode change → renegotiate on lossless, unconditionally.** The prose said
   "if mode==lossless && now requires pro but either peer is free → Renegotiating",
   but the required acceptance test `mid_stream_pro_to_free_invalid` asserts
   Renegotiating with **both peers pro** (`peer_free=false, my_free=false`). The
   spec's dominant invariant (FR-026/FR-047, "never silent downgrade"; ADR-007
   "never silent") resolves the contradiction: any `ModeChangeRequested` while
   lossless goes to Renegotiating for explicit confirmation.
4. **Effects have a consistent, fixed order** per transition (e.g. Recovering:
   `[Recover{...}, StartTimer{...}]`), asserted by `effects_order_is_consistent`.
5. **Backoff budget handling**: `next_backoff` is a pure function
   (`prev*3/2 min 30000`); the `reconnect` timeout arm advances the attempt; once
   the cap (30 s) is reached, a further `reconnect` timeout returns `Ok` and a
   follow-up `RecoveryExhausted` terminates with `Fatal{reason:
   "recovery budget exceeded"}`.
6. **Integer math stays exact** for the sequence
   1500→2250→3375→5062→7593→11389→17083→25624→30000 (saturating ops, no floats).

## Commands run

```bash
source dev/env.sh
cargo build -p wdr_session            # OK
cargo test -p wdr_session             # 7 unit tests: all pass
cargo clippy -p wdr_session --all-targets --all-features -- -D warnings   # clean
cargo fmt -p wdr_session -- --check   # clean
cargo tree -p wdr_session             # wdr_session only (zero deps)
grep -n "std::time" crates/wdr_session/src/lib.rs   # only comments, no code use
```

## Acceptance criteria & validation

| Criterion | Result |
|---|---|
| Crate builds | ✅ `cargo build -p wdr_session` clean |
| Tests green | ✅ 7/7 unit tests pass |
| Clippy clean (`-D warnings`, all targets/features) | ✅ |
| fmt check clean | ✅ |
| No `std::time` | ✅ only doc-comment mentions; all timing injected |
| No external deps | ✅ `cargo tree` shows zero deps |
| Report file present | ✅ this file |

## Tests shipped (`#[cfg(test)]`)

- `transition_table_ok` — hardcoded (state,event)→newstate table incl. Error
  paths, control timeout, sleep/wake, and a no-op row, driven with fake clocks.
- `happy_path` — Idle→…→Streaming, asserting Streaming is reachable only via
  Negotiating+NegotiationOk.
- `backoff_sequence` — injected-now driven 1500→2250→3375→5062→7593→11389→17083
  →25624→30000, cap, then RecoveryExhausted → Terminated.
- `mid_stream_pro_to_free_invalid` — lossless + both-pro `ModeChangeRequested`
  → Renegotiating (never silently keeps lossless, never back to Streaming).
- `idle_expiry` — 60 s no payload → Terminated{reason:"session idle"}; 1 ms before
  stays Streaming.
- `no_sleep_and_no_broadcast` — documents the no-`std::time` grep guarantee.
- `effects_order_is_consistent` — recovery effect ordering is stable.

## Risks / limitations

- The prose/task text and the acceptance test contradict each other on when
  lossless renegotiation triggers; Decision 3 chose the "never silent
  downgrade" invariant, which the required test enforces. If the intended
  semantics were narrower (only when a peer is free), the mode-change guard
  would need revisiting — but that would fail the required test.
- Recovery "budget" is implicit (backoff reaches and stays at the 30 s cap); the
  actual exhaustion is signalled by an explicit `RecoveryExhausted` event. A
  hardcoded attempt-count bound could be added later if needed.
- Idle/keepalive is driven by an injected `Timeout{label:"keepalive"}` event; the
  transport layer must schedule it (caller contract).

## Follow-up tasks

- Wire `wdr_session` into a real driver (B1): schedule control/pairing/reconnect
  timers from returned `Effect`s, feed `disconnected`/keepalive events from the
  transport.
- Add property tests over the transition table (e.g. proptest later, when the
  gate permits a dev-dependency) for exhaustive coverage of unlisted pairs.

## Blockers

None. Crate is standalone; zero dependencies; no sibling-crate coupling.
