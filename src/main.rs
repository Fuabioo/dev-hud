use std::io::BufRead;
use std::os::unix::net::UnixListener;
use std::path::PathBuf;

use futures::channel::mpsc;
use iced::widget::{column, container, row, space, text};
use iced::{Color, Element, Length, Subscription, Task};
use iced_layershell::build_pattern::daemon;
use iced_layershell::reexport::{Anchor, KeyboardInteractivity, Layer, NewLayerShellSettings};
use iced_layershell::settings::{LayerShellSettings, StartMode};
use iced_layershell::to_layer_message;

type IcedId = iced_layershell::reexport::IcedId;

const MARKER_SIZE: f32 = 24.0;
const EDGE_MARGIN: u16 = 40;
const MARKER_COLOR: Color = Color {
    r: 1.0,
    g: 1.0,
    b: 1.0,
    a: 0.85,
};

fn main() -> Result<(), iced_layershell::Error> {
    eprintln!("[dev-hud] starting in background mode (no initial surface)");

    let settings = LayerShellSettings {
        start_mode: StartMode::Background,
        ..Default::default()
    };

    daemon(Hud::new, Hud::namespace, Hud::update, Hud::view)
        .style(Hud::style)
        .subscription(Hud::subscription)
        .layer_settings(settings)
        .run()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HudMode {
    Hidden,
    Visible,
    Focused,
}

struct Hud {
    mode: HudMode,
    surface_id: Option<IcedId>,
}

#[to_layer_message(multi)]
#[derive(Debug, Clone)]
enum Message {
    ToggleVisibility,
    ToggleFocus,
}

fn socket_path() -> PathBuf {
    let runtime_dir = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".to_string());
    PathBuf::from(runtime_dir).join("dev-hud.sock")
}

fn visible_settings() -> NewLayerShellSettings {
    NewLayerShellSettings {
        layer: Layer::Overlay,
        anchor: Anchor::Top | Anchor::Bottom | Anchor::Left | Anchor::Right,
        keyboard_interactivity: KeyboardInteractivity::None,
        exclusive_zone: Some(-1),
        size: Some((0, 0)),
        events_transparent: true,
        ..Default::default()
    }
}

fn focused_settings() -> NewLayerShellSettings {
    NewLayerShellSettings {
        layer: Layer::Overlay,
        anchor: Anchor::Top | Anchor::Bottom | Anchor::Left | Anchor::Right,
        keyboard_interactivity: KeyboardInteractivity::OnDemand,
        exclusive_zone: Some(-1),
        size: Some((0, 0)),
        events_transparent: false,
        ..Default::default()
    }
}

fn socket_listener() -> impl futures::Stream<Item = Message> {
    let (tx, rx) = mpsc::unbounded();
    std::thread::spawn(move || {
        let path = socket_path();
        let _ = std::fs::remove_file(&path);
        let listener = match UnixListener::bind(&path) {
            Ok(l) => l,
            Err(e) => {
                eprintln!("[dev-hud] failed to bind socket {path:?}: {e}");
                return;
            }
        };
        eprintln!("[dev-hud] listening on {path:?}");
        for stream in listener.incoming().flatten() {
            let mut buf = String::new();
            if std::io::BufReader::new(stream).read_line(&mut buf).is_ok() {
                let msg = match buf.trim() {
                    "toggle" => Some(Message::ToggleVisibility),
                    "focus" => Some(Message::ToggleFocus),
                    other => {
                        eprintln!("[dev-hud] unknown command: {other:?}");
                        None
                    }
                };
                if let Some(msg) = msg {
                    if tx.unbounded_send(msg).is_err() {
                        break;
                    }
                }
            }
        }
    });
    rx
}

impl Hud {
    fn new() -> (Self, Task<Message>) {
        let (id, task) = Message::layershell_open(visible_settings());
        eprintln!("[dev-hud] booting -> Visible (surface {id})");
        (
            Self {
                mode: HudMode::Visible,
                surface_id: Some(id),
            },
            task,
        )
    }

    fn namespace() -> String {
        String::from("dev-hud")
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::ToggleVisibility => match self.mode {
                HudMode::Hidden => {
                    let (id, task) = Message::layershell_open(visible_settings());
                    self.surface_id = Some(id);
                    self.mode = HudMode::Visible;
                    eprintln!("[dev-hud] Hidden -> Visible");
                    task
                }
                mode @ (HudMode::Visible | HudMode::Focused) => {
                    let task = if let Some(id) = self.surface_id.take() {
                        Task::done(Message::RemoveWindow(id))
                    } else {
                        Task::none()
                    };
                    self.mode = HudMode::Hidden;
                    eprintln!("[dev-hud] {mode:?} -> Hidden");
                    task
                }
            },
            Message::ToggleFocus => match self.mode {
                HudMode::Hidden => {
                    let (id, task) = Message::layershell_open(focused_settings());
                    self.surface_id = Some(id);
                    self.mode = HudMode::Focused;
                    eprintln!("[dev-hud] Hidden -> Focused");
                    task
                }
                HudMode::Visible => {
                    let remove_task = if let Some(id) = self.surface_id.take() {
                        Task::done(Message::RemoveWindow(id))
                    } else {
                        Task::none()
                    };
                    let (id, open_task) = Message::layershell_open(focused_settings());
                    self.surface_id = Some(id);
                    self.mode = HudMode::Focused;
                    eprintln!("[dev-hud] Visible -> Focused");
                    Task::batch([remove_task, open_task])
                }
                HudMode::Focused => {
                    let remove_task = if let Some(id) = self.surface_id.take() {
                        Task::done(Message::RemoveWindow(id))
                    } else {
                        Task::none()
                    };
                    let (id, open_task) = Message::layershell_open(visible_settings());
                    self.surface_id = Some(id);
                    self.mode = HudMode::Visible;
                    eprintln!("[dev-hud] Focused -> Visible");
                    Task::batch([remove_task, open_task])
                }
            },
            _ => Task::none(),
        }
    }

    fn view(&self, _window_id: IcedId) -> Element<'_, Message> {
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

    fn subscription(_state: &Self) -> Subscription<Message> {
        Subscription::run(socket_listener)
    }

    fn style(&self, _theme: &iced::Theme) -> iced::theme::Style {
        iced::theme::Style {
            background_color: Color::TRANSPARENT,
            text_color: MARKER_COLOR,
        }
    }
}
