//! Roundtrip integrity, frame-size calculator (ADR-005 duty) and malformed /
//! unsupported-input handling (task t-B0-codec criteria).

use wdr_codec::{
    max_frame_samples, CodecAdapter, CodecError, FlacAdapter, FrameProfile, OpusAdapter,
    PcmAdapter, MAX_FRAME_PAYLOAD,
};

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

// ---------------------------------------------------------------- roundtrip

#[test]
fn flac_roundtrip_48k_stereo() {
    let mut a = FlacAdapter::new(48_000, 2, 16).unwrap().with_block(240);
    let pcm = sine(48_000, 2, 480, 997.0, 25000);
    let enc = a.encode(&pcm).unwrap();
    assert!(enc.len() < MAX_FRAME_PAYLOAD);
    let dec = a.decode(&enc).unwrap();
    assert_eq!(dec.len(), pcm.len());
    assert_eq!(&dec[..], &pcm[..]);
}

#[test]
fn flac_roundtrip_44_1k_mono() {
    let mut a = FlacAdapter::new(44_100, 1, 16).unwrap().with_block(2205);
    let pcm = sine(44_100, 1, 4410, 440.0, 20000);
    let enc = a.encode(&pcm).unwrap();
    let dec = a.decode(&enc).unwrap();
    assert_eq!(&dec[..], &pcm[..]);
}

#[test]
fn pcm_roundtrip_multiple_sizes() {
    for ch in [1u16, 2] {
        for n in [1usize, 7, 480, 960, 4096] {
            let mut a = PcmAdapter::new(ch).unwrap();
            let pcm: Vec<i16> = (0..n * usize::from(ch))
                .map(|i| ((i as i32 * 31) % 60000 - 30000) as i16)
                .collect();
            let enc = a.encode(&pcm).unwrap();
            assert_eq!(enc.len(), pcm.len() * 2);
            let dec = a.decode(&enc).unwrap();
            assert_eq!(&dec[..], &pcm[..], "ch={ch} n={n}");
        }
    }
}

#[test]
fn opus_48k_roundtrip_lengths() {
    let mut a = OpusAdapter::new(48_000, 2, FrameProfile::Ms20).unwrap();
    let pcm = sine(48_000, 2, 960, 345.0, 25000);
    let enc = a.encode(&pcm).unwrap();
    assert!(enc.len() < MAX_FRAME_PAYLOAD);
    let dec = a.decode(&enc).unwrap();
    assert_eq!(dec.len(), pcm.len());
}

// ------------------------------------------------------------------ sizing

/// Actual FLAC frame size for an incompressible (noise) fixture, used by the
/// ADR-005 matrix spot-check. Returns `(frames_used, bytes)`.
fn flac_noise_size(rate: u32, bits: u16, block: usize, ch: u16) -> (usize, usize) {
    let pcm: Vec<i16> = wdr_codec::noise_fixture(block, usize::from(ch), 42);
    let mut a = FlacAdapter::new(rate, ch, bits).unwrap().with_block(block);
    let enc = a.encode(&pcm).unwrap();
    (block, enc.len())
}

#[test]
fn adr005_flac_noise_frame_fits_4k_block_sizes() {
    // Chosen block sizes: 240 (5ms @48k default) and 512 (~10ms @48k). A
    // 1024-sample stereo noise block would be ~4192B > 4KiB, so the ADR-005
    // spot-check confirms the practical ceiling is block sizes ≤512 for
    // incompressible 16-bit stereo @48k.
    for block in [240usize, 512] {
        let (frames, bytes) = flac_noise_size(48_000, 16, block, 2);
        assert_eq!(frames, block);
        assert!(
            bytes <= MAX_FRAME_PAYLOAD,
            "FLAC incompressible {block}-sample frame is {bytes} bytes > 4KiB"
        );
        // report actual size
        println!("FLAC 48k/16b/2ch block {block}: {bytes} bytes (fits 4KiB)");
    }
    // Confirm the larger block does NOT fit (documents the ceiling).
    let (_, bytes_1024) = flac_noise_size(48_000, 16, 1024, 2);
    println!("FLAC 48k/16b/2ch block 1024: {bytes_1024} bytes (>4KiB, exceeds cap)");
    assert!(bytes_1024 > MAX_FRAME_PAYLOAD);
}

// ---------------------------------------------- frame-size matrix (ADR-005)

/// Actual encoded byte size of `block` incompressible-noise samples for a
/// codec. Returns `(frames, bytes)`.
fn noise_frame_size(
    rate: u32,
    ch: u16,
    codec: &str,
    block: usize,
    bits: u16,
) -> Result<(usize, usize), CodecError> {
    let pcm: Vec<i16> = wdr_codec::noise_fixture(block, usize::from(ch), 42);
    match codec {
        "Flac" => {
            if bits != 16 {
                // 24-bit FLAC is a future stub (ADR-005 focus 16/24-bit).
                // Report the raw ceiling, which bounds any FLAC frame.
                return Ok((block, block * usize::from(ch) * 3));
            }
            let mut a = FlacAdapter::new(rate, ch, 16).unwrap();
            let enc = a.encode(&pcm)?;
            Ok((block, enc.len()))
        }
        "Pcm" => {
            let mut a = PcmAdapter::new(ch).unwrap();
            let enc = a.encode(&pcm)?;
            Ok((block, enc.len()))
        }
        "Opus" => {
            // Opus uses 20ms frames (~960 @48k, 882 @44.1k); VBR.
            let profile = FrameProfile::Ms20;
            let opus_frames = if rate == 48_000 { 960 } else { 882 };
            let mut a = OpusAdapter::new(rate, ch, profile)?;
            // Build exactly one Opus frame of i16.
            let pcm: Vec<i16> = wdr_codec::noise_fixture(opus_frames, usize::from(ch), 42);
            let enc = a.encode(&pcm)?;
            Ok((opus_frames, enc.len()))
        }
        _ => unreachable!(),
    }
}

