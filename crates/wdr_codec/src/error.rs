//! Error type shared by the codec adapters, frame-size calculator and dither.

use std::fmt;

/// Error returned by the codec adapters and helpers.
///
/// All failures are typed (`Result<_, CodecError>`); the adapters never panic
/// on malformed/oversized input (PROTOCOL_SPEC §Error taxonomy, SEC-05).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CodecError {
    /// The requested format / sample representation is not implemented yet.
    ///
    /// Used by the `I24Packed` / `F32` / `I32` stubs (ADR-005 focuses on
    /// 16/24-bit first; `I16` is implemented, 24-bit plays through the dither
    /// path).
    Unsupported(String),
    /// A source sample rate above 48 kHz was routed to a lossy codec.
    ///
    /// Per ADR-004 / PROTOCOL_SPEC §Codec profiles, >48 kHz sources **never**
    /// route through lossy — callers must not silently re-route.
    RateUnsupported {
        /// The offending sample rate.
        sample_rate: u32,
        /// The codec ([`crate::CodecKind`]) that rejected it.
        codec: String,
    },
    /// The input PCM buffer length is not a multiple of the channel count.
    InvalidPcmLen {
        /// Buffer length in samples.
        len: usize,
        /// Channel count.
        channels: u16,
    },
    /// A frame of samples exceeds the configured profile/granularity.
    InvalidFrameSize {
        /// What was expected (e.g. "multiple of 480").
        expected: String,
        /// What was provided.
        got: usize,
    },
    /// The libopus/libFLAC back-end returned a negative status.
    Backend(String),
    /// The input byte buffer cannot be decoded as a well-formed frame for the
    /// codec in question (truncated, corrupt, oversized, mismatch).
    MalformedFrame(String),
    /// An encoded/payload buffer exceeds `MAX_FRAME_PAYLOAD` (checked before
    /// allocation).
    PayloadTooLarge {
        /// The offending size in bytes.
        size: usize,
        /// The cap that was exceeded.
        cap: usize,
    },
    /// The raw WAV/PCM input is inconsistent for this codec (bit depth,
    /// sample rate, channel layout).
    Format(String),
    /// Dithering a buffer whose byte length is not a whole number of samples.
    OddSampleBuffer {
        /// Byte length of the input.
        len: usize,
        /// Bytes per source sample (3 for the 24-bit packing).
        bytes_per_sample: usize,
    },
}

impl fmt::Display for CodecError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CodecError::Unsupported(what) => write!(f, "unsupported: {what}"),
            CodecError::RateUnsupported { sample_rate, codec } => write!(
                f,
                "{codec} does not accept {sample_rate} Hz (lossy codecs support \
                 ≤48 kHz only; do not route >48 kHz through lossy, ADR-004)"
            ),
            CodecError::InvalidPcmLen { len, channels } => write!(
                f,
                "PCM buffer length {len} is not a multiple of channel count {channels}"
            ),
            CodecError::InvalidFrameSize { expected, got } => {
                write!(f, "invalid frame size {got}: expected {expected}")
            }
            CodecError::Backend(what) => write!(f, "codec back-end error: {what}"),
            CodecError::MalformedFrame(what) => write!(f, "malformed frame: {what}"),
            CodecError::PayloadTooLarge { size, cap } => {
                write!(
                    f,
                    "payload of {size} bytes exceeds the {cap} byte frame cap"
                )
            }
            CodecError::Format(what) => write!(f, "format error: {what}"),
            CodecError::OddSampleBuffer {
                len,
                bytes_per_sample,
            } => write!(
                f,
                "dither input of {len} bytes is not a multiple of {bytes_per_sample} \
                 bytes per sample"
            ),
        }
    }
}

impl std::error::Error for CodecError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_messages_are_non_empty() {
        let cases = [
            CodecError::Unsupported("I24Packed".into()),
            CodecError::RateUnsupported {
                sample_rate: 96_000,
                codec: "Opus".into(),
            },
            CodecError::InvalidPcmLen {
                len: 3,
                channels: 2,
            },
            CodecError::InvalidFrameSize {
                expected: "480".into(),
                got: 7,
            },
            CodecError::Backend("opus_encode: BUFFER_TOO_SMALL".into()),
            CodecError::MalformedFrame("truncated".into()),
            CodecError::PayloadTooLarge {
                size: 8192,
                cap: 4096,
            },
            CodecError::Format("bad wav".into()),
            CodecError::OddSampleBuffer {
                len: 5,
                bytes_per_sample: 3,
            },
        ];
        for e in &cases {
            assert!(!e.to_string().is_empty(), "{e:?} renders empty");
            // Error trait impl present.
            let _: &dyn std::error::Error = e;
        }
    }
}
