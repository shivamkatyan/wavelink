//! Wire encode/decode helpers with bounded, typed errors.
//!
//! Control messages are capped at `MAX_CONTROL_MSG`; audio frames are capped
//! at `MAX_FRAME_PAYLOAD` (payload cap enforced *inside* `Frame`'s
//! deserializer, before any payload allocation). All decode paths here are
//! error-typed and never panic.

use serde::{de::DeserializeOwned, Serialize};

use crate::{frame, DecodeError, MAX_CONTROL_MSG, MAX_FRAME_PAYLOAD};

/// Encode failure (typed, non-panicking).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EncodeError {
    /// The encoded value exceeded a configured capacity bound.
    SizeExceeded,
    /// A frame payload exceeded `MAX_FRAME_PAYLOAD`.
    PayloadOverflow,
}

/// Serialize `value` to a `Vec<u8>` with postcard, enforcing the
/// control-message cap: any encoding larger than `MAX_CONTROL_MSG` is
/// rejected (symmetric with `unpack`, SEC-05).
pub fn pack<T: Serialize>(value: &T) -> Result<Vec<u8>, EncodeError> {
    let bytes = pack_stdvec(value)?;
    if bytes.len() > MAX_CONTROL_MSG {
        return Err(EncodeError::SizeExceeded);
    }
    Ok(bytes)
}

/// Low-level postcard encode to a `Vec<u8>`.
pub fn pack_stdvec<T: Serialize>(value: &T) -> Result<Vec<u8>, EncodeError> {
    postcard::to_allocvec(value).map_err(|_| EncodeError::SizeExceeded)
}

/// Deserialize `T` from `bytes`, enforcing the control-message cap on input
/// length before building any allocation-sized buffer from it.
pub fn unpack<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, DecodeError> {
    if bytes.len() > MAX_CONTROL_MSG {
        return Err(DecodeError::LimitExceeded);
    }
    postcard::from_bytes(bytes).map_err(map_decode_err)
}

/// Encode a frame via postcard (uses the generic path; `Frame`'s serializer
/// is length-tight and pre-checks `MAX_FRAME_PAYLOAD`).
pub fn encode_postcard<T: Serialize>(value: &T) -> Result<Vec<u8>, EncodeError> {
    pack_stdvec(value)
}

/// Decode a frame via postcard, rejecting any input that cannot fit a valid
/// frame (`header_max_len + MAX_FRAME_PAYLOAD`) before parsing.
pub fn decode_postcard<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, DecodeError> {
    if bytes.len() > frame::Frame::header_max_len() + MAX_FRAME_PAYLOAD {
        return Err(DecodeError::PayloadOverflow);
    }
    postcard::from_bytes(bytes).map_err(map_decode_err)
}

fn map_decode_err(e: postcard::Error) -> DecodeError {
    use postcard::Error;
    match e {
        Error::DeserializeUnexpectedEnd => DecodeError::Truncated,
        Error::SerializeBufferFull => DecodeError::LimitExceeded,
        Error::DeserializeBadVarint
        | Error::DeserializeBadBool
        | Error::DeserializeBadChar
        | Error::DeserializeBadUtf8
        | Error::DeserializeBadOption
        | Error::DeserializeBadEnum
        | Error::DeserializeBadEncoding => DecodeError::DataInvalid,
        _ => DecodeError::DataInvalid,
    }
}
