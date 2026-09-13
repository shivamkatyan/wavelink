//! Control-plane message set (PROTOCOL_SPEC §Control / §Capability negotiation).
//!
//! All messages are `serde`/`postcard`-serializable; `ControlMessage` is the
//! tagged wire envelope. Encoding/decoding goes through the crate-level
//! helpers that enforce `MAX_CONTROL_MSG` before allocation (SEC-05).

use serde::{Deserialize, Serialize};

use crate::{
    BufferProfile, ChannelLayout, Codec, Error, ErrorCode, FeatureFlags, PolicyBits,
    TransportFeatures,
};

/// Protocol version exchanged during negotiation (PROTO_MAJOR.MINOR).
/// Minor bumps forward-compatible; major bumps require a new fixture set.
pub const PROTO_MAJOR: u16 = 1;
pub const PROTO_MINOR: u16 = 0;

/// Wire header carried by every control message (`pack` wraps this + payload).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct MsgHeader {
    /// Protocol message version (currently PROTO_MAJOR << 0 with MINOR in nibble-gap; simple monotonic).
    pub msg_ver: u16,
    /// Session-scoped stream identifier.
    pub stream_id: u32,
    /// Control sequence number (u64; wrap-safe).
    pub seq: u64,
    /// Media timestamp reference at send time (u64 emitter sample counter).
    pub media_ts: u64,
}

/// Capability descriptor carried by request/response and the session descriptor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Capability {
    pub codec: Codec,
    pub sample_rate: u32,
    pub channels: ChannelLayout,
    pub bit_depth: u16,
    pub frame_duration_ms: u16,
    pub transport: TransportFeatures,
    pub buffer: BufferProfile,
    pub features: FeatureFlags,
}

impl Default for Capability {
    fn default() -> Self {
        Self {
            codec: Codec::Opus,
            sample_rate: 48000,
            channels: ChannelLayout::Stereo,
            bit_depth: 16,
            frame_duration_ms: 20,
            transport: TransportFeatures::default(),
            buffer: BufferProfile::default(),
            features: FeatureFlags::default(),
        }
    }
}

/// Session descriptor (FR-006 / FR-046 fan-out ready).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SessionDescriptor {
    pub session_id: u32,
    pub emitter: Capability,
    pub receiver: Capability,
    pub agreed: Capability,
}

/// Feedback from receiver→emitter (SR/RR substitute).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Feedback {
    /// Round-trip time estimate (microseconds).
    pub rtt_us: u32,
    /// Measured jitter (microseconds).
    pub jitter_us: u32,
    /// Loss percentage (0–1000 for 0.0–100.0%).
    pub loss_pct: u16,
    /// Reorder count in the window (packets).
    pub reorder: u16,
    /// Late-discard count (packets).
    pub late_discard: u16,
    /// Current buffer fill (samples).
    pub buffer_fill: u32,
    /// Underrun count (events).
    pub underruns: u32,
    /// Output clock estimate (ppm offset; i32 to allow negative).
    pub output_clock_estimate: i32,
    /// Requested adaptation (e.g. buffer change hint).
    pub requested_adapt: u8,
}

/// Pairing handshake payloads (SAS / confirm-reject).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SasOffer {
    /// 6 decimal digits (0–999999) bound to the final handshake hash.
    pub sas: u32,
    /// Whether the SAS was self-verified against a QR/pubkey.
    pub bound: bool,
}

/// Signed revocation record container (SECURITY_SPEC §2.6).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RevokeRecord {
    pub format_version: u32,
    pub record_id: [u8; 16],
    pub issuer_pub: [u8; 32],
    pub subject_pub: [u8; 32],
    pub reason: RevokeReason,
    pub issued_at: u64,
    pub map_version: u64,
    pub prev_sha: Option<[u8; 32]>,
    pub signature: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum RevokeReason {
    Sold,
    Lost,
    Leaked,
    Rekeyed,
    Admin,
}

/// Single control message (wire envelope).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ControlMessage {
    Hello {
        header: MsgHeader,
        device_name: String,
    },
    CapabilityRequest {
        header: MsgHeader,
    },
    CapabilityResponse {
        header: MsgHeader,
        capability: Capability,
    },
    SessionDescriptor {
        header: MsgHeader,
        descriptor: SessionDescriptor,
    },
    PairingStart {
        header: MsgHeader,
        local_fingerprint: [u8; 32],
    },
    SasOffer {
        header: MsgHeader,
        offer: SasOffer,
    },
    SasConfirmReject {
        header: MsgHeader,
        confirmed: bool,
    },
    PolicyAdvertisement {
        header: MsgHeader,
        tier: PolicyTier,
        bits: PolicyBits,
    },
    StartStream {
        header: MsgHeader,
        source_stream_id: u32,
    },
    StopStream {
        header: MsgHeader,
        source_stream_id: u32,
    },
    Pause {
        header: MsgHeader,
        source_stream_id: u32,
    },
    Resume {
        header: MsgHeader,
        source_stream_id: u32,
    },
    Feedback {
        header: MsgHeader,
        feedback: Feedback,
    },
    Error {
        header: MsgHeader,
        error: Error,
    },
    Keepalive {
        header: MsgHeader,
        nonce: u64,
    },
    PairAttemptRejected {
        header: MsgHeader,
        reason: ErrorCode,
    },
    RevokeRecord {
        header: MsgHeader,
        record: RevokeRecord,
    },
}

impl ControlMessage {
    /// Extract the common `MsgHeader` (all variants carry one).
    #[must_use]
    pub fn header(&self) -> MsgHeader {
        match self {
            Self::Hello { header, .. }
            | Self::CapabilityRequest { header }
            | Self::CapabilityResponse { header, .. }
            | Self::SessionDescriptor { header, .. }
            | Self::PairingStart { header, .. }
            | Self::SasOffer { header, .. }
            | Self::SasConfirmReject { header, .. }
            | Self::PolicyAdvertisement { header, .. }
            | Self::StartStream { header, .. }
            | Self::StopStream { header, .. }
            | Self::Pause { header, .. }
            | Self::Resume { header, .. }
            | Self::Feedback { header, .. }
            | Self::Error { header, .. }
            | Self::Keepalive { header, .. }
            | Self::PairAttemptRejected { header, .. }
            | Self::RevokeRecord { header, .. } => *header,
        }
    }
}

/// Policy tier for `PolicyAdvertisement` (Free/Pro + feature bits).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PolicyTier {
    Free,
    Pro,
}
