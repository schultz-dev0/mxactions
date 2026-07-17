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

/// Spawns `wtype` for key chords (Wayland virtual-keyboard protocol; no daemon
/// or `/dev/uinput` permissions needed) and `ydotool` for mouse clicks.
#[derive(Debug, Default)]
pub struct SystemInjector;

impl InputInjector for SystemInjector {
    fn key_chord(&mut self, chord: &str) -> Result<(), ActionError> {
        let mut command = std::process::Command::new("wtype");
        command.args(wtype_args(chord)?);
        spawn_reaped(command)
    }

    fn click(&mut self, button: ClickButton) -> Result<(), ActionError> {
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

/// Turns a `+`-joined chord like `"ctrl+shift+p"` into `wtype` CLI args,
/// e.g. `["-M", "ctrl", "-M", "shift", "-k", "p"]`.
fn wtype_args(chord: &str) -> Result<Vec<String>, ActionError> {
    let mut tokens: Vec<&str> = chord.split('+').map(str::trim).collect();
    let Some(key) = tokens.pop().filter(|k| !k.is_empty()) else {
        return Err(ActionError::Empty);
    };
    let mut args = Vec::with_capacity(tokens.len() * 2 + 2);
    for modifier in tokens {
        args.push("-M".to_string());
        args.push(wtype_modifier_name(modifier));
    }
    args.push("-k".to_string());
    args.push(wtype_key_name(key).to_string());
    Ok(args)
}

/// Maps common modifier aliases to the names `wtype -M` accepts
/// (`shift`, `capslock`, `ctrl`, `logo`, `win`, `alt`, `altgr`).
fn wtype_modifier_name(modifier: &str) -> String {
    match modifier.to_ascii_lowercase().as_str() {
        "super" | "meta" | "cmd" | "win" | "logo" => "logo".to_string(),
        "control" => "ctrl".to_string(),
        other => other.to_string(),
    }
}

/// Maps punctuation to the XKB keysym name `wtype -k` expects (libxkbcommon
/// resolves single letters/digits and named keys like `Return`/`F1` as-is,
/// but not literal punctuation characters).
fn wtype_key_name(key: &str) -> &str {
    match key {
        "`" => "grave",
        "-" => "minus",
        "=" => "equal",
        "[" => "bracketleft",
        "]" => "bracketright",
        "\\" => "backslash",
        ";" => "semicolon",
        "'" => "apostrophe",
        "," => "comma",
        "." => "period",
        "/" => "slash",
        other => other,
    }
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
    fn wtype_args_builds_modifiers_then_key() {
        assert_eq!(
            wtype_args("ctrl+shift+p").unwrap(),
            vec!["-M", "ctrl", "-M", "shift", "-k", "p"]
        );
    }

    #[test]
    fn wtype_args_maps_punctuation_to_xkb_names() {
        assert_eq!(
            wtype_args("ctrl+`").unwrap(),
            vec!["-M", "ctrl", "-k", "grave"]
        );
        assert_eq!(
            wtype_args("ctrl+-").unwrap(),
            vec!["-M", "ctrl", "-k", "minus"]
        );
    }

    #[test]
    fn wtype_args_maps_super_alias_to_logo() {
        assert_eq!(
            wtype_args("super+d").unwrap(),
            vec!["-M", "logo", "-k", "d"]
        );
    }

    #[test]
    fn wtype_args_with_no_modifiers() {
        assert_eq!(wtype_args("Escape").unwrap(), vec!["-k", "Escape"]);
    }

    #[test]
    fn wtype_args_rejects_empty_chord() {
        assert_eq!(wtype_args("   "), Err(ActionError::Empty));
    }

    #[test]
    fn runner_spawns_shell() {
        let mut runner = ActionRunner {
            injector: RecordingInjector::default(),
        };
        runner.run("true").unwrap();
    }
}
