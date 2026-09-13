//! `desktop-gui` CLI entrypoint — placeholder for the iced window.
//!
//! The transport driver (`lib.rs`) is real and verifiable. The iced WINDOW is
//! staged behind the `gui` feature (needs a native runner's display/GPU libs);
//! until then this binary documents the state and exits cleanly.
use desktop_gui::{CaptureOffline, StreamDriver};

fn main() {
    println!("Wavelink — cross-platform desktop GUI scaffold");
    println!(
        "This is the driver-verification build ({})",
        CaptureOffline.description()
    );
    println!("Build the iced window with:  cargo run --features gui  (on the native runner)");
}
