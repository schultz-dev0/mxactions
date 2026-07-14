# mxactions Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.
>
> **Commits:** This plan includes commit steps for clean history. Do **not** commit unless the user explicitly asks in that turn (project rule). Skip or pause on commit steps until asked.

**Goal:** Ship a Wayland Rust daemon that diverts the MX Master 4 Haptic Sense Panel, shows a hold-to-select Actions Ring, and runs actions from `~/.config/mxactions/command.json`.

**Architecture:** Single binary/library split: pure logic (config, match, hit-test, controller, action parse) is unit-tested without hardware; HID++, focus, overlay, and injection are thin adapters behind traits. Event loop: HID press/release → controller → overlay show/hide → action runner.

**Tech Stack:** Rust 2024, `serde`/`serde_json`, `hidapi` (HID++), `iced` + `iced_layershell` (overlay; fall back to GTK4 Layer Shell only if iced path blocks), `wayland-client` + protocols for focus, `sh -c` for shell actions, uinput/`enigo` (or `ydotool` spawn) for `key:`/`click:`.

**Spec:** `docs/superpowers/specs/2026-07-14-mxactions-design.md`

---

## File map

| Path | Responsibility |
|------|----------------|
| `Cargo.toml` | Package metadata + dependencies |
| `src/lib.rs` | Library root; re-exports modules |
| `src/main.rs` | Daemon entry: load config, start adapters, run loop |
| `src/config.rs` | `command.json` types, load/save defaults, ring matching |
| `src/action.rs` | Parse `key:`/`click:`/shell; `ActionRunner` + injector trait |
| `src/geometry.rs` | Ring layout + pointer hit-test (bubble / hub / miss) |
| `src/controller.rs` | Hold/open/hover/release state machine |
| `src/focus.rs` | `FocusSource` trait + Wayland foreign-toplevel impl |
| `src/hidpp/mod.rs` | `HidEventSource` trait |
| `src/hidpp/mx_master4.rs` | Device open, Sense Panel divert, press/release |
| `src/overlay/mod.rs` | Overlay commands (`Show`/`Hide`/`SetHover`) |
| `src/overlay/ring.rs` | iced_layershell radial UI |
| `src/defaults/command.json` | Bundled defaults (`include_str!`) |
| `contrib/mxactions.service` | User systemd unit template |
| `tests/*.rs` | Integration-style lib tests if not all in-module |

---

### Task 1: Library scaffold

**Files:**
- Modify: `Cargo.toml`
- Create: `src/lib.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: Turn the crate into lib + bin**

Set `Cargo.toml` to:

```toml
[package]
name = "mxactions"
version = "0.1.0"
edition = "2024"
license = "MIT"
description = "Wayland Actions Ring for the Logitech MX Master 4 Haptic Sense Panel"

[dependencies]
serde = { version = "1", features = ["derive"] }
serde_json = "1"
thiserror = "2"
dirs = "6"

[dev-dependencies]
tempfile = "3"
```

Replace `src/lib.rs` with:

```rust
pub mod action;
pub mod config;
pub mod controller;
pub mod geometry;

pub use action::{Action, ActionError, parse_command};
pub use config::{Config, ConfigError, Ring, load_or_init};
pub use controller::{Controller, ControllerEvent, RingCommand};
pub use geometry::{Hit, RingLayout, hit_test};
```

Replace `src/main.rs` with:

```rust
fn main() {
    eprintln!("mxactions: daemon wiring comes in later tasks");
}
```

Create stub modules so the lib compiles:

```rust
// src/config.rs
#![allow(dead_code)]
```

(and the same one-liner allow for `action.rs`, `controller.rs`, `geometry.rs` for now — Task 2+ fills them.)

- [ ] **Step 2: Verify build**

Run: `cargo build`

Expected: success (warnings OK).

- [ ] **Step 3: Commit** (only if user asked)

```bash
git add Cargo.toml src/
git commit -m "$(cat <<'EOF'
chore: scaffold mxactions lib/bin module layout

