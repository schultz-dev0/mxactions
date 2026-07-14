# mxactions Design Spec

**Date:** 2026-07-14  
**Status:** Approved for planning (pending final user review of this document)  
**Approach:** Single Rust daemon that owns the MX Master 4 Haptic Sense Panel via HID++ divert and presents a Wayland Actions Ring.

## Goal

Replicate the Logitech MX Master 4 **Actions Ring** (opened from the Haptic Sense Panel) on Linux under **Wayland**, for any focused app via configurable rings. v1 proves the core hold → hover → release loop with a small default config and dual-purpose action strings (shell / key / click).

## Non-goals (v1)

- Config GUI or CLI editor
- Live config reload / file watcher
- X11 support
- Hyprland-only APIs as a hard requirement (stay compositor-portable within Wayland)
- Software-driven haptic ticks when crossing bubbles
- Tap-to-open + left-click selection mode
- Parity with every Logi Options+ plugin / marketplace
- Coexistence test matrix against every Solaar version

## Requirements summary

| Area | Decision |
|------|----------|
| Trigger | MX Master 4 Haptic Sense Panel only (mxactions **owns** / diverts it) |
| Selection | Press-and-hold → hover bubble → **release** to fire; release outside / on hub cancels |
| Platform | Wayland generally |
| Actions | `key:…` / `click:…` injection, or shell via `sh -c` |
| Config | `~/.config/mxactions/command.json` (swaync-style); defaults written on first run |
| Default rings | Generic desktop (`match: ["*"]`) + unified VS Code family |
| Packaging | Rust binary; typically run as a user systemd service |

## Architecture

One long-running process with clear modules:

```
MX Master 4 ──HID++──► HID++ Client ──press/release──► Ring Controller
                                                          │
                     Focus Tracker (app-id) ──────────────┤
                     Preset Store (command.json) ─────────┤
                                                          ├──► Overlay UI (layer-shell ring)
                                                          └──► Action Runner (keys / clicks / shell)
```

### Components

1. **HID++ Client** — Discover MX Master 4 (receiver and/or Bluetooth), divert the Haptic Sense Panel control (CID historically reported as ~`0x01A0` / “Haptic” in Solaar), emit press and release events. Prefer exclusive divert so the panel does not emit OS “Back” (or other) keys.
2. **Ring Controller** — State machine: `closed` → `open` on press; track hovered bubble; on release `commit` or `cancel`; never stack multiple rings; freeze the selected preset at open time.
3. **Overlay UI** — Transparent layer-shell surface; radial bubbles + center hub; highlights on hover; opens centered on cursor.
4. **Focus Tracker** — Best-effort focused window app-id via Wayland foreign-toplevel protocols (`ext-foreign-toplevel-list` / `zwlr-foreign-toplevel` as available). If unavailable → treat as generic.
5. **Preset Store** — Load `command.json`; match rings; expose the active ring’s title + actions to the controller/UI.
6. **Action Runner** — Parse and execute the selected action string; log failures without crashing the daemon.

## Interaction model

1. User **presses** the Haptic Sense Panel → daemon opens the ring at the cursor with the ring matched for the focused app (frozen for this hold).
2. User **moves the pointer** while holding → the bubble whose hit circle contains the pointer is highlighted (no nearest-neighbor steal from empty space).
3. User **releases**:
   - Over a bubble → run that action, dismiss overlay.
   - Over hub or empty space → cancel, dismiss overlay.
4. Second press while already open: do not open another ring (ignore / treat as continued session).
5. Haptic feedback between bubbles: **out of scope for v1**.

Bubble count: driven by the matched ring’s `actions` list (UI may cap via `ui.bubble_count_max`, default suggestion 8).

## Configuration

### Path

`$XDG_CONFIG_HOME/mxactions/command.json` (default: `$HOME/.config/mxactions/command.json`).

### First run

If the file does not exist, create the directory and write **bundled defaults** (generic + VS Code family rings).

### Reload

v1 reads the file at **startup only**. After editing, restart `mxactions`.

