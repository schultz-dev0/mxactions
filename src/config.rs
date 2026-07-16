use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;
use thiserror::Error;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Config {
    #[serde(default)]
    pub ui: UiSettings,
    pub rings: Vec<Ring>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct UiSettings {
    #[serde(default = "default_radius")]
    pub ring_radius: f32,
    #[serde(default)]
    pub trigger: TriggerMode,
}

fn default_radius() -> f32 {
    120.0
}

impl Default for UiSettings {
    fn default() -> Self {
        Self {
            ring_radius: default_radius(),
            trigger: TriggerMode::default(),
        }
    }
}

/// How the Sense Panel opens and confirms a selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum TriggerMode {
    /// Press opens the ring, release over a bubble fires it (v1 behavior).
    #[default]
    Hold,
    /// A completed tap opens the ring; a second tap fires or cancels.
    Tap,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Ring {
    #[serde(rename = "match")]
    pub match_ids: Vec<String>,
    pub title: String,
    pub actions: Vec<RingAction>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RingAction {
    pub label: String,
    /// Nerd Font glyph shown in the bubble; falls back to the first character
    /// of `label` at render time when omitted.
    #[serde(default)]
    pub icon: Option<String>,
    pub command: String,
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("invalid configuration: {0}")]
    Invalid(String),
}

pub fn config_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("mxactions")
        .join("command.json")
}

pub const DEFAULT_CONFIG_JSON: &str = include_str!("defaults/command.json");

pub fn parse_config_str(s: &str) -> Result<Config, ConfigError> {
    let config: Config = serde_json::from_str(s)?;
    if !config.ui.ring_radius.is_finite() || config.ui.ring_radius <= 0.0 {
        return Err(ConfigError::Invalid(
            "ui.ring_radius must be a positive finite number".into(),
        ));
    }
    Ok(config)
}

/// First non-wildcard ring whose match_ids contains app_id; else first ring that matches "*"; else None.
pub fn select_ring<'a>(config: &'a Config, app_id: Option<&str>) -> Option<&'a Ring> {
    let app = app_id.unwrap_or("");
    if let Some(ring) = config
        .rings
        .iter()
        .find(|r| r.match_ids.iter().any(|m| m != "*" && m == app))
    {
        return Some(ring);
    }
    config
        .rings
        .iter()
        .find(|r| r.match_ids.iter().any(|m| m == "*"))
}

