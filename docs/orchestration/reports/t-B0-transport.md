# t-B0-transport — QUIC Transport Spike (wdr_transport)

**task_id:** t-B0-transport · **root_task_id:** R-B0-TRANSPORT · **owner_role:** Networking · **date:** 2026-09-06 · **status:** complete

## Summary

Built `crates/wdr_transport` — a **spike** QUIC transport harness on `quinn 0.11.11`
(default rustls-ring) — and ran loopback measurements per ADR-003 §B0-spike /
PROTOCOL_SPEC §2.4 to produce **measured evidence** (not just code) for the
transport decisions. The crate also pins the transport measurement surface for
B1. **Measurements below are loopback-only** (no `netem` in the Docker context);
every number that needs path-loss/jitter evidence is flagged as a **B1 compose
harness (netem)** follow-up.

### Measurement table

| # | Measurement | Result (this run) | Notes |
|---|---|---|---|
| (a) | `conn.max_datagram_size()` (loopback, Cubic) | **1162 B** | path MTU 1452 − QUIC overhead; quinn guarantees "a little over 1 KiB" |
| (a) | 20 ms raw PCM (48k·16-bit stereo) | 3840 B payload → **3916 B packet: NO fit** | 0/64 sent, 64 `TooLarge` |
| (a) | 10 ms raw PCM | 1920 B → **1996 B packet: NO fit** | 0/64 sent, 64 `TooLarge` |
| (a) | 5 ms raw PCM | 960 B → **1036 B packet: FITS** (≤1162) | 64/64 sent |
| (a) | FLAC-of-noise | ≈ raw PCM size (**no entropy win**) | documented; also NO at 20/10 ms |
| (a) | Conclusion | **Lossless OVER a datagram is NOT viable at 20/10 ms block sizes** — raw packet exceeds the QUIC datagram MTU. Lossless must ride the reliable stream (ADR-003, matches expectation). | `NO` — confirmed measured |
| (b) | Opus 20 ms ≈400 B datagram burst (10 000 frames) | sent 10 000 in **4.84 ms** ≈ **2.07 M fps send-side** | loopback burst; not a steady-state rate |
| (b) | Datagram receive (receiver drains concurrently) | 8359 / 10 000 received in 270.7 ms | **loss-free loopback still dropped frames** at burst — receiver-side datagram buffer + driver scheduling; see risk R2 |
| (b) | Send p50/frame (wall-clock) | 9.7 µs (send-side avg 484 ns) | `Instant`-based; fake-clock seam reserved |
| (b) | Datagram echo ping p50 / p99 | **149 µs / 857 µs** | loopback echo path (server echoes each datagram); `datagram ack` is not a QUIC concept |
| (c) | 0-RTT off (media) | **✓ verified**: `into_0rtt()` fails with `enable_early_data=false`; `handshake_data()` only after completion; no pre-handshake datagram possible | rustls `enable_early_data=false` + assertion (SEC-13) |
| (c) | 0-RTT token path never used | ✓ | SECURITY_SPEC §3.6 / Appendix A #3 (off for **both** control + media) |
| (d) | Malformed datagram (128 B garbage) | `recv_datagram` yields bytes, **no panic**, connection healthy; subsequent real frame received | decode failure deferred to `wdr_proto` (SEC-05) |
| 5 | quinn path metrics (loopback, Cubic) | rtt 94 µs · cwnd 166 568 B · **lost_pkts 547** · congestion_events 7 · mtu 1452 · ACK-frames rx 141 | `datagram_lost` is **not app-observable** in quinn (datagrams unreliable, un-acked); proxies reported |
| 6 | BBR vs CUBIC | **not distinguishable on loss-free loopback** — both construct + loop OK | **BBR-vs-CUBIC under loss requires `netem` (B1 compose harness)** — this spike only confirms the factory seam works |

**Headline conclusion:** `quinn` datagram MTU (≤1.3 KB incl. frame overhead) cannot
carry raw-PCM/FLAC lossless frames at 20 ms (3840 B) or 10 ms (1920 B) block
sizes; only 5 ms blocks fit. Lossless therefore uses the reliable stream with a
retransmit deadline (≤50 ms), exactly per ADR-003. Opus 20 ms datagrams (≈400 B)
fit comfortably (2 ms worth of 20 ms frames per packet at 5 ms block: 960 B also
fits). The spike also surfaced two real findings (see Risks): quinn 0.11.17
datagram drop-path overflow under sustained over-budget sends, and datagram
drops even on loss-free loopback under a tight send burst.

