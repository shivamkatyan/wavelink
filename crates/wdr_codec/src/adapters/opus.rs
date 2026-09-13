//! Opus (lossy) codec adapter — wraps libopus (vendored). 44.1/48 kHz,
//! mono/stereo, 20 ms / 10 ms frames, VBR (ADR-004).

use super::CodecAdapter;
use crate::error::CodecError;
use crate::size::{CodecKind, MAX_FRAME_PAYLOAD};

use opus::{Application, Bitrate as OpusBitrate, Channels as OpusChannels};
use rubato::audioadapter_buffers::direct::SequentialSliceOfVecs;
use rubato::{Fft, FixedSync, Resampler};

/// Opus frame profiles (ADR-004).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FrameProfile {
    /// 20 ms frames (default). 960 samples/channel @48k (882 @44.1k).
    Ms20,
    /// 10 ms low-latency profile. 480 samples/channel @48k (441 @44.1k).
    Ms10,
}

impl FrameProfile {
    /// Samples per channel for a 20/10 ms frame at the Opus native 48 kHz.
    const fn frame_len_48k(self) -> usize {
        match self {
            FrameProfile::Ms20 => 960,
            FrameProfile::Ms10 => 480,
        }
    }

    /// Samples per channel at 44.1 kHz (before upsampling to 48k).
    const fn frame_len_44_1k(self) -> usize {
        match self {
            FrameProfile::Ms20 => 882,
            FrameProfile::Ms10 => 441,
        }
    }
}

/// The native Opus sampling rates (RFC 6716 §2.1.1).
const OPUS_NATIVE_RATE: u32 = 48_000;

/// Opus lossy adapter. Encoder runs at 48 kHz (libopus native); a 44.1 kHz
/// source is resampled to 48 kHz on encode and back to 44.1 kHz on decode.
pub struct OpusAdapter {
    input_rate: u32,
    channels: u16,
    profile: FrameProfile,
    frame_len: usize,
    encoder: opus::Encoder,
    decoder: opus::Decoder,
    /// Present when the input rate is 44.1k: 44.1k → 48k.
    upsampler: Option<Fft<f32>>,
    /// Present when the input rate is 44.1k: 48k → 44.1k.
    downsampler: Option<Fft<f32>>,
}

/// De-interleave i16 PCM into per-channel f32 vectors (used by the resampler).
fn deinterleave_f32(pcm: &[i16], channels: usize) -> Vec<Vec<f32>> {
    let frames = pcm.len() / channels;
    let mut by_ch = vec![vec![0f32; frames]; channels];
    for (i, &s) in pcm.iter().enumerate() {
        by_ch[i % channels][i / channels] = f32::from(s) / 32768.0;
    }
    by_ch
}

/// Resample `frames` interleaved i16 frames through an FFT resampler.
fn resample_interleaved(
    resampler: &mut Fft<f32>,
    pcm: &[i16],
    channels: usize,
    frames: usize,
) -> Result<Vec<i16>, CodecError> {
    let mut by_ch = deinterleave_f32(pcm, channels);
    let inp = SequentialSliceOfVecs::new(&by_ch, channels, frames)
        .map_err(|_| CodecError::Backend("rubato: input adapter size".into()))?;
    let out = resampler
        .process(&inp, None)
        .map_err(|e| CodecError::Backend(format!("rubato process: {e:?}")))?;
    let chans: Vec<f32> = out.take_data();
    let _ = &mut by_ch;
    // Interleave the owned f32 buffer back to i16.
    let total = chans.len();
    let frames_out = total / channels;
    let mut i16s = Vec::with_capacity(total);
    for f in 0..frames_out {
        for c in 0..channels {
            let clamped = chans[f * channels + c].clamp(-1.0, 1.0);
            i16s.push((clamped * 32767.0) as i16);
        }
    }
    Ok(i16s)
}

