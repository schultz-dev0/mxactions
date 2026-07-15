use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TriggerMode {
    /// Press opens the ring, release over a bubble fires it (v1 behavior).
    Hold,
    /// A completed tap opens the ring; a second tap fires or cancels.
    Tap,
}

impl Default for TriggerMode {
    fn default() -> Self {
        TriggerMode::Hold
    }
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
}

pub fn config_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("mxactions")
        .join("command.json")
}

pub const DEFAULT_CONFIG_JSON: &str = include_str!("defaults/command.json");

pub fn parse_config_str(s: &str) -> Result<Config, ConfigError> {
    Ok(serde_json::from_str(s)?)
}

/// First non-wildcard ring whose match_ids contains app_id; else first ring that matches "*"; else None.
pub fn select_ring<'a>(config: &'a Config, app_id: Option<&str>) -> Option<&'a Ring> {
    let app = app_id.unwrap_or("");
    if let Some(ring) = config.rings.iter().find(|r| {
        r.match_ids.iter().any(|m| m != "*" && m == app)
    }) {
        return Some(ring);
    }
    config.rings.iter().find(|r| r.match_ids.iter().any(|m| m == "*"))
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
        let c = parse_config_str(
            r#"{ "ui": { "trigger": "tap" }, "rings": [] }"#,
        )
        .unwrap();
        assert_eq!(c.ui.trigger, TriggerMode::Tap);
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
}
