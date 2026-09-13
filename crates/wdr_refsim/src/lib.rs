//! `wdr_refsim` — WDR B1 reference simulator (loopback e2e + CLI).
//!
//! Owned by task `t-B1-receiver` (Protocol/Audio). This crate is the
//! **receiver half** of the reference simulator and also hosts the loopback
//! end-to-end tests. The emitter binary (`ref_emitter`) is owned by a sibling
//! worker; if it does not exist in the workspace tree yet, the tests here
//! drive an in-process emitter through `wdr_transport` loopback instead.
//!
//! # What lives here
//!
//! * [`receiver`] — the receiver pipeline mirrored from `ARCHITECTURE.md`
//!   §Audio data flow: transport reader (datagram/lossy or reliable-stream/
//!   lossless), bounded `Frame::unpack` with `MAX_FRAME_PAYLOAD` + CRC guards,
//!   a bounded in-order jitter buffer (sorted by `u64` seq; drops duplicates;
//!   counts late discards beyond the reorder window), `wdr_codec` decode into
//!   the canonical `wdr_fakes` `HashSink`, and a light injectable-clock
//!   `NullRenderSink` that also counts underruns.
//! * [`metrics`] — the `receiver-sim.json` metrics contract used by the
//!   compose harness (`docker/assert.sh`): role, status, `packets_recv`,
//!   loss, duplicate, reorder, late_discard, fatal_count, hash.
//! * [`wire`] — a frame framing format (defines an optional `StreamEnd`
//!   control marker and datagram frame boundaries by length), plus
//!   deterministic send-side emitters for the e2e tests.
//! * [`util`] — env overrides, CLI parsing helpers and an injectable fake clock.
//!
//! # Golden contract
//!
//! The canonical golden hash for the pseudo-random i16 48 kHz stereo fixture
//! is `b7a3c25c…` (chunk 512, total 4096 samples — see
//! `wdr_fakes/tests/golden.rs` and `docs/orchestration/reports/t-B0-fakes.md`).
//! The lossless e2e tests assert `receiver hash == canonical golden` with
//! tolerance-free equality.

pub mod drift;
pub mod emitter;
pub mod framing;
pub mod receiver;
pub mod receiver_server;
pub mod sink;

pub use emitter::{
    lane_and_meta, policy_gate, tier_from_env, wire_meta, Emitter, EmitterConfig, EmitterError,
    Impairment, StreamKind,
};
pub use framing::{FrameWire, FrameWire as FramedFrame, FramedItem, FramingError};
pub use receiver::{
    guard_frame, parse_and_guard, BufferProfile, ClockHandle, JitterBuffer, NullRenderSink,
    Receiver, ReceiverError, ReceiverMetrics, ReceiverOutcome, MAX_QUEUE_BOUND, REORDER_WINDOW,
};
pub use sink::{dial_loopback, AudioFrameSink, FrameSink, QuicAudioSink, SinkError, SinkFormat};
