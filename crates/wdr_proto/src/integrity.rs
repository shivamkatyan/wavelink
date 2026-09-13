//! Frame integrity policy (lossless per-frame CRC32; general none/AEAD).

use serde::{Deserialize, Serialize};

/// Integrity mode carried by an audio frame.
///
/// Lossless frames (raw PCM / FLAC) always use a per-frame CRC32 of the
/// payload (PROTOCOL_SPEC §Audio frame header / §Codec profiles). Lossy
/// datagrams rely on AEAD tag authenticity at the transport layer and carry
/// `None` here; extended integrity with an explicit AEAD policy is reserved
/// under the `Aead` variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Integrity {
    /// No frame-local integrity field (lossy AEAD-authenticated path).
    None,
    /// Per-frame CRC32 over the payload (lossless raw-PCM/FLAC path).
    Crc32,
    /// Explicit AEAD policy marker (transport-managed tag; no frame-local bytes).
    Aead,
}
