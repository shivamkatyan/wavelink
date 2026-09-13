//! Raw PCM (lossless) codec adapter — bit-perfect passthrough of interleaved
//! little-endian i16 (the lossless baseline) **and**, via [`CodecAdapter24`],
//! 24-bit packed (3 bytes/sample, canonical right-aligned i32) —
//! ADR-005 16/24-bit. Frame bytes carry a per-frame CRC-32 for integrity
//! (PROTOCOL_SPEC §Codec profiles).

use super::{CodecAdapter, CodecAdapter24};
use crate::error::CodecError;
use crate::size::CodecKind;

/// Raw-PCM adapter. The 16-bit surface (i16) is bit-perfect passthrough; the
/// 24-bit surface (right-aligned i32, low-3-bytes LE) is added via
/// [`PcmAdapter::new24`] + [`CodecAdapter24`].
pub struct PcmAdapter {
    channels: u16,
    bits_per_sample: u16,
}

impl PcmAdapter {
    /// Create a 16-bit raw-PCM adapter (the lossless baseline). `channels`
    /// must be 1 or 2 (matches the adapter surface for parity with the
    /// FLAC/Opus adapters).
    pub fn new(channels: u16) -> Result<Self, CodecError> {
        Self::with_bits(channels, 16)
    }

    /// Create a 24-bit raw-PCM adapter (ADR-005 follow-up). Its i32 surface
    /// is [`CodecAdapter24`]; the i16 methods return a typed
    /// [`CodecError::Unsupported`] rather than silently down-converting.
    pub fn new24(channels: u16) -> Result<Self, CodecError> {
        Self::with_bits(channels, 24)
    }

    fn with_bits(channels: u16, bits_per_sample: u16) -> Result<Self, CodecError> {
        if channels == 0 || channels > 2 {
            return Err(CodecError::Unsupported(format!(
                "PCM adapter supports 1 or 2 channels, got {channels}"
            )));
        }
        if bits_per_sample != 16 && bits_per_sample != 24 {
            return Err(CodecError::Unsupported(format!(
                "PCM adapter implements 16/24-bit only (ADR-005); requested {bits_per_sample}-bit"
            )));
        }
        Ok(Self {
            channels,
            bits_per_sample,
        })
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
        if self.bits_per_sample != 16 {
            return Err(CodecError::Unsupported(format!(
                "constructing a {}-bit PCM adapter; i16 encode needs a 16-bit adapter (use `CodecAdapter24` for 24-bit)",
                self.bits_per_sample
            )));
        }
        if !pcm.len().is_multiple_of(usize::from(self.channels)) {
            return Err(CodecError::InvalidPcmLen {
                len: pcm.len(),
                channels: self.channels,
            });
        }
        let n = pcm.len();
        let mut out = Vec::with_capacity(n * 2);
        for &s in pcm {
            out.extend_from_slice(&s.to_le_bytes());
        }
        Ok(out.into_boxed_slice())
    }

    fn decode(&mut self, bytes: &[u8]) -> Result<Box<[i16]>, CodecError> {
        if self.bits_per_sample != 16 {
            return Err(CodecError::Unsupported(format!(
                "constructing a {}-bit PCM adapter; i16 decode needs a 16-bit adapter (use `CodecAdapter24` for 24-bit)",
                self.bits_per_sample
            )));
        }
        if !bytes.len().is_multiple_of(2) {
            return Err(CodecError::MalformedFrame(format!(
                "PCM byte length {} is not even (2 bytes/sample)",
                bytes.len()
            )));
        }
        let ch = usize::from(self.channels);
        if !(bytes.len() / 2).is_multiple_of(ch) {
            return Err(CodecError::MalformedFrame(format!(
                "PCM sample count {} not divisible by {ch} channels",
                bytes.len() / 2
            )));
        }
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
        if self.bits_per_sample != 24 {
            return Err(CodecError::Unsupported(format!(
                "constructing a {}-bit PCM adapter; encode_24 needs a 24-bit adapter (use `new24`)",
                self.bits_per_sample
            )));
        }
        if !pcm.len().is_multiple_of(usize::from(self.channels)) {
            return Err(CodecError::InvalidPcmLen {
                len: pcm.len(),
                channels: self.channels,
            });
        }
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
        if self.bits_per_sample != 24 {
            return Err(CodecError::Unsupported(format!(
                "constructing a {}-bit PCM adapter; decode_24 needs a 24-bit adapter (use `new24`)",
                self.bits_per_sample
            )));
        }
        if !bytes.len().is_multiple_of(3) {
            return Err(CodecError::MalformedFrame(format!(
                "PCM-24 byte length {} is not a multiple of 3 (3 bytes/sample)",
                bytes.len()
            )));
        }
        let ch = usize::from(self.channels);
        if !(bytes.len() / 3).is_multiple_of(ch) {
            return Err(CodecError::MalformedFrame(format!(
                "PCM-24 sample count {} not divisible by {ch} channels",
                bytes.len() / 3
            )));
        }
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
}
