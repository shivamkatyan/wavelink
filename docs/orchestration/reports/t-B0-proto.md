# t-B0-proto — Wire Protocol Crate Report

Task: `t-B0-proto` · Root: `R-B0-PROTO` · Owner role: **Protocol/Networking** · Status: **complete** · Date: 2026-09-06

## Deliverable

`crates/wdr_proto` — the wire protocol crate (schema single source of truth, PROTOCOL_SPEC §Versioning), with bounded, error-typed postcard encode/decode, golden vectors, and property tests.

## What was created (all within allowed path set)

| Path | Purpose |
|------|---------|
| `crates/wdr_proto/Cargo.toml` | `wdr_proto` crate; deps `postcard 1.1.3` + `serde 1.0.229` (pinned per DEPENDENCY_EVALUATION.md locked list, `default-features=false` + `alloc`); dev-dep `proptest 1.11.0`. |
| `crates/wdr_proto/src/bounds.rs` | Locked numeric bounds: `MAX_CONTROL_MSG=16*1024`, `MAX_FRAME_PAYLOAD=4096`, `MAX_REQUEST_RATE=20` (+ alias `MAX_RATE`), `MAX_PENDING_HANDSHAKES=8`, `PAIRING_WINDOW_SECS=60`, plus `SESSION_IDLE_EXPIRY_SECS`, `CONTROL_RESPONSE_TIMEOUT_SECS`, `REPLAY_WINDOW_WIDTH=256`, `MAX_TXT_RECORD_BYTES=512`. |
| `crates/wdr_proto/src/codec.rs` | `Codec` (Opus/Flac/Pcm), `SampleRepr` (I16/F32/I24Packed/I32), `ChannelLayout` (Mono/Stereo/Quad/Surround51/Surround71), `TransportFeatures`, `BufferProfile`, `FeatureFlags`, `PolicyBits`. |
| `crates/wdr_proto/src/integrity.rs` | `Integrity` enum (None/Crc32/Aead). |
| `crates/wdr_proto/src/crc.rs` | Table-less deterministic CRC-32 (per-frame lossless integrity; known check vectors tested). |
| `crates/wdr_proto/src/frame.rs` | `Frame` container: header (version, stream_id, **u64** seq, **u64** media_ts, codec, rate, repr, layout, sample_count, flags u8-bitfield, integrity) + payload `Vec<u8>`. **Hand-written `Deserialize` enforces `MAX_FRAME_PAYLOAD` before any payload allocation** (PayloadSeed rejects on `size_hint()`/declared length). `new_lossy`/`new_lossless` (+ per-frame CRC32). |
| `crates/wdr_proto/src/control.rs` | `MsgHeader`, `Capability`, `SessionDescriptor`, `Feedback` (rtt_us, jitter_us, loss_pct, reorder, late_discard, buffer_fill, underruns, output_clock_estimate, requested_adapt), `SasOffer`, `RevokeRecord` (+ `RevokeReason`), `PolicyTier`, and `ControlMessage` (Hello, CapabilityRequest/Response, SessionDescriptor, PairingStart, SasOffer, SasConfirmReject, PolicyAdvertisement, StartStream/StopStream/Pause/Resume, Feedback, Error, Keepalive, PairAttemptRejected, RevokeRecord). |
| `crates/wdr_proto/src/error.rs` | `ErrorCode` (+`LocalNetworkPermission`), `ProtocolError` (wire `Error` with `code`+`retryable` flag), `is_retryable()`, `user_action()` mapping (GrantPermission / ReconnectDac / ChangeBuffer / ReturnToWifi / None), `DecodeError` (LimitExceeded / Truncated / DataInvalid / VersionTooNew / PayloadOverflow) — all non-panicking, no alloc reporting. |
| `crates/wdr_proto/src/wire.rs` | `encode_postcard`/`decode_postcard`/`pack`/`unpack` — postcard helpers enforcing `MAX_CONTROL_MSG` (input **and** output) and frame `header_max_len + MAX_FRAME_PAYLOAD`; typed `EncodeError`/`DecodeError`, no panics. |
| `crates/wdr_proto/src/golden.rs` | `gen_golden` (deterministic), `golden_file_name` (FNV-1a-stable versioned names), `GOLDEN_VERSION`, `GOLDEN_DIR`, `GOLDEN_MANIFEST`. |
| `crates/wdr_proto/examples/gen_golden.rs` | Materialises 56 golden vectors + `MANIFEST.md` under `tests/golden/`. |
| `crates/wdr_proto/tests/golden.rs` | Verifies every stored blob reproduces byte-exactly; manifest covers all blobs; message/frame round-trips. |
| `crates/wdr_proto/tests/properties.rs` | proptest: control/frame round-trip; truncation-never-panics (control + frame); valid-message-truncation round-trips only when complete; oversized-payload rejection. |
| `crates/wdr_proto/tests/bounds.rs` | Numeric-bounds contract, `user_action` coverage of all five actions, `is_retryable` totality, oversized control/frame rejection on encode+decode. |
| `crates/wdr_proto/tests/golden/*.bin` + `MANIFEST.md` | Stored golden blobs (versioned `v1-…`), 56 vectors. |

