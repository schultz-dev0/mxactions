use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use iced::alignment::{self, Horizontal};
use iced::mouse;
use iced::widget::canvas::{self, Canvas, Frame, Geometry, Path, Text};
use iced::widget::{Space, container, pin, stack};
use iced::{Color, Element, Event, Length, Point, Rectangle, Renderer, Subscription, Task, Theme};
use iced_layershell::actions::ActionCallback;
use iced_layershell::application;
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

/// Reads the desktop's light/dark preference the same way `theme_controller.sh`
/// sets it, so the ring matches whatever theme is currently active. Queried once
/// at overlay startup rather than polled — a mid-session theme flip repainting the
/// ring isn't a target for this iteration.
fn detect_dark_theme() -> bool {
    std::process::Command::new("gsettings")
        .args(["get", "org.gnome.desktop.interface", "color-scheme"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| parse_color_scheme(&String::from_utf8_lossy(&o.stdout)))
        .unwrap_or(true)
}

/// `true` = dark theme. Defaults dark unless the output explicitly says `prefer-light`,
/// since that's the only other value `theme_controller.sh` ever writes.
fn parse_color_scheme(output: &str) -> bool {
    !output.contains("prefer-light")
}

struct Palette {
    hub_fill: Color,
    hub_mark: Color,
    bubble_fill: Color,
    bubble_hover_fill: Color,
    glyph: Color,
    glyph_hover: Color,
    tooltip_bg: Color,
    tooltip_text: Color,
}

fn palette(dark: bool) -> Palette {
    if dark {
        Palette {
            hub_fill: Color::from_rgba(0.025, 0.025, 0.03, 0.97),
            hub_mark: Color::from_rgba(1.0, 1.0, 1.0, 0.96),
            bubble_fill: Color::from_rgba(0.76, 0.77, 0.77, 0.84),
            bubble_hover_fill: Color::from_rgba(0.025, 0.025, 0.03, 0.97),
            glyph: Color::from_rgba(0.10, 0.10, 0.11, 0.78),
            glyph_hover: Color::from_rgba(1.0, 1.0, 1.0, 0.96),
            tooltip_bg: Color::from_rgba(0.98, 0.98, 0.97, 0.96),
            tooltip_text: Color::from_rgb(0.08, 0.08, 0.09),
        }
    } else {
        Palette {
            hub_fill: Color::from_rgba(0.025, 0.025, 0.03, 0.97),
            hub_mark: Color::from_rgba(1.0, 1.0, 1.0, 0.96),
            bubble_fill: Color::from_rgba(0.80, 0.81, 0.81, 0.90),
            bubble_hover_fill: Color::from_rgba(0.025, 0.025, 0.03, 0.97),
            glyph: Color::from_rgba(0.09, 0.09, 0.10, 0.82),
            glyph_hover: Color::from_rgba(1.0, 1.0, 1.0, 0.96),
            tooltip_bg: Color::from_rgba(1.0, 1.0, 0.99, 0.97),
            tooltip_text: Color::from_rgb(0.08, 0.08, 0.09),
        }
    }
}

/// System-installed Nerd Font providing the ring's icon glyphs (see README
/// for the package to install — no font is bundled with the binary).
const ICON_FONT: iced::Font = iced::Font::with_name("Symbols Nerd Font Mono");

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
        // iced_layershell disables MSAA by default, which leaves small canvas
        // circles visibly stair-stepped even though regular Iced enables it.
        antialiasing: true,
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
    icons: Vec<Option<String>>,
    layout: Option<RingLayout>,
    hover: Option<usize>,
    cursor: (i32, i32),
    dark_theme: bool,
}

