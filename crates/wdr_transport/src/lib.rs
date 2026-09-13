//! `wdr_transport` — WDR QUIC transport harness (**B0 spike**, task `t-B0-transport`).
//!
//! This is a **spike**, not the production transport. Its purpose is to produce
//! *measured* evidence (ADR-003 §B0 spike, PROTOCOL_SPEC §2.4 "measurement
//! discipline") for the QUIC transport decisions:
//!
//! * whether a lossless raw-PCM / FLAC audio frame fits inside a QUIC datagram
//!   at the required 20/10/5 ms block sizes (expect **NO** at ≥10 ms),
//! * datagram throughput + latency for Opus 20 ms frames under loopback,
//! * that 0-RTT / early data is **off** for the media path (SECURITY_SPEC
//!   §3.6 / SEC-13 / ADR-003; `SECURITY_SPEC.md` Appendix A #3 tightens this to
//!   both control *and* media),
//! * that `recv_datagram` returns a typed result and stays healthy when a
//!   malformed datagram is delivered (SEC-05; decode failure belongs to
//!   `wdr_proto`, not here — the transport must not panic),
//! * a stream-based "retransmit deadline" wrapper for lossless media with an
//!   injectable clock, ready for a fake clock in later work.
//!
//! The loopback measurements are host-only (no `netem` in the Docker context);
//! numbers needing path-loss/CC-starvation evidence (`netem`) are recorded as
//! **B1 compose harness** follow-ups in `docs/orchestration/reports/t-B0-transport.md`.
//!
//! # Surface
//!
//! * [`wire::TransportConnConfig`] — builder: 0-RTT **off** by default for media,
//!   CC selection (Cubic | Bbr), datagram payload cap, loopback MTU probe.
//! * [`wire::SendDatagramOutcome`] — typed results.
//! * [`metrics::Metrics`] — dropped-due-to-congestion / too-large counters.
//! * [`metrics::PathReport`] / [`wire::metrics_snapshot`] — what `quinn` actually
//!   exposes (ack/path stats via [`quinn::ConnectionStats`]).
//! * [`meas`] (feature `spike-meas`) — the byte-budget fit matrix (20/10/5 ms).
//!
//! The real loopback harness runs from `examples/transport_spike.rs` (prints
//! the measurement table); the integration tests in `tests/` assert the
//! non-measurement contract behaviours (roundtrip, 0-RTT-off, deadline abort,
//! no-panic on malformed datagram).

pub mod metrics;
pub mod wire;

#[cfg(feature = "spike-meas")]
pub mod meas;

pub use metrics::Metrics;
pub use wire::{
    accumulate_acks, drain_datagrams, handshake_probe, make_client_config, make_client_endpoint,
    metrics_snapshot, receive_datagrams, recv_datagram_timeout, send_datagrams,
    stream_receive_with_deadline, stream_send_with_deadline, try_send_datagram, CongestionControl,
    Got, SendDatagramOutcome, TransportConnConfig,
};

#[cfg(feature = "spike-meas")]
pub use wire::make_server_config;
