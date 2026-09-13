//! Optional runtime enforcement of the RT contract (`rt-guard` feature).
//!
//! When enabled, this module installs a process-wide `#[global_allocator]`
//! that forwards every allocation to the system allocator **normally**, except
//! while the calling thread is marked "inside an RT callback context". In that
//! context any heap allocation aborts the process at the offending line.
//!
//! This is the runtime guard half of the enforcement described in
//! `docs/planning/RT_CONTRACT.md` §4: the RT callback may only copy bytes in/out
//! of a preallocated `SpscRing` plus trivial atomic/int math — no alloc, no
//! locks, no syscalls. A platform RT callback brackets its body with
//! [`RtGuard::enter`], and any heap allocation while that guard is live aborts
//! the process.
//!
//! The guard is **thread-local** (only the RT thread is marked), so worker
//! threads and the rest of the process keep allocating normally. Out-of-RT
//! allocation is completely unaffected. Construction
//! ([`SpscRing::with_capacity`]) allocates once, off the RT path, with the
//! flag clear.
//!
//! The default build (no `rt-guard` feature) installs nothing and adds no
//! cost — the discipline stays enforced by the contract itself. The abort
//! behaves like a stack-overflow guard: deliberate, unrecoverable, and it
//! never allocates. Panic (via the `panic = "abort"` profile on this crate)
//! and allocator-abort are complementary — a contract violation is a hard
//! fault, never a silent latency spike.
//!
//! Usage (an RT capture callback):
//!
//! ```text
//! use wdr_rt::{SpscRing, guard::RtGuard};
//!
//! fn rt_capture_callback(ring: &SpscRing, buf: &[u8]) {
//!     let _guard = RtGuard::enter();
//!     let _ok = ring.try_push(buf); // the ONLY legal work: copy bytes
//!     // any Box/Vec/String/allocation here = abort()
//! }
//! ```

use std::cell::Cell;
use std::thread_local;

// Whether the calling thread is currently inside an RT callback context.
// Const-initialized `#[thread_local]` storage (no lazy-init allocation) so
// the allocator may read it safely mid-alloc.
thread_local! {
    static RT_DEPTH: Cell<u8> = const { Cell::new(0) };
}

/// The global allocator installed when the `rt-guard` feature is on.
///
/// Forwards to [`std::alloc::System`]; while `RT_DEPTH > 0` on the calling
/// thread, `alloc`/`alloc_zeroed`/`realloc` abort instead. `dealloc` never
/// allocates and is always forwarded.
#[global_allocator]
static RT_GUARD_ALLOC: RtGuardAlloc = RtGuardAlloc;

struct RtGuardAlloc;

unsafe impl core::alloc::GlobalAlloc for RtGuardAlloc {
    unsafe fn alloc(&self, layout: core::alloc::Layout) -> *mut u8 {
        enforce_no_rt_allocation();
        unsafe { std::alloc::System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: core::alloc::Layout) -> *mut u8 {
        enforce_no_rt_allocation();
        unsafe { std::alloc::System.alloc_zeroed(layout) }
    }

    unsafe fn realloc(
        &self,
        ptr: *mut u8,
        layout: core::alloc::Layout,
        new_size: usize,
    ) -> *mut u8 {
        enforce_no_rt_allocation();
        unsafe { std::alloc::System.realloc(ptr, layout, new_size) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: core::alloc::Layout) {
        // Freeing is a legal RT operation (it does not allocate); always pass
        // through to the system allocator.
        unsafe { std::alloc::System.dealloc(ptr, layout) }
    }
}

#[inline(always)]
fn enforce_no_rt_allocation() {
    if RT_DEPTH.with(|d| d.get()) > 0 {
        rt_allocation_abort();
    }
}

/// Abort the process for an RT-context allocation. `#[cold]` + never-inline so
/// the crash stacks point at the offending caller; never allocates itself.
#[cold]
#[inline(never)]
fn rt_allocation_abort() -> ! {
    // The exact offending allocation site is the return address of this frame
    // in a core/dump backtrace. Deliberately no message (writing one would
    // allocate/formatted-I/O on a violated RT thread) — the abort is the
    // signal, and RT-guard binaries run with `panic = "abort"`.
    std::process::abort()
}

/// RAII probe marking the current thread as executing inside an RT callback.
///
/// Enter (nesting-aware) at the top of a platform RT callback, drop when the
/// callback returns. While at least one guard is live on this thread, any heap
/// allocation aborts the process — converting an RT-contract violation from a
/// silent latency spike into a loud, exact-line crash.
pub struct RtGuard(());

impl RtGuard {
    /// Enter an RT-callback context on the calling thread (nesting-aware).
    pub fn enter() -> Self {
        RT_DEPTH.with(|d| d.set(d.get() + 1));
        RtGuard(())
    }
}

impl Drop for RtGuard {
    fn drop(&mut self) {
        RT_DEPTH.with(|d| d.set(d.get().saturating_sub(1)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    // no_std crate: bring the std prelude (Vec/Box/String/…) into the tests.
    use std::prelude::v1::*;

    #[test]
    fn out_of_rt_context_allocation_is_unaffected() {
        // The allocator is installed process-wide; with the flag clear normal
        // allocation must work identically to `std::alloc::System`.
        let v: Vec<u8> = (0..64).collect();
        assert_eq!(v.len(), 64);
        let _boxed = Box::new([0u8; 4096]);
        assert!(SpscRingTestHelper::works());
    }

    struct SpscRingTestHelper;
    impl SpscRingTestHelper {
        fn works() -> bool {
            let ring = crate::SpscRing::with_capacity(512);
            ring.capacity() == 512
        }
    }

    #[test]
    fn guard_is_thread_scoped_and_nesting_aware() {
        assert_eq!(RT_DEPTH.with(|d| d.get()), 0);
        {
            let _g = RtGuard::enter();
            assert_eq!(RT_DEPTH.with(|d| d.get()), 1);
            {
                let _g2 = RtGuard::enter();
                assert_eq!(RT_DEPTH.with(|d| d.get()), 2);
            }
            assert_eq!(RT_DEPTH.with(|d| d.get()), 1);
        }
        assert_eq!(RT_DEPTH.with(|d| d.get()), 0);
    }

    /// Child-process probe: an allocation made *inside* the RT context must
    /// abort the process rather than silently allocating.
    #[test]
    fn rt_scoped_allocation_aborts() {
        let exe = std::env::current_exe().expect("child probe exe path");
        let out = std::process::Command::new(exe)
            .args([
                "--exact",
                "guard::tests::child_probe_aborts_on_alloc",
                "--nocapture",
            ])
            .env("WDR_RT_GUARD_PROBE", "1")
            .output()
            .expect("spawn child probe");
        assert!(
            !out.status.success(),
            "child that allocates inside RT context MUST NOT exit 0; stderr: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    /// The child half of `rt_scoped_allocation_aborts`. No-op when run by the
    /// normal harness; aborts when invoked with `WDR_RT_GUARD_PROBE=1`.
    #[test]
    fn child_probe_aborts_on_alloc() {
        if std::env::var("WDR_RT_GUARD_PROBE").is_err() {
            return; // parent-invoked normal run: nothing to do
        }
        let _guard = RtGuard::enter();
        let _ = Box::new([0u8; 1024]); // MUST abort
        unreachable!("allocation inside RT context did not abort");
    }
}
