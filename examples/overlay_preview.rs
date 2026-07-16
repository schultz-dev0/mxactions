use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use mxactions::controller::RingCommand;
use mxactions::geometry::RingLayout;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let (tx, rx) = mpsc::channel();
    let (event_tx, _event_rx) = mpsc::channel();

    thread::spawn(move || {
        thread::sleep(Duration::from_millis(400));
        let layout = RingLayout::new(4, 120.0);
        tx.send(RingCommand::Show {
            title: "Preview".into(),
            labels: vec!["Top".into(), "Right".into(), "Bottom".into(), "Left".into()],
            icons: vec![None, None, None, None],
            layout,
            cursor: (960, 540),
        })
        .ok();
        thread::sleep(Duration::from_secs(3));
        tx.send(RingCommand::Hide).ok();
    });

    mxactions::overlay::run_overlay(rx, event_tx)?;
    Ok(())
}
