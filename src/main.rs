use std::sync::atomic::{AtomicU64, Ordering};

use iced::widget::{column, container, row, space, text};
use iced::{Color, Element, Length, Task};
use iced_layershell::build_pattern::application;
use iced_layershell::reexport::{Anchor, KeyboardInteractivity, Layer};
use iced_layershell::settings::LayerShellSettings;
use iced_layershell::to_layer_message;

const MARKER_SIZE: f32 = 24.0;
const EDGE_MARGIN: u16 = 40;
const MARKER_COLOR: Color = Color {
    r: 1.0,
    g: 1.0,
    b: 1.0,
    a: 0.85,
};

static VIEW_COUNT: AtomicU64 = AtomicU64::new(0);

fn main() -> Result<(), iced_layershell::Error> {
    eprintln!("[dev-hud] starting...");
    eprintln!("[dev-hud] layer: Overlay, anchors: all edges, click-through: true");

    let settings = LayerShellSettings {
        layer: Layer::Overlay,
        anchor: Anchor::Top | Anchor::Bottom | Anchor::Left | Anchor::Right,
        keyboard_interactivity: KeyboardInteractivity::None,
        exclusive_zone: -1,
        size: Some((0, 0)),
        events_transparent: true,
        ..Default::default()
    };
    eprintln!("[dev-hud] layer_settings: {:?}", settings);

    let result = application(Hud::new, Hud::namespace, Hud::update, Hud::view)
        .style(Hud::style)
        .layer_settings(settings)
        .run();

    eprintln!("[dev-hud] exited: {:?}", result);
    result
}

#[derive(Default)]
struct Hud;

#[to_layer_message]
#[derive(Debug, Clone)]
enum Message {}

impl Hud {
    fn new() -> (Self, Task<Message>) {
        eprintln!("[dev-hud] Hud::new() called - surface should be created");
        (Self, Task::none())
    }

    fn namespace() -> String {
        String::from("dev-hud")
    }

    fn update(&mut self, _message: Message) -> Task<Message> {
        Task::none()
    }

    fn view(&self) -> Element<'_, Message> {
        let n = VIEW_COUNT.fetch_add(1, Ordering::Relaxed) + 1;
        if n <= 3 {
            eprintln!("[dev-hud] view() #{} - rendering 4 markers", n);
        }

        let marker = || text("+").size(MARKER_SIZE).color(MARKER_COLOR);

        let top_row = row![marker(), space::horizontal(), marker()];
        let bottom_row = row![marker(), space::horizontal(), marker()];

        container(
            column![top_row, space::vertical(), bottom_row]
                .width(Length::Fill)
                .height(Length::Fill),
        )
        .padding(EDGE_MARGIN)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    }

    fn style(&self, _theme: &iced::Theme) -> iced::theme::Style {
        iced::theme::Style {
            background_color: Color::TRANSPARENT,
            text_color: MARKER_COLOR,
        }
    }
}
