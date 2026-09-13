//! FLAC (lossless) codec adapter — encode via libFLAC (`flac-bound`), decode
//! via pure-Rust claxon. Small fixed blocks (~240 samples @48k), per-frame
//! CRC-32 on the raw frame bytes (ADR-005 / PROTOCOL_SPEC §Codec profiles).

use super::CodecAdapter;
use crate::error::CodecError;
use crate::size::{CodecKind, MAX_FRAME_PAYLOAD};

use flac_bound::FlacEncoderConfig;
use flac_bound::{FlacEncoder, WriteWrapper};

/// Default FLAC block size (inter-channel samples) — ~240 samples @48k (5 ms),
/// the ADR-005 "small fixed blocks" default.
pub const FLAC_BLOCK_DEFAULT: usize = 240;

/// FLAC adapter. Encodes a whole frame (multiple of the block size) in one
/// `encode()` call and returns the complete native-FLAC stream bytes for the
/// frame (into a byte sink). `decode()` parses the frame back with claxon and
/// returns exactly the source samples.
pub struct FlacAdapter {
    channels: u16,
    bits_per_sample: u16,
    sample_rate: u32,
    block: usize,
}

impl FlacAdapter {
    /// Create a FLAC adapter for `channels` × `bits_per_sample` at
    /// `sample_rate`. Only 16-bit is implemented for the decode side (claxon
    /// decodes to i32 samples; the 24-bit path is a future stub via
    /// [`crate::SampleRepr`], ADR-005 focuses 16/24-bit).
    pub fn new(sample_rate: u32, channels: u16, bits_per_sample: u16) -> Result<Self, CodecError> {
        if bits_per_sample != 16 {
            return Err(CodecError::Unsupported(format!(
                "FLAC adapter implements 16-bit only for now (ADR-005); requested {bits_per_sample}-bit"
            )));
        }
        if channels == 0 || channels > 2 {
            return Err(CodecError::Unsupported(format!(
                "FLAC adapter supports 1 or 2 channels, got {channels}"
            )));
        }
        if sample_rate == 0 {
            return Err(CodecError::Format("sample rate must be non-zero".into()));
        }
        Ok(Self {
            channels,
            bits_per_sample,
            sample_rate,
            block: FLAC_BLOCK_DEFAULT,
        })
    }

    /// Set an explicit block size (inter-channel frames). Must be > 0.
    pub fn with_block(mut self, block: usize) -> Self {
        if block > 0 {
            self.block = block;
        }
        self
    }

    /// The fixed block size in frames.
    #[must_use]
    pub const fn block(&self) -> usize {
        self.block
    }

    /// Encode the given PCM into a complete FLAC stream (native, non-OGG).
    /// `pcm` length must be a multiple of `channels`.
    fn encode_flac(&self, pcm: &[i32]) -> Result<Box<[u8]>, CodecError> {
        let channels = usize::from(self.channels);
        if pcm.is_empty() || !pcm.len().is_multiple_of(channels) {
            return Err(CodecError::InvalidPcmLen {
                len: pcm.len(),
                channels: self.channels,
            });
        }
        let frames = pcm.len() / channels;
        let mut sink: Vec<u8> = Vec::new();
        let mut wrapper = WriteWrapper(&mut sink);

        let config: FlacEncoderConfig = FlacEncoder::new()
            .ok_or_else(|| CodecError::Backend("FLAC__stream_encoder_new failed".into()))?;
        let mut enc = config
            .channels(self.channels as u32)
            .bits_per_sample(self.bits_per_sample as u32)
            .sample_rate(self.sample_rate)
            .blocksize(self.block as u32)
            .compression_level(5)
            .total_samples_estimate(frames as u64)
            .init_write(&mut wrapper)
            .map_err(|e| CodecError::Backend(format!("FLAC encoder init: {e:?}")))?;

        enc.process_interleaved(pcm, frames as u32).map_err(|()| {
            CodecError::Backend(format!(
                "FLAC encode process failed (state {:?})",
                enc.state()
            ))
        })?;

        enc.finish().map(|_| ()).map_err(|e| {
            CodecError::Backend(format!("FLAC encode finish failed: {:?}", e.state()))
        })?;
        Ok(sink.into_boxed_slice())
    }

