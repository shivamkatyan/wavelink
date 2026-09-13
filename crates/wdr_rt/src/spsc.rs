//! Bounded, lock-free, single-producer / single-consumer byte ring — the only
//! data structure a platform RT audio callback may touch (RT_CONTRACT §3).
//!
//! The buffer is allocated exactly once ([`SpscRing::with_capacity`]) off the
//! RT path; [`SpscRing::try_push`] and [`SpscRing::try_pop_exact`] perform only
//! atomic loads/stores and byte copies — no allocation, no locks, no syscalls,
//! no blocking. `#![no_std]`: the single allocation (`with_capacity`) is the
//! only `alloc` use in this module (`alloc::boxed::Box` / `alloc::vec::Vec`).

use alloc::{boxed::Box, vec::Vec};
use core::sync::atomic::{AtomicU8, AtomicUsize, Ordering};

/// Bounded, lock-free, single-producer / single-consumer byte ring.
///
/// Capacity must be a non-zero power of two (asserted at construction) so the
/// cursor can wrap with a cheap mask instead of a modulo. Whole-frame push/pop
/// guarantees the RT callback never observes a torn frame.
pub struct SpscRing {
    buf: Box<[AtomicU8]>,
    cap: usize,
    mask: usize,
    /// Producer publishes finished frames by `Release`-storing the write cursor.
    write_pos: AtomicUsize,
    /// Consumer publishes consumed bytes by `Release`-storing the read cursor.
    read_pos: AtomicUsize,
}

impl SpscRing {
    /// Allocate a ring of `cap` bytes (non-zero power of two).
    ///
    /// This is the *only* allocation the ring ever performs; it must happen in
    /// worker/startup context, never inside an RT callback.
    pub fn with_capacity(cap: usize) -> SpscRing {
        assert!(
            cap.is_power_of_two() && cap > 0,
            "SpscRing capacity must be a non-zero power of two, got {cap}"
        );
        let buf = (0..cap)
            .map(|_| AtomicU8::new(0))
            .collect::<Vec<_>>()
            .into_boxed_slice();
        SpscRing {
            buf,
            cap,
            mask: cap - 1,
            write_pos: AtomicUsize::new(0),
            read_pos: AtomicUsize::new(0),
        }
    }

    /// Ring capacity in bytes (the `cap` passed to [`with_capacity`](Self::with_capacity)).
    #[inline]
    pub const fn capacity(&self) -> usize {
        self.cap
    }

    /// Producer-side view: bytes of free space currently available to push into.
    ///
    /// Safe to call only from the producer thread.
    #[inline]
    pub fn free_space(&self) -> usize {
        let w = self.write_pos.load(Ordering::Relaxed);
        let r = self.read_pos.load(Ordering::Acquire);
        self.cap - w.wrapping_sub(r)
    }

    /// Consumer-side view: bytes currently available to pop out of.
    ///
    /// Safe to call only from the consumer thread.
    #[inline]
    pub fn available_len(&self) -> usize {
        let r = self.read_pos.load(Ordering::Relaxed);
        let w = self.write_pos.load(Ordering::Acquire);
        w.wrapping_sub(r)
    }

    /// Push one whole frame into the ring, or return `false` if it does not fit.
    ///
    /// If `true`, `data` is copied in and later read back by the consumer as one
    /// whole, untorn frame. Never blocks and never allocates.
    ///
    /// Safe to call only from the producer thread.
    pub fn try_push(&self, data: &[u8]) -> bool {
        if data.len() > self.cap {
            return false;
        }
        let w = self.write_pos.load(Ordering::Relaxed);
        let r = self.read_pos.load(Ordering::Acquire);
        if data.len() > self.cap - w.wrapping_sub(r) {
            return false; // full
        }
        let start = w & self.mask;
        let head = data.len().min(self.cap - start);
        self.copy_in(start, &data[..head]);
        if head < data.len() {
            self.copy_in(0, &data[head..]);
        }
        // Publish the whole frame before the consumer may acquire it.
        self.write_pos
            .store(w.wrapping_add(data.len()), Ordering::Release);
        true
    }

