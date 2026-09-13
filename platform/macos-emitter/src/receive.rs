//! `--receive` driver: the macOS desktop **receiver** role (WS3).
//!
//! Mirrors `--stream`: headless, newline-delimited-JSON on stdout
//! (`{"ev":"start"|"complete"|"fatal"}`), driven synchronously from a worker
//! thread. It listens for **one** emitter connection on the `wdr_refsim`
//! `QuicRenderReceiver` seam (the same `receiver_server` lane `ref_receiver`
//! uses), decodes the media plane and renders into a selectable `RenderSink`:
//!
//! * `--sink null`  — the null render device (canonical blake3 hash + counts).
//!   Exit 0 with `"hash":"<hex>"` proves lossless reception end-to-end on the
//!   host with **no hardware** (see `scripts/verify/macos-receive-smoke.sh`).
//! * `--sink audio` — [`crate::backend::render::CoreAudioRenderSink`]: preflights
//!   a real HAL output device / USB DAC and errors loudly with the device gate
//!   if none is present (real playback = `usb-dac-device` / TCC session gate).
//!
//! Exit: `0` on a clean end-of-stream with a verified hash; non-zero on error
//! or end-of-stream timeout (bounded poll, never sleep-and-assume).

use wdr_refsim::receiver::{BufferProfile, ReceiverOutcome};
use wdr_refsim::receiver_server;
use wdr_refsim::sink::{QuicRenderReceiver, SinkError};

struct ReceiveArgs {
    addr: std::net::SocketAddr,
    sink: SinkKind,
    buffer: BufferProfile,
    timeout_secs: u64,
}

enum SinkKind {
    Null,
    Audio,
}

pub fn usage() -> &'static str {
    "usage: macos-emitter --receive [--addr <host:port>] [--sink null|audio] [--buffer low|balanced|resilient] [--timeout <secs>]\n\
     default: 127.0.0.1:9000  sink=null  buffer=balanced  timeout=90"
}

fn parse_receive_args(args: &[String]) -> Result<ReceiveArgs, String> {
    let mut addr = None;
    let mut sink = SinkKind::Null;
    let mut buffer = BufferProfile::Balanced;
    let mut timeout_secs = 90u64;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--addr" => {
                let v = it.next().ok_or("--addr needs a value")?;
                addr = Some(
                    v.parse::<std::net::SocketAddr>()
                        .map_err(|_| format!("invalid --addr '{v}'"))?,
                );
            }
            "--sink" => {
                sink = match it.next().ok_or("--sink needs a value")?.as_str() {
                    "null" => SinkKind::Null,
                    "audio" => SinkKind::Audio,
                    other => return Err(format!("unknown --sink '{other}' (null|audio)")),
                };
            }
            "--buffer" => {
                buffer = BufferProfile::parse(it.next().ok_or("--buffer needs a value")?)?;
            }
            "--timeout" => {
                timeout_secs = it
                    .next()
                    .ok_or("--timeout needs a value")?
                    .parse()
                    .map_err(|_| "invalid --timeout (seconds)")?;
            }
            "--help" | "-h" => return Err(usage().to_string()),
            other if other.starts_with('-') => {
                return Err(format!("unknown option '{other}'; {}", usage()))
            }
            other => return Err(format!("unexpected positional '{other}'; {}", usage())),
        }
    }
    let addr = addr.unwrap_or("127.0.0.1:9000".parse().expect("static addr"));
    Ok(ReceiveArgs {
        addr,
        sink,
        buffer,
        timeout_secs,
    })
}

/// Pick the render sink for the run. `null` needs no hardware; `audio`
/// preflights a HAL output device and errors loudly if the device gate isn't
/// met (so a headless run never pretends to play).
fn build_sink(kind: &SinkKind) -> Result<Box<dyn wdr_refsim::sink::RenderSink + Send>, SinkError> {
    match kind {
        SinkKind::Null => Ok(receiver_server::null_render_sink()),
        SinkKind::Audio => {
            #[cfg(target_os = "macos")]
            {
                crate::backend::render::CoreAudioRenderSink::try_new()
                    .map(|s| Box::new(s) as Box<dyn wdr_refsim::sink::RenderSink + Send>)
            }
            #[cfg(not(target_os = "macos"))]
            {
                Err(SinkError::Format(
                    "--sink audio requires macOS Core Audio (this build is not macOS)".into(),
                ))
            }
        }
    }
}

fn emit(event: &str, fields: &[(&str, serde_json::Value)]) {
    let mut obj = serde_json::Map::new();
    obj.insert("ev".into(), serde_json::Value::String(event.to_string()));
    for (k, v) in fields {
        obj.insert((*k).into(), v.clone());
    }
    println!("{}", serde_json::Value::Object(obj));
}

fn outcome_event(outcome: &ReceiverOutcome) {
    emit(
        "complete",
        &[
            ("status", serde_json::json!("complete")),
            ("hash", serde_json::json!(outcome.hash_hex())),
            (
                "packets_recv",
                serde_json::json!(outcome.metrics.packets_recv),
            ),
            ("loss", serde_json::json!(outcome.metrics.loss)),
            ("duplicate", serde_json::json!(outcome.metrics.duplicate)),
            ("reorder", serde_json::json!(outcome.metrics.reorder)),
            (
                "late_discard",
                serde_json::json!(outcome.metrics.late_discard),
            ),
            ("underruns", serde_json::json!(outcome.metrics.underruns)),
            ("malformed", serde_json::json!(outcome.metrics.malformed)),
        ],
    );
}

/// Drive one receive session to completion. Never panics on data; every
/// failure is a typed `fatal` event + non-zero exit.
pub fn run_receive(args: &[String]) -> i32 {
    let args = match parse_receive_args(args) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("[macos-emitter] {e}");
            return 2;
        }
    };
    let sink = match build_sink(&args.sink) {
        Ok(s) => s,
        Err(e) => {
            emit("fatal", &[("error", serde_json::json!(e.to_string()))]);
            return 1;
        }
    };
    let mut receiver = match QuicRenderReceiver::listen(args.addr, args.buffer, sink) {
        Ok(r) => r,
        Err(e) => {
            emit("fatal", &[("error", serde_json::json!(e.to_string()))]);
            return 1;
        }
    };
    emit(
        "start",
        &[
            (
                "sink",
                serde_json::json!(match args.sink {
                    SinkKind::Null => "null",
                    SinkKind::Audio => "audio",
                }),
            ),
            ("addr", serde_json::json!(args.addr.to_string())),
            ("buffer", serde_json::json!(args.buffer.as_str())),
        ],
    );
    match receiver.wait(std::time::Duration::from_secs(args.timeout_secs)) {
        Ok(outcome) => {
            outcome_event(&outcome);
            0
        }
        Err(e) => {
            emit("fatal", &[("error", serde_json::json!(e.to_string()))]);
            1
        }
    }
}
