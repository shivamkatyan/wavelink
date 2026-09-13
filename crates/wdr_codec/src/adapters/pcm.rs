//! Raw PCM (lossless) codec adapter — bit-perfect passthrough of interleaved
//! little-endian i16. This is the lossless baseline (ADR-005). Frame bytes
//! carry a per-frame CRC-32 for integrity (PROTOCOL_SPEC §Codec profiles).

use super::CodecAdapter;
use crate::error::CodecError;
use crate::size::CodecKind;

/// Raw-PCM adapter (i16 interleaved). `encode` returns the exact little-endian
/// bytes of the input; `decode` reproduces the input bit-exactly.
pub struct PcmAdapter {
    channels: u16,
}

impl PcmAdapter {
    /// Create a raw-PCM adapter. `channels` must be 1 or 2 (matches the
    /// adapter surface for parity with the FLAC/Opus adapters).
    pub fn new(channels: u16) -> Result<Self, CodecError> {
        if channels == 0 || channels > 2 {
            return Err(CodecError::Unsupported(format!(
                "PCM adapter supports 1 or 2 channels, got {channels}"
            )));
        }
        Ok(Self { channels })
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
}
