//! Property tests (proptest) for the codec crate: PCM roundtrip exactness,
//! frame-size calculator invariants, and panic-freedom on arbitrary bytes
//! fed to decoders.

use proptest::prelude::*;
use wdr_codec::{
    max_frame_samples, CodecAdapter, FlacAdapter, FrameProfile, OpusAdapter, PcmAdapter,
};

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    /// PCM encode→decode is bit-exact for arbitrary lengths & values.
    #[test]
    fn pcm_roundtrip_exact_any_length(
        ch in 1u16..=2,
        samples in prop::collection::vec(any::<i16>(), 0..512usize),
    ) {
        let mut a = PcmAdapter::new(ch).unwrap();
        // Build a length the adapter accepts (multiple of ch) — drop tail.
        let usable = samples.len() - samples.len() % usize::from(ch);
        let buf = &samples[..usable];
        let enc = a.encode(buf).unwrap();
        assert_eq!(enc.len(), buf.len() * 2);
        let dec = a.decode(&enc).unwrap();
        assert_eq!(&dec[..], buf);
    }

    /// Malformed/arbitrary byte buffers must never panic decoders.
    #[test]
    fn arbitrary_bytes_never_panic_flac_decode(
        bytes in prop::collection::vec(any::<u8>(), 0..4000usize),
    ) {
        let mut a = FlacAdapter::new(48_000, 2, 16).unwrap();
        let _ = a.decode(&bytes); // must not panic
    }

    /// Arbitrary bytes must never panic the Opus decoder.
    #[test]
    fn arbitrary_bytes_never_panic_opus_decode(
        bytes in prop::collection::vec(any::<u8>(), 1..100usize),
    ) {
        let mut a = OpusAdapter::new(48_000, 2, FrameProfile::Ms20).unwrap();
        let _ = a.decode(&bytes);
    }

    /// Frame-size calculator: budget floor must hold (raw bound exact for PCM).
    #[test]
    fn frame_size_calc_is_exact_divided_floor(
        ch in 1u16..4,
        bytes_ps in 1u16..5,
        budget in 1usize..1_000_000,
    ) {
        let max = max_frame_samples(48_000, ch, bytes_ps, budget);
        let ch_u = usize::from(ch);
        let b_u = usize::from(bytes_ps);
        let exact = budget / (ch_u * b_u);
        assert_eq!(max, exact.min(wdr_codec::MAX_FRAME_SAMPLES_FALLBACK));
        // A frame of `max` raw samples fits budget; one more does not.
        if max < exact {
            // capped at fallback
            assert_eq!(max, 8192);
        } else {
            assert!(max * ch_u * b_u <= budget, "over budget");
        }
    }
}
