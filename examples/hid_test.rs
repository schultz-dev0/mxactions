//! Hardware smoke test: print Sense Panel press/release events.
//!
//! Quit Solaar / other divert owners first. Requires read/write access to the
//! Logitech HID++ hidraw node (udev rule or root).
//!
//! ```sh
//! cargo run --example hid_test
//! ```

use std::time::Duration;

use mxactions::{HidEvent, HidEventSource, MxMaster4};

fn main() {
    env_logger::init();
    let mut mouse = match MxMaster4::open() {
        Ok(m) => m,
        Err(e) => {
            eprintln!("mxactions hid_test: {e}");
            std::process::exit(1);
        }
    };
    eprintln!(
        "Listening on Sense Panel CID 0x{:04X} — press/release the haptic panel (Ctrl+C to quit)",
        mouse.sense_panel_cid()
    );
    loop {
        if let Some(ev) = mouse.recv_timeout(Duration::from_millis(250)) {
            match ev {
                HidEvent::Press => println!("Press"),
                HidEvent::Release => println!("Release"),
            }
        }
    }
}