impl OpusAdapter {
    /// Create an Opus adapter.
    ///
    /// `sample_rate` must be 44_100 or 48_000 (ADR-004). Rates >48 kHz return
    /// [`CodecError::RateUnsupported`]; other unsupported rates return
    /// `Unsupported`. `channels` must be 1 (mono) or 2 (stereo).
    pub fn new(sample_rate: u32, channels: u16, profile: FrameProfile) -> Result<Self, CodecError> {
        if sample_rate > 48_000 {
            return Err(CodecError::RateUnsupported {
                sample_rate,
                codec: "Opus".to_string(),
            });
        }
        if sample_rate != 44_100 && sample_rate != 48_000 {
            return Err(CodecError::Unsupported(format!(
                "Opus adapter supports 44.1k/48k only, got {} Hz",
                sample_rate
            )));
        }
        let opus_channels = match channels {
            1 => OpusChannels::Mono,
            2 => OpusChannels::Stereo,
            _ => {
                return Err(CodecError::Unsupported(format!(
                    "Opus adapter supports mono/stereo only, got {channels} channels"
                )));
            }
        };

        let frame_len = profile.frame_len_48k();
        let mut encoder =
            opus::Encoder::new(OPUS_NATIVE_RATE, opus_channels, Application::Audio)
                .map_err(|e| CodecError::Backend(format!("opus_encoder_create: {e:?}")))?;
        let target = if channels == 2 { 160_000 } else { 80_000 };
        encoder
            .set_bitrate(OpusBitrate::Bits(target))
            .map_err(|e| CodecError::Backend(format!("set_bitrate: {e:?}")))?;
        encoder
            .set_vbr(true)
            .map_err(|e| CodecError::Backend(format!("set_vbr: {e:?}")))?;
        // Bounded CPU on low-end phones (ADR-004).
        encoder
            .set_complexity(5)
            .map_err(|e| CodecError::Backend(format!("set_complexity: {e:?}")))?;

        let decoder = opus::Decoder::new(OPUS_NATIVE_RATE, opus_channels)
            .map_err(|e| CodecError::Backend(format!("opus_decoder_create: {e:?}")))?;

        let channels_usize = usize::from(channels);
        let (upsampler, downsampler) = if sample_rate == 44_100 {
            let up = Fft::<f32>::new(
                44_100,
                OPUS_NATIVE_RATE as usize,
                profile.frame_len_44_1k(),
                channels_usize,
                FixedSync::Input,
            )
            .map_err(|e| CodecError::Backend(format!("rubato 44.1→48 build: {e:?}")))?;
            // Decode gives 960 frames @48k; down to 882 @44.1k.
            let down = Fft::<f32>::new(
                OPUS_NATIVE_RATE as usize,
                44_100,
                frame_len,
                channels_usize,
                FixedSync::Input,
            )
            .map_err(|e| CodecError::Backend(format!("rubato 48→44.1 build: {e:?}")))?;
            (Some(up), Some(down))
        } else {
            (None, None)
        };

        Ok(Self {
            input_rate: sample_rate,
            channels,
            profile,
            frame_len,
            encoder,
            decoder,
            upsampler,
            downsampler,
        })
    }

    /// The natural (pre-adapter) input sample rate.
    #[must_use]
    pub const fn input_sample_rate(&self) -> u32 {
        self.input_rate
    }

    /// Frames per channel per Opus frame (at the Opus native 48 kHz).
    #[must_use]
    pub const fn frame_len(&self) -> usize {
        self.frame_len
    }

    /// How many i16 samples per channel a caller must supply per `encode()`.
    #[must_use]
    pub const fn input_frame_len(&self) -> usize {
        if self.input_rate == 44_100 {
            self.profile.frame_len_44_1k()
        } else {
            self.profile.frame_len_48k()
        }
    }

    /// The resampler lead-in (in frames) the 44.1k path inserts before
    /// steady-state output; 0 when the input rate is already 48k.
    #[must_use]
    pub fn downsampler_delay(&self) -> Option<usize> {
        self.downsampler.as_ref().map(|d| d.output_delay())
    }
}

