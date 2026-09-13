//! Audio frame header + payload container.
//!
//! Header layout mirrors `PROTOCOL_SPEC.md` §Audio frame header: version,
//! stream ID, **u64** sequence and **u64** media timestamp (emitter sample
//! counter + wall-clock base; wrap-proof at 2^64, prop-tested), codec, sample
//! rate, sample representation, channel layout, frame sample count, flags and
//! integrity. The payload cap (`MAX_FRAME_PAYLOAD`) is enforced at decode time
//! **before** any buffer is filled from untrusted input (SECURITY_SPEC §4).

use serde::{
    de::{self, DeserializeSeed, Deserializer, SeqAccess, Visitor},
    Deserialize, Serialize, Serializer,
};

use crate::{ChannelLayout, Codec, DecodeError, Integrity, SampleRepr, MAX_FRAME_PAYLOAD};

/// Minimum supported on-wire audio-frame version (protocol floor).
pub const FRAME_VERSION: u16 = 1;

/// Attribute flags carried in the frame header (u8 bitfield).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub struct FrameFlags {
    /// End-of-burst marker (flush point).
    pub burst_end: bool,
    /// Lossless-per-frame ordering boundary.
    pub frame_boundary: bool,
    /// Reserved for future use (written 0, ignored on read).
    pub reserved: u8,
}

/// Frame payload-container integrity payload: either nothing, a per-frame
/// CRC32 (`Crc32(u32)`), or an AEAD-tag placeholder (`Aead`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum FrameIntegrity {
    None,
    Crc32(u32),
    Aead,
}

/// Audio frame: header + bounded payload (`Vec<u8>`, ≤ `MAX_FRAME_PAYLOAD`).
///
/// `Serialize` is hand-written so the payload `Vec` is length-prefixed with
/// its exact byte length (postcard default). `Deserialize` is hand-written so
/// the payload length is validated against `MAX_FRAME_PAYLOAD` *before* any
/// payload `Vec` allocation occurs (bounds before allocation, SEC-05).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Frame {
    /// On-wire frame version (currently `FRAME_VERSION`).
    pub version: u16,
    /// Session-scoped stream identifier.
    pub stream_id: u32,
    /// Monotonic sequence number (u64; wrap-safe).
    pub seq: u64,
    /// Emitter media timestamp (sample counter + wall-clock base; u64).
    pub media_ts: u64,
    /// Codec for the payload.
    pub codec: Codec,
    /// Emitter sample rate in Hz.
    pub sample_rate: u32,
    /// Sample representation / bit depth.
    pub sample_repr: SampleRepr,
    /// Channel layout of the payload.
    pub channel_layout: ChannelLayout,
    /// Number of (per-channel) samples in this frame.
    pub frame_sample_count: u32,
    /// Header attribute flags (u8 bitfield).
    pub flags: FrameFlags,
    /// Per-frame integrity policy.
    pub integrity: Integrity,
    /// Per-frame integrity payload (CRC32 on lossless; none/AEAD otherwise).
    pub frame_integrity: FrameIntegrity,
    /// Bounded payload bytes; decode rejects `len > MAX_FRAME_PAYLOAD`
    /// before buffering.
    pub payload: Vec<u8>,
}

impl Frame {
    /// Worst-case encoded size of the header + payload-length prefix before
    /// the payload bytes.
    ///
    /// Sum of worst-case varints: version 3, stream_id 5, seq 10, media_ts 10,
    /// codec 1, sample_rate 5, sample_repr 1, channel_layout 1,
    /// frame_sample_count 5, flags 3, integrity 1, frame_integrity 6 and
    /// payload_len 5 (58 total). A generous 64 keeps the pre-check free of
    /// heap use while still rejecting any total that cannot be a valid frame.
    pub const fn header_max_len() -> usize {
        64
    }

    /// Build a lossy frame (integrity `None`, no CRC).
    #[allow(clippy::too_many_arguments)]
    pub fn new_lossy(
        stream_id: u32,
        seq: u64,
        media_ts: u64,
        codec: Codec,
        sample_rate: u32,
        sample_repr: SampleRepr,
        channel_layout: ChannelLayout,
        frame_sample_count: u32,
        flags: FrameFlags,
        payload: Vec<u8>,
    ) -> Self {
        Self {
            version: FRAME_VERSION,
            stream_id,
            seq,
            media_ts,
            codec,
            sample_rate,
            sample_repr,
            channel_layout,
            frame_sample_count,
            flags,
            integrity: Integrity::None,
            frame_integrity: FrameIntegrity::None,
            payload,
        }
    }

