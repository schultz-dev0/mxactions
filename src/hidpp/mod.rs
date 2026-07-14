//! HID++ input layer for the MX Master 4 Haptic Sense Panel.
//!
//! mxactions owns the Sense Panel via temporary diversion on HID++ feature
//! `0x1B04` (ReprogControlsV4). Press/release events come from
//! `divertedButtonsEvent` notifications while the panel is diverted.
//!
//! ## Hardware smoke test
//!
//! With an MX Master 4 connected (Bluetooth direct or via Bolt receiver) and
//! no other tool diverting the panel (quit Solaar / OpenLogi first):
//!
//! ```sh
//! cargo run --example hid_test
//! ```
//!
//! Press and release the thumb haptic Sense Panel; the example prints
//! `Press` / `Release` lines. Without a device you should see a clear
//! `DeviceNotFound` or permission error instead of a panic.

mod protocol;

pub mod mx_master4;

use std::collections::VecDeque;
use std::time::Duration;

pub use mx_master4::{HidError, MxMaster4};
pub use protocol::SENSE_PANEL_CID;

/// Press or release on the diverted Sense Panel control.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HidEvent {
    Press,
    Release,
}

/// Blocking poll source for Sense Panel events (real device or test double).
pub trait HidEventSource {
    fn recv_timeout(&mut self, timeout: Duration) -> Option<HidEvent>;
}

/// In-memory queue for controller/integration tests without hardware.
#[derive(Debug, Default)]
pub struct MockHid {
    pub q: VecDeque<HidEvent>,
}

impl HidEventSource for MockHid {
    fn recv_timeout(&mut self, _t: Duration) -> Option<HidEvent> {
        self.q.pop_front()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn mock_hid_drains_queue_in_order() {
        let mut mock = MockHid {
            q: VecDeque::from([HidEvent::Press, HidEvent::Release, HidEvent::Press]),
        };
        assert_eq!(mock.recv_timeout(Duration::ZERO), Some(HidEvent::Press));
        assert_eq!(mock.recv_timeout(Duration::ZERO), Some(HidEvent::Release));
        assert_eq!(mock.recv_timeout(Duration::ZERO), Some(HidEvent::Press));
        assert_eq!(mock.recv_timeout(Duration::ZERO), None);
    }
}
