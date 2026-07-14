use std::sync::mpsc::{self, Sender};
use std::thread;
use std::time::Duration;

use mxactions::action::{ActionRunner, YdotoolInjector};
use mxactions::config::{config_path, load_or_init};
use mxactions::controller::{Controller, ControllerEvent, RingCommand};
use mxactions::focus::{FocusSource, WaylandFocus};
use mxactions::hidpp::{HidEvent, HidEventSource, MxMaster4};
use mxactions::overlay::{run_overlay, OverlayEvent};
use mxactions::query_pointer_position;

const POLL_MS: u64 = 16;
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
    let config = load_or_init(&path)?;
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
    let mut controller = Controller::new();
    let mut runner = ActionRunner {
        injector: YdotoolInjector,
    };
    let mut ring_center: Option<(i32, i32)> = None;
    let mut last_pointer: Option<(i32, i32)> = None;

    let poll = Duration::from_millis(POLL_MS);

    loop {
        focus.poll();

        while let Ok(OverlayEvent::Pointer { x, y }) = event_rx.try_recv() {
            last_pointer = Some((x, y));
            if let Some(center) = ring_center {
                let (cmds, fire) = controller.handle(
                    ControllerEvent::Pointer {
                        x: x as f32 - center.0 as f32,
                        y: y as f32 - center.1 as f32,
                    },
                    &config,
                    focus.focused_app_id().as_deref(),
                );
                send_cmds(&cmd_tx, cmds);
                if let Some(action) = fire {
                    run_action(&mut runner, &action);
                }
            }
        }
        if let Some(hid_ev) = hid.recv_timeout(poll) {
            match hid_ev {
                HidEvent::Press => {
                    let cursor = query_pointer_position()
                        .or(last_pointer)
                        .unwrap_or(DEFAULT_CURSOR);
                    let (cmds, fire) = controller.handle(
                        ControllerEvent::Press { cursor },
                        &config,
                        focus.focused_app_id().as_deref(),
                    );
                    for cmd in &cmds {
                        if let RingCommand::Show { cursor: c, .. } = cmd {
                            ring_center = Some(*c);
                        }
                    }
                    send_cmds(&cmd_tx, cmds);
                    if let Some(action) = fire {
                        run_action(&mut runner, &action);
                    }
                }
                HidEvent::Release => {
                    let (cmds, fire) = controller.handle(
                        ControllerEvent::Release,
                        &config,
                        focus.focused_app_id().as_deref(),
                    );
                    if cmds.iter().any(|c| matches!(c, RingCommand::Hide)) {
                        ring_center = None;
                    }
                    send_cmds(&cmd_tx, cmds);
                    if let Some(action) = fire {
                        run_action(&mut runner, &action);
                    }
                }
            }
        }
    }
}

fn send_cmds(tx: &Sender<RingCommand>, cmds: Vec<RingCommand>) {
    for cmd in cmds {
        if tx.send(cmd).is_err() {
            log::error!("overlay command channel closed");
        }
    }
}

fn run_action(runner: &mut ActionRunner<YdotoolInjector>, action: &str) {
    if let Err(e) = runner.run(action) {
        log::warn!("action {action:?} failed: {e}");
    }
}
