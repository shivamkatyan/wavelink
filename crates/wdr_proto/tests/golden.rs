//! Golden-vector verification: every stored blob must reproduce exactly from
//! the current wire schema (single source of truth, PROTOCOL_SPEC §Versioning).
//!
//! The vectors are materialised deterministically by `examples/gen_golden.rs`;
//! this test re-builds each vector and byte-compares against the stored blob,
//! so any unintentional wire change fails loudly.

use std::{fs, path::PathBuf};

use wdr_proto::*;

fn golden_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/golden")
}

fn read_manifest() -> String {
    fs::read_to_string(golden_dir().join(GOLDEN_MANIFEST)).expect("MANIFEST.md present")
}

/// Re-materialise every vector exactly as `examples/gen_golden.rs` does.
fn generate_all() -> Vec<(String, Vec<u8>)> {
    let mut v = Vec::new();
    let hd = |stream: u32, seq: u64, ts: u64| MsgHeader {
        msg_ver: PROTO_MAJOR,
        stream_id: stream,
        seq,
        media_ts: ts,
    };

    let mut emit = |name: &str, m: &ControlMessage| {
        let bytes = gen_golden(name, m);
        v.push((name.to_owned(), bytes));
    };

    emit(
        "msg-hello",
        &ControlMessage::Hello {
            header: hd(0, 1, 1000),
            device_name: "front-speaker".into(),
        },
    );
    emit(
        "msg-cap-req",
        &ControlMessage::CapabilityRequest {
            header: hd(1, 2, 2000),
        },
    );
    emit(
        "msg-cap-resp",
        &ControlMessage::CapabilityResponse {
            header: hd(1, 3, 3000),
            capability: Capability::default(),
        },
    );
    emit(
        "msg-session-desc",
        &ControlMessage::SessionDescriptor {
            header: hd(1, 4, 4000),
            descriptor: SessionDescriptor {
                session_id: 77,
                emitter: Capability::default(),
                receiver: Capability::default(),
                agreed: Capability::default(),
            },
        },
    );
    emit(
        "msg-pairing-start",
        &ControlMessage::PairingStart {
            header: hd(0, 5, 0),
            local_fingerprint: [9u8; 32],
        },
    );
    emit(
        "msg-sas-offer",
        &ControlMessage::SasOffer {
            header: hd(0, 6, 0),
            offer: SasOffer {
                sas: 123456,
                bound: true,
            },
        },
    );
    emit(
        "msg-sas-confirm",
        &ControlMessage::SasConfirmReject {
            header: hd(0, 7, 0),
            confirmed: true,
        },
    );
    emit(
        "msg-policy-ad",
        &ControlMessage::PolicyAdvertisement {
            header: hd(1, 8, 0),
            tier: PolicyTier::Pro,
            bits: PolicyBits {
                lossless: true,
                lossy: true,
                resample: true,
                reconnect: true,
            },
        },
    );
    emit(
        "msg-start",
        &ControlMessage::StartStream {
            header: hd(1, 9, 0),
            source_stream_id: 3,
        },
    );
    emit(
        "msg-stop",
        &ControlMessage::StopStream {
            header: hd(1, 10, 0),
            source_stream_id: 3,
        },
    );
    emit(
        "msg-pause",
        &ControlMessage::Pause {
            header: hd(1, 11, 0),
            source_stream_id: 3,
        },
    );
    emit(
        "msg-resume",
        &ControlMessage::Resume {
            header: hd(1, 12, 0),
            source_stream_id: 3,
        },
    );
    emit(
        "msg-feedback",
        &ControlMessage::Feedback {
            header: hd(1, 13, 0),
            feedback: Feedback {
                rtt_us: 12_000,
                jitter_us: 900,
                loss_pct: 120,
                reorder: 2,
                late_discard: 1,
                buffer_fill: 4800,
                underruns: 0,
                output_clock_estimate: -12,
                requested_adapt: 3,
            },
        },
    );
    emit(
        "msg-error",
        &ControlMessage::Error {
            header: hd(1, 14, 0),
            error: Error::protocol(ErrorCode::PolicyViolation),
        },
    );
    emit(
        "msg-keepalive",
        &ControlMessage::Keepalive {
            header: hd(1, 15, 0),
            nonce: 0xDEAD_BEEF,
        },
    );
    emit(
        "msg-pair-rejected",
        &ControlMessage::PairAttemptRejected {
            header: hd(0, 16, 0),
            reason: ErrorCode::SasMismatch,
        },
    );
    emit(
        "msg-revoke",
        &ControlMessage::RevokeRecord {
            header: hd(0, 17, 0),
            record: RevokeRecord {
                format_version: 1,
                record_id: [1u8; 16],
                issuer_pub: [2u8; 32],
                subject_pub: [3u8; 32],
                reason: RevokeReason::Lost,
                issued_at: 1_700_000_000,
                map_version: 4,
                prev_sha: Some([5u8; 32]),
                signature: vec8(64),
            },
        },
    );

    for (ri, repr) in [
        SampleRepr::I16,
        SampleRepr::F32,
        SampleRepr::I24Packed,
        SampleRepr::I32,
    ]
    .iter()
    .enumerate()
    {
        for (ci, layout) in [
            ChannelLayout::Mono,
            ChannelLayout::Stereo,
            ChannelLayout::Quad,
        ]
        .iter()
        .enumerate()
        {
            for (ki, codec) in [Codec::Opus, Codec::Flac, Codec::Pcm].iter().enumerate() {
                let rate = [48_000u32, 44_100, 96_000][ki % 3];
                let payload =
                    vec8(if *codec == Codec::Opus { 400 } else { 2100 } + (ri * 100 + ci * 33));
                let f = Frame::new_lossy(
                    (ri + 1) as u32,
                    u64::MAX - 3 + ri as u64,
                    u64::MAX - 3 + ci as u64,
                    *codec,
                    rate,
                    *repr,
                    *layout,
                    960,
                    FrameFlags {
                        burst_end: ri == 0,
                        frame_boundary: false,
                        reserved: 0,
                    },
                    payload,
                );
                let name = format!(
                    "frame-{}-{}-{}",
                    codec_name(*codec),
                    repr_name(*repr),
                    layout_name(*layout)
                );
                v.push((name.clone(), gen_golden(&name, &f)));
            }
        }
    }

    for (ci, layout) in [ChannelLayout::Stereo, ChannelLayout::Surround51]
        .iter()
        .enumerate()
    {
        let f = Frame::new_lossless(
            9 + ci as u32,
            u64::MAX - 20,
            u64::MAX - 30,
            Codec::Flac,
            48_000,
            SampleRepr::I24Packed,
            *layout,
            240,
            FrameFlags::default(),
            vec8(3000 + ci * 500),
        );
        let name = format!("frame-lossless-{}", layout_name(*layout));
        v.push((name.clone(), gen_golden(&name, &f)));
    }

    let f = Frame::new_lossy(
        1,
        u64::MAX - 1,
        u64::MAX,
        Codec::Opus,
        48_000,
        SampleRepr::I16,
        ChannelLayout::Stereo,
        960,
        FrameFlags::default(),
        vec8(64),
    );
    v.push((
        "frame-wrap-window".to_owned(),
        gen_golden("frame-wrap-window", &f),
    ));

    v
}

