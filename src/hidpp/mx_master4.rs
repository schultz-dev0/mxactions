//! MX Master 4 Sense Panel ownership via HID++ ReprogControlsV4.

// This is also written by AI. -- Cursor

use std::ffi::NulError;
use std::time::Duration;

use hidapi::{DeviceInfo, HidApi, HidDevice};
use thiserror::Error;

use crate::hidpp::protocol::{
    FEATURE_REPROG_CONTROLS_V4, HIDPP_USAGE, HIDPP_USAGE_PAGE_BOLT, HIDPP_USAGE_PAGE_CLASSIC,
    IROOT_FEATURE_INDEX, KEY_FLAG_DIVERTABLE, SENSE_PANEL_CID, cid_reporting_bfield, cids_contain,
    long_request, mapping_is_diverted, parse_cid_reporting_flags, parse_ctrl_id_info,
    parse_diverted_button_cids, parse_report,
};
use crate::hidpp::{HidEvent, HidEventSource};

/// Logitech USB vendor ID.
pub const LOGITECH_VID: u16 = 0x046D;

/// MX Master 4 over Bluetooth (kernel `hid-logitech-hidpp`, WPID B042).
pub const PID_MX_MASTER_4_BT: u16 = 0xB042;

/// Logitech Bolt receiver (paired MX Master 4 uses a sub-device index on this).
pub const PID_BOLT_RECEIVER: u16 = 0xC548;

/// Legacy Unifying receiver — some setups still pair MX devices here.
pub const PID_UNIFYING_RECEIVER: u16 = 0xC52B;

/// Fixed software id in the low nibble of report byte 3 (Solaar-style; avoid 0).
const SOFTWARE_ID: u8 = 0x0B;
const READ_BUF_LEN: usize = 256;
const REQUEST_TIMEOUT: Duration = Duration::from_millis(1000);

/// Device indices to probe (direct/BT first, then typical receiver slots).
const DEVICE_INDICES: [u8; 7] = [0xFF, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06];

#[derive(Debug, Error)]
pub enum HidError {
    #[error("MX Master 4 not found (connect via Bluetooth or Bolt; need HID++ interface)")]
    DeviceNotFound,

    #[error("permission denied opening {path}: {source}")]
    PermissionDenied {
        path: String,
        #[source]
        source: hidapi::HidError,
    },

    #[error("failed to open HID device at {path}: {source}")]
    OpenFailed {
        path: String,
        #[source]
        source: hidapi::HidError,
    },

    #[error("HID++ transport error: {0}")]
    Transport(String),

    #[error("Sense Panel (CID 0x{cid:04X}) is not exposed or not divertable on this device")]
    SensePanelNotFound { cid: u16 },

    #[error("failed to divert Sense Panel (CID 0x{cid:04X}): {detail}")]
    DivertFailed { cid: u16, detail: String },

