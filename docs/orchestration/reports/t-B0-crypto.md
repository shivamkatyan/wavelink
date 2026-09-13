# t-B0-crypto — Wavelink Security Foundation

- **task_id:** t-B0-crypto
- **root_task_id:** R-B0-CRYPTO
- **owner_role:** Security
- **date:** 2026-09-06
- **status:** complete

## Summary

Implemented the standalone `wdr_crypto` Rust crate implementing the locked crypto
contract of `docs/planning/SECURITY_SPEC.md` (and ADR-006): Noise XX pairing,
ed25519 identity + deterministic ed25519→x25519 static mapping, AEAD session
ciphers (XChaCha20-Poly1305 default + 12-byte-nonce ChaCha path), DTLS-style
replay window, per-plane/per-direction key separation, rekey triggers, and the
0-RTT guard. `wdr_crypto` is standalone (does **not** depend on the `wdr_proto`
crate being landed by the parallel worker). The root `Cargo.toml` already uses
`members = ["crates/*"]`, so no root edit was required.

## Files changed

| Path | Note |
|---|---|
| `crates/wdr_crypto/Cargo.toml` | New crate manifest; pinned deps per DEPENDENCY_EVALUATION.md (snow 0.10.0, ed25519-dalek 3.0.0, x25519-dalek 3.0.0, chacha20poly1305 0.11.0, hkdf 0.13.0 + sha2 0.11.0, rand 0.10.2; dev: proptest 1.11.0). |
| `crates/wdr_crypto/src/lib.rs` | Crate root + re-exports + `constants` (REPLAY_WINDOW_SIZE 256, REKEY 24h/2^56, auth-failure 3/60s). |
| `crates/wdr_crypto/src/noise.rs` | Noise XX (`Noise_XX_25519_ChaChaPoly_BLAKE2s`, psk=0, no fallback) via snow; explicit `InitiatorHandshake`/`ResponderWaitingMsg3` state machine; `NoiseHandshake` output carries the **final transcript hash** (SAS source), the peer static X25519 pubkey (QR binding), and the directional split keys; `pair_initiator`/`pair_responder` high-level helpers. Fingerprint-pinning rejects mismatched identities. |
| `crates/wdr_crypto/src/identity.rs` | ed25519 identity keypair (sign/verify), x25519 ephemeral helper + shared-secret DH, and deterministic **ed25519→x25519 static** conversion (SHA-512 expansion + RFC 7748 clamping), with round-trip tests. |
| `crates/wdr_crypto/src/aead.rs` | `XChaChaSessionCipher` (24-byte nonce = `random_salt16 ‖ counter8(BE)`, locked default); `ChaCha12SessionCipher` (12-byte nonce = `salt4 ‖ counter8(BE)` driven by a persisted monotonic counter); `SaltedNonce`; `MonotonicCounter` (never wraps; `None` at overflow → rekey). Authenticated failure throughout. |
| `crates/wdr_crypto/src/replay.rs` | `ReplayWindow`: DTLS-style 256-entry bit window over u64 indices, `Mutex`-protected; accepts in-order/reorder-within-window, rejects duplicates and out-of-window; slide on newest-jump. |
| `crates/wdr_crypto/src/session.rs` | `SessionStatus` (0-RTT guard), `SessionKeys` (control/media × tx/rx), `derive_session_keys` (HKDF separation), `sas_digits`, `RekeyPolicy`. |
| `crates/wdr_crypto/src/data_cipher.rs` | `SessionDataCipher`: `encrypt_session_data` / `decrypt_session_data` per (plane, direction) tying AEAD + monotonic index + replay window + 0-RTT gate together. |
| `crates/wdr_crypto/tests/noise_handshake.rs` | Integration tests (10): success w/ matching hash+SAS, mutual transport encryption, wrong-fingerprint rejection on both roles, pinned-success, fresh keys per session, plane separation, 0-RTT guard. |
| `crates/wdr_crypto/tests/property_tests.rs` | Proptest (7 strategies, 512 cases): AEAD `decrypt(encrypt(x))==x`, tamper/wrong-nonce rejection, replay-window duplicate-drop, nonce uniqueness, monotonic-counter monotonicity. |
| `docs/orchestration/reports/t-B0-crypto.md` | This report. |

## Decisions

