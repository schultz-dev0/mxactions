//! Minimal HID++ 2.0 wire helpers (short and long reports).

pub const SHORT_REPORT_ID: u8 = 0x10;
pub const SHORT_REPORT_LEN: usize = 7;

pub const LONG_REPORT_ID: u8 = 0x11;
pub const LONG_REPORT_LEN: usize = 20;

pub const IROOT_FEATURE_INDEX: u8 = 0x00;
pub const FEATURE_REPROG_CONTROLS_V4: u16 = 0x1B04;

/// Haptic Sense Panel control ID on MX Master 4 (Solaar: `unknown:01A0` / Haptic).
pub const SENSE_PANEL_CID: u16 = 0x01A0;

/// Logitech HID++ raw interface (usage page from CPG / Solaar).
pub const HIDPP_USAGE_PAGE: u16 = 0xFF43;
pub const HIDPP_USAGE: u16 = 0x0001;

/// Mapping flags for `setCidReporting` (ReprogControlsV4).
pub const MAPPING_DIVERTED: u8 = 0x01;

/// Capability bit in `getCtrlIdInfo` flags: control can be temporarily diverted.
pub const KEY_FLAG_DIVERTABLE: u16 = 0x0020;

/// Build a 7-byte short HID++ request/report body (including report ID).
pub fn short_request(
    device_index: u8,
    feature_index: u8,
    function: u8,
    software_id: u8,
    params: &[u8],
) -> [u8; SHORT_REPORT_LEN] {
    let mut buf = [0u8; SHORT_REPORT_LEN];
    buf[0] = SHORT_REPORT_ID;
    buf[1] = device_index;
    buf[2] = feature_index;
    buf[3] = function;
    buf[4] = software_id;
    for (i, b) in params.iter().take(2).enumerate() {
        buf[5 + i] = *b;
    }
    buf
}

/// Build a 20-byte long HID++ request (needed for `setCidReporting` and friends).
pub fn long_request(
    device_index: u8,
    feature_index: u8,
    function: u8,
    software_id: u8,
    params: &[u8],
) -> [u8; LONG_REPORT_LEN] {
    let mut buf = [0u8; LONG_REPORT_LEN];
    buf[0] = LONG_REPORT_ID;
    buf[1] = device_index;
    buf[2] = feature_index;
    buf[3] = function;
    buf[4] = software_id;
    for (i, b) in params.iter().take(LONG_REPORT_LEN - 5).enumerate() {
        buf[5 + i] = *b;
    }
    buf
}

/// Parsed HID++ report (short or long).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HidppReport {
    Short([u8; SHORT_REPORT_LEN]),
    Long([u8; LONG_REPORT_LEN]),
}

impl HidppReport {
    pub fn device_index(&self) -> u8 {
        match self {
            Self::Short(r) => r[1],
            Self::Long(r) => r[1],
        }
    }

    pub fn feature_index(&self) -> u8 {
        match self {
            Self::Short(r) => r[2],
            Self::Long(r) => r[2],
        }
    }

    pub fn function(&self) -> u8 {
        match self {
            Self::Short(r) => r[3],
            Self::Long(r) => r[3],
        }
    }

    pub fn software_id(&self) -> u8 {
        match self {
            Self::Short(r) => r[4],
            Self::Long(r) => r[4],
        }
    }

    pub fn params(&self) -> &[u8] {
        match self {
            Self::Short(r) => &r[5..SHORT_REPORT_LEN],
            Self::Long(r) => &r[5..LONG_REPORT_LEN],
        }
    }
}

/// Parse a raw hidraw read buffer; returns the first HID++ report found.
pub fn parse_report(data: &[u8]) -> Option<HidppReport> {
    let data = skip_leading_zero(data);
    if data.len() >= SHORT_REPORT_LEN && data[0] == SHORT_REPORT_ID {
        let mut out = [0u8; SHORT_REPORT_LEN];
        out.copy_from_slice(&data[..SHORT_REPORT_LEN]);
        return Some(HidppReport::Short(out));
    }
    if data.len() >= LONG_REPORT_LEN && data[0] == LONG_REPORT_ID {
        let mut out = [0u8; LONG_REPORT_LEN];
        out.copy_from_slice(&data[..LONG_REPORT_LEN]);
        return Some(HidppReport::Long(out));
    }
    None
}

fn skip_leading_zero(data: &[u8]) -> &[u8] {
    if data.first() == Some(&0) && data.get(1).is_some_and(|b| *b == SHORT_REPORT_ID || *b == LONG_REPORT_ID) {
        &data[1..]
    } else {
        data
    }
}

/// Back-compat wrapper for short-only callers.
#[allow(dead_code)]
pub fn parse_short_report(data: &[u8]) -> Option<[u8; SHORT_REPORT_LEN]> {
    match parse_report(data)? {
        HidppReport::Short(r) => Some(r),
        HidppReport::Long(_) => None,
    }
}

/// `divertedButtonsEvent` payload: up to four big-endian CIDs currently held.
pub fn parse_diverted_button_cids(params: &[u8]) -> [u16; 4] {
    let mut cids = [0u16; 4];
    for (i, cid) in cids.iter_mut().enumerate() {
        let off = i * 2;
        if params.len() >= off + 2 {
            *cid = u16::from_be_bytes([params[off], params[off + 1]]);
        }
    }
    cids
}

