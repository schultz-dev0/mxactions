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

/// Spawns `ydotool` for key chords and mouse clicks (requires `ydotool` + `ydotoold`).
#[derive(Debug, Default)]
pub struct YdotoolInjector;

impl InputInjector for YdotoolInjector {
    fn key_chord(&mut self, chord: &str) -> Result<(), ActionError> {
        let key = chord.replace('-', "+");
        let mut command = std::process::Command::new("ydotool");
        command.args(["key", &key]);
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
    fn runner_spawns_shell() {
        let mut runner = ActionRunner {
            injector: RecordingInjector::default(),
        };
        runner.run("true").unwrap();
    }
}
