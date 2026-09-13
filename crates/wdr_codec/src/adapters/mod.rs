//! The `CodecAdapter` trait and its three implementations: Opus (lossy), FLAC
//! (lossless), PCM (raw lossless baseline). See the module docs and ADR-004 /
//! ADR-005 / PROTOCOL_SPEC §Codec profiles.
//!
//! # Design notes
//!
//! * All **i16** adapters take interleaved `i16` PCM (len multiple of
//!   channels) and return encoded frame bytes; decode returns interleaved
//!   `i16`. 24-bit lossless rides the parallel [`CodecAdapter24`] trait over
//!   right-aligned `i32` (canonical `wdr_fakes` 24-bit form).
//! * **Integrity** — the lossless adapters (FLAC/PCM) expose the raw frame
//!   bytes to a CRC-32 ([`crate::frame_crc32`]); the lossy path (Opus) relies
//!   on the frame container's AEAD tag instead. The `CodecAdapter::encode`
//!   payload for the lossless adapters is the exact byte buffer over which the
//!   protocol's per-frame CRC is computed (PROTOCOL_SPEC §Codec profiles).
//! * **>48 kHz through lossy** — Opus re-validates on every call and returns a
//!   typed [`CodecError::RateUnsupported`]; never silently re-routed (FR-014).
//! * **44.1 kHz Opus** — libopus (RFC 6716) accepts only 8000/12000/16000/
//!   24000/48000 Hz. To honour ADR-004's "44.1/48 kHz" lossy profile the Opus
//!   adapter transparently resamples 44.1k → 48k on encode and 48k → 44.1k on
//!   decode (rubato synchronous FFT resampler; bounded + deterministic). This
//!   is a documented, unavoidable deviation (44.1k is not a native Opus rate).

mod flac;
mod opus;
mod pcm;

pub use flac::FlacAdapter;
pub use opus::{FrameProfile, OpusAdapter};
pub use pcm::PcmAdapter;

use crate::error::CodecError;

/// Codec identifiers for the adapter layer (matches PROTOCOL_SPEC §Codec
/// profiles and `wdr_proto::Codec`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CodecKind {
    /// Opus lossy (44.1/48 kHz).
    Opus,
    /// FLAC lossless, small fixed blocks.
    Flac,
    /// Raw PCM lossless baseline.
    Pcm,
}

impl CodecKind {
    /// Protocol-facing name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            CodecKind::Opus => "Opus",
            CodecKind::Flac => "Flac",
            CodecKind::Pcm => "Pcm",
        }
    }
}