EOF
)"
```

---

### Task 2: Config load, defaults, ring match

**Files:**
- Create: `src/defaults/command.json`
- Modify: `src/config.rs`
- Test: unit tests inside `src/config.rs`

- [ ] **Step 1: Write failing tests for match + parse**

Replace `src/config.rs` with types + tests first (implementations can `todo!()` initially, but prefer writing full tests then minimal impl in steps 3–4).

```rust
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

fn default_bubble_max() -> usize { 8 }
fn default_radius() -> f32 { 120.0 }

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
    pub match_ids: Vec<String>,
    pub title: String,
    pub actions: Vec<RingAction>,
}

// JSON field is "match" — use rename
// In real code:
// #[serde(rename = "match")]
// pub match_ids: Vec<String>,

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
        parse_config_str(r#"{
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
        }"#).unwrap()
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
}
```

**Important:** use `#[serde(rename = "match")]` on `match_ids` so JSON matches the spec (`"match": [...]`).

- [ ] **Step 2: Add bundled default JSON**

Create `src/defaults/command.json`:

```json
{
  "ui": {
    "bubble_count_max": 8,
    "ring_radius": 120
  },
  "rings": [
    {
      "match": ["*"],
      "title": "Desktop",
      "actions": [
        { "label": "Launcher", "command": "true" }
      ]
    },
    {
      "match": ["code", "code-oss", "cursor", "codium", "vscodium"],
      "title": "VS Code",
      "actions": [
        { "label": "Command", "command": "key:ctrl+shift+p" },
        { "label": "Terminal", "command": "key:ctrl+`" },
        { "label": "Format", "command": "key:shift+alt+f" }
      ]
    }
  ]
}
```

- [ ] **Step 3: Run tests**

Run: `cargo test -q config::`

Expected: PASS for `matches_vscode_family` and `writes_defaults_when_missing`.

- [ ] **Step 4: Commit** (only if user asked)

```bash
git add src/config.rs src/defaults/command.json
git commit -m "$(cat <<'EOF'
feat: load command.json with defaults and app-id ring match

EOF
)"
```

---

### Task 3: Action command parsing

**Files:**
- Modify: `src/action.rs`

- [ ] **Step 1: Write tests + implementation**

```rust
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
}

pub fn parse_command(raw: &str) -> Result<Action, ActionError> {
    let s = raw.trim();
    if s.is_empty() {
        return Err(ActionError::Empty);
    }
    if let Some(rest) = s.strip_prefix("key:") {
        return Ok(Action::Keys(rest.to_string()));
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
        assert_eq!(parse_command("key:ctrl+shift+p").unwrap(), Action::Keys("ctrl+shift+p".into()));
        assert_eq!(parse_command("click:left").unwrap(), Action::Click(ClickButton::Left));
        assert_eq!(parse_command("walker").unwrap(), Action::Shell("walker".into()));
    }
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test -q action::`

Expected: PASS.

- [ ] **Step 3: Commit** (only if user asked)

```bash
git add src/action.rs
git commit -m "$(cat <<'EOF'
feat: parse key/click/shell action command strings

EOF
)"
```

---

### Task 4: Ring geometry hit-test

**Files:**
- Modify: `src/geometry.rs`

- [ ] **Step 1: Implement layout + hit-test with tests**

Place bubbles evenly on a circle of `ring_radius` around origin; hub radius = `ring_radius * 0.3`; bubble radius = `ring_radius * 0.28`.

```rust
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Hit {
    Hub,
    Bubble(usize),
    Miss,
}

#[derive(Debug, Clone)]
pub struct RingLayout {
    pub hub_radius: f32,
    pub bubble_radius: f32,
    pub bubbles: Vec<(f32, f32)>, // centers
}

