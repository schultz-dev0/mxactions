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
    #[serde(default = "default_bubble_max")]
    pub bubble_count_max: usize,
    #[serde(default = "default_radius")]
    pub ring_radius: f32,
}

fn default_bubble_max() -> usize {
    8
}
fn default_radius() -> f32 {
    120.0
}

impl Default for UiSettings {
    fn default() -> Self {
        Self {
            bubble_count_max: default_bubble_max(),
            ring_radius: default_radius(),
        }
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
          "ui": { "bubble_count_max": 8, "ring_radius": 120 },
          "rings": [
            {
              "match": ["*"],
              "title": "Desktop",
              "actions": [{ "label": "Launcher", "command": "true" }]
            },
            {
              "match": ["code", "cursor"],
              "title": "VS Code",
              "actions": [{ "label": "Command", "command": "key:ctrl+shift+p" }]
            }
          ]
        }"#,
        )
        .unwrap()
    }

    #[test]
    fn matches_vscode_family() {
        let c = sample();
        assert_eq!(
            select_ring(&c, Some("cursor")).unwrap().title,
            "VS Code"
        );
        assert_eq!(
            select_ring(&c, Some("firefox")).unwrap().title,
            "Desktop"
        );
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
}