    #[error("HID API initialization failed: {0}")]
    HidApi(#[from] hidapi::HidError),

    #[error("invalid device path: {0}")]
    PathNul(#[from] NulError),
}

struct HidppDevice {
    dev: HidDevice,
    device_index: u8,
    reprog_index: u8,
    software_id: u8,
}

impl HidppDevice {
    fn request(&self, feature_index: u8, function: u8, params: &[u8]) -> Result<Vec<u8>, HidError> {
        // Prefer long (20-byte) HID++ reports — Bolt/Unifying receivers and HID++ ≥2.0
        // devices behave more reliably this way (matches Solaar `long_message=True`).
        let req = long_request(
            self.device_index,
            feature_index,
            function,
            self.software_id,
            params,
        );
        self.dev
            .write(&req)
            .map_err(|e| HidError::Transport(e.to_string()))?;

        let deadline = std::time::Instant::now() + REQUEST_TIMEOUT;
        let mut buf = [0u8; READ_BUF_LEN];
        while std::time::Instant::now() < deadline {
            let n = self
                .dev
                .read_timeout(&mut buf, remaining_ms(deadline))
                .map_err(|e| HidError::Transport(e.to_string()))?;
            if n == 0 {
                continue;
            }
            let Some(report) = parse_report(&buf[..n]) else {
                continue;
            };
            if report.device_index() == self.device_index
                && report.feature_index() == feature_index
                && report.function() == function
                && report.software_id() == self.software_id
            {
                return Ok(report.params().to_vec());
            }
        }
        Err(HidError::Transport(format!(
            "timeout waiting for feature {feature_index} fn {function}"
        )))
    }

    fn ping(&self) -> Result<(), HidError> {
        let _ = self.request(IROOT_FEATURE_INDEX, 0x01, &[0x00])?;
        Ok(())
    }

    fn feature_index(&self, feature_id: u16) -> Result<u8, HidError> {
        let id_bytes = feature_id.to_be_bytes();
        let params = self.request(IROOT_FEATURE_INDEX, 0x00, &id_bytes)?;
        if params.is_empty() {
            return Err(HidError::Transport("empty getFeature response".into()));
        }
        let index = params[0];
        if index == 0 {
            return Err(HidError::Transport(format!(
                "feature 0x{feature_id:04X} not supported"
            )));
        }
        Ok(index)
    }

    fn reprog_count(&self, reprog_index: u8) -> Result<u8, HidError> {
        let params = self.request(reprog_index, 0x00, &[])?;
        params
            .first()
            .copied()
            .ok_or_else(|| HidError::Transport("empty getCount for ReprogControlsV4".into()))
    }

    fn reprog_cid_info(&self, reprog_index: u8, index: u8) -> Result<(u16, u16), HidError> {
        // ReprogControlsV4 function 1 = getCtrlIdInfo (Solaar often writes this as 0x10
        // because it ORs software_id into the low nibble of the request word).
        let params = self.request(reprog_index, 0x01, &[index])?;
        parse_ctrl_id_info(&params).ok_or_else(|| {
            HidError::Transport(format!(
                "short getCtrlIdInfo response (index {index})"
            ))
        })
    }

    fn get_cid_reporting(&self, reprog_index: u8, cid: u16) -> Result<u8, HidError> {
        let cid_bytes = cid.to_be_bytes();
        // Function 2 = getCidReporting
        let params = self.request(reprog_index, 0x02, &cid_bytes)?;
        parse_cid_reporting_flags(&params).ok_or_else(|| {
            HidError::Transport("short getCidReporting response".into())
        })
    }

    fn set_cid_diverted(
        &self,
        reprog_index: u8,
        cid: u16,
        diverted: bool,
    ) -> Result<(), HidError> {
        let bfield = cid_reporting_bfield(diverted);
        let mut pkt = Vec::with_capacity(5);
        pkt.extend_from_slice(&cid.to_be_bytes());
        pkt.push(bfield);
        pkt.extend_from_slice(&0u16.to_be_bytes());
        // Function 3 = setCidReporting
        let echo = self.request(reprog_index, 0x03, &pkt)?;
        if echo.len() >= 3 {
            let echoed_cid = u16::from_be_bytes([echo[0], echo[1]]);
            let echoed_bfield = echo[2];
            if echoed_cid == cid && mapping_is_diverted(echoed_bfield) == diverted {
                return Ok(());
            }
        }
        // Fall back to a read-after-write check — another owner (e.g. Solaar) may block us.
        let flags = self.get_cid_reporting(reprog_index, cid)?;
        if mapping_is_diverted(flags) == diverted {
            return Ok(());
        }
        Err(HidError::DivertFailed {
            cid,
            detail: if diverted {
                "device did not accept temporary diversion (another app may own this control)"
                    .into()
            } else {
                "device did not clear diversion".into()
            },
        })
    }
}

/// MX Master 4 HID++ client with the Haptic Sense Panel temporarily diverted.
pub struct MxMaster4 {
    inner: HidppDevice,
    sense_cid: u16,
    held: bool,
}

impl std::fmt::Debug for MxMaster4 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MxMaster4")
            .field("sense_cid", &self.sense_cid)
            .field("held", &self.held)
            .field("device_index", &self.inner.device_index)
            .finish_non_exhaustive()
    }
}

impl MxMaster4 {
    /// Find an MX Master 4 HID++ node, locate the Sense Panel CID, and divert it.
    pub fn open() -> Result<Self, HidError> {
        let api = HidApi::new()?;
        let candidates = enumerate_candidates(&api);
        if candidates.is_empty() {
            return Err(HidError::DeviceNotFound);
        }

        let mut last_err: Option<HidError> = None;
        for info in candidates {
            match try_open_on_interface(&api, info) {
                Ok(client) => return Ok(client),
                Err(e @ (HidError::PermissionDenied { .. } | HidError::OpenFailed { .. })) => {
                    return Err(e);
                }
                Err(e) => last_err = Some(e),
            }
        }

        Err(last_err.unwrap_or(HidError::DeviceNotFound))
    }

