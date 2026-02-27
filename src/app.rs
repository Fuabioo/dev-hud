use crate::demo::create_demo_widget;
use crate::events::TaggedEvent;
use crate::ipc;
use crate::loader::*;
use crate::session::*;
use crate::shell;
use crate::surface::*;
use crate::theme::{self, ThemeColors, ThemeMode};

use iced::{Color, Element, Font, Subscription, Task};
use iced_layershell::build_pattern::daemon;
use iced_layershell::settings::{LayerShellSettings, StartMode};
use iced_layershell::to_layer_message;

pub(crate) const EDGE_MARGIN: u16 = 40;

const TICK_MS: u64 = 80;

// --- HUD State ---

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HudMode {
    Hidden,
    Visible,
    Focused,
}

pub(crate) struct Hud {
    pub(crate) mode: HudMode,
    pub(crate) surface_id: Option<IcedId>,
    pub(crate) font_index: usize,
    pub(crate) demo_loader: Option<DemoLoader>,
    pub(crate) demo_claude: Option<ClaudeWidget>,
    pub(crate) claude: Option<ClaudeWidget>,
    pub(crate) modal: Option<ModalState>,
    pub(crate) archive_modal: Option<ArchiveModalState>,
    pub(crate) hovered_session: Option<usize>,
    pub(crate) hovered_archive: bool,
    pub(crate) theme_mode: ThemeMode,
    pub(crate) colors: ThemeColors,
    pub(crate) backdrop: bool,
    pub(crate) target_output: Option<String>,
    pub(crate) shells: Option<shell::ShellState>,
}

impl Hud {
    pub(crate) fn current_font(&self) -> Font {
        FONT_OPTIONS[self.font_index].1
    }

    pub(crate) fn current_font_label(&self) -> &'static str {
        FONT_OPTIONS[self.font_index].0
    }

    fn close_modal_task(&mut self) -> Task<Message> {
        if let Some(modal) = self.modal.take() {
            Task::done(Message::RemoveWindow(modal.surface_id))
        } else {
            Task::none()
        }
    }

    fn close_archive_modal_task(&mut self) -> Task<Message> {
        if let Some(archive) = self.archive_modal.take() {
            Task::done(Message::RemoveWindow(archive.surface_id))
        } else {
            Task::none()
        }
    }

    /// Returns the active claude widget: live takes precedence over demo.
    pub(crate) fn active_claude(&self) -> Option<&ClaudeWidget> {
        self.claude.as_ref().or(self.demo_claude.as_ref())
    }

    /// Recreate the main surface on the current target output.
    fn recreate_surface(&mut self) -> Task<Message> {
        let modal_task = self.close_modal_task();
        let archive_task = self.close_archive_modal_task();
        let remove_task = if let Some(id) = self.surface_id.take() {
            Task::done(Message::RemoveWindow(id))
        } else {
            Task::none()
        };
        let settings = match self.mode {
            HudMode::Hidden => return Task::batch([modal_task, archive_task]),
            HudMode::Visible => visible_settings(self.target_output.as_deref()),
            HudMode::Focused => focused_settings(self.target_output.as_deref()),
        };
        let (id, open_task) = Message::layershell_open(settings);
        self.surface_id = Some(id);
        Task::batch([modal_task, archive_task, remove_task, open_task])
    }
}

#[to_layer_message(multi)]
#[derive(Debug, Clone)]
pub(crate) enum Message {
    ToggleVisibility,
    ToggleFocus,
    DemoLoaderToggle,
    DemoLoaderChange,
    DemoClaudeToggle,
    ClaudeLiveToggle,
    FontChange,
    Tick,
    OpenSessionModal(usize),
    CloseModal,
    SelectActivity(usize),
    HoverSession(usize),
    UnhoverSession(usize),
    HoverEntry(usize),
    UnhoverEntry(usize),
    WatcherEvent(TaggedEvent),
    CopySessionId(String),
    ThemeSet(ThemeMode),
    ThemeToggle,
    ThemeRefresh,
    BackdropToggle,
    ScreenCycle,
    ScreenSet(String),
    OpenArchiveModal,
    CloseArchiveModal,
    HoverArchive,
    UnhoverArchive,
    SelectArchivedSession(usize),
    HoverArchivedSession(usize),
    UnhoverArchivedSession(usize),
    SelectArchivedEntry(usize),
    HoverArchivedEntry(usize),
    UnhoverArchivedEntry(usize),
    ShellEvent(shell::ShellEvent),
    ShellToggle,
    SetAttention(String, bool),
}