impl RingLayout {
    pub fn new(action_count: usize, ring_radius: f32) -> Self {
        let hub_radius = ring_radius * 0.3;
        let bubble_radius = ring_radius * 0.28;
        let bubbles = if action_count == 0 {
            vec![]
        } else {
            (0..action_count)
                .map(|i| {
                    let a = (i as f32) * std::f32::consts::TAU / action_count as f32
                        - std::f32::consts::FRAC_PI_2;
                    (ring_radius * a.cos(), ring_radius * a.sin())
                })
                .collect()
        };
        Self { hub_radius, bubble_radius, bubbles }
    }
}

pub fn hit_test(layout: &RingLayout, x: f32, y: f32) -> Hit {
    for (i, (bx, by)) in layout.bubbles.iter().enumerate() {
        let dx = x - bx;
        let dy = y - by;
        if dx * dx + dy * dy <= layout.bubble_radius * layout.bubble_radius {
            return Hit::Bubble(i);
        }
    }
    if x * x + y * y <= layout.hub_radius * layout.hub_radius {
        return Hit::Hub;
    }
    Hit::Miss
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hits_top_bubble_and_hub() {
        let layout = RingLayout::new(4, 100.0);
        // first bubble at angle -PI/2 → (0, -100)
        assert_eq!(hit_test(&layout, 0.0, -100.0), Hit::Bubble(0));
        assert_eq!(hit_test(&layout, 0.0, 0.0), Hit::Hub);
        assert_eq!(hit_test(&layout, 200.0, 200.0), Hit::Miss);
    }
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test -q geometry::`

Expected: PASS.

- [ ] **Step 3: Commit** (only if user asked)

```bash
git add src/geometry.rs
git commit -m "$(cat <<'EOF'
feat: radial ring layout and pointer hit-test

EOF
)"
```

---

### Task 5: Ring controller state machine

**Files:**
- Modify: `src/controller.rs`

- [ ] **Step 1: Implement controller + tests**

```rust
use crate::config::{Config, Ring};
use crate::geometry::{Hit, RingLayout, hit_test};

#[derive(Debug, Clone, PartialEq)]
pub enum RingCommand {
    Show {
        title: String,
        labels: Vec<String>,
        layout: RingLayout,
    },
    SetHover(Option<usize>),
    Hide,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControllerEvent {
    Press,
    Pointer { x: f32, y: f32 }, // coords relative to ring center
    Release,
}

#[derive(Debug)]
pub struct Controller {
    open: bool,
    layout: Option<RingLayout>,
    labels: Vec<String>,
    title: String,
    hover: Option<usize>,
    /// Index into config.rings frozen at open, plus actions for commit.
    actions: Vec<String>,
}

impl Controller {
    pub fn new() -> Self {
        Self {
            open: false,
            layout: None,
            labels: vec![],
            title: String::new(),
            hover: None,
            actions: vec![],
        }
    }

    pub fn handle(
        &mut self,
        event: ControllerEvent,
        config: &Config,
        app_id: Option<&str>,
    ) -> (Vec<RingCommand>, Option<String>) {
        let mut cmds = Vec::new();
        let mut fire: Option<String> = None;

        match event {
            ControllerEvent::Press => {
                if self.open {
                    return (cmds, None);
                }
                let ring = crate::config::select_ring(config, app_id);
                let (title, labels, actions) = match ring {
                    Some(r) => Self::from_ring(r, config.ui.bubble_count_max),
                    None => ("?".into(), vec![], vec![]),
                };
                let layout = RingLayout::new(labels.len(), config.ui.ring_radius);
                self.open = true;
                self.title = title.clone();
                self.labels = labels.clone();
                self.actions = actions;
                self.layout = Some(layout.clone());
                self.hover = None;
                cmds.push(RingCommand::Show { title, labels, layout });
            }
            ControllerEvent::Pointer { x, y } => {
                if !self.open {
                    return (cmds, None);
                }
                let layout = self.layout.as_ref().unwrap();
                let hover = match hit_test(layout, x, y) {
                    Hit::Bubble(i) => Some(i),
                    _ => None,
                };
                if hover != self.hover {
                    self.hover = hover;
                    cmds.push(RingCommand::SetHover(hover));
                }
            }
            ControllerEvent::Release => {
                if !self.open {
                    return (cmds, None);
                }
                if let Some(i) = self.hover {
                    fire = self.actions.get(i).cloned();
                }
                self.open = false;
                self.layout = None;
                self.hover = None;
                cmds.push(RingCommand::Hide);
            }
        }
        (cmds, fire)
    }

