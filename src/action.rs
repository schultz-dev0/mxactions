use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    Keys(String),
    Click(ClickButton),
    Shell(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClickButton {
    Left,
    Right,
    Middle,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ActionError {
    #[error("unknown click button: {0}")]
    UnknownClick(String),
    #[error("unknown key name: {0}")]
    UnknownKey(String),
    #[error("empty command")]
    Empty,
    #[error("io error: {0}")]
    Io(String),
}

pub trait InputInjector {
    fn key_chord(&mut self, chord: &str) -> Result<(), ActionError>;
    fn click(&mut self, button: ClickButton) -> Result<(), ActionError>;
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct RecordingInjector {
    pub keys: Vec<String>,
    pub clicks: Vec<ClickButton>,
}

impl InputInjector for RecordingInjector {
    fn key_chord(&mut self, chord: &str) -> Result<(), ActionError> {
        self.keys.push(chord.to_string());
        Ok(())
    }

    fn click(&mut self, button: ClickButton) -> Result<(), ActionError> {
        self.clicks.push(button);
        Ok(())
    }
}

/// Spawns `ydotool` for key chords and mouse clicks (requires `ydotool` +
/// `ydotoold`). Uses raw Linux keycodes: `ydotool key` does NOT accept key
/// names, and silently turns unparseable args into no-op delays — so chords
/// must be translated to `<keycode>:<state>` pairs. `wtype` was tried but the
/// Wayland virtual-keyboard protocol is not honored on Hyprland.
#[derive(Debug, Default)]
pub struct YdotoolInjector;

impl InputInjector for YdotoolInjector {
    fn key_chord(&mut self, chord: &str) -> Result<(), ActionError> {
        let mut command = std::process::Command::new("ydotool");
        command.arg("key");
        command.args(ydotool_key_args(chord)?);
        spawn_reaped(command)
    }

    fn click(&mut self, button: ClickButton) -> Result<(), ActionError> {
        // ydotool click codes are a bitmask: high nibble 0x40=down, 0x80=up
        // (0xC0 = press+release in one call), low nibble selects the button
        // (0=left, 1=right, 2=middle).
        let code = match button {
            ClickButton::Left => "0xC0",
            ClickButton::Right => "0xC1",
            ClickButton::Middle => "0xC2",
        };
        let mut command = std::process::Command::new("ydotool");
        command.args(["click", code]);
        spawn_reaped(command)
    }
}

/// Turns a `+`-joined chord like `"ctrl+shift+p"` into `ydotool key` args:
/// press each modifier, press then release the key, release modifiers in
/// reverse, e.g. `["29:1", "42:1", "25:1", "25:0", "42:0", "29:0"]`.
fn ydotool_key_args(chord: &str) -> Result<Vec<String>, ActionError> {
    let mut tokens: Vec<&str> = chord
        .split('+')
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .collect();
    let Some(key) = tokens.pop() else {
        return Err(ActionError::Empty);
    };
    let mods = tokens
        .iter()
        .map(|t| keycode(t).ok_or_else(|| ActionError::UnknownKey((*t).to_string())))
        .collect::<Result<Vec<_>, _>>()?;
    let key_code = keycode(key).ok_or_else(|| ActionError::UnknownKey(key.to_string()))?;

    let mut args = Vec::with_capacity(mods.len() * 2 + 2);
    for m in &mods {
        args.push(format!("{m}:1"));
    }
    args.push(format!("{key_code}:1"));
    args.push(format!("{key_code}:0"));
    for m in mods.iter().rev() {
        args.push(format!("{m}:0"));
    }
    Ok(args)
}

/// Maps a modifier alias or key name to its Linux input keycode
/// (`/usr/include/linux/input-event-codes.h`). Returns `None` for names not in
/// the table so the caller can surface an error instead of silently mis-firing.
fn keycode(name: &str) -> Option<u16> {
    let code = match name.to_ascii_lowercase().as_str() {
        // modifiers
        "ctrl" | "control" => 29,
        "shift" => 42,
        "alt" => 56,
        "altgr" => 100,
        "super" | "meta" | "win" | "cmd" | "logo" | "mod4" => 125,
        // letters
        "a" => 30,
        "b" => 48,
        "c" => 46,
        "d" => 32,
        "e" => 18,
        "f" => 33,
        "g" => 34,
        "h" => 35,
        "i" => 23,
        "j" => 36,
        "k" => 37,
        "l" => 38,
        "m" => 50,
        "n" => 49,
        "o" => 24,
        "p" => 25,
        "q" => 16,
        "r" => 19,
        "s" => 31,
        "t" => 20,
        "u" => 22,
        "v" => 47,
        "w" => 17,
        "x" => 45,
        "y" => 21,
        "z" => 44,
        // digits
        "1" => 2,
        "2" => 3,
        "3" => 4,
        "4" => 5,
        "5" => 6,
        "6" => 7,
        "7" => 8,
        "8" => 9,
        "9" => 10,
        "0" => 11,
        // named keys
        "enter" | "return" => 28,
        "tab" => 15,
        "esc" | "escape" => 1,
        "space" => 57,
        "backspace" => 14,
        "delete" | "del" => 111,
        "insert" | "ins" => 110,
        "home" => 102,
        "end" => 107,
        "pageup" | "pgup" => 104,
        "pagedown" | "pgdn" => 109,
        "up" => 103,
        "down" => 108,
        "left" => 105,
        "right" => 106,
        "menu" => 139,
        "capslock" => 58,
        "f1" => 59,
        "f2" => 60,
        "f3" => 61,
        "f4" => 62,
        "f5" => 63,
        "f6" => 64,
        "f7" => 65,
        "f8" => 66,
        "f9" => 67,
        "f10" => 68,
        "f11" => 87,
        "f12" => 88,
        // punctuation (literal char or XKB-ish name)
        "-" | "minus" => 12,
        "=" | "equal" => 13,
        "[" | "bracketleft" => 26,
        "]" | "bracketright" => 27,
        "\\" | "backslash" => 43,
        ";" | "semicolon" => 39,
        "'" | "apostrophe" => 40,
        "`" | "grave" => 41,
        "," | "comma" => 51,
        "." | "period" | "dot" => 52,
        "/" | "slash" => 53,
        _ => return None,
    };
    Some(code)
}

pub struct ActionRunner<I: InputInjector> {
    pub injector: I,
}

impl<I: InputInjector> ActionRunner<I> {
    pub fn run(&mut self, raw: &str) -> Result<(), ActionError> {
        match parse_command(raw)? {
            Action::Keys(chord) => self.injector.key_chord(&chord),
            Action::Click(button) => self.injector.click(button),
            Action::Shell(cmd) => {
                let mut command = std::process::Command::new("sh");
                command.arg("-c").arg(&cmd);
                spawn_reaped(command)
            }
        }
    }
}

fn spawn_reaped(mut command: std::process::Command) -> Result<(), ActionError> {
    let mut child = command
        .spawn()
        .map_err(|e| ActionError::Io(e.to_string()))?;

    // Rust does not wait when Child is dropped. Reap asynchronously so frequent
    // actions cannot leave zombies behind while keeping dispatch non-blocking.
    std::thread::Builder::new()
        .name("mxactions-child-reaper".into())
        .spawn(move || {
            if let Err(error) = child.wait() {
                log::debug!("failed to reap action process: {error}");
            }
        })
        .map_err(|e| ActionError::Io(e.to_string()))?;
    Ok(())
}

pub fn parse_command(raw: &str) -> Result<Action, ActionError> {
    let s = raw.trim();
    if s.is_empty() {
        return Err(ActionError::Empty);
    }
    if let Some(rest) = s.strip_prefix("key:") {
        let chord = rest.trim();
        if chord.is_empty() {
            return Err(ActionError::Empty);
        }
        return Ok(Action::Keys(chord.to_string()));
    }
    if let Some(rest) = s.strip_prefix("click:") {
        let btn = match rest.trim() {
            "left" => ClickButton::Left,
            "right" => ClickButton::Right,
            "middle" => ClickButton::Middle,
            other => return Err(ActionError::UnknownClick(other.to_string())),
        };
        return Ok(Action::Click(btn));
    }
    Ok(Action::Shell(s.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_prefixes() {
        assert_eq!(
            parse_command("key:ctrl+shift+p").unwrap(),
            Action::Keys("ctrl+shift+p".into())
        );
        assert_eq!(
            parse_command("click:left").unwrap(),
            Action::Click(ClickButton::Left)
        );
        assert_eq!(
            parse_command("walker").unwrap(),
            Action::Shell("walker".into())
        );
    }

    #[test]
    fn runner_records_keys() {
        let mut runner = ActionRunner {
            injector: RecordingInjector::default(),
        };
        runner.run("key:ctrl+p").unwrap();
        assert_eq!(runner.injector.keys, vec!["ctrl+p"]);
    }

    #[test]
    fn rejects_empty_key_chord() {
        assert_eq!(parse_command("key:   "), Err(ActionError::Empty));
    }

    #[test]
    fn key_args_press_mods_then_key_then_release_reversed() {
        // ctrl=29 shift=42 p=25
        assert_eq!(
            ydotool_key_args("ctrl+shift+p").unwrap(),
            vec!["29:1", "42:1", "25:1", "25:0", "42:0", "29:0"]
        );
    }

    #[test]
    fn key_args_maps_punctuation() {
        // ctrl=29 grave=41 minus=12
        assert_eq!(
            ydotool_key_args("ctrl+`").unwrap(),
            vec!["29:1", "41:1", "41:0", "29:0"]
        );
        assert_eq!(
            ydotool_key_args("ctrl+-").unwrap(),
            vec!["29:1", "12:1", "12:0", "29:0"]
        );
    }

    #[test]
    fn key_args_super_alias_maps_to_leftmeta() {
        // super=125 d=32
        assert_eq!(
            ydotool_key_args("super+d").unwrap(),
            vec!["125:1", "32:1", "32:0", "125:0"]
        );
    }

    #[test]
    fn key_args_no_modifiers() {
        // esc=1
        assert_eq!(ydotool_key_args("Escape").unwrap(), vec!["1:1", "1:0"]);
    }

    #[test]
    fn key_args_rejects_empty_chord() {
        assert_eq!(ydotool_key_args("   "), Err(ActionError::Empty));
    }

    #[test]
    fn key_args_rejects_unknown_key() {
        assert_eq!(
            ydotool_key_args("ctrl+nope"),
            Err(ActionError::UnknownKey("nope".into()))
        );
    }

    #[test]
    fn runner_spawns_shell() {
        let mut runner = ActionRunner {
            injector: RecordingInjector::default(),
        };
        runner.run("true").unwrap();
    }
}
