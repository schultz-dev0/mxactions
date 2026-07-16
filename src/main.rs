use std::sync::mpsc::{self, Sender};
use std::thread;
use std::time::{Duration, Instant};

use mxactions::action::{ActionRunner, YdotoolInjector};
use mxactions::config::{Config, ConfigReloader, TriggerMode, config_path, load_or_init};
use mxactions::controller::{Controller, ControllerEvent, RingCommand};
use mxactions::focus::{FocusSource, WaylandFocus};
use mxactions::hidpp::{HidEvent, HidEventSource, MxMaster4};
use mxactions::overlay::{OverlayEvent, run_overlay};
use mxactions::query_pointer_position;

const POLL_MS: u64 = 16;
const FOCUS_POLL_MS: u64 = 100;
const CONFIG_POLL_MS: u64 = 1_000;
/// Fallback ring center when no pointer position has been reported yet.
const DEFAULT_CURSOR: (i32, i32) = (960, 540);

fn main() {
    if let Err(e) = run() {
        eprintln!("mxactions: {e}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();
    let path = config_path();
    let mut config = load_or_init(&path)?;
    let mut config_reloader = ConfigReloader::new(&path);
    log::info!("loaded config from {}", path.display());

    let (cmd_tx, cmd_rx) = mpsc::channel::<RingCommand>();
    let (event_tx, event_rx) = mpsc::channel::<OverlayEvent>();

    thread::spawn(move || {
        if let Err(e) = run_overlay(cmd_rx, event_tx) {
            log::error!("overlay thread exited: {e}");
        }
    });

    let mut hid = MxMaster4::open().map_err(|e| {
        format!(
            "failed to open MX Master 4: {e}\n\
             Ensure the mouse is connected (Bluetooth or Bolt) and that Solaar/OpenLogi \
             are not diverting the Haptic Sense Panel."
        )
    })?;
    log::info!(
        "MX Master 4 Sense Panel diverted (CID 0x{:04X})",
        hid.sense_panel_cid()
    );

    let mut focus = WaylandFocus::new();
    let mut session = RingSession {
        controller: Controller::new(),
        ring_center: None,
        cmd_tx,
        runner: ActionRunner {
            injector: YdotoolInjector,
        },
    };
    let mut last_pointer: Option<(i32, i32)> = None;

    let poll = Duration::from_millis(POLL_MS);
    let focus_poll = Duration::from_millis(FOCUS_POLL_MS);
    let config_poll = Duration::from_millis(CONFIG_POLL_MS);
    let mut next_focus_poll = Instant::now();
    let mut next_config_poll = Instant::now() + config_poll;

    loop {
        let now = Instant::now();
        if now >= next_focus_poll {
            focus.poll();
            next_focus_poll = now + focus_poll;
        }
        if now >= next_config_poll {
            config_reloader.reload_if_changed(&mut config);
            next_config_poll = now + config_poll;
        }

        while let Ok(OverlayEvent::Pointer { x, y }) = event_rx.try_recv() {
            last_pointer = Some((x, y));
            session.pointer(x, y, &config, focus.focused_app_id().as_deref());
        }

        if let Some(hid_ev) = hid.recv_timeout(poll) {
            let app_id = focus.focused_app_id();
            let app_id = app_id.as_deref();
            match trigger_dispatch(hid_ev, config.ui.trigger, session.controller.is_open()) {
                RingIntent::Open => {
                    let cursor = query_pointer_position()
                        .or(last_pointer)
                        .unwrap_or(DEFAULT_CURSOR);
                    session.open(&config, app_id, cursor);
                }
                RingIntent::CloseOrFire => session.close(&config, app_id),
                RingIntent::Ignore => {}
            }
        }
    }
}

/// What to do with a raw HID edge, given the configured trigger mode and whether
/// a ring is already open. A pure function so hold-vs-tap semantics are testable
/// without hardware or the mpsc channels (see the `tests` module below).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RingIntent {
    Open,
    CloseOrFire,
    Ignore,
}

fn trigger_dispatch(event: HidEvent, mode: TriggerMode, is_open: bool) -> RingIntent {
    match (event, mode) {
        (HidEvent::Press, TriggerMode::Hold) => RingIntent::Open,
        // A tap's press edge carries no signal on its own — only the completed
        // tap (Release, below) drives open/select/cancel.
        (HidEvent::Press, TriggerMode::Tap) => RingIntent::Ignore,
        (HidEvent::Release, TriggerMode::Hold) => RingIntent::CloseOrFire,
        (HidEvent::Release, TriggerMode::Tap) => {
            if is_open {
                RingIntent::CloseOrFire
            } else {
                RingIntent::Open
            }
        }
    }
}

/// Bundles the mutable daemon state needed to open, close, and drive hover on
/// the ring, so hold-mode and tap-mode dispatch (above) don't duplicate it.
struct RingSession {
    controller: Controller,
    ring_center: Option<(i32, i32)>,
    cmd_tx: Sender<RingCommand>,
    runner: ActionRunner<YdotoolInjector>,
}

impl RingSession {
    fn open(&mut self, config: &Config, app_id: Option<&str>, cursor: (i32, i32)) {
        let (cmds, fire) =
            self.controller
                .handle(ControllerEvent::Press { cursor }, config, app_id);
        for cmd in &cmds {
            if let RingCommand::Show { cursor: c, .. } = cmd {
                self.ring_center = Some(*c);
            }
        }
        self.dispatch(cmds, fire);
    }

    fn close(&mut self, config: &Config, app_id: Option<&str>) {
        let (cmds, fire) = self
            .controller
            .handle(ControllerEvent::Release, config, app_id);
        if cmds.iter().any(|c| matches!(c, RingCommand::Hide)) {
            self.ring_center = None;
        }
        self.dispatch(cmds, fire);
    }

    fn pointer(&mut self, x: i32, y: i32, config: &Config, app_id: Option<&str>) {
        let Some(center) = self.ring_center else {
            return;
        };
        let (cmds, fire) = self.controller.handle(
            ControllerEvent::Pointer {
                x: x as f32 - center.0 as f32,
                y: y as f32 - center.1 as f32,
            },
            config,
            app_id,
        );
        self.dispatch(cmds, fire);
    }

    fn dispatch(&mut self, cmds: Vec<RingCommand>, fire: Option<String>) {
        for cmd in cmds {
            if self.cmd_tx.send(cmd).is_err() {
                log::error!("overlay command channel closed");
            }
        }
        if let Some(action) = fire
            && let Err(e) = self.runner.run(&action)
        {
            log::warn!("action {action:?} failed: {e}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hold_mode_opens_on_press_and_closes_on_release() {
        assert_eq!(
            trigger_dispatch(HidEvent::Press, TriggerMode::Hold, false),
            RingIntent::Open
        );
        assert_eq!(
            trigger_dispatch(HidEvent::Release, TriggerMode::Hold, true),
            RingIntent::CloseOrFire
        );
    }

    #[test]
    fn tap_mode_ignores_press_and_toggles_on_release() {
        assert_eq!(
            trigger_dispatch(HidEvent::Press, TriggerMode::Tap, false),
            RingIntent::Ignore
        );
        assert_eq!(
            trigger_dispatch(HidEvent::Release, TriggerMode::Tap, false),
            RingIntent::Open
        );
        assert_eq!(
            trigger_dispatch(HidEvent::Release, TriggerMode::Tap, true),
            RingIntent::CloseOrFire
        );
    }
}