    fn from_ring(ring: &Ring, max: usize) -> (String, Vec<String>, Vec<String>) {
        let take = ring.actions.len().min(max);
        let labels = ring.actions[..take].iter().map(|a| a.label.clone()).collect();
        let actions = ring.actions[..take].iter().map(|a| a.command.clone()).collect();
        (ring.title.clone(), labels, actions)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::parse_config_str;

    fn cfg() -> Config {
        parse_config_str(crate::config::DEFAULT_CONFIG_JSON).unwrap()
    }

    #[test]
    fn press_release_on_bubble_fires() {
        let mut c = Controller::new();
        let config = cfg();
        let (cmds, fire) = c.handle(ControllerEvent::Press, &config, Some("cursor"));
        assert!(matches!(cmds[0], RingCommand::Show { .. }));
        assert!(fire.is_none());

        // pointer on first bubble of 3-action vscode ring
        let layout = match &cmds[0] {
            RingCommand::Show { layout, .. } => layout.clone(),
            _ => panic!(),
        };
        let (bx, by) = layout.bubbles[0];
        let (_cmds, _) = c.handle(ControllerEvent::Pointer { x: bx, y: by }, &config, Some("cursor"));
        let (cmds, fire) = c.handle(ControllerEvent::Release, &config, Some("cursor"));
        assert_eq!(cmds, vec![RingCommand::Hide]);
        assert_eq!(fire.as_deref(), Some("key:ctrl+shift+p"));
    }

    #[test]
    fn release_on_miss_cancels() {
        let mut c = Controller::new();
        let config = cfg();
        c.handle(ControllerEvent::Press, &config, None);
        c.handle(ControllerEvent::Pointer { x: 999.0, y: 999.0 }, &config, None);
        let (_, fire) = c.handle(ControllerEvent::Release, &config, None);
        assert!(fire.is_none());
    }
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test -q controller::`

Expected: PASS.

- [ ] **Step 3: Commit** (only if user asked)

```bash
git add src/controller.rs
git commit -m "$(cat <<'EOF'
feat: hold-to-select ring controller state machine

EOF
)"
```

---

### Task 6: Action runner (shell + injectable keys/clicks)

**Files:**
- Modify: `src/action.rs`

- [ ] **Step 1: Add traits + runner + shell test**

Append to `src/action.rs`:

```rust
use std::process::Command as ProcCommand;

pub trait InputInjector {
    fn key_chord(&mut self, chord: &str) -> Result<(), ActionError>;
    fn click(&mut self, button: ClickButton) -> Result<(), ActionError>;
}

#[derive(Debug, Default)]
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

pub struct ActionRunner<I: InputInjector> {
    pub injector: I,
}

impl<I: InputInjector> ActionRunner<I> {
    pub fn run(&mut self, raw: &str) -> Result<(), ActionError> {
        match parse_command(raw)? {
            Action::Keys(chord) => self.injector.key_chord(&chord),
            Action::Click(btn) => self.injector.click(btn),
            Action::Shell(cmd) => {
                ProcCommand::new("sh")
                    .arg("-c")
                    .arg(&cmd)
                    .spawn()
                    .map_err(|e| ActionError::Io(e))?;
                Ok(())
            }
        }
    }
}

// Extend ActionError:
// #[error("io: {0}")]
// Io(#[from] std::io::Error),
// Remove PartialEq on ActionError if Io variant breaks it, or map Io to String.
```

Adjust `ActionError` so tests still work (`Io(String)` or drop `PartialEq`).

Add test:

```rust
#[test]
fn runner_records_keys() {
    let mut runner = ActionRunner { injector: RecordingInjector::default() };
    runner.run("key:ctrl+p").unwrap();
    assert_eq!(runner.injector.keys, vec!["ctrl+p"]);
}
```

Shell smoke (optional): `runner.run("true")` should spawn without error.

- [ ] **Step 2: Run tests**

Run: `cargo test -q action::`

Expected: PASS.

- [ ] **Step 3: Commit** (only if user asked)

```bash
git add src/action.rs
git commit -m "$(cat <<'EOF'
feat: action runner with shell spawn and injectable input

EOF
)"
```

---

### Task 7: Focus source trait + stub

**Files:**
- Create: `src/focus.rs`
- Modify: `src/lib.rs`

- [ ] **Step 1: Define trait and static stub**

```rust
pub trait FocusSource {
    fn focused_app_id(&self) -> Option<String>;
}

#[derive(Debug, Default, Clone)]
pub struct StaticFocus(pub Option<String>);

impl FocusSource for StaticFocus {
    fn focused_app_id(&self) -> Option<String> {
        self.0.clone()
    }
}
```

Wire `pub mod focus;` in `lib.rs`.

- [ ] **Step 2: Build**

Run: `cargo test -q`

Expected: all existing tests PASS.

- [ ] **Step 3: Commit** (only if user asked)

```bash
git add src/focus.rs src/lib.rs
git commit -m "$(cat <<'EOF'
feat: add FocusSource trait and static stub

EOF
)"
```

Later task adds Wayland foreign-toplevel behind the same trait. Daemon can ship with stub returning `None` (always Desktop) until Wayland focus lands — but Task 11 should implement real focus before “done”.

---

### Task 8: Overlay module + iced_layershell ring (show/hide)

**Files:**
- Create: `src/overlay/mod.rs`, `src/overlay/ring.rs`
- Modify: `Cargo.toml` — add `iced`, `iced_layershell`, `tokio` if required by iced version
- Modify: `src/lib.rs`

- [ ] **Step 1: Add dependencies matching iced 0.14 / iced_layershell 0.18**

Pin versions that compile together (check docs.rs examples at implementation time). Minimum:

```toml
iced = "0.14"
iced_layershell = "0.18"
```

- [ ] **Step 2: Skeleton overlay driven by channels**

Design: overlay runs on the UI thread; `main` sends `RingCommand` over `std::sync::mpsc` or `iced` subscription channel.

`src/overlay/mod.rs` should expose:

```rust
pub mod ring;
pub use ring::run_overlay;
```

`ring.rs`: iced_layershell application that:
- Starts hidden / zero input region
- On `Show`, renders hub + labeled bubbles using `RingLayout`
- On `SetHover`, thickens stroke on index
- On `Hide`, clears and shrinks input region

Use a dark translucent full-output layer, center at cursor position passed in `Show` (extend `RingCommand::Show` with `cursor: (i32, i32)` if needed — update controller + tests in the same change).

- [ ] **Step 3: Manual visual check** (optional harness)

Add `examples/overlay_preview.rs` or `cargo run -- --preview-overlay` that shows a fake ring for 3 seconds.

Run: `cargo run --example overlay_preview` (if created)

Expected: radial ring appears on Wayland.

- [ ] **Step 4: Commit** (only if user asked)

```bash
git add Cargo.toml src/overlay/ src/controller.rs src/lib.rs
git commit -m "$(cat <<'EOF'
feat: iced layer-shell Actions Ring overlay

