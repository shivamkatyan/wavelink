//! Property tests (proptest):
//! 1. encode→decode round-trip for arbitrary valid messages/frames;
//! 2. truncation safety: any valid message truncated at each byte offset must
//!    not panic, and must either error or round-trip only when complete;
//! 3. oversized-payload rejection: payloads > MAX_FRAME_PAYLOAD are rejected.

use proptest::prelude::*;
use wdr_proto::*;

fn codec_strategy() -> impl Strategy<Value = Codec> {
    prop_oneof![Just(Codec::Opus), Just(Codec::Flac), Just(Codec::Pcm),]
}

fn repr_strategy() -> impl Strategy<Value = SampleRepr> {
    prop_oneof![
        Just(SampleRepr::I16),
        Just(SampleRepr::F32),
        Just(SampleRepr::I24Packed),
        Just(SampleRepr::I32),
    ]
}

fn layout_strategy() -> impl Strategy<Value = ChannelLayout> {
    prop_oneof![
        Just(ChannelLayout::Mono),
        Just(ChannelLayout::Stereo),
        Just(ChannelLayout::Quad),
        Just(ChannelLayout::Surround51),
        Just(ChannelLayout::Surround71),
    ]
}

fn header_strategy() -> impl Strategy<Value = MsgHeader> {
    (any::<u16>(), any::<u32>(), any::<u64>(), any::<u64>()).prop_map(
        |(msg_ver, stream_id, seq, media_ts)| MsgHeader {
            msg_ver,
            stream_id,
            seq,
            media_ts,
        },
    )
}

fn ability() -> Capability {
    Capability::default()
}

fn policy_bits_strategy() -> impl Strategy<Value = PolicyBits> {
    (any::<bool>(), any::<bool>(), any::<bool>(), any::<bool>()).prop_map(|(a, b, c, d)| {
        PolicyBits {
            lossless: a,
            lossy: b,
            resample: c,
            reconnect: d,
        }
    })
}

fn error_code_strategy() -> impl Strategy<Value = ErrorCode> {
    use ErrorCode::*;
    prop_oneof![
        Just(Malformed),
        Just(TooLarge),
        Just(VersionTooOld),
        Just(PatternMismatch),
        Just(RateLimited),
        Just(PendingHandshakeFull),
        Just(PairingTimeout),
        Just(PairAttemptRejected),
        Just(SasMismatch),
        Just(AuthenticationFailure),
        Just(PolicyViolation),
        Just(Revoked),
        Just(MissingResource),
        Just(Internal),
        Just(Reserved),
    ]
}

fn feedback_strategy() -> impl Strategy<Value = Feedback> {
    (
        any::<u32>(),
        any::<u32>(),
        any::<u16>(),
        any::<u16>(),
        any::<u16>(),
        any::<u32>(),
        any::<u32>(),
        any::<i32>(),
        any::<u8>(),
    )
        .prop_map(
            |(rtt, jitter, loss, reorder, late, fill, und, clock, adapt)| Feedback {
                rtt_us: rtt,
                jitter_us: jitter,
                loss_pct: loss % 1001,
                reorder,
                late_discard: late,
                buffer_fill: fill,
                underruns: und,
                output_clock_estimate: clock,
                requested_adapt: adapt,
            },
        )
}

fn revoke_strategy() -> impl Strategy<Value = RevokeRecord> {
    (
        any::<u64>(),
        prop_oneof![
            Just(RevokeReason::Sold),
            Just(RevokeReason::Lost),
            Just(RevokeReason::Leaked),
            Just(RevokeReason::Rekeyed),
            Just(RevokeReason::Admin),
        ],
        any::<bool>(),
    )
        .prop_map(|(map_version, reason, has_prev)| RevokeRecord {
            format_version: 1,
            record_id: [7u8; 16],
            issuer_pub: [2u8; 32],
            subject_pub: [3u8; 32],
            reason,
            issued_at: 1_700_000_000,
            map_version,
            prev_sha: if has_prev { Some([5u8; 32]) } else { None },
            signature: vec![0x42u8; 64],
        })
}