    /// Sense Panel control ID this instance is bound to.
    pub fn sense_panel_cid(&self) -> u16 {
        self.sense_cid
    }

    fn handle_diverted_buttons(&mut self, params: &[u8]) -> Option<HidEvent> {
        let cids = parse_diverted_button_cids(params);
        let now = cids_contain(&cids, self.sense_cid);
        let event = if now && !self.held {
            Some(HidEvent::Press)
        } else if !now && self.held {
            Some(HidEvent::Release)
        } else {
            None
        };
        self.held = now;
        event
    }
}

impl HidEventSource for MxMaster4 {
    fn recv_timeout(&mut self, timeout: Duration) -> Option<HidEvent> {
        let mut buf = [0u8; READ_BUF_LEN];
        let ms = timeout.as_millis().min(u32::MAX as u128) as i32;
        let n = match self.inner.dev.read_timeout(&mut buf, ms) {
            Ok(n) => n,
            Err(e) => {
                log::debug!("hid read error: {e}");
                return None;
            }
        };
        if n == 0 {
            return None;
        }
        let report = parse_report(&buf[..n])?;
        if report.device_index() != self.inner.device_index {
            return None;
        }
        if report.feature_index() != self.inner.reprog_index || report.function() != 0x00 {
            return None;
        }
        self.handle_diverted_buttons(report.params())
    }
}

impl Drop for MxMaster4 {
    fn drop(&mut self) {
        if let Err(e) = self
            .inner
            .set_cid_diverted(self.inner.reprog_index, self.sense_cid, false)
        {
            log::warn!(
                "best-effort undivert of Sense Panel 0x{:04X} failed: {e}",
                self.sense_cid
            );
        }
    }
}

fn remaining_ms(deadline: std::time::Instant) -> i32 {
    deadline
        .saturating_duration_since(std::time::Instant::now())
        .as_millis()
        .min(i32::MAX as u128) as i32
}

fn is_hidpp_interface(info: &DeviceInfo) -> bool {
    let page = info.usage_page();
    (page == HIDPP_USAGE_PAGE_BOLT || page == HIDPP_USAGE_PAGE_CLASSIC)
        && (info.usage() == HIDPP_USAGE || info.usage() == 0x0002)
}

fn name_matches_mx_master_4(info: &DeviceInfo) -> bool {
    info.product_string()
        .is_some_and(|s| s.to_ascii_lowercase().contains("mx master 4"))
}

fn pid_matches(info: &DeviceInfo) -> bool {
    matches!(
        info.product_id(),
        PID_MX_MASTER_4_BT | PID_BOLT_RECEIVER | PID_UNIFYING_RECEIVER
    )
}

fn enumerate_candidates(api: &HidApi) -> Vec<&DeviceInfo> {
    let mut out: Vec<&DeviceInfo> = api
        .device_list()
        .filter(|d| d.vendor_id() == LOGITECH_VID)
        .filter(|d| is_hidpp_interface(d) || pid_matches(d) || name_matches_mx_master_4(d))
        .collect();
    out.sort_by_key(|d| {
        (
            !is_hidpp_interface(d),
            // Prefer Bolt interface 2 (HID++ vendor collection).
            !(d.product_id() == PID_BOLT_RECEIVER && d.interface_number() == 2),
            !name_matches_mx_master_4(d),
            !pid_matches(d),
            d.path().to_string_lossy().len(),
        )
    });
    out
}

fn try_open_on_interface(api: &HidApi, info: &DeviceInfo) -> Result<MxMaster4, HidError> {
    let path = info.path().to_string_lossy().into_owned();
    // Validate we can open once before probing indices.
    let _ = api
        .open_path(info.path())
        .map_err(|e| open_err(&path, e))?;

    for &device_index in &DEVICE_INDICES {
        let probe = HidppDevice {
            dev: api
                .open_path(info.path())
                .map_err(|e| open_err(&path, e))?,
            device_index,
            reprog_index: 0,
            software_id: SOFTWARE_ID,
        };
        if probe.ping().is_err() {
            continue;
        }
        let Ok(reprog_index) = probe.feature_index(FEATURE_REPROG_CONTROLS_V4) else {
            continue;
        };
        let Ok(count) = probe.reprog_count(reprog_index) else {
            continue;
        };
        let sense_cid = match find_sense_panel_cid(&probe, reprog_index, count) {
            Ok(cid) => cid,
            Err(e) => {
                log::debug!(
                    "no Sense Panel on {path} index {device_index}: {e}"
                );
                continue;
            }
        };
        // Sense Panel CID is unique to MX Master 4 for our purposes; name check is best-effort.
        if !device_name_is_mx_master_4(&probe)
            && !name_matches_mx_master_4(info)
            && info.product_id() != PID_MX_MASTER_4_BT
            && info.product_id() != PID_BOLT_RECEIVER
            && info.product_id() != PID_UNIFYING_RECEIVER
        {
            continue;
        }

        let inner = HidppDevice {
            dev: api.open_path(info.path()).map_err(|e| open_err(&path, e))?,
            device_index,
            reprog_index,
            software_id: SOFTWARE_ID,
        };
        inner.set_cid_diverted(reprog_index, sense_cid, true)?;
        log::info!(
            "diverted MX Master 4 Sense Panel CID 0x{sense_cid:04X} (device index {device_index}, path {path})"
        );
        return Ok(MxMaster4 {
            inner,
            sense_cid,
            held: false,
        });
    }

    Err(HidError::DeviceNotFound)
}