EOF
)"
```

**Fallback:** If `iced_layershell` integration stalls >0.5 day, switch this task to `gtk4` + `gtk4-layer-shell` with the same `RingCommand` API — do not change controller/config.

---

### Task 9: HID++ Sense Panel divert (MX Master 4)

**Files:**
- Create: `src/hidpp/mod.rs`, `src/hidpp/mx_master4.rs`
- Modify: `Cargo.toml` — add `hidapi`, `log`, `env_logger`
- Modify: `src/lib.rs`

- [ ] **Step 1: Define event source trait**

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HidEvent {
    Press,
    Release,
}

pub trait HidEventSource {
    /// Blocking or next-event poll used by the daemon loop.
    fn recv_timeout(&mut self, timeout: std::time::Duration) -> Option<HidEvent>;
}

pub struct MockHid {
    pub q: std::collections::VecDeque<HidEvent>,
}

impl HidEventSource for MockHid {
    fn recv_timeout(&mut self, _t: std::time::Duration) -> Option<HidEvent> {
        self.q.pop_front()
    }
}
```

- [ ] **Step 2: Research + implement divert**

Work from Solaar / HID++ 2.0 knowledge:

1. Open MX Master 4 via `hidapi` (Bluetooth or Bolt receiver path).
2. Locate reprogrammable controls feature; find Sense Panel CID (commonly `0x01A0`).
3. **Divert** the control so OS does not see Back.
4. Subscribe to diversion notifications → map to `HidEvent::Press` / `Release`.
5. On drop / shutdown, undivert (best effort).