1. **Raw split keys via `risky-raw-split`:** snow 0.10's `TransportState` does not
   expose key bytes; enabling snow's `risky-raw-split` feature gives the
   canonical Noise `Split` keys (`dangerously_get_raw_split`), which we use — the
   cleanest correct source for per-direction keys. We do **not** use that feature
   to bypass transport encryption (we still build a real `TransportState` for the
   transport-message path).
2. **Ed25519→X25519 static:** implemented directly (SHA-512 expansion per
   RFC 8032, then RFC 7748 clamping) rather than a dedicated conversion crate —
   the correct, injective, round-trip-testable mapping; identity stays the
   ed25519 pubkey. Reverse (x25519→ed25519) is impossible in general (documented
   limitation).
3. **Plane/direction separation via HKDF over the concatenated split keys**, with
   the per-session `session_id` mixed into the extract salt — so even a
   pathological split reuse still yields per-session keys (SEC-02), and control ≠
   media (T1/T8).
4. **Per-key-scope salt is derived deterministically from the flow key** via HKDF,
   so both peers of a flow derive the same salt without an explicit handshake
   (the flow key already uniquely scopes the plane/direction).
5. **0-RTT guard** is a `SessionStatus::can_transport_media()` gate + `debug_assert`
   + unit/integration tests (SEC-13).
6. **Replay window** implemented as a fixed 256-bit bitset (4×u64 words) with
   slide-on-jump, rather than a single u64 — avoids shift overflow and matches
   the spec's 256-entry width exactly.
7. **12-byte nonce path** is opt-in `ChaCha12SessionCipher` + `MonotonicCounter`
   (persisted-counter contract documented); the *default* remains XChaCha
   (24-byte). Both share authenticated-failure semantics.

## Commands run

```bash
# (source dev/env.sh first)
cargo build --workspace            # OK
cargo test -p wdr_crypto           # 28 unit + 10 integration + 7 proptest: all pass
cargo test --workspace             # 13 suites ok (incl. wdr_proto/wdr_entitlement)
cargo clippy --workspace --all-targets -- -D warnings   # clean
cargo fmt --all -- --check         # clean
```

## Acceptance criteria & validation

| Criterion | Result |
|---|---|
| Workspace builds | ✅ `cargo build --workspace` clean |
| Crypto tests green (incl. property: AEAD roundtrip, replay dup-drop) | ✅ 45 tests + 7×512 proptest cases |
| Clippy clean (`-D warnings`, all targets) | ✅ |
| Handshake transcript hash exposed for SAS | ✅ `NoiseHandshake::handshake_hash()` + `sas_digits` |
| Wrong-fingerprint responder rejected | ✅ unit + integration (both roles) |
| Separate control/media + fresh-per-session keys | ✅ tests |
| 0-RTT guard (no media pre-confirm) | ✅ `SessionStatus` + `debug_assert` + tests |
| Replay window 256-entry per (plane,direction) | ✅ `ReplayWindow` + `SessionDataCipher` |

## Risks / limitations

- `ed25519→x25519` uses the RFC 8032/7748 direct recipe (no vendor conversion
  crate); correctness relies on the round-trip tests and matches `StaticSecret`
  semantics. If a reviewer insists on a third-party conversion (e.g.
  `curve25519-dalek`'s own helper), it must produce the identical clamped bytes —
  worth a cross-check at the B6 hardening gate.
- `risky-raw-split` exposes split key bytes only transiently in-process; keys stay
  memory-only, never persisted (SECURITY_SPEC §3.2).
- Handshake message boundaries/innocence: XX messages here carry empty payloads
  (identities are in-band); the transport (de)serialization between peers is the
  caller's concern (B1 session core).
- Replay window is per-(plane,direction) *instance*; the session core must give
  each direction its own `SessionDataCipher` (keyed by the separate tx/rx keys).
- 12-byte-nonce persisted-counter durability (fsync) is the caller's contract,
  documented but not implemented here (platform shellB1 concern).

## Follow-up tasks

- B1 session core: wire `NoiseHandshake` (transcript → SAS/QR UI, peer static →
  trust store) into pairing FSM; drive `SessionDataCipher` per (plane, direction).
- Cross-validate `ed25519→x25519` bytes against a vendor helper at B6.
- `cargo audit`/`cargo-deny` gate for the new dependency closure (ossdep
  lockfile finalization).

## Blockers

None. `wdr_crypto` is standalone; no dependency on `wdr_proto`.
