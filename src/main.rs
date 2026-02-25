use std::io::BufRead;
use std::os::unix::net::UnixListener;
use std::path::PathBuf;
use std::time::Duration;

use futures::channel::mpsc;
use iced::widget::{column, container, image as iced_image, row, space, svg, text};
use iced::widget::text::Shaping;
use iced::{Color, Element, Font, Length, Subscription, Task};
use iced_layershell::build_pattern::daemon;
use iced_layershell::reexport::{Anchor, KeyboardInteractivity, Layer, NewLayerShellSettings};
use iced_layershell::settings::{LayerShellSettings, StartMode};
use iced_layershell::to_layer_message;
use image::AnimationDecoder;

type IcedId = iced_layershell::reexport::IcedId;

const MARKER_SIZE: f32 = 24.0;
const EDGE_MARGIN: u16 = 40;
const MARKER_COLOR: Color = Color {
    r: 1.0,
    g: 1.0,
    b: 1.0,
    a: 0.85,
};
const MUTED_COLOR: Color = Color {
    r: 1.0,
    g: 1.0,
    b: 1.0,
    a: 0.4,
};

const TICK_MS: u64 = 80;
const LOADER_TEXT_SIZE: f32 = MARKER_SIZE * 0.5;
const LOADER_IMAGE_SIZE: f32 = 20.0;
const SVG_FRAME_COUNT: usize = 12;

const LOADER_GIF_BYTES: &[u8] = include_bytes!("../assets/loader.gif");

// --- Embedded Fonts ---

const FONT_IOSEVKA_BYTES: &[u8] =
    include_bytes!("../assets/fonts/IosevkaNerdFont-Regular.ttf");
const FONT_JETBRAINSMONO_BYTES: &[u8] =
    include_bytes!("../assets/fonts/JetBrainsMonoNerdFont-Regular.ttf");
const FONT_PROGGYCLEAN_BYTES: &[u8] =
    include_bytes!("../assets/fonts/ProggyCleanNerdFont-Regular.ttf");
const FONT_SPACEMONO_BYTES: &[u8] =
    include_bytes!("../assets/fonts/SpaceMonoNerdFont-Regular.ttf");
const FONT_TERMINESS_BYTES: &[u8] =
    include_bytes!("../assets/fonts/TerminessNerdFont-Regular.ttf");
const FONT_ZEDMONO_BYTES: &[u8] = include_bytes!("../assets/fonts/ZedMonoNerdFont-Regular.ttf");

const fn nerd_font(name: &'static str) -> Font {
    Font {
        family: iced::font::Family::Name(name),
        weight: iced::font::Weight::Normal,
        stretch: iced::font::Stretch::Normal,
        style: iced::font::Style::Normal,
    }
}

const FONT_OPTIONS: &[(&str, Font)] = &[
    ("system mono", Font::MONOSPACE),
    ("iosevka", nerd_font("Iosevka Nerd Font")),
    ("jetbrainsmono", nerd_font("JetBrainsMono Nerd Font")),
    ("proggyclean", nerd_font("ProggyClean Nerd Font")),
    ("spacemono", nerd_font("SpaceMono Nerd Font")),
    ("terminess", nerd_font("Terminess Nerd Font")),
    ("zedmono", nerd_font("ZedMono Nerd Font")),
];

// --- Loader Widget ---

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LoaderStyle {
    Braille,
    Bounce,
    Pipe,
    Gif,
    Svg,
}

impl LoaderStyle {
    const ALL: [LoaderStyle; 5] = [
        LoaderStyle::Braille,
        LoaderStyle::Bounce,
        LoaderStyle::Pipe,
        LoaderStyle::Gif,
        LoaderStyle::Svg,
    ];

    fn text_frames(self) -> &'static [&'static str] {
        match self {
            LoaderStyle::Braille => &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"],
            LoaderStyle::Bounce => &[
                "▁", "▂", "▃", "▄", "▅", "▆", "▇", "█", "▇", "▆", "▅", "▄", "▃", "▂",
            ],
            LoaderStyle::Pipe => &["|", "/", "-", "\\"],
            LoaderStyle::Gif | LoaderStyle::Svg => &[],
        }
    }

    fn label(self) -> &'static str {
        match self {
            LoaderStyle::Braille => "braille",
            LoaderStyle::Bounce => "bounce",
            LoaderStyle::Pipe => "pipe",
            LoaderStyle::Gif => "gif",
            LoaderStyle::Svg => "svg",
        }
    }

    fn next(self) -> LoaderStyle {
        let idx = LoaderStyle::ALL
            .iter()
            .position(|&s| s == self)
            .unwrap_or(0);
        LoaderStyle::ALL[(idx + 1) % LoaderStyle::ALL.len()]
    }
}

