# mxactions

Wayland daemon that diverts the Logitech **MX Master 4** Haptic Sense Panel and shows a hold-to-select **Actions Ring** at the pointer. Actions come from `~/.config/mxactions/command.json`.

**Wayland only** — no X11 support in v1.

## Requirements

- MX Master 4 connected via Bluetooth or Bolt receiver
- wlroots-based compositor (Hyprland, Sway, River, etc.) for layer-shell overlay and focus tracking
- [`ydotool`](https://github.com/ReimuNotMoe/ydotool) + `ydotoold` for `key:` and `click:` actions
- udev permissions to open the mouse HID++ interface (see below)

## Solaar / OpenLogi conflict

mxactions **owns** the Sense Panel via HID++ temporary diversion. If **Solaar**, **OpenLogi**, or another tool already diverts the panel, startup fails with a clear error. Quit those tools before running mxactions.

## Configuration

**Path:** `~/.config/mxactions/command.json` (or `$XDG_CONFIG_HOME/mxactions/command.json`)

On first run, mxactions creates the directory and writes bundled defaults (Desktop `*` ring + VS Code family ring).

**Schema (summary):**

```json
{
  "ui": { "bubble_count_max": 8, "ring_radius": 120 },
  "rings": [
    {
      "match": ["*"],
      "title": "Desktop",
      "actions": [{ "label": "Launcher", "command": "walker" }]
    },
    {
      "match": ["code", "cursor"],
      "title": "VS Code",
      "actions": [{ "label": "Command", "command": "key:ctrl+shift+p" }]
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

**Reload:** config is read at **startup only**. After editing, restart mxactions.

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

Restart after config changes:

```sh
systemctl --user restart mxactions
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