If divert fails (permissions / Solaar conflict): return a typed error; `main` prints and exits non-zero.

Implement against real hardware on the author’s machine; keep `MockHid` for automated tests.

- [ ] **Step 3: Hardware smoke**

Run a temporary binary path or `cargo run -- --hid-test` that prints press/release without overlay.

Expected: holding Sense Panel prints `press` then `release`; browser Back does not fire.

- [ ] **Step 4: Commit** (only if user asked)

```bash
git add Cargo.toml src/hidpp/ src/lib.rs
git commit -m "$(cat <<'EOF'
feat: divert MX Master 4 haptic sense panel via HID++

EOF
)"
```

---

### Task 10: Wayland focus tracker

**Files:**
- Modify: `src/focus.rs`
- Modify: `Cargo.toml` — `wayland-client`, `wayland-protocols`, `wayland-protocols-wlr` as needed

- [ ] **Step 1: Implement `WaylandFocus`**

Bind `ext-foreign-toplevel-list-v1` and/or `zwlr_foreign_toplevel_manager_v1`, track activated toplevel’s `app_id`, expose via `FocusSource`.

On compositor without protocol: `focused_app_id()` returns `None` (Desktop ring).

- [ ] **Step 2: Manual check**

Log app-id whenever focus changes while mxactions runs with `RUST_LOG=debug`.

Expected: focusing Cursor/VS Code shows `cursor` / `code`; focusing Firefox falls through to Desktop.

- [ ] **Step 3: Commit** (only if user asked)

```bash
git add src/focus.rs Cargo.toml
git commit -m "$(cat <<'EOF'
feat: track focused Wayland app-id for ring matching

EOF
)"
```

---

### Task 11: Real key/click injector + daemon main loop

**Files:**
- Modify: `src/action.rs` — add `UinputInjector` or `YdotoolInjector`
- Modify: `src/main.rs`
- Create: `contrib/mxactions.service`
- Modify: `README.md`

- [ ] **Step 1: Pick one injector and implement**

Preferred order:
1. Direct uinput (`evdev`/`uinput` crate) if `/dev/uinput` is usable for the user.
2. Else spawn `ydotool` / `wtype` with a documented dependency.

Preferred v1 injector (document `ydotool` + `ydotoold` as a dependency):

```rust
pub struct YdotoolInjector;

impl InputInjector for YdotoolInjector {
    fn key_chord(&mut self, chord: &str) -> Result<(), ActionError> {
        // "ctrl+shift+p" → ["ctrl+shift+p"] after normalizing separators for ydotool.
        let key = chord.replace('-', "+");
        ProcCommand::new("ydotool")
            .args(["key", &key])
            .spawn()
            .map_err(|e| ActionError::Io(e))?;
        Ok(())
    }
    fn click(&mut self, button: ClickButton) -> Result<(), ActionError> {
        let code = match button {
            ClickButton::Left => "0xC0",
            ClickButton::Right => "0xC1",
            ClickButton::Middle => "0xC2",
        };
        ProcCommand::new("ydotool")
            .args(["click", code])
            .spawn()
            .map_err(|e| ActionError::Io(e))?;
        Ok(())
    }
}
```

