//! `linux-emitter` CLI — packaging entrypoint for the Linux PipeWire emitter.
//!
//! Portable surface only (no `pipewire` feature, no daemon required), so the
//! same source checks cleanly on this macOS host and links into a real
//! `linux-emitter` binary on the Linux runner / dev container. Commands:
//! `--list-format` / `--endpoint-template` / `--version`.
//!
//! "Installed but won't open" (the `.desktop` `Exec=linux-emitter` with no
//! args): with a **TTY stdin** the app runs a small interactive menu that stays
//! open until the user quits (a full GUI is a documented follow-up). Non-TTY
//! contexts (CI, piped) keep the old usage+exit byte-identical.

#![deny(rust_2018_idioms)]

use linux_emitter::{CaptureSource, EndpointInfo, FakeCaptureSource, FormatMeta};
use std::io::IsTerminal as _;

fn stereo48() -> FormatMeta {
    FormatMeta {
        rate: 48_000,
        bits: 16,
        channels: 2,
    }
}

fn print_list_format() {
    let mut src = FakeCaptureSource::new(stereo48());
    let _ = src.start();
    let fmt = src.format();
    println!(
        "rate={} bits={} channels={}",
        fmt.rate, fmt.bits, fmt.channels
    );
}

/// A template USB-DAC node the user can grep from `pw-cli list-nodes`.
fn print_endpoint_template() {
    let ep = EndpointInfo {
        name: "alsa_output.usb-*.analog-stereo".into(),
        is_usb: true,
        per_app_capable: true,
    };
    println!(
        "name={} usb={} per_app_capable={}",
        ep.name, ep.is_usb, ep.per_app_capable
    );
}

/// Interactive launcher menu for no-args + TTY launches (the `.desktop` path).
/// Stays open until "q" / Ctrl-C / EOF instead of usage-and-exit.
fn interactive_menu() {
    use std::io::Write;
    println!("Wavelink — Linux emitter");
    println!("(interactive launcher; a full GUI is a follow-up)");
    loop {
        print!("\n1) List format   2) Endpoint template   3) Version   q) Quit\n> ");
        let _ = std::io::stdout().flush();
        let mut line = String::new();
        match std::io::stdin().read_line(&mut line) {
            Err(_) | Ok(0) => break, // EOF/closed stdin — don't spin
            Ok(_) => match line.trim() {
                "1" => print_list_format(),
                "2" => print_endpoint_template(),
                "3" => println!("linux-emitter {}", env!("CARGO_PKG_VERSION")),
                "q" | "quit" | "" => break,
                other => eprintln!("unknown choice '{other}'"),
            },
        }
    }
}

fn main() {
    match std::env::args().nth(1).as_deref() {
        Some("--list-format") => print_list_format(),
        Some("--endpoint-template") => print_endpoint_template(),
        Some("--version") => println!("linux-emitter {}", env!("CARGO_PKG_VERSION")),
        _ => {
            // "Opening" the app with no args: with a TTY attached, run the
            // interactive menu instead of usage-and-exit.
            if std::io::stdin().is_terminal() {
                interactive_menu();
            } else {
                eprintln!(
                    "usage: linux-emitter [--list-format | --endpoint-template | --version]\n\
                     (packaging entrypoint for the WDR Linux emitter)"
                );
                std::process::exit(2);
            }
        }
    }
}
