//! Raw PCM (lossless) codec adapter — bit-perfect passthrough of interleaved
//! little-endian i16 (the lossless baseline), 24-bit packed via [`CodecAdapter24`],
//! and (WS-E) raw IEEE-f32 / 32-bit-int passthrough — ADR-005 16/24-bit + the
//! declared F32/I32 surface. Frame bytes carry a per-frame CRC-32 for integrity
//! (PROTOCOL_SPEC §Codec profiles).

use super::{CodecAdapter, CodecAdapter24};
use crate::error::CodecError;
use crate::size::CodecKind;

/// Which raw-PCM wire format an adapter instance reads/writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PcmFmt {
    /// i16, 2 bytes/sample LE (the [`CodecAdapter`] baseline).
    I16,
    /// Canonical i24, low-3 bytes/sample LE over right-aligned i32
    /// ([`CodecAdapter24`]).
    I24,
    /// IEEE-754 f32, 4 bytes/sample LE (WS-E).
    F32,
    /// i32, 4 bytes/sample LE (WS-E).
    I32,
}

/// Raw-PCM adapter. Every method gates on the constructed format: calling a
/// method of a different depth returns a typed [`CodecError::Unsupported`]
/// rather than a silent reformat.
pub struct PcmAdapter {
    channels: u16,
    fmt: PcmFmt,
}

impl PcmAdapter {
    /// Create a 16-bit raw-PCM adapter (the lossless baseline). `channels`
    /// must be 1 or 2.
    pub fn new(channels: u16) -> Result<Self, CodecError> {
        Self::with_fmt(channels, PcmFmt::I16)
    }

    /// Create a 24-bit raw-PCM adapter (ADR-005 follow-up). Its i32 surface
    /// is [`CodecAdapter24`].
    pub fn new24(channels: u16) -> Result<Self, CodecError> {
        Self::with_fmt(channels, PcmFmt::I24)
    }

    /// Create a raw IEEE-f32 PCM adapter (WS-E): 4 bytes/sample LE passthrough.
    pub fn new_f32(channels: u16) -> Result<Self, CodecError> {
        Self::with_fmt(channels, PcmFmt::F32)
    }

    /// Create a raw 32-bit-int PCM adapter (WS-E): 4 bytes/sample LE passthrough.
    pub fn new_i32(channels: u16) -> Result<Self, CodecError> {
        Self::with_fmt(channels, PcmFmt::I32)
    }

    fn with_fmt(channels: u16, fmt: PcmFmt) -> Result<Self, CodecError> {
        if channels == 0 || channels > 2 {
            return Err(CodecError::Unsupported(format!(
                "PCM adapter supports 1 or 2 channels, got {channels}"
            )));
        }
        Ok(Self { channels, fmt })
    }

    /// Encode interleaved f32 (IEEE-754 LE, 4 bytes/sample) — raw passthrough.
    pub fn encode_f32(&mut self, pcm: &[f32]) -> Result<Box<[u8]>, CodecError> {
        if self.fmt != PcmFmt::F32 {
            return Err(CodecError::Unsupported(format!(
                "PCM adapter holds {:?}; f32 encode needs `new_f32`",
                self.fmt
            )));
        }
        self.check_align(pcm.len())?;
        let mut out = Vec::with_capacity(pcm.len() * 4);
        for &s in pcm {
            out.extend_from_slice(&s.to_le_bytes());
        }
        Ok(out.into_boxed_slice())
    }