impl core::fmt::Display for CodecKind {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Sample representation for the codec path (mirrors PROTOCOL_SPEC/`
/// `wdr_proto::SampleRepr`, kept local so `wdr_codec` stays decoupled at B0).
/// `I16` and `I24Packed` are implemented (ADR-005 16/24-bit); `F32`/`I32`
/// remain future stubs that return [`CodecError::Unsupported`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SampleRepr {
    /// 16-bit signed little-endian interleaved samples (implemented).
    I16,
    /// 32-bit float — *future stub* (returns `Unsupported`).
    F32,
    /// 24-bit packed (3 bytes/sample) — implemented via [`CodecAdapter24`]
    /// (the canonical `wdr_fakes` i32-by-value / low-3-bytes form).
    I24Packed,
    /// 32-bit signed integer — *future stub* (returns `Unsupported`).
    I32,
}

impl SampleRepr {
    /// Packed byte width of a sample in this representation.
    #[must_use]
    pub const fn bytes_per_sample(self) -> Option<u16> {
        match self {
            SampleRepr::I16 => Some(2),
            SampleRepr::F32 => Some(4),
            SampleRepr::I24Packed => Some(3),
            SampleRepr::I32 => Some(4),
        }
    }

    /// Map the protocol-facing representation to a supported adapter
    /// representation; the remaining stubs return `Unsupported`.
    pub fn validate(self) -> Result<(), CodecError> {
        match self {
            SampleRepr::I16 => Ok(()),
            SampleRepr::F32 => Err(CodecError::Unsupported(
                "F32 sample representation is a future stub (ADR-005: 16/24-bit first)".into(),
            )),
            SampleRepr::I24Packed => Ok(()),
            SampleRepr::I32 => Err(CodecError::Unsupported(
                "I32 sample representation is a future stub (ADR-005)".into(),
            )),
        }
    }
}

/// The codec-adaptation interface (ADR-001 seam for platform/codec choice).
///
/// A single adapter instance is stateful and **must only be used from one
/// thread at a time** (libopus/libFLAC codec state is single-threaded);
/// separate streams use separate adapter instances.
pub trait CodecAdapter {
    /// Which codec this adapter wraps.
    fn kind(&self) -> CodecKind;

    /// Encode a frame of interleaved `i16` PCM (length multiple of the
    /// adapter's channel count) into codec payload bytes.
    fn encode(&mut self, pcm: &[i16]) -> Result<Box<[u8]>, CodecError>;

    /// Decode a codec payload back into interleaved `i16` PCM.
    fn decode(&mut self, bytes: &[u8]) -> Result<Box<[i16]>, CodecError>;

    /// Human-readable codec name (logs / capability negotiation).
    fn name(&self) -> &'static str;
}

/// The 24-bit codec-adaptation interface (ADR-005 follow-up).
///
/// Same contract as [`CodecAdapter`] but over interleaved **24-bit** `i32`
/// PCM in the canonical `wdr_fakes` form: right-aligned values in
/// `[-(2^23)+1, 2^23-1)`, packed 3 bytes/sample little-endian on the wire
/// (low 3 bytes, top byte stripped). Lossless means sample-identical —
/// `hash(decoded) == hash(source)` with no tolerance.
///
/// Kept as a **separate trait** so the i16 adapters and their tests stay
/// untouched: opting in to 24-bit is explicit, and Opus stays i16-only.
/// The FLAC/PCM adapters expose both sides — `PcmAdapter::new24` /
/// `FlacAdapter::new(.., 24)` construct a 24-bit-capable adapter; calling
/// the wrong-depth methods on it returns a typed [`CodecError::Unsupported`]
/// rather than silently down-converting.
pub trait CodecAdapter24 {
    /// Which codec this adapter wraps.
    fn kind(&self) -> CodecKind;

    /// Encode a frame of interleaved 24-bit `i32` PCM (length multiple of the
    /// adapter's channel count) into codec payload bytes.
    fn encode_24(&mut self, pcm: &[i32]) -> Result<Box<[u8]>, CodecError>;

    /// Decode a codec payload back into interleaved 24-bit `i32` PCM.
    fn decode_24(&mut self, bytes: &[u8]) -> Result<Box<[i32]>, CodecError>;

    /// Human-readable codec name (logs / capability negotiation).
    fn name(&self) -> &'static str;
}

/// CRC-32 over the frame payload bytes for the lossless path (per-frame
/// integrity, PROTOCOL_SPEC §Codec profiles). Deterministic IEEE 802.3.
#[must_use]
pub fn frame_crc32(bytes: &[u8]) -> u32 {
    let mut crc: u32 = 0xFFFF_FFFF;
    for &b in bytes {
        crc ^= u32::from(b);
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sample_repr_bytes_per_sample() {
        assert_eq!(SampleRepr::I16.bytes_per_sample(), Some(2));
        assert_eq!(SampleRepr::I24Packed.bytes_per_sample(), Some(3));
        assert_eq!(SampleRepr::F32.bytes_per_sample(), Some(4));
        assert_eq!(SampleRepr::I32.bytes_per_sample(), Some(4));
    }

    #[test]
    fn sample_repr_implemented_vs_stubs() {
        assert!(matches!(SampleRepr::I16.validate(), Ok(())));
        // 24-bit is implemented via `CodecAdapter24` (ADR-005 follow-up).
        assert!(matches!(SampleRepr::I24Packed.validate(), Ok(())));
        assert!(matches!(
            SampleRepr::F32.validate(),
            Err(CodecError::Unsupported(_))
        ));
        assert!(matches!(
            SampleRepr::I32.validate(),
            Err(CodecError::Unsupported(_))
        ));
    }

    #[test]
    fn frame_crc32_known_vectors() {
        assert_eq!(frame_crc32(b""), 0x0000_0000);
        assert_eq!(frame_crc32(b"123456789"), 0xCBF4_3926);
        assert_eq!(frame_crc32(&[0u8; 32]), 0x190A_55AD);
    }
}