    /// Decode a native FLAC stream into interleaved i32 samples via claxon.
    fn decode_flac(&self, bytes: &[u8]) -> Result<Box<[i32]>, CodecError> {
        let cur = std::io::Cursor::new(bytes);
        let mut reader = claxon::FlacReader::new(cur)
            .map_err(|e| CodecError::MalformedFrame(format!("claxon open: {e}")))?;
        let si = reader.streaminfo();
        if si.channels as u16 != self.channels || si.bits_per_sample as u16 != self.bits_per_sample
        {
            return Err(CodecError::MalformedFrame(format!(
                "FLAC frame streaminfo mismatch: got {}ch/{}bps, expected {}ch/{}bps",
                si.channels, si.bits_per_sample, self.channels, self.bits_per_sample
            )));
        }
        let mut out: Vec<i32> = Vec::new();
        for s in reader.samples() {
            let v = s.map_err(|e| CodecError::MalformedFrame(format!("claxon sample: {e}")))?;
            if !(-(1i32 << 16)..(1i32 << 16)).contains(&v) {
                return Err(CodecError::MalformedFrame(format!(
                    "FLAC frame sample {} out of 16-bit range",
                    v
                )));
            }
            out.push(v);
        }
        Ok(out.into_boxed_slice())
    }
}

impl CodecAdapter for FlacAdapter {
    fn kind(&self) -> CodecKind {
        CodecKind::Flac
    }

    fn name(&self) -> &'static str {
        "Flac"
    }

    fn encode(&mut self, pcm: &[i16]) -> Result<Box<[u8]>, CodecError> {
        if pcm.is_empty() {
            return Err(CodecError::InvalidPcmLen {
                len: 0,
                channels: self.channels,
            });
        }
        if !pcm.len().is_multiple_of(usize::from(self.channels)) {
            return Err(CodecError::InvalidPcmLen {
                len: pcm.len(),
                channels: self.channels,
            });
        }
        // i16 -> i32 (libFLAC wants i32).
        let wide: Vec<i32> = pcm.iter().map(|&s| i32::from(s)).collect();
        self.encode_flac(&wide)
    }

    fn decode(&mut self, bytes: &[u8]) -> Result<Box<[i16]>, CodecError> {
        if bytes.is_empty() || bytes.len() > MAX_FRAME_PAYLOAD {
            return Err(if bytes.is_empty() {
                CodecError::MalformedFrame("empty FLAC frame".into())
            } else {
                CodecError::PayloadTooLarge {
                    size: bytes.len(),
                    cap: MAX_FRAME_PAYLOAD,
                }
            });
        }
        let wide = self.decode_flac(bytes)?;
        Ok(wide
            .iter()
            .map(|&v| v as i16)
            .collect::<Vec<i16>>()
            .into_boxed_slice())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::CodecAdapter;
    use crate::frame_crc32;

    fn sine_i16(rate: u32, ch: u16, frames: usize, freq: f64, amp: i16) -> Vec<i16> {
        let c = usize::from(ch);
        (0..frames * c)
            .map(|i| {
                let f = i / c;
                let p = 2.0 * core::f64::consts::PI * freq * f as f64 / rate as f64;
                (p.sin() * f64::from(amp)) as i16
            })
            .collect()
    }

    #[test]
    fn flac_16bit_48k_stereo_roundtrip_exact() {
        let mut a = FlacAdapter::new(48_000, 2, 16).unwrap();
        let pcm = sine_i16(48_000, 2, 480, 997.0, 20000);
        let enc = a.encode(&pcm).unwrap();
        assert!(!enc.is_empty());
        let dec = a.decode(&enc).unwrap();
        assert_eq!(dec, pcm.into_boxed_slice(), "lossless must be exact");
        // CRC present & stable.
        let crc = frame_crc32(&enc);
        assert_eq!(crc, frame_crc32(&enc));
    }

    #[test]
    fn flac_mono_16bit_44_1k_exact() {
        let mut a = FlacAdapter::new(44_100, 1, 16).unwrap().with_block(2205);
        let pcm = sine_i16(44_100, 1, 2205, 440.0, 30000);
        let enc = a.encode(&pcm).unwrap();
        let dec = a.decode(&enc).unwrap();
        assert_eq!(dec, pcm.into_boxed_slice());
    }

    #[test]
    fn flac_noise_frame_fits_4kb() {
        // Incompressible noise, block 240 @48k, stereo 16-bit: must fit 4 KiB.
        let mut a = FlacAdapter::new(48_000, 2, 16).unwrap();
        let pcm: Vec<i16> = crate::size::noise_fixture(240, 2, 42);
        let enc = a.encode(&pcm).unwrap();
        assert!(
            enc.len() < MAX_FRAME_PAYLOAD,
            "FLAC noise frame {} bytes > 4 KiB",
            enc.len()
        );
    }

    #[test]
    fn flac_rejects_24bit_for_now() {
        assert!(matches!(
            FlacAdapter::new(48_000, 2, 24),
            Err(CodecError::Unsupported(_))
        ));
    }

    #[test]
    fn flac_malformed_decode_errors_not_panes() {
        let mut a = FlacAdapter::new(48_000, 2, 16).unwrap();
        for bad in [&[0u8][..], b"fLaC", &[0xffu8; 64]] {
            assert!(a.decode(bad).is_err(), "{bad:?} should err");
        }
    }
}