    /// Build a lossless frame carrying a per-frame CRC32 of the payload.
    #[allow(clippy::too_many_arguments)]
    pub fn new_lossless(
        stream_id: u32,
        seq: u64,
        media_ts: u64,
        codec: Codec,
        sample_rate: u32,
        sample_repr: SampleRepr,
        channel_layout: ChannelLayout,
        frame_sample_count: u32,
        flags: FrameFlags,
        payload: Vec<u8>,
    ) -> Self {
        let crc = crate::crc32(&payload);
        Self {
            version: FRAME_VERSION,
            stream_id,
            seq,
            media_ts,
            codec,
            sample_rate,
            sample_repr,
            channel_layout,
            frame_sample_count,
            flags,
            integrity: Integrity::Crc32,
            frame_integrity: FrameIntegrity::Crc32(crc),
            payload,
        }
    }

    /// Serialize the frame (header + payload) with postcard.
    pub fn pack(&self) -> Result<Vec<u8>, crate::EncodeError> {
        if self.payload.len() > MAX_FRAME_PAYLOAD {
            return Err(crate::EncodeError::PayloadOverflow);
        }
        crate::encode_postcard(self)
    }

    /// Deserialize a frame, enforcing `MAX_FRAME_PAYLOAD` on the payload
    /// **before** the payload `Vec` is allocated (SECURITY_SPEC §4 / SEC-05).
    pub fn unpack(bytes: &[u8]) -> Result<Self, DecodeError> {
        if bytes.len() > Self::header_max_len() + MAX_FRAME_PAYLOAD {
            return Err(DecodeError::PayloadOverflow);
        }
        crate::decode_postcard(bytes)
    }
}

impl Serialize for Frame {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        use serde::ser::SerializeTuple;
        // 13 header value-pairs are serialized as a plain tuple (no prefix).
        let mut t = serializer.serialize_tuple(13)?;
        t.serialize_element(&self.version)?;
        t.serialize_element(&self.stream_id)?;
        t.serialize_element(&self.seq)?;
        t.serialize_element(&self.media_ts)?;
        t.serialize_element(&self.codec)?;
        t.serialize_element(&self.sample_rate)?;
        t.serialize_element(&self.sample_repr)?;
        t.serialize_element(&self.channel_layout)?;
        t.serialize_element(&self.frame_sample_count)?;
        // flags: single u8 (2 low bits + 6 reserved).
        t.serialize_element(&u8ify_flags(&self.flags))?;
        t.serialize_element(&self.integrity)?;
        t.serialize_element(&self.frame_integrity)?;
        // payload Vec<u8>: serialized as a length-prefixed byte sequence.
        t.serialize_element(&self.payload)?;
        t.end()
    }
}

impl<'de> Deserialize<'de> for Frame {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_tuple(13, FrameVisitor)
    }
}

struct FrameVisitor;

impl<'de> de::Visitor<'de> for FrameVisitor {
    type Value = Frame;

    fn expecting(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("a WDR audio frame (13-tuple header + payload)")
    }

    fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
    where
        A: de::SeqAccess<'de>,
    {
        let version = next(&mut seq)?;
        let stream_id = next(&mut seq)?;
        let seq_no = next(&mut seq)?;
        let media_ts = next(&mut seq)?;
        let codec = next(&mut seq)?;
        let sample_rate = next(&mut seq)?;
        let sample_repr = next(&mut seq)?;
        let channel_layout = next(&mut seq)?;
        let frame_sample_count = next(&mut seq)?;
        let flags = next_u8(&mut seq).map(flags_from_u8)?;
        let integrity = next(&mut seq)?;
        let frame_integrity = next(&mut seq)?;
        let payload = seq
            .next_element_seed(PayloadSeed)?
            .ok_or_else(|| de::Error::invalid_length(0, &"frame payload"))?;
        Ok(Frame {
            version,
            stream_id,
            seq: seq_no,
            media_ts,
            codec,
            sample_rate,
            sample_repr,
            channel_layout,
            frame_sample_count,
            flags,
            integrity,
            frame_integrity,
            payload,
        })
    }
}

fn next<'de, A, T>(seq: &mut A) -> Result<T, A::Error>
where
    A: SeqAccess<'de>,
    T: Deserialize<'de>,
{
    seq.next_element::<T>()?
        .ok_or_else(|| de::Error::invalid_length(0, &"frame header field"))
}

fn next_u8<'de, A>(seq: &mut A) -> Result<u8, A::Error>
where
    A: SeqAccess<'de>,
{
    next(seq)
}

/// A `DeserializeSeed` that deserializes the frame payload as a length-prefixed
/// sequence, enforcing `MAX_FRAME_PAYLOAD` on the declared length via
/// `SeqAccess::size_hint()` **before** iterating (i.e. before any payload
/// `Vec` allocation, SECURITY_SPEC §4 / SEC-05).
struct PayloadSeed;