## Files changed

| Path | Change |
|---|---|
| `crates/wdr_transport/Cargo.toml` | New crate: `quinn 0.11.11` (default rustls-ring), `tokio 1.53.1`, `bytes 1.12.1`; optional `rcgen 0.14.0` behind `spike-meas`; dev `rand 0.10.2`. Feature `spike-meas` isolates the measurement surface. |
| `crates/wdr_transport/src/lib.rs` | Crate root docs + re-exports. |
| `crates/wdr_transport/src/wire.rs` | `TransportConnConfig` (default **0-RTT OFF**, `congestion_control(Cubic\|Bbr)` via `congestion_controller_factory`, `datagram_max_payload`, `peer_max_udp_payload_size`, `stream_receive_window`, `set_datagram_send_buffer`); `make_client_config` (rustls `enable_early_data=false` since quinn's helper enables it by default); typed `SendDatagramOutcome`/`Got`; `try_send_datagram`, `recv_datagram_timeout`, `drain_datagrams`, `handshake_probe`, `stream_send_with_deadline`/`stream_receive_with_deadline` (injectable `now` wall-clock deadline, abort on blow), `rustls_configs` (loopback-only), metrics helpers. |
| `crates/wdr_transport/src/meas.rs` | `raw_pcm_ms_bytes`/`flac_noise_ms_bytes`/`fit_matrix` byte-budget (analytic cross-check of the measured MTU). (spike-meas) |
| `crates/wdr_transport/src/metrics.rs` | `Metrics` (frames_sent, dropped_congestion, dropped_too_large, frames_received) + `PathReport` documenting exactly what quinn exposes for datagram loss (none on ack). |
| `crates/wdr_transport/tests/transport_contract.rs` | Loopback contract tests (7): datagram+stream roundtrip, 0-RTT-off assertion, deadline abort, malformed-datagram no-panic, metrics counters, drain. |
| `crates/wdr_transport/examples/transport_spike.rs` | The measurement harness that produced the table above. (spike-meas) |
| `docs/orchestration/reports/t-B0-transport.md` | This report. |

Root `Cargo.toml` and the root lockfile: not edited here. The root already uses
`members = ["crates/*"]`. `Cargo.lock` was regenerated by cargo (adds quinn/
rustls/ring closure + rcgen/rand dev), consistent with DEPENDENCY_EVALUATION.
`crates/wdr_codec` (parallel in-flight worker, mid-edit) was **left untouched**
and is excluded from the validation set below (same approach as t-B0-ent).

## Decisions

1. **0-RTT off enforced on the rustls side too:** `quinn::ClientConfig::with_root_certificates()`
   sets `enable_early_data = true` by default. `make_client_config` hand-builds
   the `rustls::ClientConfig` (ring provider, TLS1.3-only, loopback accept-any
   verifier) and sets `enable_early_data = cfg.zrt_enabled_media`; default
   `TransportConnConfig::disable_0rtt()` ⇒ `false` (SECURITY_SPEC §3.6 / ADR-003
   / RFC 9221 — datagrams forbidden in 0-RTT). SEC-13 asserted in tests.
2. **CC selection is per-`TransportConfig` factory, not a live setter:** quinn 0.11
   has **no** `Connection::set_congestion_controller`; the controller is fixed at
   connection build from `congestion_controller_factory` (default Cubic; `Bbr`
   available as `quinn::congestion::BbrConfig`). Option exposed as
   `congestion_control(Cubic|Bbr)`; a BBR-vs-CUBIC *measurement* therefore needs
   two connections under identical load + **loss** (`netem`) → B1.
3. **Lossless stream deadline is an app-layer wall-clock wrapper:** stream
   retransmits are internal to QUIC — `SendStream::write` completing does not
   mean acked. The ≤50 ms budget is enforced by racing each `write` against
   `deadline − now.elapsed()` with an injectable `now` (`stream_send_with_deadline`);
   on blow the stream is aborted. The receiver-side reader is a stub signature
   (the true reader lives in the B1 session core) — tested via the sender side.
4. **Datagram loss is measured by proxy, honestly:** quinn 0.11 does not expose
   per-datagram acks (`datagram_lost` not app-measurable). Reported: send-side
   `TooLarge`/send-buffer counters (ours), and `ConnectionStats.path`
   (`lost_packets`, `congestion_events`, `rtt`, `cwnd`, `mtu`) + `frame_rx.acks`.
5. **`spike-meas` feature gate:** the measurement example + `rcgen` + byte-budget
   module sit behind `spike-meas` so the crate the B1 system depends on stays
   lean; contract tests need no feature.
6. **Loopback certificates:** self-signed `localhost` cert generated with `rcgen`
   (dev-only, feature-gated); client uses an accept-any verifier purely for
   loopback. Production replaces with platform verifier + fingerprint pinning
   (SECURITY_SPEC §3.4) — documented in code, not implemented (spike scope).

## Commands run

```bash
source dev/env.sh
cargo build -p wdr_transport                                   # default (no feature) OK
cargo build -p wdr_transport --features spike-meas             # OK
cargo test  -p wdr_transport                                   # 3 unit + 7 integration green
cargo test  -p wdr_transport --features spike-meas             # green
cargo clippy -p wdr_transport --all-targets --all-features -- -D warnings   # clean
cargo fmt    -p wdr_transport -- --check                       # clean
# measurement run (produced the table):
cargo run -p wdr_transport --features spike-meas --example transport_spike
# workspace subset (excl. in-flight wdr_codec, same as t-B0-ent):
cargo build -p wdr_crypto -p wdr_proto -p wdr_entitlement -p wdr_dev -p wdr_transport  # OK
cargo test  -p wdr_crypto -p wdr_proto -p wdr_entitlement -p wdr_dev -p wdr_transport  # 104 OK
# quinn overflow reproduction (dev-time probe, in /tmp — not committed):
cargo run --manifest-path /tmp/opencode/drop_probe/Cargo.toml  # 20k×400B tiny buffer → panic at datagrams.rs:203
```

## Validation results

- `cargo test -p wdr_transport`: material on **3 unit + 7 integration, 0 failed**.
  Integration names: `loopback_roundtrip_datagram_and_stream`, `zero_rtt_off_media_path`,
  `stream_deadline_abort`, `malformed_datagram_no_panic`, `metrics_counters_reflect_outcome`,
  `drain_nonblocking_and_receivers`, `cc_variants_distinct`.
- `cargo clippy --all-targets --all-features -D warnings`: **clean** (incl. example).
- `cargo fmt -- --check`: **clean**.
- Workspace subset build + tests: **green** (104 tests: crypto 45, dev 2, entitlement 22,
  proto 25, transport 10). `crates/wdr_codec` excluded (parallel worker mid-edit).
- 0-RTT-off: `into_0rtt()` returns `Err(Self)` with early data disabled (matches
  quinn's own `zero_rtt` test semantics); `handshake_data()` is `Some` only after
  handshake — asserted in `zero_rtt_off_media_path` and printed by the example.
- Malformed-datagram: no panic, typed result, connection stays healthy (SEC-05/06 posture).

## Acceptance criteria

| Criterion | Result |
|---|---|
| `wdr_transport` crate green (build/tests/clippy/fmt) | ✅ default + `spike-meas` both green; clippy `-D warnings` clean |
| Datagram MTU + fit-or-not at 20/10/5 ms recorded | ✅ 1162 B; 20/10 ms **NO**, 5 ms FITS; lossless-over-datagram NOT viable at required sizes (**confirm measured, NO**) |
| Latency p50/p99 recorded | ✅ 149 µs / 857 µs (loopback echo); fake-clock seam in `stream_send_with_deadline` (`now: Instant` injectable) |
| BBR–CUBIC observation | ✅ both factories construct; not distinguishable on loss-free loopback — **measured to need netem** (B1) |
| 0-RTT off verified | ✅ `enable_early_data=false` + `into_0rtt()` fails + `handshake_data()` post-completion (SEC-13) |
| Malformed datagram no-panic | ✅ test + example |
| Metrics counters exposed | ✅ `Metrics` + `PathReport` (ack/loss/cwnd/rtt/mtu) with the datagram-loss gap documented |
| Contract tests | ✅ roundtrip (datagram + stream), 0-RTT-off, deadline abort, no-panic, counters |
| Dead code/spike vs production documented | ✅ in code docs + risks; loopback-only numbers clearly separated from netem-needed |

## Risks / limitations

- **R1 — quinn 0.11.17 datagram-buffer drop overflows (upstream bug).** When the
  outgoing datagram send buffer budget is exceeded, `quinn-proto` drops old
  datagrams via `send` + `pop_front` which **double-decrements `payload_bytes`
  and underflows** (`src/connection/datagrams.rs:203`, panics → poisons the
  connection mutex). Reproduced with a tiny buffer + 20k×400 B burst. At the
  pinned 0.11.11 default 1 MiB budget this needs a large burst/mismatch, but any
  production datagram-credit path that drops must be tested against it; **track
  upstream (quinn#??) and re-verify at pin before B1 soak**. Mitigation in this
  spike: `send_datagram` (drop=true) path is the risk; we set a large buffer for
  the throughput number and documented the drop path.
- **R2 — datagram drops even on loss-free loopback.** Bursting 10 000 × 400 B
  faster than the single-threaded receiver drains it dropped ~16% of frames on
  loopback (receiver datagram buffer + driver scheduling). This is exactly the
  "datagram credit starvation / CC collapse" concern in ADR-003 — the app must
  **not** burst faster than the receiver drains; the PROTOCOL_SPEC "datagram
  credit high-water metric asserted in soak" and send-side credit accounting are
  **required**, and pacing must be measured under loss (netem) at B1.
- **R3 — loopback-only numbers.** p50/p99, achieved burst rate, cwnd/loss all
  reflect `127.0.0.1` with zero real congestion/loss/jitter. BBR vs CUBIC,
  retransmit-deadline under loss, and CC collapse need the **B1 compose harness
  with `netem`** (wire `tc netem` in the box image).
- **R4 — default quinn MTU 1162 B is IPv4-loopback-specific.** 1452 is the DOCSIS
  safe UDP payload; on real Ethernet paths with jumbo frames the datagram bound
  may rise, but 3840 B (20 ms PCM) still cannot fit any sane datagram — the
  lossless-over-stream conclusion is robust.
- **R5 — `handshake_data` is a `Box<dyn Any>`.** We assert presence/absence, not
  the `crypto::rustls::HandshakeData` ALPN fields; that refinement belongs to the
  B1 session core.
- **R6 — rcgen is a dev-ish dependency** (feature `spike-meas`). Per
  DEPENDENCY_EVALUATION it must earn a row if we ever ship the cert generator in
  a production artifact; currently loopback/test-only. `cargo audit` / `deny`
  gate is the ossdep worker's B0 lockfile job.
- **Dead code documented:** `accumulate_acks`/`stream_receive_with_deadline`
  (stub) are placeholders for the planned production adapter/B1 receiver seam —
  labelled in code, not production paths.

## Follow-up tasks

- **B1 compose harness (netem):** wire `tc netem` into the box image; then measure
  BBR vs CUBIC, retransmit-deadline behavior under loss, and CC-starvation at the
  required degraded profile; re-check datagram credit starvation (R2) + the quinn
  drop-path overflow (R1) at pin.
- **B1 session core:** build the true async receiver (stream + datagram) behind
  the adapter seam; drive `stream_receive_with_deadline` and real
  `FeedbackMessage` control-stream send path (PROTOCOL_SPEC §Feedback) from the
  control scheduler.
- **Production transport adapter** (ADR-001/ARCHITECTURE): replace accept-any
  loopback verifier with platform verifier + pinned fingerprint; configure server
  certs from the trust store.
- **ossdep:** finalize B0 lockfile; add `cargo audit`/`cargo-deny`; add rcgen +
  quinn closure rows to DEPENDENCY_EVALUATION rubric.
- Re-run the 0-RTT-off / fit / latency suite from the netem harness to upgrade
  loopback numbers to LAN numbers.

## Blockers

- None for this task. Workspace-wide validation currently excludes the parallel,
  in-flight `crates/wdr_codec` (its worker is mid-edit; not touched here) — same
  merge-order caveat as t-B0-ent/index.
