//! `wdr_proto` — Wavelink wire protocol.
//!
//! The single source of truth for the on-wire schema (postcard + serde
//! derives), from which bindings and golden vectors are generated
//! (PROTOCOL_SPEC §Versioning). Encode/decode is bounded and error-typed: no
//! parser panics and no parser allocates on attacker-controlled sizes
//! (SECURITY_SPEC §4 / SEC-05).

mod bounds;
mod codec;
mod control;
mod crc;
mod error;
mod frame;
mod golden;
mod integrity;
mod wire;

#[doc(inline)]
pub use bounds::*;
#[doc(inline)]
pub use codec::*;
#[doc(inline)]
pub use control::*;
#[doc(inline)]
pub use crc::*;
#[doc(inline)]
pub use error::*;
#[doc(inline)]
pub use frame::*;
#[doc(inline)]
pub use golden::*;
#[doc(inline)]
pub use integrity::*;
pub use wire::{decode_postcard, encode_postcard, pack, unpack, EncodeError};
