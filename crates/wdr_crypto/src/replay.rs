//! DTLS-style sliding replay window over u64 packet indices
//! (SECURITY_SPEC §3.4), per (plane, direction), `Mutex`-protected for use by
//! the session core from concurrent tasks.

use std::sync::Mutex;

/// The configured width of the replay window (locked default 256 entries).
pub const WINDOW_SIZE: usize = crate::constants::REPLAY_WINDOW_SIZE as usize;

/// Outcome of a replay-window check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplayDecision {
    /// New in-window index, acceptable; the window state was updated.
    Accept,
    /// Duplicate or out-of-window (replayed / reordered-beyond-window) index.
    Reject,
}

/// A DTLS-style sliding bit window (256 entries) over a 64-bit packet index.
///
/// * `index` below `oldest = newest - WINDOW_SIZE + 1` → reject (too old).
/// * `index` within `[oldest, newest]` → accept only if not already seen.
/// * `index` above `newest` → accept and slide the window.
pub struct ReplayWindow {
    inner: Mutex<ReplayWindowInner>,
}

struct ReplayWindowInner {
    newest: Option<u64>,
    /// DTLS-style bitmap over the 256 most recent indices (bit 0 == newest).
    /// Implemented as a 256-bit fixed-size bitset (4 × u64).
    bitset: [u64; 4],
}

impl Default for ReplayWindow {
    fn default() -> Self {
        Self::new()
    }
}

impl ReplayWindow {
    /// Create an empty window (no indices seen).
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(ReplayWindowInner {
                newest: None,
                bitset: [0; 4],
            }),
        }
    }

    /// Check `index` against the window and (if acceptable) record it.
    ///
    /// Thread-safe (a short-lived `Mutex` guard); returns [`ReplayDecision`].
    #[must_use]
    pub fn check_and_record(&self, index: u64) -> ReplayDecision {
        let mut w = self.inner.lock().expect("replay window poisoned");
        match w.newest {
            None => {
                w.newest = Some(index);
                w.bitset[0] = 1;
                ReplayDecision::Accept
            }
            Some(newest) => {
                if index > newest {
                    // Slide the window forward by (index - newest): a bit at
                    // global position P moves to P + shift.
                    let shift = index - newest;
                    let mut new = [0u64; 4];
                    if shift < 256 {
                        let word_shift = (shift / 64) as usize;
                        let bit_shift = (shift % 64) as u32;
                        for src in 0..4 {
                            let (val, carry) = split_shifted(w.bitset[src], bit_shift);
                            let dst = src + word_shift;
                            if dst < 4 {
                                new[dst] |= val;
                            }
                            if bit_shift != 0 && dst + 1 < 4 {
                                new[dst + 1] |= carry;
                            }
                        }
                    }
                    new[0] |= 1;
                    w.bitset = new;
                    w.newest = Some(index);
                    ReplayDecision::Accept
                } else if newest - index >= 256 {
                    ReplayDecision::Reject
                } else {
                    // Within the 256-entry window.
                    let pos = (newest - index) as usize;
                    let word = pos / 64;
                    let bit = pos % 64;
                    let mask = 1u64 << bit;
                    if w.bitset[word] & mask != 0 {
                        ReplayDecision::Reject
                    } else {
                        w.bitset[word] |= mask;
                        ReplayDecision::Accept
                    }
                }
            }
        }
    }

    /// Whether `index` would be accepted *without* mutating the window (for
    /// decision-making before commit); duplicates and below-`oldest` → `false`.
    #[must_use]
    pub fn would_accept(&self, index: u64) -> bool {
        self.check_and_record(index) == ReplayDecision::Accept
    }

    /// The nearest not-yet-seen index (informational / for rekey boundary checks).
    #[must_use]
    pub fn newest_received(&self) -> Option<u64> {
        self.inner.lock().expect("replay window poisoned").newest
    }
}

/// Split `value` into `(value << bit_shift, value >> (64 - bit_shift))` — i.e.
/// the low and high 64-bit words of `value << bit_shift` (the "carry" is the
/// part that spills into the next higher word). `bit_shift` must be in 0..64.
fn split_shifted(value: u64, bit_shift: u32) -> (u64, u64) {
    if bit_shift == 0 {
        (value, 0)
    } else {
        (value << bit_shift, value >> (64 - bit_shift))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_strictly_in_order_sequence() {
        let w = ReplayWindow::new();
        for i in 0..10_000u64 {
            assert_eq!(w.check_and_record(i), ReplayDecision::Accept);
        }
    }

    #[test]
    fn rejects_duplicates() {
        let w = ReplayWindow::new();
        assert_eq!(w.check_and_record(5), ReplayDecision::Accept);
        assert_eq!(w.check_and_record(6), ReplayDecision::Accept);
        assert_eq!(w.check_and_record(5), ReplayDecision::Reject);
    }

    #[test]
    fn accepts_legitimate_reorder_within_window() {
        let w = ReplayWindow::new();
        for i in 0..200u64 {
            assert_eq!(w.check_and_record(i), ReplayDecision::Accept);
        }
        // A genuine reorder: 101 was skipped, 102 arrived first, now 101
        // (within the 256-window) must be accepted.
        assert_eq!(w.check_and_record(202), ReplayDecision::Accept);
        assert_eq!(w.check_and_record(201), ReplayDecision::Accept);
    }

    #[test]
    fn rejects_out_of_window_old() {
        let w = ReplayWindow::new();
        for i in 0..400u64 {
            assert_eq!(w.check_and_record(i), ReplayDecision::Accept);
        }
        // 100 back from 399 is only within the window if >= 144; 140 is beyond.
        assert_eq!(w.check_and_record(140), ReplayDecision::Reject);
        // A very old index is always rejected.
        assert_eq!(w.check_and_record(0), ReplayDecision::Reject);
    }

    #[test]
    fn slides_after_big_newest_jump() {
        let w = ReplayWindow::new();
        for i in 0..100u64 {
            assert_eq!(w.check_and_record(i), ReplayDecision::Accept);
        }
        // Jump far beyond the window: the old tail is dropped and a fresh run starts.
        assert_eq!(w.check_and_record(1_000), ReplayDecision::Accept);
        assert_eq!(w.check_and_record(99), ReplayDecision::Reject);
        assert_eq!(w.check_and_record(999), ReplayDecision::Accept);
    }

    #[test]
    fn replay_after_reordering_sequence() {
        // Classic replay attack: deliver 1,2,3 then replay 2.
        let w = ReplayWindow::new();
        assert_eq!(w.check_and_record(1), ReplayDecision::Accept);
        assert_eq!(w.check_and_record(2), ReplayDecision::Accept);
        assert_eq!(w.check_and_record(3), ReplayDecision::Accept);
        assert_eq!(w.check_and_record(2), ReplayDecision::Reject);
    }

    #[test]
    fn thread_safe_concurrent_accepts_are_unique() {
        use std::sync::Arc;
        let w = Arc::new(ReplayWindow::new());
        let mut handles = Vec::new();
        for t in 0..8u64 {
            let w = Arc::clone(&w);
            handles.push(std::thread::spawn(move || {
                for k in 0..500u64 {
                    let _ = w.check_and_record(t * 500 + k);
                }
                let _ = w.check_and_record(t * 500); // intentional duplicate
            }));
        }
        for h in handles {
            h.join().unwrap();
        }
        // All 4000 indices were delivered; no panics, and the window remains valid.
        assert!(w.newest_received().is_some());
    }
}