    /// Pop one whole frame into `out`, or return `false` if not that many bytes
    /// are available yet. When `true`, `out` is fully filled.
    ///
    /// Never blocks and never allocates.
    ///
    /// Safe to call only from the consumer thread.
    pub fn try_pop_exact(&self, out: &mut [u8]) -> bool {
        let r = self.read_pos.load(Ordering::Relaxed);
        let w = self.write_pos.load(Ordering::Acquire);
        if out.len() > w.wrapping_sub(r) {
            return false; // empty (or partial frame available)
        }
        let start = r & self.mask;
        let head = out.len().min(self.cap - start);
        self.copy_out(start, &mut out[..head]);
        if head < out.len() {
            self.copy_out(0, &mut out[head..]);
        }
        // Publish consumption before the producer may reuse that space.
        self.read_pos
            .store(r.wrapping_add(out.len()), Ordering::Release);
        true
    }

    fn copy_in(&self, start: usize, src: &[u8]) {
        for (i, &b) in src.iter().enumerate() {
            self.buf[start + i].store(b, Ordering::Relaxed);
        }
    }

    fn copy_out(&self, start: usize, dst: &mut [u8]) {
        for (i, slot) in self.buf[start..start + dst.len()].iter().enumerate() {
            dst[i] = slot.load(Ordering::Relaxed);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push_reports_full_on_overflow_and_pop_recovers() {
        let ring = SpscRing::with_capacity(16);
        assert_eq!(ring.free_space(), 16);

        assert!(
            ring.try_push(&[0u8; 16]),
            "empty ring accepts exactly its capacity"
        );
        assert_eq!(ring.free_space(), 0, "ring is now full");
        assert!(
            !ring.try_push(&[0u8; 1]),
            "full ring must reject the producer, not block or grow"
        );

        let mut out = [0u8; 16];
        assert!(
            ring.try_pop_exact(&mut out),
            "popping exactly what was pushed"
        );
        assert_eq!(out, [0u8; 16]);
        assert_eq!(ring.available_len(), 0, "consumer drained the ring");
        assert_eq!(ring.free_space(), 16, "producer can reuse the space");
    }

    #[test]
    fn whole_frame_is_never_torn() {
        let ring = SpscRing::with_capacity(4);
        let mut out = [0u8; 4];

        // Fill the ring exactly, then drain it so the write cursor wraps.
        assert!(ring.try_push(&[1, 2, 3, 4]));
        assert!(ring.try_pop_exact(&mut out));
        assert_eq!(out, [1, 2, 3, 4]);

        assert!(ring.try_push(&[5, 6, 7]));
        assert_eq!(ring.free_space(), 1);
        // A partial frame that does not fit yet must be rejected atomically.
        assert!(!ring.try_pop_exact(&mut [0u8; 4]), "only 3 bytes available");

        assert!(ring.try_push(&[8]), "finishes the frame across the wrap");
        assert!(ring.try_pop_exact(&mut out));
        assert_eq!(out, [5, 6, 7, 8], "wrap-around preserves frame order");
    }

    #[test]
    fn two_thread_stress_no_race_no_loss() {
        const CAP: usize = 64;
        const FRAME: usize = 8;
        const MSGS: u32 = 200_000;

        let ring = SpscRing::with_capacity(CAP);

        std::thread::scope(|s| {
            // Producer: push monotonic frames, spinning (never locking) on full.
            s.spawn(|| {
                for i in 0..MSGS {
                    let id = i.to_le_bytes();
                    let tag = i.wrapping_mul(0x9E37_79B9).to_le_bytes();
                    let frame = [id[0], id[1], id[2], id[3], tag[0], tag[1], tag[2], tag[3]];
                    while !ring.try_push(&frame) {
                        std::hint::spin_loop();
                    }
                }
            });

            // Consumer: pop and verify order, one frame each, exactly once.
            s.spawn(|| {
                let mut expected = 0u32;
                let mut frame = [0u8; FRAME];
                while expected < MSGS {
                    if ring.try_pop_exact(&mut frame) {
                        let id = u32::from_le_bytes([frame[0], frame[1], frame[2], frame[3]]);
                        assert_eq!(
                            id, expected,
                            "consumer must see every frame, in order, once"
                        );
                        expected += 1;
                    } else {
                        std::hint::spin_loop();
                    }
                }
            });
        });
    }
}
