//! `macos-emitter` CLI — packaging entrypoint for the macOS emitter.
//!
//! Deliberately minimal (<= ~60 lines) and dependency-free: it only queries
//! the crate's public API so the release binary can be produced with a plain
//! `cargo build --release` inside `platform/macos-emitter`. The TCC
//! `--permission-state` probe (`PermissionGate` + `ScShareableContentProbe`)
//! is compile- and run-safe on a headless macOS host: `SCShareableContent.get`
//! returns a probe outcome (often `NotDetermined` without a logged-in grant);
//! it never triggers a TCC prompt by itself.

#![deny(rust_2018_idioms)]

use macos_emitter::backend::permission::{PermissionGate, ScShareableContentProbe};
#[cfg(target_os = "macos")]
use macos_emitter::receive::run_receive;
#[cfg(target_os = "macos")]
use macos_emitter::stream::run_stream;
use macos_emitter::{CaptureSource, FakeCaptureSource, FormatMeta};

fn stereo48() -> FormatMeta {
    FormatMeta {
        rate: 48_000,
        bits: 16,
        channels: 2,
    }
}

fn main() {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    match argv.first().map(String::as_str) {
        Some("--stream") => {
            // Real capture or fixture → core transport seam → QUIC receiver.
            #[cfg(target_os = "macos")]
            std::process::exit(run_stream(&argv[1..]));
            #[cfg(not(target_os = "macos"))]
            {
                eprintln!("--stream is macOS-only (ScreenCaptureKit backend)");
                std::process::exit(2);
            }
        }
        Some("--receive") => {
            // QUIC emitter stream → core receiver pipeline → RenderSink
            // (WS3 desktop receiver render path).
            #[cfg(target_os = "macos")]
            std::process::exit(run_receive(&argv[1..]));
            #[cfg(not(target_os = "macos"))]
            {
                eprintln!("--receive is macOS-only (Core Audio render path)");
                std::process::exit(2);
            }
        }
        Some("--list-format") => {
            let fmt = format_meta_of_source();
            println!(
                "rate={} bits={} channels={}",
                fmt.rate, fmt.bits, fmt.channels
            );
        }
        Some("--permission-state") => {
            let mut gate = PermissionGate::new(ScShareableContentProbe);
            let state = gate.refresh();
            println!(
                "screen-recording-tcc={state:?} capture_allowed={}",
                gate.capture_allowed()
            );
        }
        Some("--version") => {
            println!("macos-emitter {}", env!("CARGO_PKG_VERSION"));
        }
        _ => {
            eprintln!(
                "usage: macos-emitter [--list-format | --permission-state | --version | --stream <args> | --receive <args>]\n\
                 (packaging entrypoint for the WDR macOS emitter)\n\
                 --stream:  capture/fixture → QUIC receiver (see stream.rs)\n\
                 --receive: QUIC emitter → RenderSink (see receive.rs)"
            );
            std::process::exit(2);
        }
    }
}

/// Format the default endpoint reports (via the deterministic fake source).
fn format_meta_of_source() -> FormatMeta {
    let mut src = FakeCaptureSource::new(stereo48());
    let _ = src.start();
    src.format()
}