impl RingOverlay {
    fn new(rx: Arc<Mutex<Receiver<RingCommand>>>, event_tx: Sender<OverlayEvent>) -> Self {
        Self {
            rx,
            event_tx,
            visible: false,
            title: String::new(),
            labels: Vec::new(),
            icons: Vec::new(),
            layout: None,
            hover: None,
            cursor: (0, 0),
            dark_theme: detect_dark_theme(),
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
                            icons,
                            layout,
                            cursor,
                        } => {
                            self.visible = true;
                            self.title = title;
                            self.labels = labels;
                            self.icons = icons;
                            self.layout = Some(layout);
                            self.hover = None;
                            self.cursor = cursor;
                            tasks
                                .push(Task::done(Message::AnchorSizeChange(full_anchor(), (0, 0))));
                            tasks.push(Task::done(Message::SetInputRegion(input_region_full())));
                        }
                        RingCommand::SetHover(hover) => {
                            self.hover = hover;
                        }
                        RingCommand::Hide => {
                            self.visible = false;
                            self.title.clear();
                            self.labels.clear();
                            self.icons.clear();
                            self.layout = None;
                            self.hover = None;
                            tasks.push(Task::done(Message::AnchorSizeChange(
                                full_anchor(),
                                HIDDEN_SIZE,
                            )));
                            tasks.push(Task::done(Message::SetInputRegion(input_region_none())));
                        }
                    }
                }
                Task::batch(tasks)
            }
            _ => Task::none(),
        }
    }

    fn view(&self) -> Element<'_, Message> {
        let mut layers: Vec<Element<'_, Message>> = Vec::new();

        if self.visible
            && let Some(layout) = &self.layout
        {
            let pal = palette(self.dark_theme);
            let center = Point::new(self.cursor.0 as f32, self.cursor.1 as f32);
            let diameter = layout.bubble_radius * 2.0;

            for (index, offset) in layout.bubbles.iter().enumerate() {
                let bounds = bubble_bounds(center, *offset, layout.bubble_radius);
                let fill = if self.hover == Some(index) {
                    pal.bubble_hover_fill
                } else {
                    pal.bubble_fill
                };
                let bubble = container(Space::new())
                    .width(Length::Fixed(diameter))
                    .height(Length::Fixed(diameter))
                    .style(move |_| container::Style {
                        background: Some(fill.into()),
                        border: iced::Border {
                            radius: iced::border::radius(diameter / 2.0),
                            ..iced::Border::default()
                        },
                        ..container::Style::default()
                    });
                layers.push(pin(bubble).position(bounds.position()).into());
            }
        }

        let canvas = Canvas::new(RingCanvas {
            visible: self.visible,
            labels: &self.labels,
            icons: &self.icons,
            layout: self.layout.as_ref(),
            hover: self.hover,
            cursor: self.cursor,
            dark_theme: self.dark_theme,
        })
        .width(Length::Fill)
        .height(Length::Fill);
        layers.push(canvas.into());

        container(stack(layers).width(Length::Fill).height(Length::Fill))
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
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
    labels: &'a [String],
    icons: &'a [Option<String>],
    layout: Option<&'a RingLayout>,
    hover: Option<usize>,
    cursor: (i32, i32),
    dark_theme: bool,
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
        let pal = palette(self.dark_theme);

        let mut frame = Frame::new(renderer, bounds.size());
        let center = Point::new(self.cursor.0 as f32, self.cursor.1 as f32);

        draw_hub(&mut frame, center, layout.hub_radius, &pal);

        let mut hovered_pos = None;
        for (i, (bx, by)) in layout.bubbles.iter().enumerate() {
            let pos = Point::new(center.x + bx, center.y + by);
            let hovered = self.hover == Some(i);
            if hovered {
                hovered_pos = Some(pos);
            }

            let icon = self
                .icons
                .get(i)
                .and_then(|icon| icon.as_deref())
                .filter(|icon| !icon.is_empty());
            let (glyph, font) = match icon {
                Some(icon) => (icon.to_string(), ICON_FONT),
                None => (
                    self.labels
                        .get(i)
                        .and_then(|label| label.chars().next())
                        .map(|character| character.to_string())
                        .unwrap_or_default(),
                    iced::Font::DEFAULT,
                ),
            };
            let text = Text {
                content: glyph,
                position: pos,
                color: if hovered { pal.glyph_hover } else { pal.glyph },
                size: iced::Pixels((layout.bubble_radius * 0.66).clamp(16.0, 21.0)),
                font,
                align_x: Horizontal::Center.into(),
                align_y: alignment::Vertical::Center,
                ..Text::default()
            };
            frame.fill_text(text);
        }

        if let (Some(pos), Some(i)) = (hovered_pos, self.hover)
            && let Some(label) = self.labels.get(i)
        {
            draw_tooltip(&mut frame, center, pos, layout.bubble_radius, label, &pal);
        }

        vec![frame.into_geometry()]
    }
}