fn control_message_strategy() -> impl Strategy<Value = ControlMessage> {
    // Each variant takes its own fresh header sample; no strategy cloning.
    prop_oneof![
        header_strategy().prop_map(|h| ControlMessage::CapabilityRequest { header: h }),
        (header_strategy(),).prop_map(|(header,)| ControlMessage::SessionDescriptor {
            header,
            descriptor: SessionDescriptor { session_id: 77, emitter: ability(), receiver: ability(), agreed: ability() },
        }),
        (header_strategy(),).prop_map(|(header,)| ControlMessage::PairingStart { header, local_fingerprint: [9u8; 32] }),
        (header_strategy(),).prop_map(|(header,)| ControlMessage::PairAttemptRejected { header, reason: ErrorCode::SasMismatch }),
        (any::<u32>(), any::<u64>()).prop_flat_map(|(sid, nonce)| {
            prop_oneof![
                (header_strategy(),).prop_map(move |(header,)| ControlMessage::StartStream { header, source_stream_id: sid }),
                (header_strategy(),).prop_map(move |(header,)| ControlMessage::StopStream { header, source_stream_id: sid }),
                (header_strategy(),).prop_map(move |(header,)| ControlMessage::Pause { header, source_stream_id: sid }),
                (header_strategy(),).prop_map(move |(header,)| ControlMessage::Resume { header, source_stream_id: sid }),
                (header_strategy(),).prop_map(move |(header,)| ControlMessage::Keepalive { header, nonce }),
                header_strategy().prop_flat_map(move |header| {
                    sas_strategy(header, nonce)
                }),
            ]
        }),
        (header_strategy(), policy_bits_strategy()).prop_map(
            |(header, bits)| ControlMessage::PolicyAdvertisement { header, tier: PolicyTier::Pro, bits },
        ),
        (header_strategy(), error_code_strategy()).prop_map(
            |(header, code)| ControlMessage::Error { header, error: Error::protocol(code) },
        ),
        (header_strategy(), feedback_strategy()).prop_map(
            |(header, feedback)| ControlMessage::Feedback { header, feedback },
        ),
        (header_strategy(), revoke_strategy()).prop_map(
            |(header, record)| ControlMessage::RevokeRecord { header, record },
        ),
        (header_strategy(),).prop_map(|(header,)| ControlMessage::Hello { header, device_name: "s".into() }),
        (header_strategy(),).prop_map(|(header,)| ControlMessage::CapabilityResponse { header, capability: ability() }),
        (header_strategy(), any::<bool>()).prop_map(
            |(header, confirmed)| ControlMessage::SasConfirmReject { header, confirmed },
        ),
    ]
    .boxed()
}

fn sas_strategy(h: MsgHeader, _nonce: u64) -> impl Strategy<Value = ControlMessage> {
    (any::<u32>(), any::<bool>()).prop_map(move |(sas, bound)| ControlMessage::SasOffer {
        header: h,
        offer: SasOffer {
            sas: sas % 1_000_000,
            bound,
        },
    })
}

fn frame_strategy() -> impl Strategy<Value = Frame> {
    (
        codec_strategy(),
        repr_strategy(),
        layout_strategy(),
        any::<u32>(),
        any::<u64>(),
        any::<u64>(),
        any::<u32>(),
        any::<u32>(),
        prop::collection::vec(any::<u8>(), 0..4096),
        any::<bool>(),
        any::<bool>(),
    )
        .prop_map(
            |(codec, repr, layout, sid, seq, ts, rate, count, payload, be, fb)| {
                let flags = FrameFlags {
                    burst_end: be,
                    frame_boundary: fb,
                    reserved: 0,
                };
                if codec == Codec::Flac || codec == Codec::Pcm {
                    Frame::new_lossless(
                        sid, seq, ts, codec, rate, repr, layout, count, flags, payload,
                    )
                } else {
                    Frame::new_lossy(
                        sid, seq, ts, codec, rate, repr, layout, count, flags, payload,
                    )
                }
            },
        )
}

proptest! {
    #[test]
    fn control_roundtrip(msg in control_message_strategy()) {
        let bytes = pack(&msg).unwrap();
        let back: ControlMessage = unpack(&bytes).unwrap();
        prop_assert_eq!(msg, back);
    }

    #[test]
    fn frame_roundtrip(f in frame_strategy()) {
        let bytes = f.pack().unwrap();
        let back = Frame::unpack(&bytes).unwrap();
        prop_assert_eq!(f, back);
    }

    #[test]
    fn truncation_never_panics_control(bytes in prop::collection::vec(any::<u8>(), 0..512)) {
        for i in 0..bytes.len() {
            let _ = unpack::<ControlMessage>(&bytes[..i]);
        }
    }

    #[test]
    fn truncation_never_panics_frame(bytes in prop::collection::vec(any::<u8>(), 0..512)) {
        for i in 0..bytes.len() {
            let _ = Frame::unpack(&bytes[..i]);
        }
    }

    #[test]
    fn valid_control_truncated_at_each_offset_never_panics(seed in 0u64..0xFFFF_FFFF_FFFF, tail in prop::collection::vec(any::<u8>(), 0..16)) {
        let msg = ControlMessage::Keepalive {
            header: MsgHeader { msg_ver: PROTO_MAJOR, stream_id: 7, seq: seed, media_ts: seed.wrapping_mul(7) },
            nonce: tail.len() as u64,
        };
        let complete = pack(&msg).unwrap();
        for i in 0..complete.len() {
            let truncated = &complete[..i];
            if unpack::<ControlMessage>(truncated).is_ok() {
                prop_assert_eq!(truncated, complete.as_slice(), "only the complete message may round-trip");
            }
        }
    }

    #[test]
    fn oversized_payload_rejected(payload in prop::collection::vec(any::<u8>(), 4097..8000)) {
        let f = Frame::new_lossy(1, 0, 0, Codec::Opus, 48000, SampleRepr::I16, ChannelLayout::Stereo, 960, FrameFlags::default(), payload);
        prop_assert_eq!(f.pack(), Err(EncodeError::PayloadOverflow));
    }
}
