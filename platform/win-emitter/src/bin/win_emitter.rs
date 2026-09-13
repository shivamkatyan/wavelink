//! `win-emitter` CLI — packaging entrypoint for the Windows WASAPI emitter.
//!
//! Portable surface only (zero deps, no `#[cfg(windows)]` code), so the same
//! source is `cargo check --target x86_64-pc-windows-msvc`-clean on this macOS
//! host AND links into a real `win_emitter.exe` on the Windows CI runner.
//! Commands: `--list-format` / `--endpoint-info` / `--version`.
//!
//! Double-click ("installed but won't open"): with **no args and a TTY stdin**,
//! the app runs a small interactive menu that stays open until the user quits —
//! double-clicking genuinely opens something usable (a full GUI is a documented
//! follow-up). Non-TTY contexts (CI, piped) keep the old usage + double-click
//! pause byte-identical.

#![deny(rust_2018_idioms)]

use std::io::IsTerminal as _;
use win_emitter::{CaptureSource, EndpointInfo, FakeCaptureSource, FormatMeta};

fn stereo48() -> FormatMeta {
    FormatMeta {
        rate: 48_000,
        bits: 16,
        channels: 2,
    }
}

/// WASAPI loopback: system-wide render device; per-app is unsupported.
fn print_endpoint_info() {
    let ep = EndpointInfo {
        name: "Default render device".into(),
        is_usb: false,
        is_default: true,
    };
    println!(
        "name={} usb={} default={}",
        ep.name, ep.is_usb, ep.is_default
    );
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

/// Interactive launcher menu for double-click (no-args + TTY) launches. Stays
/// open until "q" / Ctrl-C / EOF; a user double-clicking the exe lands here
/// instead of a flash-and-exit console.
fn interactive_menu() {
    use std::io::Write;
    println!("Wavelink — Windows emitter");
    println!("(interactive launcher; a full GUI is a follow-up)");
    loop {
        print!("\n1) List format   2) Endpoint info   3) Version   q) Quit\n> ");
        let _ = std::io::stdout().flush();
        let mut line = String::new();
        match std::io::stdin().read_line(&mut line) {
            Err(_) | Ok(0) => break, // EOF/closed stdin — don't spin
            Ok(_) => match line.trim() {
                "1" => print_list_format(),
                "2" => print_endpoint_info(),
                "3" => println!("win-emitter {}", env!("CARGO_PKG_VERSION")),
                "q" | "quit" | "" => break,
                other => eprintln!("unknown choice '{other}'"),
            },
        }
    }
}

/// Windows-only: when `win-emitter.exe` is double-clicked (not run from a
/// terminal), the console window closes the instant the process exits — so the
/// user sees a flash and thinks the app "didn't open". GetConsoleProcessList()
/// reports how many processes share the console: a lone process means Explorer
/// launched it, so hold the window open until Enter so the usage is readable.
/// Running from cmd/PowerShell shares the console (count > 1) and stays
/// unaffected. On non-Windows hosts this is a no-op stub.
#[cfg(windows)]
fn pause_if_double_clicked() {
    use std::io::Write;
    use windows::Win32::System::Console::GetConsoleProcessList;

    let mut pids = [0u32; 2];
    let attached = unsafe { GetConsoleProcessList(&mut pids) };
    if attached <= 1 {
        let mut buf = String::new();
        print!("\npress Enter to close...");
        let _ = std::io::stdout().flush();
        let _ = std::io::stdin().read_line(&mut buf);
    }
}

#[cfg(not(windows))]
fn pause_if_double_clicked() {}

fn main() {
    match std::env::args().nth(1).as_deref() {
        Some("--list-format") => print_list_format(),
        Some("--endpoint-info") => print_endpoint_info(),
        Some("--version") => println!("win-emitter {}", env!("CARGO_PKG_VERSION")),
        _ => {
            // "Opening" the app with no args: with a TTY attached, run the
            // interactive menu (double-click = Explorer's console has stdin).
            if std::io::stdin().is_terminal() {
                interactive_menu();
            } else {
                eprintln!(
                    "usage: win-emitter [--list-format | --endpoint-info | --version]\n\
                     (packaging entrypoint for the WDR Windows emitter)"
                );
                // Non-interactive fallback: keep the console readable.
                pause_if_double_clicked();
                std::process::exit(2);
            }
        }
    }
}
