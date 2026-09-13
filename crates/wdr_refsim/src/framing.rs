//! Frame wire framing for the reference sim (task t-B1-receiver).
//!
//! `wdr_proto::Frame` is serialized with `postcard`; a single packed frame can
//! ride **either** a QUIC datagram (lossy path: one datagram = one packed
//! frame) or a reliable stream (lossless path: `[u16 len][item]`-framed so
//! many frames plus an end marker stream cleanly).
//!
//! # End-of-stream marker
//!
//! `wdr_proto` does not define a `StreamEnd` control message, so this module
//! defines a small, deterministic out-of-band end marker for the sim plane:
//!
//! ```text
//! b"WDRZE" (5 bytes) + u64 little-endian total_frames
//! ```
//!
//! The marker travels as its own datagram (lossy/control plane) or as a
//! length-prefixed item on the reliable stream after the last frame. It lets
//! the receiver terminate with bounded state (no sleep-and-assume) and report
//! exact loss (`total_frames` is authoritative).
//!
//! All parsing is bounds-checked and error-typed; nothing here panics on
//! untrusted input (SECURITY_SPEC §4 / SEC-05). A frame whose total encoded
//! size would exceed `MAX_FRAME_PAYLOAD + header_max_len` is rejected before
//! any payload buffer is filled (`Frame::unpack` enforces the rest).

use wdr_proto::{DecodeError, Frame, MAX_FRAME_PAYLOAD};

/// Magic bytes distinguishing the end marker from a packed frame item.
pub const END_MAGIC: [u8; 5] = *b"WDRZE";

/// Maximum total encoded size of one packed frame item.
pub const MAX_FRAME_TOTAL: usize = Frame::header_max_len() + MAX_FRAME_PAYLOAD;

/// One item carried by the sim wire.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FramedItem {
    /// A valid audio frame (already unpacked and guarded).
    Audio(Frame),
    /// End-of-stream marker, carrying the total frame count the sender sent.
    End { total_frames: u64 },
}

/// Typed failure for the sim framing layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FramingError {
    /// The bytes are neither a valid end marker nor a parsable frame.
    Malformed,
    /// The declared frame length exceeds `MAX_FRAME_TOTAL` / is truncated.
    Bounds,
    /// The inner `Frame::unpack` stated a payload-overflow/guard failure.
    PayloadOverflow,
}

impl core::fmt::Display for FramingError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(match self {
            FramingError::Malformed => "framing: malformed item",
            FramingError::Bounds => "framing: item exceeds size bounds",
            FramingError::PayloadOverflow => "framing: frame payload overflow",
        })
    }
}

impl std::error::Error for FramingError {}

/// Namespace for the sim frame wire codec.
pub struct FrameWire;

impl FrameWire {
    /// Wrap any item bytes (a packed frame or the end marker) with a `u16`
    /// little-endian length prefix for the reliable stream.
    pub fn wrap_item(bytes: &[u8]) -> Vec<u8> {
        assert!(
            bytes.len() <= u16::MAX as usize,
            "item cannot exceed the stream framing prefix (guarded by MAX_FRAME_PAYLOAD)"
        );
        let mut out = Vec::with_capacity(2 + bytes.len());
        out.extend_from_slice(&(bytes.len() as u16).to_le_bytes());
        out.extend_from_slice(bytes);
        out
    }

    /// Length-prefix a packed `Frame` (thick wrapper around [`Self::wrap_item`]).
    pub fn wrap_frame(frame: &Frame) -> Vec<u8> {
        let body = frame
            .pack()
            .expect("frame with in-bounds payload packs; guarded by Frame::pack");
        Self::wrap_item(&body)
    }

    /// Pack the end-of-stream marker.
    pub fn pack_known_end(total_frames: u64) -> Vec<u8> {
        let mut out = Vec::with_capacity(13);
        out.extend_from_slice(&END_MAGIC);
        out.extend_from_slice(&total_frames.to_le_bytes());
        out
    }

    /// Pack the end-of-stream marker (alias for the CLI/tests).
    pub fn pack_end(total_frames: u64) -> Vec<u8> {
        Self::pack_known_end(total_frames)
    }

    /// Parse one raw item: the whole datagram bytes, or the payload portion of
    /// a length-prefixed stream item.
    pub fn parse(bytes: &[u8]) -> Result<FramedItem, FramingError> {
        if Self::is_end(bytes) {
            let total = u64::from_le_bytes(
                bytes[END_MAGIC.len()..END_MAGIC.len() + 8]
                    .try_into()
                    .map_err(|_| FramingError::Malformed)?,
            );
            return Ok(FramedItem::End {
                total_frames: total,
            });
        }
        let frame = Frame::unpack(bytes).map_err(map_unpack_err)?;
        Ok(FramedItem::Audio(frame))
    }

