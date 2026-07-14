use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use iced::alignment::{self, Horizontal};
use iced::mouse;
use iced::widget::canvas::{self, Canvas, Frame, Geometry, Path, Stroke, Text};
use iced::widget::container;
use iced::{Color, Element, Event, Length, Point, Rectangle, Renderer, Subscription, Task, Theme};
use iced_layershell::application;
use iced_layershell::actions::ActionCallback;
use iced_layershell::reexport::{Anchor, KeyboardInteractivity, Layer};
use iced_layershell::settings::{LayerShellSettings, Settings, StartMode};
use iced_layershell::to_layer_message;

use crate::controller::RingCommand;
use crate::geometry::RingLayout;
use crate::overlay::OverlayEvent;

const POLL_MS: u64 = 8;
const HIDDEN_SIZE: (u32, u32) = (1, 1);
/// Large enough to cover any output when the layer is fullscreen.
const INPUT_REGION_MAX: i32 = 16384;

fn full_anchor() -> Anchor {
    Anchor::Top | Anchor::Bottom | Anchor::Left | Anchor::Right
}
const HUB_FILL: Color = Color::from_rgba(0.12, 0.12, 0.14, 0.88);
const BUBBLE_FILL: Color = Color::from_rgba(0.18, 0.18, 0.22, 0.92);
const BUBBLE_HOVER: Color = Color::from_rgba(0.28, 0.45, 0.78, 0.95);
const STROKE: Color = Color::from_rgba(0.9, 0.9, 0.95, 0.35);
const STROKE_HOVER: Color = Color::from_rgba(0.55, 0.75, 1.0, 0.9);
const LABEL: Color = Color::from_rgba(0.95, 0.95, 0.98, 1.0);

/// Run the layer-shell overlay until the process exits.
pub fn run_overlay(
    rx: Receiver<RingCommand>,
    event_tx: Sender<OverlayEvent>,
) -> Result<(), iced_layershell::Error> {
    iced_layershell::disable_clipboard();

    let rx = Arc::new(Mutex::new(rx));

    application(
        {
            let rx = Arc::clone(&rx);
            move || RingOverlay::new(Arc::clone(&rx), event_tx.clone())
        },
        || "mxactions-ring".into(),
        RingOverlay::update,
        RingOverlay::view,
    )
    .style(RingOverlay::style)
    .subscription(RingOverlay::subscription)
    .settings(Settings {
        layer_settings: LayerShellSettings {
            anchor: full_anchor(),
            layer: Layer::Overlay,
            exclusive_zone: 0,
            size: Some(HIDDEN_SIZE),
            margin: (0, 0, 0, 0),
            keyboard_interactivity: KeyboardInteractivity::None,
            start_mode: StartMode::Active,
            events_transparent: true,
        },
        ..Default::default()
    })
    .run()
}

struct RingOverlay {
    rx: Arc<Mutex<Receiver<RingCommand>>>,
    event_tx: Sender<OverlayEvent>,
    visible: bool,
    title: String,
    labels: Vec<String>,
    layout: Option<RingLayout>,
    hover: Option<usize>,
    cursor: (i32, i32),
}

impl RingOverlay {
    fn new(rx: Arc<Mutex<Receiver<RingCommand>>>, event_tx: Sender<OverlayEvent>) -> Self {
        Self {
            rx,
            event_tx,
            visible: false,
            title: String::new(),
            labels: Vec::new(),
            layout: None,
            hover: None,
            cursor: (0, 0),
        }
    }

