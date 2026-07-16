# mxactions

Wayland daemon that diverts the Logitech **MX Master 4** Haptic Sense Panel and shows a hold-to-select **Actions Ring** at the pointer. Actions come from `~/.config/mxactions/command.json`.

**Wayland only** — no X11 support in v1.

## Requirements

- MX Master 4 connected via Bluetooth or Bolt receiver
- wlroots-based compositor (Hyprland, Sway, River, etc.) for layer-shell overlay and focus tracking
- [`ydotool`](https://github.com/ReimuNotMoe/ydotool) + `ydotoold` for `key:` and `click:` actions
- `ttf-nerd-fonts-symbols` (Arch) or equivalent Nerd Font package for icon glyphs in bubbles (an action with no `icon` set falls back to the first letter of its label; an `icon` set without the font installed renders as a blank/tofu glyph rather than falling back)
- udev permissions to open the mouse HID++ interface (see below)

## Solaar / OpenLogi conflict

mxactions **owns** the Sense Panel via HID++ temporary diversion. If **Solaar**, **OpenLogi**, or another tool already diverts the panel, startup fails with a clear error. Quit those tools before running mxactions.

## Configuration

**Path:** `~/.config/mxactions/command.json` (or `$XDG_CONFIG_HOME/mxactions/command.json`)

On first run, mxactions creates the directory and writes bundled defaults (Desktop `*` ring + VS Code family ring).

**Schema (summary):**

```json
{
  "ui": { "ring_radius": 120, "trigger": "hold" },
  "rings": [
    {
      "match": ["*"],
      "title": "Desktop",
      "actions": [{ "label": "Launcher", "icon": "", "command": "walker" }]
    },
    {
      "match": ["code", "cursor"],
      "title": "VS Code",
      "actions": [
        { "label": "Command", "icon": "", "command": "key:ctrl+shift+p" },
        { "label": "Terminal", "icon": "", "command": "key:ctrl+`" }
      ]
    }
  ]
}
```

**Action `command` strings:**

| Form | Meaning |
|------|---------|
| `key:ctrl+shift+p` | Keyboard chord via ydotool |
| `click:left` / `click:right` / `click:middle` | Mouse button via ydotool |
| anything else | Shell command via `sh -c` (non-blocking spawn) |

**Matching:** first ring whose `match` list contains the focused app-id; `*` is the fallback. Focus uses `zwlr_foreign_toplevel_manager_v1` when available.

**Reload:** `command.json` is re-read automatically within about a second of being edited; an invalid edit is logged and ignored, so the daemon keeps running on the last-good config.

### Trigger mode

- **`"trigger": "hold"` (default)** — press and hold the Haptic Sense Panel to open the ring, move to a bubble, and release to fire the action. Release over the hub or empty space cancels.
- **`"trigger": "tap"`** — the first completed tap opens the ring. A second tap over a bubble fires it, or over the hub/empty space cancels the ring. Left click remains unused (reserved for future expansion).

### Blur (optional, Hyprland)

The ring itself draws translucent flat circles. On Hyprland, you can layer real backdrop blur behind the ring with a layer rule targeting the ring's layer-shell namespace:

```
layerrule = blur, mxactions-ring
```

Add this to your Hyprland config (e.g., `~/.config/hypr/hyprland.conf`). Other compositors get the flat-translucent look by default with no additional config.

## Build and install

```sh
cargo build --release
install -Dm755 target/release/mxactions ~/.local/bin/mxactions
```

## Run

```sh
# foreground (debug logs)
RUST_LOG=info mxactions

# or as a user systemd service
install -Dm644 contrib/mxactions.service ~/.config/systemd/user/mxactions.service
systemctl --user daemon-reload
systemctl --user enable --now mxactions.service
```

## Interaction

1. **Press** the Haptic Sense Panel → ring opens at the cursor position (queried at press time when possible).
2. **Move** the pointer while holding → bubble under the cursor highlights.
3. **Release** on a bubble → action runs; on hub or empty space → cancel.

## Pointer position note (v1)

At press time, mxactions queries the global cursor via compositor tools (best-effort):

1. **Hyprland** — `hyprctl cursorpos` when `$HYPRLAND_INSTANCE_SIGNATURE` is set (accurate press position on Hyprland).
2. **Sway** — `swaymsg -t get_seats` (first seat `x`/`y`).
3. **Fallback** — last pointer position reported by the overlay while a previous ring was open.
4. **Last resort** — `(960, 540)` if nothing else is available.

While the ring is open, pointer motion on the layer surface drives hover highlighting. Other compositors without a query path may use the fallback until a portable protocol-based cursor read exists.

## HID permissions

If open fails with permission denied, add a udev rule for Logitech HID++ devices or run once with appropriate group membership (`plugdev` / `input` depending on distro).

## Examples

```sh
cargo run --example hid_test          # Sense Panel press/release smoke test
cargo run --example overlay_preview   # Visual ring preview (3 seconds)
```