    /// Decode raw IEEE-f32 bytes back to interleaved f32 (WS-E).
    pub fn decode_f32(&mut self, bytes: &[u8]) -> Result<Box<[f32]>, CodecError> {
        if self.fmt != PcmFmt::F32 {
            return Err(CodecError::Unsupported(format!(
                "PCM adapter holds {:?}; f32 decode needs `new_f32`",
                self.fmt
            )));
        }
        self.check_bytes(bytes.len(), 4)?;
        let mut out = Vec::with_capacity(bytes.len() / 4);
        for chunk in bytes.as_chunks::<4>().0 {
            out.push(f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]));
        }
        Ok(out.into_boxed_slice())
    }

    /// Encode interleaved i32 (LE, 4 bytes/sample) — raw passthrough (WS-E).
    pub fn encode_i32(&mut self, pcm: &[i32]) -> Result<Box<[u8]>, CodecError> {
        if self.fmt != PcmFmt::I32 {
            return Err(CodecError::Unsupported(format!(
                "PCM adapter holds {:?}; i32 encode needs `new_i32`",
                self.fmt
            )));
        }
        self.check_align(pcm.len())?;
        let mut out = Vec::with_capacity(pcm.len() * 4);
        for &s in pcm {
            out.extend_from_slice(&s.to_le_bytes());
        }
        Ok(out.into_boxed_slice())
    }

    /// Decode raw 32-bit-int bytes back to interleaved i32 (WS-E).
    pub fn decode_i32(&mut self, bytes: &[u8]) -> Result<Box<[i32]>, CodecError> {
        if self.fmt != PcmFmt::I32 {
            return Err(CodecError::Unsupported(format!(
                "PCM adapter holds {:?}; i32 decode needs `new_i32`",
                self.fmt
            )));
        }
        self.check_bytes(bytes.len(), 4)?;
        let mut out = Vec::with_capacity(bytes.len() / 4);
        for chunk in bytes.as_chunks::<4>().0 {
            out.push(i32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]));
        }
        Ok(out.into_boxed_slice())
    }

    fn check_align(&self, values: usize) -> Result<(), CodecError> {
        if !values.is_multiple_of(usize::from(self.channels)) {
            return Err(CodecError::InvalidPcmLen {
                len: values,
                channels: self.channels,
            });
        }
        Ok(())
    }

    fn check_bytes(&self, len: usize, bytes_per_sample: usize) -> Result<(), CodecError> {
        if !len.is_multiple_of(bytes_per_sample) {
            return Err(CodecError::MalformedFrame(format!(
                "PCM byte length {len} is not a multiple of {bytes_per_sample} (bytes/sample for {:?})",
                self.fmt
            )));
        }
        let ch = usize::from(self.channels);
        if !(len / bytes_per_sample).is_multiple_of(ch) {
            return Err(CodecError::MalformedFrame(format!(
                "PCM sample count {} not divisible by {ch} channels",
                len / bytes_per_sample
            )));
        }
        Ok(())
    }
}

impl CodecAdapter for PcmAdapter {
    fn kind(&self) -> CodecKind {
        CodecKind::Pcm
    }

    fn name(&self) -> &'static str {
        "Pcm"
    }

    fn encode(&mut self, pcm: &[i16]) -> Result<Box<[u8]>, CodecError> {
        if self.fmt != PcmFmt::I16 {
            return Err(CodecError::Unsupported(format!(
                "PCM adapter holds {:?}; i16 encode needs a 16-bit adapter \
                 (use `new24`/`new_f32`/`new_i32` for the other surfaces)",
                self.fmt
            )));
        }
        self.check_align(pcm.len())?;
        let n = pcm.len();
        let mut out = Vec::with_capacity(n * 2);
        for &s in pcm {
            out.extend_from_slice(&s.to_le_bytes());
        }
        Ok(out.into_boxed_slice())
    }

    fn decode(&mut self, bytes: &[u8]) -> Result<Box<[i16]>, CodecError> {
        if self.fmt != PcmFmt::I16 {
            return Err(CodecError::Unsupported(format!(
                "PCM adapter holds {:?}; i16 decode needs a 16-bit adapter \
                 (use `new24`/`new_f32`/`new_i32` for the other surfaces)",
                self.fmt
            )));
        }
        self.check_bytes(bytes.len(), 2)?;
        let mut out = Vec::with_capacity(bytes.len() / 2);
        for chunk in bytes.as_chunks::<2>().0 {
            out.push(i16::from_le_bytes([chunk[0], chunk[1]]));
        }
        Ok(out.into_boxed_slice())
    }
}