struct DemoLoader {
    style: LoaderStyle,
    frame: usize,
    gif_frames: Vec<iced_image::Handle>,
    svg_frames: Vec<svg::Handle>,
}

impl DemoLoader {
    fn new() -> Self {
        let gif_frames = decode_gif_frames();
        let svg_frames = generate_svg_frames(SVG_FRAME_COUNT);
        eprintln!(
            "[dev-hud] loader assets: {} gif frames, {} svg frames",
            gif_frames.len(),
            svg_frames.len()
        );
        Self {
            style: LoaderStyle::Braille,
            frame: 0,
            gif_frames,
            svg_frames,
        }
    }

    fn frame_count(&self) -> usize {
        match self.style {
            LoaderStyle::Braille | LoaderStyle::Bounce | LoaderStyle::Pipe => {
                self.style.text_frames().len()
            }
            LoaderStyle::Gif => self.gif_frames.len().max(1),
            LoaderStyle::Svg => self.svg_frames.len().max(1),
        }
    }

    fn tick(&mut self) {
        self.frame = (self.frame + 1) % self.frame_count();
    }

    fn cycle_style(&mut self) {
        self.style = self.style.next();
        self.frame = 0;
    }
}

fn decode_gif_frames() -> Vec<iced_image::Handle> {
    let cursor = std::io::Cursor::new(LOADER_GIF_BYTES);
    let decoder = match image::codecs::gif::GifDecoder::new(cursor) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("[dev-hud] failed to decode loader.gif: {e}");
            return Vec::new();
        }
    };
    match decoder.into_frames().collect_frames() {
        Ok(frames) => frames
            .iter()
            .map(|f| {
                let buf = f.buffer();
                let (w, h) = (buf.width(), buf.height());
                iced_image::Handle::from_rgba(w, h, buf.as_raw().clone())
            })
            .collect(),
        Err(e) => {
            eprintln!("[dev-hud] failed to collect gif frames: {e}");
            Vec::new()
        }
    }
}

fn generate_svg_frames(n: usize) -> Vec<svg::Handle> {
    (0..n)
        .map(|i| {
            let angle = (i as f64 / n as f64) * 360.0;
            let content = format!(
                r#"<svg xmlns="http://www.w3.org/2000/svg" width="128" height="128" viewBox="0 0 128 128">
  <line x1="64" y1="16" x2="64" y2="56"
    stroke="white" stroke-width="6" stroke-linecap="round"
    transform="rotate({angle} 64 64)"/>
</svg>"#
            );
            svg::Handle::from_memory(content.into_bytes())
        })
        .collect()
}

// --- Claude Code Visualizer Widget (Demo) ---

const CLAUDE_TEXT_SIZE: f32 = MARKER_SIZE * 0.45;

#[derive(Debug, Clone, Copy)]
enum SessionKind {
    Terminal,
    Code,
    Markdown,
}

impl SessionKind {
    fn icon(self, focused: bool) -> &'static str {
        match self {
            SessionKind::Terminal => "\u{f489}",
            SessionKind::Code => {
                if focused {
                    "\u{f0844}"
                } else {
                    "\u{f121}"
                }
            }
            SessionKind::Markdown => "\u{f0226}",
        }
    }
}

struct DemoSession {
    repo: &'static str,
    activity: &'static str,
    active: bool,
    kind: SessionKind,
}

struct DemoClaudeWidget {
    sessions: Vec<DemoSession>,
    spinner_frame: usize,
}

impl DemoClaudeWidget {
    fn new() -> Self {
        eprintln!("[dev-hud] demo claude: on (4 simulated sessions)");
        Self {
            sessions: vec![
                DemoSession {
                    repo: "my-repo-1",
                    activity: "Bash(git log --oneline | grep '^fix' => /tmp/out)",
                    active: true,
                    kind: SessionKind::Terminal,
                },
                DemoSession {
                    repo: "my-repo-2",
                    activity: "Write(items.filter(x => x != null && x.id >= 0 || x.flag === true))",
                    active: true,
                    kind: SessionKind::Code,
                },
                DemoSession {
                    repo: "my-repo-3",
                    activity: "Write(../appointment-view/README.md)",
                    active: false,
                    kind: SessionKind::Markdown,
                },
                DemoSession {
                    repo: "my-repo-4",
                    activity: "Edit(fn run() -> Result<(), Error> { let val <= 0xff; www ==> ok })",
                    active: true,
                    kind: SessionKind::Code,
                },
            ],
            spinner_frame: 0,
        }
    }

    fn tick(&mut self) {
        let frames = LoaderStyle::Braille.text_frames();
        self.spinner_frame = (self.spinner_frame + 1) % frames.len();
    }