### Schema

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
        { "label": "Launcher", "command": "walker" }
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

### Matching rules

- Each ring has `match`: a list of Wayland app-ids (exact string match, case-sensitive as reported by the compositor).
- Special entry `["*"]` (or a ring whose match includes `*`) is the **generic fallback**.
- Prefer the **first** non-wildcard ring whose `match` contains the focused app-id; else use the wildcard ring.
- If no rings match at all, still open the overlay with **hub only** (no bubbles) so release always cancels cleanly; log a warning.

### Action `command` string

Single string field (swaync-like simplicity):

| Form | Meaning |
|------|---------|
| `key:…` | Inject keyboard chord into the focused client (e.g. `key:ctrl+shift+p`) |
| `click:…` | Inject a mouse button: `click:left`, `click:right`, `click:middle` |
| anything else | Run via `sh -c` as the user (non-blocking spawn; do not block the daemon on long jobs) |

Multi-step sequences are out of v1 JSON; users compose with shell scripts if needed.

## Platform plumbing

### HID++

- Use `hidapi` (or equivalent) and Logitech HID++ to divert the Sense Panel.
- On divert conflict (e.g. Solaar already diverting): log a clear error; prefer failing startup if ownership cannot be obtained (matches “mxactions owns the button”).
- Support reconnect: if the device disappears, retry without crashing.

### Overlay

- Wayland **layer-shell** radial overlay.
- Prefer **iced** + layer-shell integration; fall back to **GTK4 Layer Shell** if iced path is blocked.
- Transparent, input-active only while open.

### Focus

- foreign-toplevel protocols when present; otherwise always generic (`*` ring).

### Injection

- Document **one** primary Wayland-wide path for keys/clicks (prefer uinput-style injection; use xdg-desktop-portal RemoteDesktop only if uinput is insufficient on target compositors).
- Shell actions always run as `sh -c "<command>"` with the user’s environment; the config file is trusted code.

## Errors & edge cases

| Situation | Behavior |
|-----------|----------|
| Device missing / disconnect | Log, reconnect loop; ring unavailable until device returns |
| Divert fails | Clear stderr/journal error; non-zero exit if divert required at startup |
| Layer-shell unsupported | Fail startup with explicit message |
| No focus protocol | Use `*` ring |
| Action failure | Log; ring already closed; no automatic retry |
| Shell actions | Run as the user; document trust boundary (config file is code) |
| Mid-hold app switch | Preset frozen at open |

## Testing

- **Unit:** JSON parse; ring match (`*` vs vscode family); command string prefix dispatch; hit-test geometry (bubble / hub / outside).
- **Integration (no hardware):** synthetic press/release stream → controller transitions (open / commit / cancel).
- **Manual hardware:** divert panel; hold-release fire; cancel outside; VS Code vs desktop match; one `key:` and one shell action.
- **Not in v1 CI:** full compositor matrix, Solaar coexistence battery, haptic waveforms.

## Default content (indicative)

Exact labels/commands can change during implementation; must ship:

1. **Desktop / `*`** — a few safe desktop helpers appropriate to the author’s environment (launcher, etc.).
2. **VS Code family** — editor-centric shortcuts (`key:` forms) for Command Palette-style workflows.

## Future work (explicitly later)

- Live config reload
- Tap-open + click-to-select mode
- Haptic ticks via HID++ when crossing bubbles
- Per-user richer action objects / sequences in JSON
- Config GUI
- Broader handmade presets (Zed, browsers, creative tools)
- Smoother Solaar coexistence mode

## Success criteria (v1)

1. With MX Master 4 connected on a supported Wayland compositor, holding the Haptic Sense Panel opens the ring and releasing on a bubble runs the configured command.
2. Sense Panel does not simultaneously act as browser Back (or other OS binding) while mxactions is running with a successful divert.
3. Focusing VS Code (or a listed fork) vs the desktop selects different rings from `command.json`.
4. Missing config is created with defaults; editing the file and restarting changes the ring.
