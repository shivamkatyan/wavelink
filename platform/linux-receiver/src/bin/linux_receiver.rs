//! `linux-receiver` CLI — packaging entrypoint for the Linux A2DP-sink receiver.
//!
//! Portable surface only (the `bt` feature's zbus dep is Linux-target-gated),
//! so this same source checks cleanly on this macOS host AND links into a real
//! `linux-receiver` binary on a Linux runner / bt-lab. Commands:
//! `--list-format` / `--bluetooth` / `--register` / `--version`.
//!
//! Double-click / "installed but won't open": with **no args and a TTY stdin**
//! the app runs a small interactive menu that stays open until the user quits —
//! a headless service role now has a real entrypoint (a full GUI is a
//! documented follow-up). Non-TTY contexts keep old-style usage+exit.
//!
//! Honesty: `--register` only actually calls BlueZ on a Linux + `bt` build with
//! a live system bus; everywhere else it prints the concrete gate (bt-lab) —
//! nothing here implies a live A2DP registration happened.

#![deny(rust_2018_idioms)]

use linux_receiver::FormatMeta;
use std::io::IsTerminal as _;

fn stereo48() -> FormatMeta {
    FormatMeta {
        rate: 48_000,
        bits: 16,
        channels: 2,
    }
}

/// The canonical render format (what a received stream is delivered as).
fn print_list_format() {
    let f = stereo48();
    println!(
        "rate={} bits={} channels={} (receiver render format)",
        f.rate, f.bits, f.channels
    );
}

/// The honest A2DP-sink support position (FR-033): only Linux/BlueZ is a full
/// public path; everything else falls back to free lossy Wi-Fi.
fn print_bluetooth() {
    use linux_receiver::BluetoothMesh;
    println!(
        "Bluetooth receive cells (FR-033): {} rows — only Linux A2DP-sink (BlueZ) is a full path;",
        BluetoothMesh::rows().len()
    );
    println!(
        "stock phone/desktop sinks are unsupported-by-public-API; fallback = free lossy Wi-Fi."
    );
}

/// Register the `a2dp_sink` BlueZ profile — only possible on Linux + feature
/// `bt` with a live system bus (bt-lab gate). Anywhere else: concrete gate.
#[cfg(all(target_os = "linux", feature = "bt"))]
fn register_a2dp() {
    use linux_receiver::bluez::BluezProfileServer;
    let mut server = BluezProfileServer::new();
    match server.connect() {
        Ok(()) => {}
        Err(e) => {
            eprintln!("BlueZ connect failed: {e} — needs a BlueZ system bus (bt-lab gate)");
            return;
        }
    }
    match server.register_a2dp_sink() {
        Ok(()) => println!(
            "A2DP sink profile registered (role: a2dp_sink). Pair a phone and play — live render = bt-lab."
        ),
        Err(e) => eprintln!("register_a2dp_sink failed: {e} — not live without a BlueZ daemon"),
    }
}

#[cfg(not(all(target_os = "linux", feature = "bt")))]
fn register_a2dp() {
    println!(
        "A2DP-sink registration requires a Linux box with a BlueZ daemon (feature `bt`, \
         target linux — bt-lab gate). This build ({} / {}) only proves the portable render seam; \
         see build-check.md.",
        std::env::consts::OS,
        std::env::consts::ARCH
    );
}

/// Interactive launcher menu for no-args + TTY launches (the headless service
/// now has a real, usable entrypoint). Stays open until "q" / Ctrl-C / EOF.
fn interactive_menu() {
    use std::io::Write;
    println!("Wavelink — Linux A2DP-sink receiver");
    println!("(headless receiver launcher; a full GUI is a follow-up)");
    loop {
        print!("\n1) Render format   2) Bluetooth support   3) Register A2DP sink   4) Version   q) Quit\n> ");
        let _ = std::io::stdout().flush();
        let mut line = String::new();
        match std::io::stdin().read_line(&mut line) {
            Err(_) | Ok(0) => break, // EOF/closed stdin — don't spin
            Ok(_) => match line.trim() {
                "1" => print_list_format(),
                "2" => print_bluetooth(),
                "3" => register_a2dp(),
                "4" => println!("linux-receiver {}", env!("CARGO_PKG_VERSION")),
                "q" | "quit" | "" => break,
                other => eprintln!("unknown choice '{other}'"),
            },
        }
    }
}

fn main() {
    match std::env::args().nth(1).as_deref() {
        Some("--list-format") => print_list_format(),
        Some("--bluetooth") => print_bluetooth(),
        Some("--register") => register_a2dp(),
        Some("--version") => println!("linux-receiver {}", env!("CARGO_PKG_VERSION")),
        _ => {
            // "Opening" the app with no args: with a TTY attached, run the
            // interactive menu.
            if std::io::stdin().is_terminal() {
                interactive_menu();
            } else {
                eprintln!(
                    "usage: linux-receiver [--list-format | --bluetooth | --register | --version]\n\
                     (headless A2DP-sink receiver entrypoint — menu mode requires a TTY)"
                );
                std::process::exit(2);
            }
        }
    }
}