fn bubble_bounds(center: Point, offset: (f32, f32), radius: f32) -> Rectangle {
    Rectangle::new(
        Point::new(center.x + offset.0 - radius, center.y + offset.1 - radius),
        iced::Size::new(radius * 2.0, radius * 2.0),
    )
}

fn draw_hub(frame: &mut Frame, center: Point, radius: f32, pal: &Palette) {
    let side = radius * 1.4;
    let half = side / 2.0;
    let diamond = Path::rounded_rectangle(
        Point::new(-half, -half),
        iced::Size::new(side, side),
        iced::border::radius(radius * 0.28),
    );

    frame.with_save(|frame| {
        frame.translate(iced::Vector::new(center.x, center.y));
        frame.rotate(std::f32::consts::FRAC_PI_4);
        frame.fill(&diamond, pal.hub_fill);
    });

    let offset = radius * 0.27;
    let dot_radius = (radius * 0.105).max(1.2);
    for (x, y) in [(0.0, -offset), (offset, 0.0), (0.0, offset), (-offset, 0.0)] {
        frame.fill(
            &Path::circle(Point::new(center.x + x, center.y + y), dot_radius),
            pal.hub_mark,
        );
    }
}

fn draw_tooltip(
    frame: &mut Frame,
    center: Point,
    bubble_pos: Point,
    bubble_radius: f32,
    label: &str,
    pal: &Palette,
) {
    let horizontal_padding = 9.0;
    let height = 22.0;
    let width = (6.5 * label.chars().count() as f32 + horizontal_padding * 2.0).max(44.0);
    let top_left = tooltip_position(
        frame.size(),
        center,
        bubble_pos,
        bubble_radius,
        iced::Size::new(width, height),
    );
    let pill = Path::rounded_rectangle(
        top_left,
        iced::Size::new(width, height),
        iced::border::radius(height / 2.0),
    );
    frame.fill(&pill, pal.tooltip_bg);

    let text = Text {
        content: label.to_string(),
        position: Point::new(top_left.x + width / 2.0, top_left.y + height / 2.0),
        color: pal.tooltip_text,
        size: iced::Pixels(11.0),
        align_x: Horizontal::Center.into(),
        align_y: alignment::Vertical::Center,
        ..Text::default()
    };
    frame.fill_text(text);
}

fn tooltip_position(
    frame: iced::Size,
    center: Point,
    bubble: Point,
    bubble_radius: f32,
    tooltip: iced::Size,
) -> Point {
    let edge_gap = 8.0;
    let screen_padding = 8.0;
    let x = if bubble.x >= center.x {
        bubble.x + bubble_radius + edge_gap
    } else {
        bubble.x - bubble_radius - edge_gap - tooltip.width
    };
    let y = bubble.y - tooltip.height / 2.0;
    let max_x = (frame.width - tooltip.width - screen_padding).max(screen_padding);
    let max_y = (frame.height - tooltip.height - screen_padding).max(screen_padding);

    Point::new(
        x.clamp(screen_padding, max_x),
        y.clamp(screen_padding, max_y),
    )
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

    #[test]
    fn parses_prefer_dark() {
        assert!(parse_color_scheme("'prefer-dark'\n"));
    }

    #[test]
    fn parses_prefer_light() {
        assert!(!parse_color_scheme("'prefer-light'\n"));
    }

    #[test]
    fn defaults_to_dark_on_unknown_or_empty_output() {
        assert!(parse_color_scheme("'default'\n"));
        assert!(parse_color_scheme(""));
    }

    #[test]
    fn tooltip_sits_outside_the_bubble_and_stays_on_screen() {
        let frame = iced::Size::new(300.0, 200.0);
        let tooltip = iced::Size::new(80.0, 22.0);
        let center = Point::new(150.0, 100.0);

        let right = tooltip_position(frame, center, Point::new(170.0, 50.0), 20.0, tooltip);
        assert_eq!(right, Point::new(198.0, 39.0));

        let left = tooltip_position(frame, center, Point::new(10.0, 190.0), 20.0, tooltip);
        assert_eq!(left, Point::new(8.0, 170.0));
    }

    #[test]
    fn bubble_bounds_center_the_widget_on_layout_coordinates() {
        assert_eq!(
            bubble_bounds(Point::new(100.0, 80.0), (20.0, -30.0), 10.0),
            Rectangle::new(Point::new(110.0, 40.0), iced::Size::new(20.0, 20.0))
        );
    }
}