/// Returns `true` when `target` appears in the four CID slots (0 = unused).
pub fn cids_contain(cids: &[u16; 4], target: u16) -> bool {
    cids.iter().any(|&c| c == target)
}

/// Compute the `bfield` byte for `setCidReporting` (Solaar-compatible).
pub fn cid_reporting_bfield(set_diverted: bool) -> u8 {
    let diverted = if set_diverted { MAPPING_DIVERTED } else { 0 };
    diverted | (MAPPING_DIVERTED << 1)
}

/// Read `DIVERTED` from `getCidReporting` mapping flags (low byte).
pub fn mapping_is_diverted(mapping_flags: u8) -> bool {
    mapping_flags & MAPPING_DIVERTED != 0
}

/// Parse `getCtrlIdInfo` (ReprogControlsV4 fn `0x10`) response params.
///
/// Solaar layout: `!HHBBBBB` — CID, task ID, flags1, pos, group, gmask, flags2.
pub fn parse_ctrl_id_info(params: &[u8]) -> Option<(u16, u16)> {
    if params.len() < 5 {
        return None;
    }
    let cid = u16::from_be_bytes([params[0], params[1]]);
    let flags = u16::from(params[4]) | (u16::from(params.get(8).copied().unwrap_or(0)) << 8);
    Some((cid, flags))
}

/// Parse `getCidReporting` (ReprogControlsV4 fn `0x20`) mapping flags byte.
///
/// Solaar layout: `!HBH` — CID, mapping flags, mapped-to CID.
pub fn parse_cid_reporting_flags(params: &[u8]) -> Option<u8> {
    if params.len() < 3 {
        return None;
    }
    Some(params[2])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_diverted_buttons_extracts_cids() {
        let params = [0x01, 0xA0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00];
        let cids = parse_diverted_button_cids(&params);
        assert_eq!(cids[0], 0x01A0);
        assert_eq!(cids[1], 0);
        assert!(cids_contain(&cids, 0x01A0));
        assert!(!cids_contain(&cids, 0x00C3));
    }

    #[test]
    fn cid_reporting_bfield_matches_solaar() {
        assert_eq!(cid_reporting_bfield(true), 0x03);
        assert_eq!(cid_reporting_bfield(false), 0x02);
    }

    #[test]
    fn parse_short_report_handles_optional_leading_zero() {
        let raw = [0x10, 0x02, 0x05, 0x00, 0x5A, 0x01, 0xA0];
        assert_eq!(parse_short_report(&raw).unwrap(), raw);
        let padded = [0x00, 0x10, 0x02, 0x05, 0x00, 0x5A, 0x01, 0xA0];
        assert_eq!(parse_short_report(&padded).unwrap(), raw);
    }

    #[test]
    fn parse_ctrl_id_info_matches_solaar_layout() {
        // CID=0x01A0, task=0x0109, flags1=0x20 (DIVERTABLE), pos/group/gmask, flags2=0x01
        let params = [0x01, 0xA0, 0x01, 0x09, 0x20, 0x03, 0x02, 0x00, 0x01];
        let (cid, flags) = parse_ctrl_id_info(&params).unwrap();
        assert_eq!(cid, 0x01A0);
        assert_eq!(flags, 0x0120);
        assert!(flags & KEY_FLAG_DIVERTABLE != 0);
    }

    #[test]
    fn parse_ctrl_id_info_requires_flags1_byte() {
        assert!(parse_ctrl_id_info(&[0x01, 0xA0, 0x01, 0x09]).is_none());
        let (cid, flags) = parse_ctrl_id_info(&[0x01, 0xA0, 0x01, 0x09, 0x20]).unwrap();
        assert_eq!(cid, 0x01A0);
        assert_eq!(flags, 0x0020);
    }

    #[test]
    fn parse_cid_reporting_flags_uses_byte_after_cid() {
        // CID=0x01A0, mapping_flags=0x03 (diverted), mapped_to=0
        let params = [0x01, 0xA0, 0x03, 0x00, 0x00];
        assert_eq!(parse_cid_reporting_flags(&params), Some(0x03));
        assert!(mapping_is_diverted(parse_cid_reporting_flags(&params).unwrap()));
    }

    #[test]
    fn parse_cid_reporting_flags_rejects_short_payload() {
        assert_eq!(parse_cid_reporting_flags(&[0x01, 0xA0]), None);
    }

    #[test]
    fn sense_panel_press_release_edges() {
        let cid = 0x01A0;
        let mut held = false;
        let mut events = Vec::new();

        let step = |cids: [u16; 4], held: &mut bool, events: &mut Vec<&str>| {
            let now = cids_contain(&cids, cid);
            if now && !*held {
                events.push("Press");
            } else if !now && *held {
                events.push("Release");
            }
            *held = now;
        };

        step([cid, 0, 0, 0], &mut held, &mut events);
        assert_eq!(events, ["Press"]);
        step([cid, 0, 0, 0], &mut held, &mut events);
        step([0, 0, 0, 0], &mut held, &mut events);
        assert_eq!(events, ["Press", "Release"]);
    }
}