If ydotool key syntax differs on the installed version, adjust the argv in this task after a one-line smoke test (`ydotool key ctrl+shift+p`); keep the injector as the single place that knows the CLI.

- [ ] **Step 2: Wire `main`**

```rust
fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();
    let path = mxactions::config::config_path();
    let config = mxactions::config::load_or_init(&path)?;
    // spawn overlay thread / iced runtime with command channel
    // open HidMxMaster4
    // loop: hid events + pointer positions → controller.handle → overlay + runner
    let _ = config;
    Ok(())
}
```

Pointer positions while open: read global cursor (compositor-specific or `Pointer` device). For v1 on wlroots/Hyprland, use a practical method (e.g. Hyprland IPC cursor pos **only as optional fast path**; prefer a compositor-agnostic approach such as tracking pointer motion on the layer surface once open — pointer is already over the overlay).

**v1 pointer rule:** once the layer-shell surface is open and focused for input, use pointer motion events **in surface coordinates**, convert to ring-center-relative, feed `ControllerEvent::Pointer`.

- [ ] **Step 3: systemd user unit**

`contrib/mxactions.service`:

```ini
[Unit]
Description=mxactions — MX Master 4 Actions Ring
PartOf=graphical-session.target
After=graphical-session.target

[Service]
ExecStart=%h/.local/bin/mxactions
Restart=on-failure

[Install]
WantedBy=graphical-session.target
```

- [ ] **Step 4: README**

Update `README.md` with: what it is, Wayland-only, config path + schema summary, Solaar divert conflict note, install/run, restart after config edit.

- [ ] **Step 5: End-to-end hardware verification**

Checklist (manual):
1. Config created under `~/.config/mxactions/command.json`
2. Hold Sense Panel → ring shows
3. Release on bubble → `key:` or shell runs
4. Release outside → cancel
5. Focus VS Code fork → VS Code titles; elsewhere → Desktop
6. Sense Panel does not emit Back while diverted

- [ ] **Step 6: Commit** (only if user asked)

```bash
git add src/main.rs src/action.rs contrib/README.md README.md
git commit -m "$(cat <<'EOF'
feat: wire daemon loop, injector, and systemd unit

EOF
)"
```

---

## Spec coverage check

| Spec item | Task |
|-----------|------|
| HID++ own/divert Sense Panel | 9 |
| Hold → hover → release | 5, 8, 11 |
| Cancel outside/hub | 4, 5 |
| Wayland generally / layer-shell | 8 |
| `command.json` + first-run defaults | 2 |
| `key:` / `click:` / shell `sh -c` | 3, 6, 11 |
| VS Code family + `*` rings | 2 + defaults JSON |
| Focus via foreign-toplevel | 10 |
| Errors: divert fail, reconnect | 9, 11 |
| Unit tests: match, parse, hit-test, controller | 2–5 |
| No GUI / no live reload | honored (README says restart) |

## Plan self-review notes

- `RingCommand::Show` may gain `origin_x/origin_y` when overlay lands; update controller tests in Task 8 in the same commit.
- `ActionError` must stay consistent after adding `Io` (drop `PartialEq` or use `Io(String)`).
- GTK fallback only if iced is blocked — same `RingCommand` API.

---

## Execution handoff

Plan complete and saved to `docs/superpowers/plans/2026-07-14-mxactions.md`. Two execution options:

1. **Subagent-Driven (recommended)** — fresh subagent per task, review between tasks  
2. **Inline Execution** — execute tasks in this session with executing-plans checkpoints  

Which approach?
