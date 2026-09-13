//! Golden-vector generation: deterministic, reproducible blobs.
//!
//! `gen_golden(name, value)` produces the deterministic, canonical postcard
//! encoding of `value` and maps it to a stable, versioned file name
//! (`v1-<name>-<fnv1a64>.bin`). `examples/gen_golden.rs` materialises the
//! blobs plus a manifest under `tests/golden/`; `tests/golden.rs` verifies
//! that the stored blobs reproduce exactly.

use serde::Serialize;

/// Golden vector version prefix; bump to invalidate stale vectors.
pub const GOLDEN_VERSION: &str = "v1";

/// Deterministic FNV-1a 64-bit hash (file-name suffix only).
#[must_use]
pub fn fnv1a64(data: &[u8]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for &b in data {
        hash ^= u64::from(b);
        hash = hash.wrapping_mul(0x1000_0000_01b3);
    }
    hash
}

/// Stable, versioned file name for a golden vector.
#[must_use]
pub fn golden_file_name(name: &str, data: &[u8]) -> String {
    format!("{GOLDEN_VERSION}-{name}-{:016x}.bin", fnv1a64(data))
}

/// Deterministically produce the golden bytes for a value (no RNG).
///
/// The value is serialized with the *canonical* postcard path; capacity of any
/// `Vec` field never leaks into the encoded bytes, so the blob is the stable
/// wire image.
pub fn gen_golden<M: Serialize>(_name: &str, message: &M) -> Vec<u8> {
    postcard::to_allocvec(message).expect("canonical golden value must serialize")
}

/// Directory (relative to the crate root) holding stored golden blobs.
pub const GOLDEN_DIR: &str = "tests/golden";

/// Manifest file name within `GOLDEN_DIR`.
pub const GOLDEN_MANIFEST: &str = "MANIFEST.md";