impl<'de> DeserializeSeed<'de> for PayloadSeed {
    type Value = Vec<u8>;

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_seq(PayloadVisitor)
    }
}

struct PayloadVisitor;

impl<'de> Visitor<'de> for PayloadVisitor {
    type Value = Vec<u8>;

    fn expecting(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("a length-prefixed frame payload within MAX_FRAME_PAYLOAD")
    }

    fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        // postcard's SeqAccess::size_hint is min(declared_len, remaining bytes);
        // for well-formed input it is the exact declared payload length.
        if let Some(declared) = seq.size_hint() {
            if declared > MAX_FRAME_PAYLOAD {
                return Err(de::Error::invalid_length(
                    MAX_FRAME_PAYLOAD,
                    &"frame payload within MAX_FRAME_PAYLOAD",
                ));
            }
        }

        // Bounded pre-allocation (never more than MAX_FRAME_PAYLOAD).
        let mut v = Vec::with_capacity(seq.size_hint().unwrap_or(0).min(MAX_FRAME_PAYLOAD));
        while let Some(b) = seq.next_element::<u8>()? {
            // Defense-in-depth: never exceed the cap even if size_hint lied.
            if v.len() >= MAX_FRAME_PAYLOAD {
                return Err(de::Error::invalid_length(
                    MAX_FRAME_PAYLOAD,
                    &"frame payload within MAX_FRAME_PAYLOAD",
                ));
            }
            v.push(b);
        }
        Ok(v)
    }
}

/// flags u8 bitfield: bit0 = burst_end, bit1 = frame_boundary, bits 2..8 reserved.
fn u8ify_flags(f: &FrameFlags) -> u8 {
    f.burst_end as u8 | (f.frame_boundary as u8) << 1 | (f.reserved & 0x3F) << 2
}

fn flags_from_u8(v: u8) -> FrameFlags {
    FrameFlags {
        burst_end: v & 0x01 != 0,
        frame_boundary: v & 0x02 != 0,
        reserved: (v >> 2) & 0x3F,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crc32;

    #[test]
    fn lossy_frame_roundtrip() {
        let f = Frame::new_lossy(
            7,
            42,
            u64::MAX - 1,
            Codec::Opus,
            48000,
            SampleRepr::I16,
            ChannelLayout::Stereo,
            960,
            FrameFlags::default(),
            vec![0xAB, 0xCD, 0xEF],
        );
        let bytes = f.pack().unwrap();
        let back = Frame::unpack(&bytes).unwrap();
        assert_eq!(back, f);
        assert_eq!(back.integrity, Integrity::None);
        assert_eq!(back.frame_integrity, FrameIntegrity::None);
    }

    #[test]
    fn lossless_frame_carries_crc() {
        let payload = vec![1u8; 64];
        let f = Frame::new_lossless(
            1,
            0,
            0,
            Codec::Flac,
            48000,
            SampleRepr::I16,
            ChannelLayout::Stereo,
            960,
            FrameFlags::default(),
            payload.clone(),
        );
        assert_eq!(f.frame_integrity, FrameIntegrity::Crc32(crc32(&payload)));
        let back = Frame::unpack(&f.pack().unwrap()).unwrap();
        assert_eq!(back.frame_integrity, FrameIntegrity::Crc32(crc32(&payload)));
        assert_eq!(back.integrity, Integrity::Crc32);
    }

    #[test]
    fn flags_roundtrip_all_bits() {
        let flags = FrameFlags {
            burst_end: true,
            frame_boundary: true,
            reserved: 0x3F,
        };
        let f = Frame::new_lossy(
            1,
            0,
            0,
            Codec::Pcm,
            44100,
            SampleRepr::I24Packed,
            ChannelLayout::Mono,
            441,
            flags,
            vec![],
        );
        assert_eq!(Frame::unpack(&f.pack().unwrap()).unwrap().flags, flags);
        assert_eq!(u8ify_flags(&flags), 0xFF);
        assert_eq!(u8ify_flags(&FrameFlags::default()), 0);
    }

    #[test]
    fn pack_rejects_oversized_payload() {
        let f = Frame::new_lossy(
            1,
            0,
            0,
            Codec::Pcm,
            48000,
            SampleRepr::I16,
            ChannelLayout::Mono,
            0,
            FrameFlags::default(),
            vec![0u8; MAX_FRAME_PAYLOAD + 1],
        );
        assert!(f.pack().is_err());
    }

    #[test]
    fn unpack_rejects_oversized_total() {
        let jumbo = vec![0u8; MAX_FRAME_PAYLOAD + Frame::header_max_len() + 1];
        assert_eq!(
            Frame::unpack(&jumbo).unwrap_err(),
            DecodeError::PayloadOverflow
        );
    }
}