    fn subscription(&self) -> Subscription<Message> {
        let poll = iced::time::every(Duration::from_millis(POLL_MS)).map(|_| Message::Poll);
        let pointer = if self.visible {
            iced::event::listen_with(|event, _status, _window| {
                if let Event::Mouse(mouse::Event::CursorMoved { position }) = event {
                    Some(Message::PointerMove(position))
                } else {
                    None
                }
            })
        } else {
            Subscription::none()
        };
        Subscription::batch(vec![poll, pointer])
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::PointerMove(point) => {
                if self.visible {
                    let _ = self.event_tx.send(OverlayEvent::Pointer {
                        x: point.x.round() as i32,
                        y: point.y.round() as i32,
                    });
                }
                Task::none()
            }
            Message::Poll => {
                let mut tasks = Vec::new();
                let Ok(rx) = self.rx.lock() else {
                    return Task::none();
                };
                while let Ok(cmd) = rx.try_recv() {
                    match cmd {
                        RingCommand::Show {
                            title,
                            labels,
                            layout,
                            cursor,
                        } => {
                            self.visible = true;
                            self.title = title;
                            self.labels = labels;
                            self.layout = Some(layout);
                            self.hover = None;
                            self.cursor = cursor;
                            tasks.push(Task::done(Message::AnchorSizeChange(
                                full_anchor(),
                                (0, 0),
                            )));
                            tasks.push(Task::done(Message::SetInputRegion(
                                input_region_full(),
                            )));
                        }
                        RingCommand::SetHover(hover) => {
                            self.hover = hover;
                        }
                        RingCommand::Hide => {
                            self.visible = false;
                            self.title.clear();
                            self.labels.clear();
                            self.layout = None;
                            self.hover = None;
                            tasks.push(Task::done(Message::AnchorSizeChange(
                                full_anchor(),
                                HIDDEN_SIZE,
                            )));
                            tasks.push(Task::done(Message::SetInputRegion(
                                input_region_none(),
                            )));
                        }
                    }
                }
                Task::batch(tasks)
            }
            _ => Task::none(),
        }
    }

    fn view(&self) -> Element<'_, Message> {
        let canvas = Canvas::new(RingCanvas {
            visible: self.visible,
            title: &self.title,
            labels: &self.labels,
            layout: self.layout.as_ref(),
            hover: self.hover,
            cursor: self.cursor,
        })
        .width(Length::Fill)
        .height(Length::Fill);

        container(canvas).width(Length::Fill).height(Length::Fill).into()
    }

    fn style(_state: &Self, theme: &Theme) -> iced::theme::Style {
        iced::theme::Style {
            background_color: Color::TRANSPARENT,
            text_color: theme.palette().text,
        }
    }
}

fn input_region_full() -> ActionCallback {
    ActionCallback::new(|region| {
        region.add(0, 0, INPUT_REGION_MAX, INPUT_REGION_MAX);
    })
}

fn input_region_none() -> ActionCallback {
    ActionCallback::new(|_| {})
}

#[to_layer_message]
#[derive(Debug, Clone)]
enum Message {
    Poll,
    PointerMove(Point),
}

struct RingCanvas<'a> {
    visible: bool,
    title: &'a str,
    labels: &'a [String],
    layout: Option<&'a RingLayout>,
    hover: Option<usize>,
    cursor: (i32, i32),
}

impl<Message> canvas::Program<Message> for RingCanvas<'_> {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        if !self.visible {
            return Vec::new();
        }
        let Some(layout) = self.layout else {
            return Vec::new();
        };

        let mut frame = Frame::new(renderer, bounds.size());
        let center = Point::new(self.cursor.0 as f32, self.cursor.1 as f32);

        let hub = Path::circle(center, layout.hub_radius);
        frame.fill(&hub, HUB_FILL);
        frame.stroke(
            &hub,
            Stroke::default().with_width(2.0).with_color(STROKE),
        );

        if !self.title.is_empty() {
            let title = Text {
                content: self.title.to_string(),
                position: center,
                color: LABEL,
                size: iced::Pixels(14.0),
                align_x: Horizontal::Center.into(),
                align_y: alignment::Vertical::Center,
                ..Text::default()
            };
            frame.fill_text(title);
        }

        for (i, (bx, by)) in layout.bubbles.iter().enumerate() {
            let pos = Point::new(center.x + bx, center.y + by);
            let bubble = Path::circle(pos, layout.bubble_radius);
            let hovered = self.hover == Some(i);
            let fill = if hovered { BUBBLE_HOVER } else { BUBBLE_FILL };
            let stroke_color = if hovered { STROKE_HOVER } else { STROKE };
            let stroke_width = if hovered { 3.0 } else { 1.5 };

            frame.fill(&bubble, fill);
            frame.stroke(
                &bubble,
                Stroke::default()
                    .with_width(stroke_width)
                    .with_color(stroke_color),
            );

            if let Some(label) = self.labels.get(i) {
                let text = Text {
                    content: label.clone(),
                    position: pos,
                    color: LABEL,
                    size: iced::Pixels(11.0),
                    align_x: Horizontal::Center.into(),
                    align_y: alignment::Vertical::Center,
                    ..Text::default()
                };
                frame.fill_text(text);
            }
        }

        vec![frame.into_geometry()]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ring_overlay_starts_hidden() {
        let overlay = RingOverlay::new(
            Arc::new(Mutex::new(std::sync::mpsc::channel().1)),
            std::sync::mpsc::channel().0,
        );
        assert!(!overlay.visible);
        assert!(overlay.layout.is_none());
    }
}
