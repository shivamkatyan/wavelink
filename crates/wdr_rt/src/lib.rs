//! `wdr_rt` — real-time contract primitives for the Wavelink.
//!
//! The single primitive shipped here is the bounded, lock-free,
//! single-producer / single-consumer byte ring ([`spsc::SpscRing`]) — the
//! *only* data structure a platform RT audio callback (WASAPI / SCK / PipeWire
//! / Oboe / AAudio / AVAudioEngine) may legally touch, per
//! `docs/planning/RT_CONTRACT.md`.
//!
//! # Enforcing the contract
//!
//! The SPSC discipline is enforced by the RT *contract* (who calls what on
//! which thread): RT callback → `SpscRing::try_push` / `try_pop_exact`, worker
//! threads drive the opposite halves off the RT path. `try_push` /
//! `try_pop_exact` contain no allocation, no locks, no syscalls and no
//! blocking — only atomic loads/stores on a preallocated slice. Construction
//! ([`SpscRing::with_capacity`](spsc::SpscRing::with_capacity)) allocates
//! exactly once off the RT path.
//!
//! Optionally, the `rt-guard` feature adds **runtime** enforcement
//! (RT_CONTRACT.md §4): a process-wide `#[global_allocator]` that aborts on any
//! heap allocation made while a thread is marked inside an RT callback (see
//! [`guard::RtGuard`]). Default builds ship the contract and the zero-cost
//! primitives; RT-era binaries opt into the abort-on-alloc guard.

pub mod spsc;

#[cfg(feature = "rt-guard")]
pub mod guard;

pub use spsc::SpscRing;