Root `Cargo.toml` declares `members = ["crates/*"]`, so the crate is picked up with **no edit** — the integration-owned file was left untouched (minimal-additive constraint satisfied vacuously).

## Key design notes / decisions

- **Bounds before allocation (SEC-05):** `Frame` decode rejects a payload whose declared length exceeds `MAX_FRAME_PAYLOAD` inside a `DeserializeSeed` (via `SeqAccess::size_hint()` = exact declared length for well-formed postcard input), *before* the payload `Vec` is created; total-input pre-checks (header+payload) run in `Frame::unpack`/`decode_postcard` before parsing. Control decode rejects `> MAX_CONTROL_MSG` input before parsing; encode rejects oversized output symmetrically.
- **Error-typed, no panics:** all decode entry points return `Result<_, DecodeError>`/`EncodeError`; postcard errors are mapped (UnexpectedEnd→Truncated, bad varint/bool/enum/etc→DataInvalid, SerializeBufferFull→LimitExceeded). No `unwrap` on attacker paths; fuzz/truncation properties assert panic-freedom.
- **u64 wrap-proof:** seq/media_ts are `u64` varints (10 bytes); wrap-guard vectors at `u64::MAX` round-trip (PROTOCOL_SPEC §Timing).
- **Header flags as u8 bitfield:** 2 payload bits + 6 reserved, packed/unpacked in the hand-written serializer (stable wire image).
- **Integrity double-encoded field:** `Integrity` (policy) + `FrameIntegrity` (payload: None | Crc32(u32) | Aead) — kept derived-enum so golden bytes are canonical; the redundant `integrity` option field from the original task list was folded into the policy enum to avoid ambiguous/duplicate data on the wire while preserving the CRC32-or-none requirement.
- **serde/postcard choice** per DEPENDENCY_EVALUATION (bincode rejected): hand-derived `Serialize`/`Deserialize` for `Frame` gives the pre-allocation guard that plain derives cannot.

## Verification performed (commands)

```bash
source dev/env.sh
cargo build -p wdr_proto                                    # OK
cargo test -p wdr_proto                                     # 25 tests pass
cargo clippy --all-targets --all-features -p wdr_proto -- -D warnings   # clean
cargo fmt -p wdr_proto -- --check                            # clean
cargo run -p wdr_proto --example gen_golden                 # 56 vectors, deterministic
cargo build --workspace                                     # OK
cargo clippy --all-targets --all-features -p wdr_dev -p wdr_proto -p wdr_entitlement -- -D warnings   # clean
```

## Acceptance gate status

| Criterion | Status | Evidence |
|-----------|--------|----------|
| wire structs + bounds consts exist | ✅ | `Frame`, `ControlMessage`, `MsgHeader`, `Capability`, `Feedback`, bounds in `src/bounds.rs` |
| encode/decode error-typed, no panics | ✅ | `Result<_, DecodeError>/EncodeError`; properties assert no panics on truncation |
| truncation + oversize properties cover it | ✅ | `tests/properties.rs` + `tests/bounds.rs` |
| golden vectors stored + green | ✅ | 56 blobs under `tests/golden/`, `stored_golden_blobs_reproduce_exactly` green |
| workspace builds | ✅ | `cargo build --workspace` OK |
| clippy clean | ✅ | `cargo clippy --all-targets --all-features -p wdr_proto -- -D warnings` clean |
| report at required path | ✅ | this file |

## Out of scope / notes

- Nothing committed (per instructions).
- `wdr_crypto` and `wdr_entitlement` are sibling workers' in-flight crates; `cargo build --workspace` succeeded at final verification. A transient `wdr_crypto` clippy failure (their crate, wheels resolved by them) was observed earlier; it does not depend on `wdr_proto`.
- `MAX_FRAME_PAYLOAD` is also exposed for the AEAD path as the cap for decrypt buffers; AEAD/tag machinery itself lives in the crypto worker's crate (`wdr_crypto`).