pub(crate) fn run() -> Result<(), iced_layershell::Error> {
    eprintln!(
        "[dev-hud] v{} ({}) starting in background mode",
        env!("DEV_HUD_VERSION"),
        env!("DEV_HUD_COMMIT")
    );

    let settings = LayerShellSettings {
        start_mode: StartMode::Background,
        ..Default::default()
    };

    daemon(Hud::new, Hud::namespace, Hud::update, Hud::view)
        .style(Hud::style)
        .subscription(Hud::subscription)
        .font(FONT_JETBRAINSMONO_BYTES)
        .font(FONT_SPACEMONO_BYTES)
        .layer_settings(settings)
        .run()
}

impl Hud {
    fn new() -> (Self, Task<Message>) {
        let theme_mode = ThemeMode::Dark;
        let colors = theme::resolve(theme_mode);

        // Default output: DEV_HUD_SCREEN env var, falling back to any active monitor
        let target_output = std::env::var("DEV_HUD_SCREEN")
            .ok()
            .filter(|s| !s.is_empty());
        if let Some(ref name) = target_output {
            eprintln!("[dev-hud] target screen: {name} (from DEV_HUD_SCREEN)");
        }

        // Auto-enable live watcher if Claude Code is installed
        let claude = if std::process::Command::new("claude")
            .arg("--version")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
        {
            eprintln!("[dev-hud] claude-live: auto-enabled (claude found in PATH)");
            Some(ClaudeWidget::new())
        } else {
            None
        };

        // Auto-enable shell widgets if config file exists
        let shells = if shell::config_file_path().exists() {
            eprintln!("[dev-hud] shells: auto-enabled (config file found)");
            Some(shell::ShellState::default())
        } else {
            None
        };

        let (id, task) = Message::layershell_open(visible_settings(target_output.as_deref()));
        eprintln!("[dev-hud] booting -> Visible (surface {id})");
        (
            Self {
                mode: HudMode::Visible,
                surface_id: Some(id),
                font_index: 0,
                demo_loader: None,
                demo_claude: None,
                claude,
                modal: None,
                archive_modal: None,
                hovered_session: None,
                hovered_archive: false,
                theme_mode,
                colors,
                backdrop: false,
                target_output,
                shells,
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
                    let (id, task) = Message::layershell_open(visible_settings(self.target_output.as_deref()));
                    self.surface_id = Some(id);
                    self.mode = HudMode::Visible;
                    eprintln!("[dev-hud] Hidden -> Visible");
                    task
                }
                mode @ (HudMode::Visible | HudMode::Focused) => {
                    let modal_task = self.close_modal_task();
                    let archive_task = self.close_archive_modal_task();
                    let task = if let Some(id) = self.surface_id.take() {
                        Task::done(Message::RemoveWindow(id))
                    } else {
                        Task::none()
                    };
                    self.mode = HudMode::Hidden;
                    eprintln!("[dev-hud] {mode:?} -> Hidden");
                    Task::batch([modal_task, archive_task, task])
                }
            },
            Message::ToggleFocus => match self.mode {
                HudMode::Hidden => {
                    let (id, task) = Message::layershell_open(focused_settings(self.target_output.as_deref()));
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
                    let (id, open_task) = Message::layershell_open(focused_settings(self.target_output.as_deref()));
                    self.surface_id = Some(id);
                    self.mode = HudMode::Focused;
                    eprintln!("[dev-hud] Visible -> Focused");
                    Task::batch([remove_task, open_task])
                }
                HudMode::Focused => {
                    let modal_task = self.close_modal_task();
                    let archive_task = self.close_archive_modal_task();
                    let remove_task = if let Some(id) = self.surface_id.take() {
                        Task::done(Message::RemoveWindow(id))
                    } else {
                        Task::none()
                    };
                    let (id, open_task) = Message::layershell_open(visible_settings(self.target_output.as_deref()));
                    self.surface_id = Some(id);
                    self.mode = HudMode::Visible;
                    eprintln!("[dev-hud] Focused -> Visible");
                    Task::batch([modal_task, archive_task, remove_task, open_task])
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
                    let modal_task = self.close_modal_task();
                    let archive_task = self.close_archive_modal_task();
                    self.demo_claude = None;
                    eprintln!("[dev-hud] demo claude: off");
                    Task::batch([modal_task, archive_task])
                } else {
                    self.demo_claude = Some(create_demo_widget());
                    Task::none()
                }
            }
            Message::ClaudeLiveToggle => {
                if self.claude.is_some() {
                    let modal_task = self.close_modal_task();
                    let archive_task = self.close_archive_modal_task();
                    self.claude = None;
                    eprintln!("[dev-hud] claude live: off");
                    Task::batch([modal_task, archive_task])
                } else {
                    self.claude = Some(ClaudeWidget::new());
                    eprintln!("[dev-hud] claude live: on (watcher starting)");
                    Task::none()
                }
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
                if let Some(claude) = &mut self.claude {
                    claude.tick();
                }
                Task::none()
            }
            Message::OpenSessionModal(idx) => {
                if self.mode != HudMode::Focused || self.modal.is_some() {
                    return Task::none();
                }
                // Ensure there's an active claude widget with this index
                let has_session = self
                    .active_claude()
                    .is_some_and(|c| idx < c.sessions.len());
                if !has_session {
                    return Task::none();
                }
                // Close archive modal if open (mutual exclusion)
                let archive_task = self.close_archive_modal_task();
                self.hovered_session = None;
                let (id, task) = Message::layershell_open(modal_settings(self.target_output.as_deref()));
                self.modal = Some(ModalState {
                    surface_id: id,
                    session_index: idx,
                    selected_entry: None,
                    hovered_entry: None,
                });
                eprintln!("[dev-hud] modal opened for session {idx}");
                Task::batch([archive_task, task])
            }
            Message::CloseModal => self.close_modal_task(),
            Message::SelectActivity(i) => {
                if let Some(ref mut modal) = self.modal {
                    if modal.selected_entry == Some(i) {
                        modal.selected_entry = None;
                    } else {
                        modal.selected_entry = Some(i);
                    }
                }
                Task::none()
            }
            Message::HoverSession(i) => {
                self.hovered_session = Some(i);
                Task::none()
            }
            Message::UnhoverSession(i) => {
                if self.hovered_session == Some(i) {
                    self.hovered_session = None;
                }
                Task::none()
            }
            Message::HoverEntry(i) => {
                if let Some(ref mut modal) = self.modal {
                    modal.hovered_entry = Some(i);
                }
                Task::none()
            }
            Message::UnhoverEntry(i) => {
                if let Some(ref mut modal) = self.modal {
                    if modal.hovered_entry == Some(i) {
                        modal.hovered_entry = None;
                    }
                }
                Task::none()
            }
            Message::WatcherEvent(tagged) => {
                if let Some(claude) = &mut self.claude {
                    claude.process_event(tagged);
                }
                Task::none()
            }
            Message::CopySessionId(uuid) => {
                std::thread::spawn(move || {
                    match std::process::Command::new("wl-copy")
                        .arg(&uuid)
                        .status()
                    {
                        Ok(s) if s.success() => eprintln!("[dev-hud] copied session UUID"),
                        Ok(s) => eprintln!("[dev-hud] wl-copy exited: {s}"),
                        Err(e) => eprintln!("[dev-hud] wl-copy failed: {e}"),
                    }
                });
                Task::none()
            }
            Message::ThemeSet(mode) => {
                self.theme_mode = mode;
                self.colors = theme::resolve(mode);
                if mode == ThemeMode::Adaptive {
                    self.backdrop = true;
                }
                eprintln!("[dev-hud] theme -> {mode:?}");
                Task::none()
            }
            Message::ThemeToggle => {
                // Flip appearance without changing theme_mode.
                // Auto/Adaptive will re-evaluate on the next ThemeRefresh cycle;
                // Dark/Light stay flipped permanently.
                self.colors = if self.colors.is_dark {
                    ThemeColors::light()
                } else {
                    ThemeColors::dark()
                };
                eprintln!(
                    "[dev-hud] theme toggle -> {} (mode stays {:?})",
                    if self.colors.is_dark { "dark" } else { "light" },
                    self.theme_mode
                );
                Task::none()
            }
            Message::ThemeRefresh => {
                match self.theme_mode {
                    ThemeMode::Auto => {
                        let dark = theme::detect_system_dark();
                        let was_dark = self.colors.is_dark;
                        self.colors =
                            if dark { ThemeColors::dark() } else { ThemeColors::light() };
                        if was_dark != self.colors.is_dark {
                            eprintln!(
                                "[dev-hud] auto: switched to {}",
                                if self.colors.is_dark { "dark" } else { "light" }
                            );
                        }
                    }
                    ThemeMode::Adaptive => {
                        if let Some(lum) = theme::sample_bg_luminance() {
                            let was_dark = self.colors.is_dark;
                            self.colors = if lum <= 0.5 {
                                ThemeColors::dark()
                            } else {
                                ThemeColors::light()
                            };
                            if was_dark != self.colors.is_dark {
                                eprintln!(
                                    "[dev-hud] adaptive: switched to {} (lum={lum:.3})",
                                    if self.colors.is_dark { "dark" } else { "light" }
                                );
                            }
                        }
                    }
                    _ => {}
                }
                Task::none()
            }
            Message::BackdropToggle => {
                self.backdrop = !self.backdrop;
                eprintln!("[dev-hud] backdrop -> {}", self.backdrop);
                Task::none()
            }
            Message::ScreenCycle => {
                let outputs = enumerate_outputs();
                if outputs.is_empty() {
                    eprintln!(
                        "[dev-hud] screen cycle: no outputs found (is wlr-randr installed?)"
                    );
                    return Task::none();
                }
                let current_idx = self
                    .target_output
                    .as_ref()
                    .and_then(|name| outputs.iter().position(|o| o == name));
                let next_idx = match current_idx {
                    Some(idx) => (idx + 1) % outputs.len(),
                    None => 0,
                };
                let next_output = &outputs[next_idx];
                self.target_output = Some(next_output.clone());
                eprintln!(
                    "[dev-hud] screen -> {} ({}/{})",
                    next_output,
                    next_idx + 1,
                    outputs.len()
                );
                self.recreate_surface()
            }
            Message::ScreenSet(ref name) => {
                self.target_output = Some(name.clone());
                eprintln!("[dev-hud] screen -> {name}");
                self.recreate_surface()
            }
            Message::OpenArchiveModal => {
                if self.mode != HudMode::Focused || self.archive_modal.is_some() {
                    return Task::none();
                }
                let has_archived = self
                    .active_claude()
                    .is_some_and(|c| c.sessions.iter().any(|s| s.archived));
                if !has_archived {
                    return Task::none();
                }
                // Close session modal if open (mutual exclusion)
                let modal_task = self.close_modal_task();
                let (id, task) = Message::layershell_open(modal_settings(self.target_output.as_deref()));
                self.archive_modal = Some(ArchiveModalState {
                    surface_id: id,
                    selected_session: None,
                    selected_entry: None,
                    hovered_session: None,
                    hovered_entry: None,
                });
                eprintln!("[dev-hud] archive modal opened");
                Task::batch([modal_task, task])
            }
            Message::CloseArchiveModal => self.close_archive_modal_task(),
            Message::HoverArchive => {
                self.hovered_archive = true;
                Task::none()
            }
            Message::UnhoverArchive => {
                self.hovered_archive = false;
                Task::none()
            }
            Message::SelectArchivedSession(i) => {
                if let Some(ref mut archive) = self.archive_modal {
                    if archive.selected_session == Some(i) {
                        archive.selected_session = None;
                        archive.selected_entry = None;
                    } else {
                        archive.selected_session = Some(i);
                        archive.selected_entry = None;
                    }
                }
                Task::none()
            }
            Message::HoverArchivedSession(i) => {
                if let Some(ref mut archive) = self.archive_modal {
                    archive.hovered_session = Some(i);
                }
                Task::none()
            }
            Message::UnhoverArchivedSession(i) => {
                if let Some(ref mut archive) = self.archive_modal {
                    if archive.hovered_session == Some(i) {
                        archive.hovered_session = None;
                    }
                }
                Task::none()
            }
            Message::SelectArchivedEntry(i) => {
                if let Some(ref mut archive) = self.archive_modal {
                    if archive.selected_entry == Some(i) {
                        archive.selected_entry = None;
                    } else {
                        archive.selected_entry = Some(i);
                    }
                }
                Task::none()
            }
            Message::HoverArchivedEntry(i) => {
                if let Some(ref mut archive) = self.archive_modal {
                    archive.hovered_entry = Some(i);
                }
                Task::none()
            }
            Message::UnhoverArchivedEntry(i) => {
                if let Some(ref mut archive) = self.archive_modal {
                    if archive.hovered_entry == Some(i) {
                        archive.hovered_entry = None;
                    }
                }
                Task::none()
            }
            Message::ShellEvent(event) => {
                if let Some(shells) = &mut self.shells {
                    shells.apply_event(&event);
                }
                Task::none()
            }
            Message::ShellToggle => {
                if self.shells.is_some() {
                    self.shells = None;
                    eprintln!("[dev-hud] shells: off");
                } else {
                    self.shells = Some(shell::ShellState::default());
                    eprintln!("[dev-hud] shells: on");
                }
                Task::none()
            }
            Message::SetAttention(ref session_id, value) => {
                if let Some(claude) = &mut self.claude {
                    if let Some(&idx) = claude.session_index_map.get(session_id) {
                        claude.sessions[idx].needs_attention = value;
                        eprintln!(
                            "[dev-hud] attention {} for session {}",
                            if value { "set" } else { "cleared" },
                            session_id
                        );
                    }
                }
                Task::none()
            }
            _ => Task::none(),
        }
    }

    fn view(&self, window_id: IcedId) -> Element<'_, Message> {
        if let Some(ref modal) = self.modal {
            if window_id == modal.surface_id {
                return self.view_modal(modal);
            }
        }
        if let Some(ref archive) = self.archive_modal {
            if window_id == archive.surface_id {
                return self.view_archive_modal(archive);
            }
        }
        self.view_hud()
    }

    fn subscription(state: &Self) -> Subscription<Message> {
        let socket = Subscription::run(ipc::socket_listener);
        let needs_tick = (state.demo_loader.is_some()
            || state.demo_claude.is_some()
            || state.claude.is_some())
            && state.mode != HudMode::Hidden;

        let mut subs = vec![socket];

        if needs_tick {
            subs.push(Subscription::run_with(TICK_MS, ipc::tick_stream));
        }

        if state.claude.is_some() {
            subs.push(Subscription::run(ipc::watcher_stream));
        }

        if state.shells.is_some() {
            subs.push(Subscription::run(ipc::shell_event_stream));
        }

        // Theme refresh for auto/adaptive modes (5s interval)
        if matches!(state.theme_mode, ThemeMode::Auto | ThemeMode::Adaptive) {
            subs.push(Subscription::run(ipc::theme_refresh_stream));
        }

        Subscription::batch(subs)
    }

    fn style(&self, _theme: &iced::Theme) -> iced::theme::Style {
        iced::theme::Style {
            background_color: Color::TRANSPARENT,
            text_color: self.colors.marker,
        }
    }
}