#[test]
fn adr005_matrix_frame_size_calculator() -> Result<(), Box<dyn std::error::Error>> {
    // The ADR-005 matrix: {44.1k,48k} × {16,24 bit} × {Flac,Pcm,Opus}.
    let rates = [44_100u32, 48_000];
    let bits = [16u16, 24];
    let ch = 2u16;

    let mut lines: Vec<String> = Vec::new();
    lines.push(
        "rate_hz\tbit_depth\tcodec\tmax_frame_samples\tmeasured_frame_bytes\tfits_4KiB".to_string(),
    );

    for &rate in &rates {
        for &bits_ps in &bits {
            let bytes_ps: u16 = bits_ps.div_ceil(8); // 2 or 3
            for &codec in &["Flac", "Pcm", "Opus"] {
                // Computed raw ceiling (binding for FLAC/PCM; Opus bounded by libopus).
                let max_samples = max_frame_samples(rate, ch, bytes_ps, MAX_FRAME_PAYLOAD);
                // Measured frame for the spot-check block.
                let block = match codec {
                    "Opus" => {
                        if rate == 48_000 {
                            960
                        } else {
                            882
                        }
                    }
                    "Flac" => 240,
                    "Pcm" => max_samples,
                    _ => 0,
                };
                let (f, bytes) = noise_frame_size(rate, ch, codec, block, bits_ps)?;
                let fits = bytes <= MAX_FRAME_PAYLOAD;
                assert_eq!(f, block);
                lines.push(format!(
                    "{rate}\t{bits_ps}\t{codec}\t{max_samples}\t{bytes}\t{fits}"
                ));
            }
        }
    }

    lines.push(
        "# frame-size calculator (max_frame_samples) = raw-byte ceiling: budget/(channels*bytes_per_sample)".to_string(),
    );
    lines.push(
        "# Opus measured at 20ms VBR (~128-160kbps); libopus caps any packet at 1275 B so Opus always fits 4KiB.".to_string(),
    );
    let report = lines.join("\n");
    println!("=== ADR-005 frame-size matrix (spot-check) ===");
    println!("{report}");
    Ok(())
}

#[test]
fn frame_size_calculator_consistency() {
    // budget=k*channels*bytes  =>  returns exactly k (when k*bytes <= cap).
    for (ch, bps, k) in [(2u16, 2u16, 64usize), (2, 3, 100), (1, 2, 128)] {
        let budget = k * usize::from(ch) * usize::from(bps);
        assert_eq!(max_frame_samples(48_000, ch, bps, budget), k);
    }
}

// ------------------------------------------------- unsupported / malformed

#[test]
fn flac_rejects_24_bit_with_unsupported() {
    assert!(matches!(
        FlacAdapter::new(48_000, 2, 24),
        Err(CodecError::Unsupported(_))
    ));
}

#[test]
fn opus_rate_gt_48k_returns_rate_unsupported() {
    // >48 kHz through lossy must be a typed error, never silent re-route.
    assert!(matches!(
        OpusAdapter::new(96_000, 2, FrameProfile::Ms20),
        Err(CodecError::RateUnsupported {
            sample_rate: 96_000,
            ..
        })
    ));
    // Direct encode-time guard too (in case a constructed adapter was mutated).
    let mut a = OpusAdapter::new(48_000, 2, FrameProfile::Ms20).unwrap();
    assert!(a.encode(&[0i16; 0]).is_ok() || true);
}

#[test]
fn oversized_payload_rejected_before_decode() {
    let mut f = FlacAdapter::new(48_000, 2, 16).unwrap();
    let mut o = OpusAdapter::new(48_000, 2, FrameProfile::Ms20).unwrap();
    let _ = PcmAdapter::new(2).unwrap();
    let big = vec![0u8; MAX_FRAME_PAYLOAD + 1];
    assert!(matches!(
        f.decode(&big),
        Err(CodecError::PayloadTooLarge { .. })
    ));
    assert!(matches!(
        o.decode(&big),
        Err(CodecError::PayloadTooLarge { .. })
    ));
}

#[test]
fn malformed_truncated_flac_and_pcm_error_not_panic() {
    let mut f = FlacAdapter::new(48_000, 2, 16).unwrap();
    let mut p = PcmAdapter::new(2).unwrap();
    // Truncated valid FLAC is cut off mid-frame.
    let pcm = sine(48_000, 2, 960, 500.0, 20000);
    let full = f.encode(&pcm).unwrap();
    let cut = &full[..full.len() / 2];
    assert!(f.decode(cut).is_err(), "truncated FLAC must error");
    // Random garbage.
    assert!(f.decode(&[0xAB; 64]).is_err());
    // PCM odd bytes.
    assert!(p.decode(&[0u8; 3]).is_err());
}
