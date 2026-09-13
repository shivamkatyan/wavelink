//! Codec / sample / channel / transport descriptors shared by the frame
//! header, capability negotiation and session descriptors.

use serde::{Deserialize, Serialize};

/// Codec identifiers (PROTOCOL_SPEC §Codec profiles).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Codec {
    /// Opus lossy audio (44.1/48 kHz; low-latency profile supported).
    Opus,
    /// FLAC lossless audio, small fixed blocks (~240 samples @48k).
    Flac,
    /// Raw PCM (lossless profile).
    Pcm,
}

/// Sample representation / bit depth container.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SampleRepr {
    /// 16-bit signed little-endian samples.
    I16,
    /// 32-bit float samples.
    F32,
    /// 24-bit samples packed into 3 bytes/sample (I24-Packed).
    I24Packed,
    /// 32-bit signed integer samples.
    I32,
}

/// Channel layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ChannelLayout {
    Mono,
    Stereo,
    Quad,
    Surround51,
    Surround71,
}

/// Transport-feature capabilities for capability negotiation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TransportFeatures {
    /// Reliable control stream supported (required; QUIC).
    pub reliable_control: bool,
    /// Unreliable datagram media path (lossy codecs).
    pub datagram_media: bool,
    /// Reliable stream for lossless media (FLAC/PCM).
    pub reliable_media: bool,
    /// Connection migration / path change support.
    pub connection_migration: bool,
}

impl Default for TransportFeatures {
    fn default() -> Self {
        Self {
            reliable_control: true,
            datagram_media: true,
            reliable_media: true,
            connection_migration: true,
        }
    }
}

/// Receiver buffer profile advertisement used during negotiation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct BufferProfile {
    /// Jitter-buffer depth in milliseconds.
    pub jitter_ms: u32,
    /// Initial pre-roll in samples.
    pub pre_roll_samples: u32,
    /// Maximum burst buffer in samples.
    pub max_burst_samples: u32,
    /// Transport latency budget in milliseconds.
    pub latency_budget_ms: u32,
}

impl Default for BufferProfile {
    fn default() -> Self {
        Self {
            jitter_ms: 60,
            pre_roll_samples: 0,
            max_burst_samples: 8192,
            latency_budget_ms: 50,
        }
    }
}

/// Feature-flag bits for capability / policy negotiation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct FeatureFlags {
    /// 24-bit→16-bit conversion may apply TPDF dither + noise shaping.
    pub tpdf_dither: bool,
    /// >48 kHz sources may not route through lossy codecs.
    pub high_rate_lossy_gate: bool,
    /// Per-frame CRC for lossless paths is enabled.
    pub per_frame_crc: bool,
    /// In-session mode/policy renegotiation supported.
    pub mid_session_renegotiation: bool,
}

impl Default for FeatureFlags {
    fn default() -> Self {
        Self {
            tpdf_dither: true,
            high_rate_lossy_gate: true,
            per_frame_crc: true,
            mid_session_renegotiation: true,
        }
    }
}

/// Common policy bits advertised by `PolicyAdvertisement` and intersected
/// during negotiation (`agree_common_policy`; never silent downgrade).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub struct PolicyBits {
    /// Lossless (FLAC/raw-PCM) routing permitted.
    pub lossless: bool,
    /// Opus lossy routing permitted.
    pub lossy: bool,
    /// Resampling for drift correction permitted.
    pub resample: bool,
    /// Reconnect on path change permitted.
    pub reconnect: bool,
}
