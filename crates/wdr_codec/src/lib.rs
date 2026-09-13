//! `wdr_codec` — codec adapters behind the `CodecAdapter` trait (task t-B0-codec).
//!
//! Audio codec back-ends for the Wavelink, implementing the
//! `CodecAdapter` seam so that platform/codec choice is testable and
//! swappable (ADR-001 / ARCHITECTURE).
//!
//! # Adapters
//!
//! * [`OpusAdapter`] — lossy, wraps libopus (vendored); 44.1/48 kHz, mono/stereo,
//!   20 ms and 10 ms frame profiles, VBR (ADR-004).
//! * [`FlacAdapter`] — lossless, encodes via libFLAC (vendored) and decodes via
//!   claxon; small fixed blocks ~240 samples @48k; per-frame CRC-32 integrity on
//!   the raw frame bytes (ADR-005 / PROTOCOL_SPEC §Codec profiles).
//! * [`PcmAdapter`] — raw interleaved little-endian i16 PCM passthrough
//!   (the lossless baseline); per-frame CRC-32 on frame bytes.
//!
//! # Frame-size calculator (ADR-005 duty)
//!
//! [`max_frame_samples`] computes the maximum number of frames (samples per
//! channel) that fit a frame-payload budget (default `MAX_FRAME_PAYLOAD`), and
//! is the subject of the ADR-005 matrix spot-check.
//!
//! # Dither (ADR-004 / PROTOCOL_SPEC)
//!
//! [`tpdf_dither_24_to_16`] performs deterministic TPDF dither when a 24-bit
//! source is down-converted to 16-bit for the lossy path.
//!
//! # Format coverage
//!
//! [`score_test`](`sample_repr` coverage): `I16` (16-bit) and `I24Packed`
//! (24-bit) are implemented — 16-bit over the [`CodecAdapter`] i16 surface and
//! 24-bit over the parallel [`CodecAdapter24`] i32 surface (canonical
//! right-aligned i32 / low-3-bytes LE, ADR-005 16/24-bit). `F32`/`I32` remain
//! future stubs returning [`CodecError::Unsupported`].
//!
//! [`max_frame_samples`]: crate::size::max_frame_samples

pub mod adapters;
pub mod dither;
pub mod error;
pub mod resample;
pub mod size;

pub use adapters::{
    frame_crc32, CodecAdapter, CodecAdapter24, CodecKind, FlacAdapter, FrameProfile, OpusAdapter,
    PcmAdapter, SampleRepr,
};
pub use dither::{tpdf_dither_24_to_16, DitherRng, TwentyFourBitSamples};
pub use error::CodecError;
pub use resample::ResamplerI16;
pub use size::{
    max_frame_samples, noise_fixture, MatrixRow, DEFAULT_BUDGET, MAX_FRAME_PAYLOAD,
    MAX_FRAME_SAMPLES_FALLBACK,
};