impl CodecAdapter24 for PcmAdapter {
    fn kind(&self) -> CodecKind {
        CodecKind::Pcm
    }

    fn name(&self) -> &'static str {
        "Pcm-24"
    }

    fn encode_24(&mut self, pcm: &[i32]) -> Result<Box<[u8]>, CodecError> {
        if self.fmt != PcmFmt::I24 {
            return Err(CodecError::Unsupported(format!(
                "PCM adapter holds {:?}; encode_24 needs `new24`",
                self.fmt
            )));
        }
        self.check_align(pcm.len())?;
        // Canonical i24 wire form: right-aligned i32, low 3 bytes LE (the
        // high byte is stripped), matching `wdr_fakes` `encode_buffered`.
        let mut out = Vec::with_capacity(pcm.len() * 3);
        for &s in pcm {
            let c = s.clamp(-(1i32 << 23) + 1, (1i32 << 23) - 1);
            let b = c.to_le_bytes();
            out.extend_from_slice(&b[..3]);
        }
        Ok(out.into_boxed_slice())
    }

    fn decode_24(&mut self, bytes: &[u8]) -> Result<Box<[i32]>, CodecError> {
        if self.fmt != PcmFmt::I24 {
            return Err(CodecError::Unsupported(format!(
                "PCM adapter holds {:?}; decode_24 needs `new24`",
                self.fmt
            )));
        }
        self.check_bytes(bytes.len(), 3)?;
        let mut out = Vec::with_capacity(bytes.len() / 3);
        for chunk in bytes.as_chunks::<3>().0 {
            let v = chunk[0] as i32 | ((chunk[1] as i32) << 8) | ((chunk[2] as i32) << 16);
            // Sign-extend the 24-bit group (mirror `wdr_fakes::unpack_i24`).
            let v = if v & (1 << 23) != 0 {
                v | !((1 << 24) - 1)
            } else {
                v
            };
            out.push(v);
        }
        Ok(out.into_boxed_slice())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::frame_crc32;

    #[test]
    fn pcm_roundtrip_exact() {
        let mut a = PcmAdapter::new(2).unwrap();
        let pcm: Vec<i16> = (0..480 * 2).map(|i| (i as i16).wrapping_mul(37)).collect();
        let enc = a.encode(&pcm).unwrap();
        assert_eq!(enc.len(), pcm.len() * 2);
        let dec = a.decode(&enc).unwrap();
        assert_eq!(dec, pcm.into_boxed_slice());
        assert_eq!(frame_crc32(&enc), 0xE581_1990);
    }

    #[test]
    fn pcm_mono() {
        let mut a = PcmAdapter::new(1).unwrap();
        let pcm = vec![i16::MIN, i16::MAX, 0, -1, 1];
        let enc = a.encode(&pcm).unwrap();
        let dec = a.decode(&enc).unwrap();
        assert_eq!(dec, pcm.into_boxed_slice());
    }

    #[test]
    fn pcm_channel_alignment() {
        let mut a = PcmAdapter::new(2).unwrap();
        assert!(matches!(
            a.encode(&[0i16, 1, 2]),
            Err(CodecError::InvalidPcmLen { .. })
        ));
    }

    #[test]
    fn pcm_malformed_odd_bytes_errors() {
        let mut a = PcmAdapter::new(1).unwrap();
        assert!(a.decode(&[0u8, 1, 2]).is_err());
    }

    #[test]
    fn pcm_rejects_bad_channels() {
        assert!(matches!(
            PcmAdapter::new(3),
            Err(CodecError::Unsupported(_))
        ));
    }

    #[test]
    fn pcm_24bit_roundtrip_exact() {
        use crate::adapters::CodecAdapter24;
        let mut a = PcmAdapter::new24(2).unwrap();
        let pcm: Vec<i32> = vec![
            0,
            0x007F_FFFF,  // most positive 24-bit
            -0x007F_FFFF, // most negative *canonical* 24-bit
            0x0012_3456,
            -0x0012_3456,
            99,
            -99,
            1234,
        ];
        let enc = a.encode_24(&pcm).unwrap();
        assert_eq!(enc.len(), pcm.len() * 3, "3 bytes/sample on the wire");
        let dec = a.decode_24(&enc).unwrap();
        assert_eq!(dec, pcm.into_boxed_slice());

        // Canonical clamp policy mirrors `wdr_fakes`: the asymmetric minimum
        // -2^23 is excluded, clamped to -(2^23)+1.
        let clamped = a.encode_24(&[-0x0080_0000i32, -0x0080_0000]).unwrap();
        let dec2 = a.decode_24(&clamped).unwrap();
        assert_eq!(dec2, vec![-0x007F_FFFF, -0x007F_FFFF].into_boxed_slice());

        // i16 methods on a 24-bit adapter are typed errors, never silent
        // down-converts.
        let mut a16 = a;
        assert!(matches!(
            a16.encode(&[0i16]),
            Err(CodecError::Unsupported(_))
        ));
        assert!(matches!(
            a16.decode(&[0u8, 1]),
            Err(CodecError::Unsupported(_))
        ));
    }

    #[test]
    fn pcm_24bit_malformed_bytes_errors() {
        use crate::adapters::CodecAdapter24;
        let mut a = PcmAdapter::new24(1).unwrap();
        assert!(
            a.decode_24(&[0u8, 1, 2, 3]).is_err(),
            "4 bytes not a 3-multiple"
        );
        let mut a2 = PcmAdapter::new24(2).unwrap();
        assert!(matches!(
            a2.encode_24(&[0i32, 1, 2]),
            Err(CodecError::InvalidPcmLen { .. })
        ));
    }

    // ---- WS-E: raw IEEE-f32 / 32-bit-int passthrough ----

    #[test]
    fn pcm_f32_roundtrip_exact() {
        let mut a = PcmAdapter::new_f32(2).unwrap();
        let pcm: Vec<f32> = vec![0.0, -1.0, 0.5, 1.0, -0.25, 48_000.0, -32_768.0, 1.5e-10];
        let enc = a.encode_f32(&pcm).unwrap();
        assert_eq!(enc.len(), pcm.len() * 4, "4 bytes/sample on the wire");
        let dec = a.decode_f32(&enc).unwrap();
        assert_eq!(dec, pcm.into_boxed_slice(), "bit-exact f32 passthrough");
        // The other surfaces on a f32 adapter are typed errors, never reformats.
        assert!(matches!(a.encode(&[0i16]), Err(CodecError::Unsupported(_))));
        assert!(matches!(
            a.decode_24(&[0u8; 3]),
            Err(CodecError::Unsupported(_))
        ));
    }

    #[test]
    fn pcm_i32_roundtrip_exact() {
        let mut a = PcmAdapter::new_i32(1).unwrap();
        let pcm: Vec<i32> = vec![i32::MIN, i32::MAX, 0, -1, 1, 12_345_678];
        let enc = a.encode_i32(&pcm).unwrap();
        assert_eq!(enc.len(), pcm.len() * 4);
        let dec = a.decode_i32(&enc).unwrap();
        assert_eq!(dec, pcm.into_boxed_slice(), "bit-exact i32 passthrough");
        assert!(matches!(a.encode(&[0i16]), Err(CodecError::Unsupported(_))));
    }

    #[test]
    fn pcm_f32_i32_reject_alignment_and_malformed() {
        let mut a = PcmAdapter::new_f32(2).unwrap();
        assert!(a.encode_f32(&[0.0, 1.0, 2.0]).is_err());
        assert!(a.decode_f32(&[0u8; 9]).is_err(), "9 bytes not a 4-multiple");
        let mut b = PcmAdapter::new_i32(2).unwrap();
        assert!(b.encode_i32(&[1, 2, 3]).is_err());
        assert!(
            b.decode_i32(&[0u8; 12]).is_err(),
            "3 samples not divisible by 2 channels"
        );
    }
}