pub fn load_or_init(path: &Path) -> Result<Config, ConfigError> {
    if path.exists() {
        let raw = fs::read_to_string(path)?;
        return parse_config_str(&raw);
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, DEFAULT_CONFIG_JSON)?;
    parse_config_str(DEFAULT_CONFIG_JSON)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FileStamp {
    modified: SystemTime,
    len: u64,
}

fn file_stamp(path: &Path) -> Option<FileStamp> {
    let metadata = fs::metadata(path).ok()?;
    Some(FileStamp {
        modified: metadata.modified().ok()?,
        len: metadata.len(),
    })
}

/// Tracks one config file without process-global cache state.
#[derive(Debug)]
pub struct ConfigReloader {
    path: PathBuf,
    last_seen: Option<FileStamp>,
}

impl ConfigReloader {
    pub fn new(path: &Path) -> Self {
        Self {
            path: path.to_path_buf(),
            last_seen: file_stamp(path),
        }
    }

    /// Keeps the last-good config when an observed edit cannot be read or parsed.
    pub fn reload_if_changed(&mut self, config: &mut Config) {
        let stamp = file_stamp(&self.path);
        if stamp == self.last_seen {
            return;
        }
        self.last_seen = stamp;

        if stamp.is_none() {
            log::warn!(
                "config file disappeared, keeping previous config: {}",
                self.path.display()
            );
            return;
        }

        match fs::read_to_string(&self.path)
            .map_err(ConfigError::from)
            .and_then(|s| parse_config_str(&s))
        {
            Ok(new_config) => {
                log::info!("reloaded config from {}", self.path.display());
                *config = new_config;
            }
            Err(e) => log::warn!("config reload failed, keeping previous config: {e}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn sample() -> Config {
        parse_config_str(
            r#"{
          "ui": { "ring_radius": 120 },
          "rings": [
            {
              "match": ["*"],
              "title": "Desktop",
              "actions": [{ "label": "Launcher", "command": "true" }]
            },
            {
              "match": ["code", "cursor"],
              "title": "VS Code",
              "actions": [{ "label": "Command", "icon": "\uf11c", "command": "key:ctrl+shift+p" }]
            }
          ]
        }"#,
        )
        .unwrap()
    }

    #[test]
    fn matches_vscode_family() {
        let c = sample();
        assert_eq!(select_ring(&c, Some("cursor")).unwrap().title, "VS Code");
        assert_eq!(select_ring(&c, Some("firefox")).unwrap().title, "Desktop");
        assert_eq!(select_ring(&c, None).unwrap().title, "Desktop");
    }

    #[test]
    fn writes_defaults_when_missing() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("command.json");
        let cfg = load_or_init(&path).unwrap();
        assert!(path.exists());
        assert!(!cfg.rings.is_empty());
    }

    #[test]
    fn trigger_defaults_to_hold_when_absent() {
        let c = sample();
        assert_eq!(c.ui.trigger, TriggerMode::Hold);
    }

    #[test]
    fn trigger_tap_parses() {
        let c = parse_config_str(r#"{ "ui": { "trigger": "tap" }, "rings": [] }"#).unwrap();
        assert_eq!(c.ui.trigger, TriggerMode::Tap);
    }

    #[test]
    fn rejects_non_positive_ring_radius() {
        for radius in ["0", "-1"] {
            let error = parse_config_str(&format!(
                r#"{{ "ui": {{ "ring_radius": {radius} }}, "rings": [] }}"#
            ))
            .unwrap_err();
            assert!(matches!(error, ConfigError::Invalid(_)));
        }
    }

    #[test]
    fn icon_is_optional_and_falls_back_to_none() {
        let c = sample();
        let desktop = select_ring(&c, None).unwrap();
        assert_eq!(desktop.actions[0].icon, None);
        let vscode = select_ring(&c, Some("cursor")).unwrap();
        assert_eq!(vscode.actions[0].icon.as_deref(), Some("\u{f11c}"));
    }

    #[test]
    fn stray_bubble_count_max_key_is_ignored_for_backward_compat() {
        // Old configs written by the v1 build still have this key; it must not fail parsing.
        let c = parse_config_str(
            r#"{ "ui": { "bubble_count_max": 8, "ring_radius": 120 }, "rings": [] }"#,
        )
        .unwrap();
        assert_eq!(c.ui.ring_radius, 120.0);
    }

    #[test]
    fn reload_if_changed_picks_up_edits() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("command.json");
        let mut config = load_or_init(&path).unwrap();
        let mut reloader = ConfigReloader::new(&path);
        assert_eq!(config.ui.ring_radius, 120.0);

        fs::write(&path, r#"{ "ui": { "ring_radius": 200 }, "rings": [] }"#).unwrap();

        reloader.reload_if_changed(&mut config);
        assert_eq!(config.ui.ring_radius, 200.0);
    }

    #[test]
    fn reload_if_changed_keeps_last_good_config_on_bad_json() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("command.json");
        let mut config = load_or_init(&path).unwrap();
        let mut reloader = ConfigReloader::new(&path);
        fs::write(&path, "not json").unwrap();

        reloader.reload_if_changed(&mut config);
        assert_eq!(config.ui.ring_radius, 120.0); // unchanged, not crashed
    }

    #[test]
    fn reload_if_changed_is_a_noop_without_a_new_mtime() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("command.json");
        let mut config = load_or_init(&path).unwrap();
        let mut reloader = ConfigReloader::new(&path);

        // Sentinel: a real reparse of the unchanged file would produce ring_radius
        // 120.0, never -999.0. If the second call below actually skips (cache hit
        // on unchanged mtime), this survives untouched.
        config.ui.ring_radius = -999.0;

        reloader.reload_if_changed(&mut config);

        assert_eq!(config.ui.ring_radius, -999.0);
    }
}