    fn spinner_char(&self) -> &'static str {
        let frames = LoaderStyle::Braille.text_frames();
        frames[self.spinner_frame % frames.len()]
    }
}

fn truncate(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        return s.to_string();
    }
    let mut end = max_len;
    while !s.is_char_boundary(end) && end > 0 {
        end -= 1;
    }
    format!("{}...", &s[..end])
}

// --- HUD State ---

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HudMode {
    Hidden,
    Visible,
    Focused,
}

struct Hud {
    mode: HudMode,
    surface_id: Option<IcedId>,
    font_index: usize,
    demo_loader: Option<DemoLoader>,
    demo_claude: Option<DemoClaudeWidget>,
}

impl Hud {
    fn current_font(&self) -> Font {
        FONT_OPTIONS[self.font_index].1
    }

    fn current_font_label(&self) -> &'static str {
        FONT_OPTIONS[self.font_index].0
    }
}

#[to_layer_message(multi)]
#[derive(Debug, Clone)]
enum Message {
    ToggleVisibility,
    ToggleFocus,
    DemoLoaderToggle,
    DemoLoaderChange,
    DemoClaudeToggle,
    FontChange,
    Tick,
}

// --- IPC ---

fn socket_path() -> PathBuf {
    let runtime_dir = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".to_string());
    PathBuf::from(runtime_dir).join("dev-hud.sock")
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
                    "demo loader-toggle" => Some(Message::DemoLoaderToggle),
                    "demo loader-change" => Some(Message::DemoLoaderChange),
                    "demo claude-toggle" => Some(Message::DemoClaudeToggle),
                    "demo font-change" => Some(Message::FontChange),
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

fn tick_stream(ms: &u64) -> mpsc::UnboundedReceiver<Message> {
    let ms = *ms;
    let (tx, rx) = mpsc::unbounded();
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_millis(ms));
        if tx.unbounded_send(Message::Tick).is_err() {
            break;
        }
    });
    rx
}

// --- Layer Shell Settings ---

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

// --- Hud Implementation ---

fn main() -> Result<(), iced_layershell::Error> {
    eprintln!("[dev-hud] starting in background mode (no initial surface)");

    let settings = LayerShellSettings {
        start_mode: StartMode::Background,
        ..Default::default()
    };

    daemon(Hud::new, Hud::namespace, Hud::update, Hud::view)
        .style(Hud::style)
        .subscription(Hud::subscription)
        .font(FONT_IOSEVKA_BYTES)
        .font(FONT_JETBRAINSMONO_BYTES)
        .font(FONT_PROGGYCLEAN_BYTES)
        .font(FONT_SPACEMONO_BYTES)
        .font(FONT_TERMINESS_BYTES)
        .font(FONT_ZEDMONO_BYTES)
        .layer_settings(settings)
        .run()
}

