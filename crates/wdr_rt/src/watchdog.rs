//! RT-callback stall watchdog (`rt-guard` feature) — RT_CONTRACT §4 +
//! TEST_PLAN's "watchdog thread that aborts on callback stall".
//!
//! A platform RT callback that exceeds its budget (or deadlocks) is an audible
//! dropout — worse, a silent one. [`StallDetector`] makes it loud: the RT /
//! worker side calls [`StallDetector::kick`] after each successful unit of
//! progress, and a low-frequency watchdog thread calls
//! [`StallDetector::check_stall_and_abort`]; if the wall clock ever advances
//! more than `max_stall_ms` past the last kick, the process aborts — the same
//! deliberate, never-allocating, never-I/O abort discipline as
//! [`guard::RtGuard`](crate::guard::RtGuard).
//!
//! The clock is injectable ([`WatchClock::Injectable`]) so host tests are fully
//! deterministic (no sleeping); the production watchdog drives
//! [`WatchClock::System`].
//!
//! ```text
//! use wdr_rt::watchdog::{StallDetector, WatchClock};
//!
//! // RT/consumer side (ownership can be cloned: it is cheap + Send+Sync).
//! let det = StallDetector::new(5_000, WatchClock::System);
//! // ...after each good frame:
//! det.kick();
//!
//! // watchdog thread:
//! det.check_stall_and_abort();
//! ```

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

/// The wall-clock source for a [`StallDetector`]. Injectable for deterministic
/// host tests; `System` for the production watchdog.
#[derive(Debug, Clone)]
pub enum WatchClock {
    /// Millisecond Unix time via `std::time::SystemTime`.
    System,
    /// A shared millisecond counter the test drives (the `ClockHandle` pattern).
    Injectable(Arc<AtomicU64>),
}

impl WatchClock {
    /// An injectable clock initialized to `now_ms`.
    pub fn injectable(now_ms: u64) -> Self {
        WatchClock::Injectable(Arc::new(AtomicU64::new(now_ms)))
    }

    /// Current wall-clock value in milliseconds.
    pub fn now_ms(&self) -> u64 {
        match self {
            WatchClock::System => SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
                .unwrap_or(0),
            WatchClock::Injectable(a) => a.load(Ordering::Acquire),
        }
    }

    /// Advance an [`WatchClock::Injectable`] clock by `ms` (no-op for `System`).
    pub fn advance_ms(&self, ms: u64) {
        if let WatchClock::Injectable(a) = self {
            a.fetch_add(ms, Ordering::AcqRel);
        }
    }
}

/// Detects an RT-callback stall and aborts the process when one exceeds the
/// budget. `kick` is called from the RT/consumer path (atomic store, no alloc,
/// no locks); the watchdog thread polls [`stalled`](Self::stalled) /
/// [`check_stall_and_abort`](Self::check_stall_and_abort).
#[derive(Debug, Clone)]
pub struct StallDetector {
    /// Wall-clock ms of the last successful progress tick.
    last_progress: Arc<AtomicU64>,
    clock: WatchClock,
    /// Maximum allowed gap between the wall clock and the last kick (ms).
    max_stall_ms: u64,
}

impl StallDetector {
    /// A detector seeded with "progress happened at the current clock" (so an
    /// immediately-polling watchdog is not a false positive).
    pub fn new(max_stall_ms: u64, clock: WatchClock) -> Self {
        let now = clock.now_ms();
        Self {
            last_progress: Arc::new(AtomicU64::new(now)),
            clock,
            max_stall_ms,
        }
    }

    /// Called after each successful unit of RT progress (a pushed or popped
    /// frame). Cheap: one atomic store.
    pub fn kick(&self) {
        self.last_progress
            .store(self.clock.now_ms(), Ordering::Release);
    }

    /// The wall-clock ms since the last kick.
    pub fn stall_ms(&self) -> u64 {
        self.clock
            .now_ms()
            .saturating_sub(self.last_progress.load(Ordering::Acquire))
    }

    /// Whether the callback is currently stalled past the budget.
    pub fn stalled(&self) -> bool {
        self.stall_ms() > self.max_stall_ms
    }

    /// Abort the process if the callback has stalled past the budget.
    /// `#[cold]` + never inline so a crash backtrace points at the caller;
    /// deliberately no message (a watchdog is not the place for I/O). The
    /// abort is the signal, matching `guard.rs`'s `panic = "abort"` world.
    #[cold]
    #[inline(never)]
    pub fn check_stall_and_abort(&self) {
        if self.stalled() {
            std::process::abort();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    // no_std crate: bring the std prelude (Vec/Box/String/…) into the tests.
    use std::prelude::v1::*;

    #[test]
    fn detects_stall_exactly_at_the_budget_and_no_false_positive() {
        let clock = WatchClock::injectable(1_000);
        let det = StallDetector::new(5_000, clock.clone());
        assert!(!det.stalled(), "seeded at now → not stalled");

        // Progress keeps the watchdog quiet.
        clock.advance_ms(4_000);
        det.kick();
        assert!(!det.stalled(), "kicked within budget");
        assert_eq!(det.stall_ms(), 0);

        // Just under the budget → not stalled.
        clock.advance_ms(5_000);
        assert_eq!(det.stall_ms(), 5_000);
        assert!(!det.stalled(), "exactly at the budget is not yet a stall");

        // Any past the budget → stalled.
        clock.advance_ms(1);
        assert!(det.stalled());

        // Progress resumes → quiet again.
        det.kick();
        assert!(!det.stalled());
    }

    /// Child-process probe: a processor that stops kicking past the budget must
    /// abort via `check_stall_and_abort` (mirrors guard.rs's abort probe).
    #[test]
    fn stall_aborts_child_process() {
        let exe = std::env::current_exe().expect("child probe exe path");
        let out = std::process::Command::new(exe)
            .args([
                "--exact",
                "watchdog::tests::child_probe_aborts_on_stall",
                "--nocapture",
            ])
            .env("WDR_RT_STALL_PROBE", "1")
            .output()
            .expect("spawn child probe");
        assert!(
            !out.status.success(),
            "child that stalls past the budget MUST NOT exit 0; stderr: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    /// The child half of `stall_aborts_child_process`. No-op when run by the
    /// normal harness; aborts when invoked with `WDR_RT_STALL_PROBE=1`.
    #[test]
    fn child_probe_aborts_on_stall() {
        if std::env::var("WDR_RT_STALL_PROBE").is_err() {
            return; // parent-invoked normal run: nothing to do
        }
        let clock = WatchClock::injectable(0);
        let det = StallDetector::new(10, clock.clone());
        det.kick(); // last progress at 0
        clock.advance_ms(1_000); // now 1s > 10ms budget
        det.check_stall_and_abort(); // MUST abort
        unreachable!("stalled watchdog did not abort");
    }
}
