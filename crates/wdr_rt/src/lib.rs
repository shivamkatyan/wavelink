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
//! This module is **`#![no_std]`** (RT_CONTRACT §4): the shipped surface uses
//! only `core` + `alloc`, with the single allocation in `with_capacity` living
//! off the RT path. A `#[cfg(test)]` counting allocator asserts the RT surface
//! (`try_push` / `try_pop_exact` / `free_space` / `available_len`) performs
//! **zero** allocations. `std` is pulled in only for the `rt-guard` feature
//! and tests.
//!
//! Optionally, the `rt-guard` feature adds **runtime** enforcement
//! (RT_CONTRACT.md §4): a process-wide `#[global_allocator]` that aborts on any
//! heap allocation made while a thread is marked inside an RT callback (see
//! [`guard::RtGuard`]), plus a [`watchdog::StallDetector`] that aborts on a
//! callback stall past a budget. Default builds ship the contract and the
//! zero-cost primitives; RT-era binaries opt in to the runtime guards.

#![no_std]

extern crate alloc;

// `std` for the runtime guards (`rt-guard` feature) and for tests. A single
// declaration: both can be on at once (test of a rt-guard build).
#[cfg(any(feature = "rt-guard", test))]
extern crate std;

pub mod spsc;

#[cfg(feature = "rt-guard")]
pub mod guard;
#[cfg(feature = "rt-guard")]
pub mod watchdog;

pub use spsc::SpscRing;

#[cfg(all(test, not(feature = "rt-guard")))]
mod alloc_probe {
    //! Zero-allocation proof for the RT surface. A counting `#[global_allocator]`
    //! (installed only when `rt-guard` is OFF — the guard installs its own)
    //! counts every allocation; the probe runs in a **child process running only
    //! that test**, because the counter is process-wide and in-process peers
    //! (the other, parallel lib tests) would perturb it. The child therefore
    //! sees a deterministic counter: allocation happens only in `with_capacity`
    //! (off the RT path), never in the RT-facing calls.
    use core::alloc::{GlobalAlloc, Layout};
    use core::sync::atomic::{AtomicUsize, Ordering};
    use std::prelude::v1::*;

    static ALLOCS: AtomicUsize = AtomicUsize::new(0);

    struct CountingAlloc;
    #[global_allocator]
    static A: CountingAlloc = CountingAlloc;

    unsafe impl GlobalAlloc for CountingAlloc {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            ALLOCS.fetch_add(1, Ordering::Relaxed);
            unsafe { std::alloc::System.alloc(layout) }
        }
        unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
            unsafe { std::alloc::System.dealloc(ptr, layout) }
        }
    }

    #[test]
    fn rt_surface_performs_zero_allocations() {
        let exe = std::env::current_exe().expect("child probe exe path");
        let out = std::process::Command::new(exe)
            .args([
                "--exact",
                "alloc_probe::child_probe_asserts_zero_alloc",
                "--nocapture",
            ])
            .env("WDR_RT_ZERO_ALLOC_PROBE", "1")
            .output()
            .expect("spawn child probe");
        assert!(
            out.status.success(),
            "RT surface allocated inside the probe (stderr: {})",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    /// The child half of `rt_surface_performs_zero_allocations`. No-op in the
    /// normal harness; performs the RT-surface calls in isolation and fails the
    /// process if any of them allocates.
    #[test]
    fn child_probe_asserts_zero_alloc() {
        if std::env::var("WDR_RT_ZERO_ALLOC_PROBE").is_err() {
            return; // parent-invoked normal run: nothing to do
        }
        let ring = crate::SpscRing::with_capacity(128);
        // Reset the counter after with_capacity (its one allocation is off the
        // RT path by contract) and count only the RT-facing calls below.
        ALLOCS.store(0, Ordering::SeqCst);

        let _ = ring.free_space();
        let _ = ring.available_len();
        assert!(ring.try_push(&[1u8; 64]));
        assert!(ring.try_push(&[2u8; 64]));
        let mut out = [0u8; 128];
        assert!(ring.try_pop_exact(&mut out));

        // NOTE: on success `assert_eq!` does not format its message (no
        // allocation); a failure allocates only inside an already-failing child.
        assert_eq!(
            ALLOCS.load(Ordering::SeqCst),
            0,
            "RT-surface calls (try_push / try_pop_exact / free_space / \
             available_len) must never allocate"
        );
    }
}