impl CodecAdapter for OpusAdapter {
    fn kind(&self) -> CodecKind {
        CodecKind::Opus
    }

    fn name(&self) -> &'static str {
        "Opus"
    }

    fn encode(&mut self, pcm: &[i16]) -> Result<Box<[u8]>, CodecError> {
        if self.input_rate > 48_000 {
            return Err(CodecError::RateUnsupported {
                sample_rate: self.input_rate,
                codec: self.name().to_string(),
            });
        }
        let ch = usize::from(self.channels);
        if !pcm.len().is_multiple_of(ch) {
            return Err(CodecError::InvalidPcmLen {
                len: pcm.len(),
                channels: self.channels,
            });
        }
        let frames = pcm.len() / ch;
        let expected = self.input_frame_len();
        if frames != expected {
            return Err(CodecError::InvalidFrameSize {
                expected: format!("{expected} samples/ch"),
                got: frames,
            });
        }

        let to_encode: Vec<i16> = match &mut self.upsampler {
            Some(up) => resample_interleaved(up, pcm, ch, frames)?,
            None => pcm.to_vec(),
        };

        let mut out = vec![0u8; 4000];
        let n = self
            .encoder
            .encode(&to_encode, &mut out)
            .map_err(|e| CodecError::Backend(format!("opus_encode: {e:?}")))?;
        out.truncate(n);
        Ok(out.into_boxed_slice())
    }

    fn decode(&mut self, bytes: &[u8]) -> Result<Box<[i16]>, CodecError> {
        if bytes.len() > MAX_FRAME_PAYLOAD {
            return Err(CodecError::PayloadTooLarge {
                size: bytes.len(),
                cap: MAX_FRAME_PAYLOAD,
            });
        }
        let ch = usize::from(self.channels);
        let mut out = vec![0i16; self.frame_len * ch];
        let n = self
            .decoder
            .decode(bytes, &mut out, false)
            .map_err(|e| CodecError::MalformedFrame(format!("opus_decode: {e:?}")))?;
        out.truncate(n * ch);

        if let Some(down) = &mut self.downsampler {
            Ok(resample_interleaved(down, &out, ch, n)?.into_boxed_slice())
        } else {
            Ok(out.into_boxed_slice())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::CodecAdapter;

    fn sine(rate: u32, channels: u16, frames: usize, freq: f64, amp: i16) -> Vec<i16> {
        let ch = usize::from(channels);
        (0..frames * ch)
            .map(|i| {
                let f = i / ch;
                let phase = 2.0 * core::f64::consts::PI * freq * f as f64 / rate as f64;
                (phase.sin() * f64::from(amp)) as i16
            })
            .collect()
    }

    #[test]
    fn opus_44_1k_20ms_roundtrip() {
        let mut a = OpusAdapter::new(44_100, 2, FrameProfile::Ms20).unwrap();
        let pcm = sine(44_100, 2, 882, 997.0, 20000);
        let enc = a.encode(&pcm).unwrap();
        assert!(!enc.is_empty());
        let dec = a.decode(&enc).unwrap();
        // 44.1k decode returns the natural 882-frame block.
        assert_eq!(dec.len(), 882 * 2);
        // Lossy: not exact. The 44.1k<->48k resampler inserts a fixed lead-in
        // (~147 frames); measure the steady-state window after that transient.
        let start = (a.downsampler_delay().unwrap_or(0) + 50) * 2;
        let win = &dec[start.min(dec.len())..];
        if !win.is_empty() {
            let rms: f64 = win
                .iter()
                .map(|&s| f64::from(s) / 32768.0)
                .map(|v| v * v)
                .sum::<f64>()
                / win.len() as f64;
            // Input sine RMS ≈ 0.43; after double resample + lossy Opus bound it.
            assert!(
                rms > 0.05 && rms < 0.6,
                "steady-state rms={rms} out of range"
            );
        }
    }

    #[test]
    fn opus_48k_10ms_roundtrip() {
        let mut a = OpusAdapter::new(48_000, 2, FrameProfile::Ms10).unwrap();
        let pcm = sine(48_000, 2, 480, 1234.0, 30000);
        let enc = a.encode(&pcm).unwrap();
        assert!(!enc.is_empty() && enc.len() < 4096);
        let dec = a.decode(&enc).unwrap();
        assert_eq!(dec.len(), 480 * 2);
    }

    #[test]
    fn opus_mono() {
        let mut a = OpusAdapter::new(48_000, 1, FrameProfile::Ms20).unwrap();
        let pcm = sine(48_000, 1, 960, 440.0, 20000);
        let enc = a.encode(&pcm).unwrap();
        let dec = a.decode(&enc).unwrap();
        assert_eq!(dec.len(), 960);
    }

    #[test]
    fn opus_rejects_over_48k() {
        assert!(matches!(
            OpusAdapter::new(88_200, 2, FrameProfile::Ms20),
            Err(CodecError::RateUnsupported {
                sample_rate: 88_200,
                ..
            })
        ));
    }

    #[test]
    fn opus_rejects_unsupported_rates() {
        assert!(matches!(
            OpusAdapter::new(32_000, 2, FrameProfile::Ms20),
            Err(CodecError::Unsupported(_))
        ));
        assert!(matches!(
            OpusAdapter::new(96_000, 2, FrameProfile::Ms20),
            Err(CodecError::RateUnsupported {
                sample_rate: 96_000,
                ..
            })
        ));
    }

    #[test]
    fn opus_rejects_bad_channels() {
        assert!(matches!(
            OpusAdapter::new(48_000, 3, FrameProfile::Ms20),
            Err(CodecError::Unsupported(_))
        ));
    }

    #[test]
    fn opus_rejects_wrong_frame_size() {
        let mut a = OpusAdapter::new(48_000, 2, FrameProfile::Ms20).unwrap();
        let pcm = vec![0i16; 479 * 2];
        assert!(matches!(
            a.encode(&pcm),
            Err(CodecError::InvalidFrameSize { .. })
        ));
    }

    #[test]
    fn opus_malformed_decode_returns_err_not_panic() {
        let mut a = OpusAdapter::new(48_000, 2, FrameProfile::Ms20).unwrap();
        // Garbage / truncated packets. (An empty packet is valid PLC, not an error.)
        for bad in [&[0xffu8; 10] as &[u8], b"not-opus-data", &[0u8; 4095]] {
            let r = a.decode(bad);
            assert!(r.is_err(), "should error on {bad:?}");
        }
    }

    #[test]
    fn opus_empty_packet_is_plc_not_error() {
        let mut a = OpusAdapter::new(48_000, 2, FrameProfile::Ms20).unwrap();
        let dec = a.decode(&[]).unwrap();
        // Packet-loss concealment returns a full frame (quiet, not silent).
        assert_eq!(dec.len(), 960 * 2);
    }

    #[test]
    fn opus_decode_encode_deterministic_across_fresh_instances() {
        // libopus has no per-instance seed; determinism means two freshly-built,
        // identically-configured adapters produce identical bytes and identical
        // decoded output for the same input.
        let pcm = sine(48_000, 2, 960, 345.0, 10000);

        fn encode_decode(rate: u32, pcm: &[i16]) -> (Box<[u8]>, Box<[i16]>) {
            let mut a = OpusAdapter::new(rate, 2, FrameProfile::Ms20).unwrap();
            let e = a.encode(pcm).unwrap();
            let d = a.decode(&e).unwrap();
            (e, d)
        }

        let (e1, d1) = encode_decode(48_000, &pcm);
        let (e2, d2) = encode_decode(48_000, &pcm);
        assert_eq!(e1, e2, "encoded bytes must be deterministic");
        assert_eq!(d1, d2, "decoded output must be deterministic");
    }
}
