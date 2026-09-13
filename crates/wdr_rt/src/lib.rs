//! `wdr_rt` — real-time contract primitives for the Wavelink.
//!
//! The single primitive shipped here is the bounded, lock-free,
//! single-producer / single-consumer byte ring ([`spsc::SpscRing`]) — the
//! *only* data structure a platform RT audio callback (WASAPI / SCK / PipeWire
//! / Oboe / AAudio / AVAudioEngine) may legally touch, per
//! `docs/planning/RT_CONTRACT.md`.
//!
//! The SPSC discipline is enforced by the RT *contract* (who calls what on
//! which thread), not by runtime guards:
//!
//! * RT capture callback → `SpscRing::try_push` (producer half)
//! * RT render callback  → `SpscRing::try_pop_exact` (consumer half)
//! * worker threads drive the opposite halves via
//!   [`worker_fill`](spsc::worker_fill) / [`worker_drain`](spsc::worker_drain)
//!
//! `try_push` / `try_pop_exact` contain no allocation, no locks, no syscalls
//! and no blocking — only atomic loads/stores on a preallocated slice — so they
//! satisfy the RT callback allowed-operation whitelist. Construction
//! ([`SpscRing::with_capacity`](spsc::SpscRing::with_capacity)) allocates
//! exactly once off the RT path.

pub mod spsc;

pub use spsc::SpscRing;
