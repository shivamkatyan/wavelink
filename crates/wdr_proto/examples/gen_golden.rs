//! Regenerate the golden vector blobs + manifest under `tests/golden/`.
//!
//! Run from the crate root: `cargo run -p wdr_proto --example gen_golden`.
//! Deterministic: the same source enum produces identical blobs every run, so
//! the stored `.bin` files are stable across machines and CI runs.

use std::fs;

use wdr_proto::*;

fn vec8(len: usize) -> Vec<u8> {
    (0..len).map(|i| (i as u8).wrapping_mul(31)).collect()
}

fn main() {
    let out_dir = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/golden");
    fs::create_dir_all(out_dir).expect("create golden dir");

    let hd = |stream: u32, seq: u64, ts: u64| MsgHeader {
        msg_ver: PROTO_MAJOR,
        stream_id: stream,
        seq,
        media_ts: ts,
    };

    // ---- Control message vectors (one per variant) ----------------------
    let mut vectors: Vec<(String, Vec<u8>)> = Vec::new();
    let mut emit = |name: &str, msg: &ControlMessage| {
        let bytes = gen_golden(name, msg);
        vectors.push((name.to_owned(), bytes));
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

    // ---- Frame vectors ---------------------------------------------------
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
                let bytes = gen_golden(&name, &f);
                vectors.push((name, bytes));
            }
        }
    }

    // Lossless frames (CRC32).
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
        let bytes = gen_golden(&name, &f);
        vectors.push((name, bytes));
    }

    // Wrap-window guards: seq and media_ts near u64::MAX.
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
    let name = "frame-wrap-window".to_owned();
    let bytes = gen_golden(&name, &f);
    vectors.push((name, bytes));

    // ---- Write blobs + manifest -----------------------------------------
    let mut manifest = String::new();
    manifest.push_str("# WDR proto — Golden vectors\n\n");
    manifest.push_str(&format!(
        "Version: {GOLDEN_VERSION}\nGenerated: deterministic (postcard canonical)\n\n"
    ));
    for (name, bytes) in &vectors {
        let file = golden_file_name(name, bytes);
        let hex = bytes.iter().map(|b| format!("{b:02x}")).collect::<String>();
        fs::write(format!("{out_dir}/{file}"), bytes).expect("write golden blob");
        manifest.push_str(&format!(
            "- `{file}` — name=`{name}`, bytes={}, len={}\n",
            hex,
            bytes.len()
        ));
    }
    fs::write(format!("{out_dir}/{GOLDEN_MANIFEST}"), manifest).expect("write manifest");
    println!("wrote {} vectors under {out_dir}", vectors.len());
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