    /// Parse a datagram (one packed frame or the end marker).
    pub fn parse_datagram(bytes: &[u8]) -> Result<FramedItem, FramingError> {
        Self::parse(bytes)
    }

    /// Is this byte slice exactly the end marker?
    pub fn is_end(bytes: &[u8]) -> bool {
        bytes.len() == END_MAGIC.len() + 8 && bytes.starts_with(&END_MAGIC)
    }

    /// Incremental reliable-stream decoder: append incoming stream bytes to
    /// `buf`, then pull out every complete length-prefixed item in order.
    ///
    /// Returns `(items, err)`: complete items plus an error for the first
    /// malformed item (the caller may stop on error). If a partial prefix/body
    /// is pending, returns an empty item list (no flush needed).
    pub fn take_stream_items(buf: &mut Vec<u8>) -> (Vec<FramedItem>, Option<FramingError>) {
        let mut items = Vec::new();
        loop {
            if buf.len() < 2 {
                break;
            }
            let len = u16::from_le_bytes([buf[0], buf[1]]) as usize;
            if len > MAX_FRAME_TOTAL + 13 {
                return (items, Some(FramingError::Bounds));
            }
            if buf.len() < 2 + len {
                break; // wait for the rest
            }
            let item_bytes = buf[2..2 + len].to_vec();
            buf.drain(..2 + len);
            match Self::parse(&item_bytes) {
                Ok(it) => items.push(it),
                Err(e) => return (items, Some(e)),
            }
        }
        (items, None)
    }
}

fn map_unpack_err(e: DecodeError) -> FramingError {
    match e {
        DecodeError::PayloadOverflow => FramingError::PayloadOverflow,
        _ => FramingError::Malformed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wdr_proto::{ChannelLayout, Codec, FrameFlags, SampleRepr};

    fn sample_frame(seq: u64) -> Frame {
        Frame::new_lossless(
            7,
            seq,
            seq * 512,
            Codec::Flac,
            48_000,
            SampleRepr::I16,
            ChannelLayout::Stereo,
            512,
            FrameFlags::default(),
            vec![0xABu8; 128],
        )
    }

    #[test]
    fn stream_roundtrip_and_end_marker() {
        let f = sample_frame(3);
        let wrapped = FrameWire::wrap_frame(&f);
        assert_eq!(wrapped.len(), 2 + f.pack().unwrap().len());

        let item = FrameWire::parse(&wrapped[2..]).unwrap();
        assert_eq!(item, FramedItem::Audio(f));

        let end = FrameWire::pack_known_end(4096);
        assert!(FrameWire::is_end(&end));
        assert_eq!(
            FrameWire::parse(&end).unwrap(),
            FramedItem::End { total_frames: 4096 }
        );
    }

    #[test]
    fn stream_items_split_incrementally() {
        let mut buf = Vec::new();
        buf.extend_from_slice(&FrameWire::wrap_frame(&sample_frame(1)));
        buf.extend_from_slice(&FrameWire::wrap_frame(&sample_frame(2)));
        buf.extend_from_slice(&FrameWire::wrap_item(&FrameWire::pack_known_end(3)));

        let (items, err) = FrameWire::take_stream_items(&mut buf);
        assert!(err.is_none());
        assert_eq!(items.len(), 3);
        assert!(buf.is_empty());
        assert!(matches!(items[0], FramedItem::Audio(_)));
        assert!(matches!(items[1], FramedItem::Audio(_)));
        assert_eq!(items[2], FramedItem::End { total_frames: 3 });
    }

    #[test]
    fn malformed_rejected_not_panic() {
        for bad in [&[0u8; 0][..], &[0xFFu8; 64], b"WDRZE", &[0xFFu8; 4096]] {
            assert!(FrameWire::parse(bad).is_err(), "{bad:?} must be rejected");
        }
    }

    #[test]
    fn degenerate_zero_frame_is_caught_by_guard_not_framing() {
        // `[0u8; 4096]` decodes (postcard) as an all-zero header frame
        // (version 0, zero-length payload, trailing varints ignored). The
        // framing layer is a pure unpacker, so it yields an Audio item; the
        // *version floor* is enforced by `receiver::guard_frame` before the
        // frame ever reaches decode/rendering.
        let item = FrameWire::parse(&[0u8; 4096]).expect("degenerate frame unpacks");
        let frame = match item {
            FramedItem::Audio(f) => f,
            _ => panic!("loaded bytes must unpack as an Audio item, not an End marker"),
        };
        assert_eq!(frame.version, 0, "all-zero header decodes as version 0");
        let err = crate::receiver::guard_frame(&frame);
        assert!(
            matches!(err, Err(crate::receiver::ReceiverError::Internal(_))),
            "receiver guard must reject version-0 garbage (got {err:?})"
        );
    }
}
