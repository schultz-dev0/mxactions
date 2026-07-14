//! Best-effort global pointer position for ring placement at press time.

/// Query the compositor cursor position via optional CLI tools (Hyprland, Sway).
pub fn query_pointer_position() -> Option<(i32, i32)> {
    hyprland_cursor().or_else(sway_cursor)
}

fn hyprland_cursor() -> Option<(i32, i32)> {
    if std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE").is_none() {
        return None;
    }
    let out = std::process::Command::new("hyprctl")
        .args(["cursorpos"])
        .output()
        .ok()?;
    if !out.status.success() {
        log::debug!("hyprctl cursorpos failed");
        return None;
    }
    parse_xy_pair(&String::from_utf8_lossy(&out.stdout))
}

fn sway_cursor() -> Option<(i32, i32)> {
    let out = std::process::Command::new("swaymsg")
        .args(["-t", "get_seats"])
        .output()
        .ok()?;
    if !out.status.success() {
        log::debug!("swaymsg get_seats failed");
        return None;
    }
    let seats: serde_json::Value = serde_json::from_slice(&out.stdout).ok()?;
    let seat = seats.as_array()?.first()?;
    let x = seat.get("x")?.as_i64()? as i32;
    let y = seat.get("y")?.as_i64()? as i32;
    Some((x, y))
}

/// Parse `x,y` or `x y` cursor coordinates from compositor tool output.
pub(crate) fn parse_xy_pair(s: &str) -> Option<(i32, i32)> {
    let parts: Vec<&str> = s
        .trim()
        .split([',', ' ', '\t'])
        .filter(|p| !p.is_empty())
        .collect();
    if parts.len() < 2 {
        return None;
    }
    let x = parts[0].parse().ok()?;
    let y = parts[1].parse().ok()?;
    Some((x, y))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_comma_separated_cursorpos() {
        assert_eq!(parse_xy_pair("958,415"), Some((958, 415)));
    }

    #[test]
    fn parses_space_separated_cursorpos() {
        assert_eq!(parse_xy_pair("958 415\n"), Some((958, 415)));
    }

    #[test]
    fn parses_tab_separated_cursorpos() {
        assert_eq!(parse_xy_pair("\t1200\t800"), Some((1200, 800)));
    }

    #[test]
    fn rejects_incomplete_cursorpos() {
        assert_eq!(parse_xy_pair("958"), None);
        assert_eq!(parse_xy_pair(""), None);
    }
}