fn open_err(path: &str, source: hidapi::HidError) -> HidError {
    if io_error_is_permission(&source) {
        HidError::PermissionDenied {
            path: path.to_owned(),
            source,
        }
    } else {
        HidError::OpenFailed {
            path: path.to_owned(),
            source,
        }
    }
}

fn find_sense_panel_cid(
    dev: &HidppDevice,
    reprog_index: u8,
    count: u8,
) -> Result<u16, HidError> {
    for index in 0..count {
        let Ok((cid, flags)) = dev.reprog_cid_info(reprog_index, index) else {
            continue;
        };
        if cid == SENSE_PANEL_CID && flags & KEY_FLAG_DIVERTABLE != 0 {
            return Ok(cid);
        }
    }
    Err(HidError::SensePanelNotFound {
        cid: SENSE_PANEL_CID,
    })
}

/// Best-effort `DeviceName` feature read (0x0005) to confirm model string.
fn device_name_is_mx_master_4(dev: &HidppDevice) -> bool {
    let Ok(name_index) = dev.feature_index(0x0005) else {
        return false;
    };
    let Ok(len_params) = dev.request(name_index, 0x00, &[]) else {
        return false;
    };
    let Some(&name_len) = len_params.first() else {
        return false;
    };
    let mut name = String::new();
    let mut offset = 0u8;
    while (offset as usize) < name_len as usize {
        // DeviceName function 1 = getDeviceName (Solaar: 0x10 before sw_id merge)
        let Ok(chunk) = dev.request(name_index, 0x01, &[offset]) else {
            break;
        };
        for &b in chunk.iter().take(16) {
            if b == 0 {
                break;
            }
            name.push(b as char);
        }
        offset = offset.saturating_add(16);
    }
    name.to_ascii_lowercase().contains("mx master 4")
}

fn io_error_is_permission(err: &hidapi::HidError) -> bool {
    let msg = err.to_string().to_ascii_lowercase();
    msg.contains("permission") || msg.contains("access") || msg.contains("denied")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sense_panel_cid_is_0x01a0() {
        assert_eq!(SENSE_PANEL_CID, 0x01A0);
    }

    #[test]
    fn mx_master4_open_is_deterministic() {
        // With hardware attached this returns Ok; without, a typed discovery error.
        match MxMaster4::open() {
            Ok(m) => {
                assert_eq!(m.sense_panel_cid(), SENSE_PANEL_CID);
                drop(m); // undivert on drop
            }
            Err(err) => assert!(
                matches!(
                    err,
                    HidError::DeviceNotFound
                        | HidError::PermissionDenied { .. }
                        | HidError::OpenFailed { .. }
                        | HidError::SensePanelNotFound { .. }
                        | HidError::DivertFailed { .. }
                ),
                "unexpected error: {err:?}"
            ),
        }
    }

    #[test]
    fn diverted_button_state_machine() {
        let cid = SENSE_PANEL_CID;
        let mut held = false;
        let step = |cids: [u16; 4], held: &mut bool| {
            let now = cids_contain(&cids, cid);
            let ev = if now && !*held {
                Some(HidEvent::Press)
            } else if !now && *held {
                Some(HidEvent::Release)
            } else {
                None
            };
            *held = now;
            ev
        };
        assert_eq!(step([cid, 0, 0, 0], &mut held), Some(HidEvent::Press));
        assert_eq!(step([cid, 0, 0, 0], &mut held), None);
        assert_eq!(step([0, 0, 0, 0], &mut held), Some(HidEvent::Release));
    }
}