fn vec8(len: usize) -> Vec<u8> {
    (0..len).map(|i| (i as u8).wrapping_mul(31)).collect()
}

fn codec_name(c: Codec) -> &'static str {
    match c {
        Codec::Opus => "opus",
        Codec::Flac => "flac",
        Codec::Pcm => "pcm",
    }
}
fn repr_name(r: SampleRepr) -> &'static str {
    match r {
        SampleRepr::I16 => "i16",
        SampleRepr::F32 => "f32",
        SampleRepr::I24Packed => "i24p",
        SampleRepr::I32 => "i32",
    }
}
fn layout_name(l: ChannelLayout) -> &'static str {
    match l {
        ChannelLayout::Mono => "mono",
        ChannelLayout::Stereo => "stereo",
        ChannelLayout::Quad => "quad",
        ChannelLayout::Surround51 => "surround51",
        ChannelLayout::Surround71 => "surround71",
    }
}

#[test]
fn stored_golden_blobs_reproduce_exactly() {
    let dir = golden_dir();
    let manifest = read_manifest();
    let mut missing: Vec<String> = Vec::new();

    for (name, bytes) in generate_all() {
        let file = golden_file_name(&name, &bytes);
        let path = dir.join(&file);
        if !path.exists() {
            missing.push(format!(
                "{name}: expected blob {file} is not stored; re-run `cargo run -p wdr_proto --example gen_golden`"
            ));
            continue;
        }
        let stored = fs::read(&path).unwrap_or_else(|e| panic!("read {file}: {e}"));
        assert_eq!(
            stored, bytes,
            "golden vector `{name}` (blob {file}) diverged from current wire encoding"
        );
    }

    if !missing.is_empty() {
        panic!("missing golden blobs:\n{}", missing.join("\n"));
    }

    // The manifest must reference every stored blob (no orphans).
    let blobs = fs::read_dir(&dir)
        .expect("read golden dir")
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|f| f.ends_with(".bin"))
        .collect::<Vec<_>>();
    for b in &blobs {
        assert!(
            manifest.contains(b),
            "stored blob `{b}` not described in {GOLDEN_MANIFEST}"
        );
    }
}

#[test]
fn manifest_describes_all_generated_vectors() {
    let manifest = read_manifest();
    for (name, bytes) in generate_all() {
        let file = golden_file_name(&name, &bytes);
        assert!(
            manifest.contains(&file),
            "generated vector {name} ({file}) missing from manifest"
        );
    }
}

/// Every stored message type must round-trip (decode from blob → encode →
/// byte-identical) — the strongest continuity guarantee.
#[test]
fn control_vectors_roundtrip() {
    let bytes = gen_golden(
        "msg-hello",
        &ControlMessage::Hello {
            header: MsgHeader {
                msg_ver: 1,
                stream_id: 0,
                seq: 1,
                media_ts: 1000,
            },
            device_name: "front-speaker".into(),
        },
    );
    let decoded: ControlMessage = unpack(&bytes).unwrap();
    assert!(matches!(decoded, ControlMessage::Hello { .. }));
    assert_eq!(pack(&decoded).unwrap(), bytes);
}

#[test]
fn frame_vectors_roundtrip() {
    let f = Frame::new_lossless(
        9,
        u64::MAX - 20,
        u64::MAX - 30,
        Codec::Flac,
        48_000,
        SampleRepr::I24Packed,
        ChannelLayout::Stereo,
        240,
        FrameFlags::default(),
        vec8(3000),
    );
    let bytes = f.pack().unwrap();
    let blob = gen_golden("roundtrip-check", &f);
    assert_eq!(bytes, blob);
    let back = Frame::unpack(&blob).unwrap();
    assert_eq!(back, f);
    assert_eq!(back.pack().unwrap(), blob);
}