impl Hud {
    fn new() -> (Self, Task<Message>) {
        let (id, task) = Message::layershell_open(visible_settings());
        eprintln!("[dev-hud] booting -> Visible (surface {id})");
        (
            Self {
                mode: HudMode::Visible,
                surface_id: Some(id),
                font_index: 0,
                demo_loader: None,
                demo_claude: None,
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
            Message::DemoLoaderToggle => {
                if self.demo_loader.is_some() {
                    self.demo_loader = None;
                    eprintln!("[dev-hud] demo loader: off");
                } else {
                    self.demo_loader = Some(DemoLoader::new());
                    eprintln!("[dev-hud] demo loader: on (braille)");
                }
                Task::none()
            }
            Message::DemoLoaderChange => {
                if let Some(loader) = &mut self.demo_loader {
                    loader.cycle_style();
                    eprintln!("[dev-hud] demo loader: style -> {}", loader.style.label());
                } else {
                    self.demo_loader = Some(DemoLoader::new());
                    eprintln!("[dev-hud] demo loader: on (braille)");
                }
                Task::none()
            }
            Message::DemoClaudeToggle => {
                if self.demo_claude.is_some() {
                    self.demo_claude = None;
                    eprintln!("[dev-hud] demo claude: off");
                } else {
                    self.demo_claude = Some(DemoClaudeWidget::new());
                }
                Task::none()
            }
            Message::FontChange => {
                self.font_index = (self.font_index + 1) % FONT_OPTIONS.len();
                eprintln!("[dev-hud] font -> {}", self.current_font_label());
                Task::none()
            }
            Message::Tick => {
                if let Some(loader) = &mut self.demo_loader {
                    loader.tick();
                }
                if let Some(claude) = &mut self.demo_claude {
                    claude.tick();
                }
                Task::none()
            }
            _ => Task::none(),
        }
    }

    fn view(&self, _window_id: IcedId) -> Element<'_, Message> {
        let mono = self.current_font();
        let shaped = Shaping::Advanced;
        let marker = || text("+").size(MARKER_SIZE).color(MARKER_COLOR);

        // Top row: markers + font label in top-right
        let font_label = text(self.current_font_label())
            .size(LOADER_TEXT_SIZE * 0.6)
            .color(MUTED_COLOR)
            .font(mono)
            .shaping(shaped);
        let top_row = row![marker(), space::horizontal(), font_label, text(" ").size(4), marker()];

        // Build bottom row (with optional loader widget)
        let bottom_row = if let Some(loader) = &self.demo_loader {
            let label: Element<'_, Message> = text(format!(" {}", loader.style.label()))
                .size(LOADER_TEXT_SIZE * 0.6)
                .color(MUTED_COLOR)
                .into();

            let widget: Element<'_, Message> = match loader.style {
                LoaderStyle::Braille | LoaderStyle::Bounce | LoaderStyle::Pipe => {
                    let frames = loader.style.text_frames();
                    let ch = frames[loader.frame % frames.len()];
                    text(format!(" {ch}"))
                        .size(LOADER_TEXT_SIZE)
                        .color(MARKER_COLOR)
                        .font(mono)
                        .shaping(shaped)
                        .into()
                }
                LoaderStyle::Gif => {
                    if loader.gif_frames.is_empty() {
                        text(" ?").size(LOADER_TEXT_SIZE).color(MARKER_COLOR).into()
                    } else {
                        let handle =
                            loader.gif_frames[loader.frame % loader.gif_frames.len()].clone();
                        container(
                            iced_image(handle)
                                .width(LOADER_IMAGE_SIZE)
                                .height(LOADER_IMAGE_SIZE),
                        )
                        .padding(iced::padding::left(4))
                        .into()
                    }
                }
                LoaderStyle::Svg => {
                    if loader.svg_frames.is_empty() {
                        text(" ?").size(LOADER_TEXT_SIZE).color(MARKER_COLOR).into()
                    } else {
                        let handle =
                            loader.svg_frames[loader.frame % loader.svg_frames.len()].clone();
                        container(svg(handle).width(LOADER_IMAGE_SIZE).height(LOADER_IMAGE_SIZE))
                            .padding(iced::padding::left(4))
                            .into()
                    }
                }
            };

            row![marker(), widget, label, space::horizontal(), marker()]
        } else {
            row![marker(), space::horizontal(), marker()]
        };

        // Build main column
        let mut main_col = column![top_row]
            .width(Length::Fill)
            .height(Length::Fill);

        main_col = main_col.push(space::vertical());

        // Claude code visualizer sessions (rendered above bottom row)
        if let Some(claude) = &self.demo_claude {
            let focused = self.mode == HudMode::Focused;
            let max_chars: usize = if focused { 512 } else { 64 };

            for session in &claude.sessions {
                let icon_str = if session.active {
                    claude.spinner_char()
                } else {
                    session.kind.icon(focused)
                };

                let activity = truncate(session.activity, max_chars);

                let mut srow = row![];

                srow = srow.push(
                    text(format!("{icon_str} "))
                        .size(CLAUDE_TEXT_SIZE)
                        .color(MARKER_COLOR)
                        .font(mono)
                        .shaping(shaped),
                );

                if focused {
                    srow = srow.push(
                        text(format!("{} ", session.repo))
                            .size(CLAUDE_TEXT_SIZE)
                            .color(MUTED_COLOR)
                            .font(mono)
                            .shaping(shaped),
                    );
                }

                srow = srow.push(
                    text(activity)
                        .size(CLAUDE_TEXT_SIZE)
                        .color(MARKER_COLOR)
                        .font(mono)
                        .shaping(shaped),
                );

                main_col = main_col.push(srow);
            }

            main_col = main_col.push(space::Space::new().height(4));
        }

        main_col = main_col.push(bottom_row);

        container(main_col)
            .padding(EDGE_MARGIN)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    fn subscription(state: &Self) -> Subscription<Message> {
        let socket = Subscription::run(socket_listener);
        let needs_tick = (state.demo_loader.is_some() || state.demo_claude.is_some())
            && state.mode != HudMode::Hidden;
        if needs_tick {
            let tick = Subscription::run_with(TICK_MS, tick_stream);
            Subscription::batch([socket, tick])
        } else {
            socket
        }
    }

    fn style(&self, _theme: &iced::Theme) -> iced::theme::Style {
        iced::theme::Style {
            background_color: Color::TRANSPARENT,
            text_color: MARKER_COLOR,
        }
    }
}
