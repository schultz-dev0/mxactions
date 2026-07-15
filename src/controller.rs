use crate::config::{Config, Ring};
use crate::geometry::{Hit, RingLayout, hit_test};

#[derive(Debug, Clone, PartialEq)]
pub enum RingCommand {
    Show {
        title: String,
        labels: Vec<String>,
        layout: RingLayout,
        /// Screen coordinates for ring center (set when pointer position is known).
        cursor: (i32, i32),
    },
    SetHover(Option<usize>),
    Hide,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ControllerEvent {
    Press { cursor: (i32, i32) },
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
    /// Actions frozen at open for commit.
    actions: Vec<String>,
}

impl Default for Controller {
    fn default() -> Self {
        Self::new()
    }
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
            ControllerEvent::Press { cursor } => {
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
                cmds.push(RingCommand::Show {
                    title,
                    labels,
                    layout,
                    cursor,
                });
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
        let (cmds, fire) =
            c.handle(ControllerEvent::Press { cursor: (0, 0) }, &config, Some("cursor"));
        assert!(matches!(cmds[0], RingCommand::Show { cursor: (0, 0), .. }));
        assert!(fire.is_none());

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
        c.handle(ControllerEvent::Press { cursor: (0, 0) }, &config, None);
        c.handle(ControllerEvent::Pointer { x: 999.0, y: 999.0 }, &config, None);
        let (_, fire) = c.handle(ControllerEvent::Release, &config, None);
        assert!(fire.is_none());
    }
}
