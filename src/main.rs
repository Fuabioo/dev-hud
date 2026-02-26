mod events;
mod theme;
mod util;
mod watcher;

use std::collections::HashMap;
use std::io::BufRead;
use std::os::unix::net::UnixListener;
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use events::{SessionEvent, TaggedEvent, ToolCategory};
use theme::{ThemeColors, ThemeMode};
use util::truncate_str;
use watcher::MultiWatcherHandle;

use futures::channel::mpsc;
use iced::widget::text::Shaping;
use iced::widget::{
    column, container, image as iced_image, mouse_area, row, scrollable, space, svg, text,
};
use iced::{mouse, Color, Element, Font, Length, Subscription, Task};
use iced_layershell::build_pattern::daemon;
use iced_layershell::reexport::{Anchor, KeyboardInteractivity, Layer, NewLayerShellSettings};
use iced_layershell::settings::{LayerShellSettings, StartMode};
use iced_layershell::to_layer_message;
use image::AnimationDecoder;

type IcedId = iced_layershell::reexport::IcedId;

const EDGE_MARGIN: u16 = 40;

const TICK_MS: u64 = 80;
const LOADER_IMAGE_SIZE: f32 = 20.0;
const SVG_FRAME_COUNT: usize = 12;

const LOADER_GIF_BYTES: &[u8] = include_bytes!("../assets/loader.gif");

// --- Embedded Fonts ---

const FONT_JETBRAINSMONO_BYTES: &[u8] =
    include_bytes!("../assets/fonts/JetBrainsMonoNerdFont-Regular.ttf");
const FONT_SPACEMONO_BYTES: &[u8] =
    include_bytes!("../assets/fonts/SpaceMonoNerdFont-Regular.ttf");

const fn nerd_font(name: &'static str) -> Font {
    Font {
        family: iced::font::Family::Name(name),
        weight: iced::font::Weight::Normal,
        stretch: iced::font::Stretch::Normal,
        style: iced::font::Style::Normal,
    }
}

const FONT_OPTIONS: &[(&str, Font)] = &[
    ("jetbrainsmono", nerd_font("JetBrainsMono Nerd Font")),
    ("spacemono", nerd_font("SpaceMono Nerd Font")),
    ("system mono", Font::MONOSPACE),
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

// --- Tool-State Loader Frame Arrays ---

fn tool_state_frames(category: ToolCategory) -> &'static [&'static str] {
    match category {
        ToolCategory::Reading => &["◉", "◎", "○", "◎"],
        ToolCategory::Writing => &["✎", "✏", "✐", "✎"],
        ToolCategory::Running => &["⚙", "⚙︎", "⚙", "⚙︎"],
        ToolCategory::Thinking => &["◐", "◓", "◑", "◒"],
        ToolCategory::Spawning => &["◇", "◆", "◇", "◆"],
        ToolCategory::Web => &["◌", "◍", "●", "◍"],
        ToolCategory::Mcp => &["⚡", "✦", "⚡", "✦"],
        ToolCategory::Awaiting => &["❯", "❯❯", "❯❯❯", "❯❯"],
        ToolCategory::Unknown => &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"],
    }
}

// --- Claude Code Visualizer Widget ---


const MAX_VISIBLE_SESSIONS: usize = 6;
const ARCHIVE_GRACE_SECS: u64 = 300; // 5 minutes

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

// --- Unified Session / Activity types (String-based) ---

struct Session {
    session_id: String,
    project_slug: String,
    active: bool,
    kind: SessionKind,
    current_tool: Option<ActiveTool>,
    activity: String,
    exited_at: Option<SystemTime>,
    archived: bool,
}

struct ActiveTool {
    #[allow(dead_code)]
    tool_name: String,
    tool_use_id: String,
    category: ToolCategory,
    #[allow(dead_code)]
    description: String,
}

#[allow(dead_code)]
struct ActivityEntry {
    timestamp: String,
    tool: String,
    summary: String,
    detail: String,
    is_error: bool,
    category: ToolCategory,
}

struct ModalState {
    surface_id: IcedId,
    session_index: usize,
    selected_entry: Option<usize>,
    hovered_entry: Option<usize>,
}

struct ArchiveModalState {
    surface_id: IcedId,
    selected_session: Option<usize>,
    selected_entry: Option<usize>,
    hovered_session: Option<usize>,
    hovered_entry: Option<usize>,
}

struct ClaudeWidget {
    sessions: Vec<Session>,
    activity_logs: Vec<Vec<ActivityEntry>>,
    spinner_frame: usize,
    session_index_map: HashMap<String, usize>,
}

impl ClaudeWidget {
    fn new() -> Self {
        Self {
            sessions: Vec::new(),
            activity_logs: Vec::new(),
            spinner_frame: 0,
            session_index_map: HashMap::new(),
        }
    }

    fn tick(&mut self) {
        self.spinner_frame = self.spinner_frame.wrapping_add(1);
        let now = SystemTime::now();
        for session in &mut self.sessions {
            if let Some(exited_at) = session.exited_at {
                if !session.archived {
                    if let Ok(elapsed) = now.duration_since(exited_at) {
                        if elapsed >= Duration::from_secs(ARCHIVE_GRACE_SECS) {
                            session.archived = true;
                        }
                    }
                }
            }
        }
    }

    fn spinner_char(&self) -> &'static str {
        let frames = LoaderStyle::Braille.text_frames();
        frames[(self.spinner_frame / 4) % frames.len()]
    }

    /// Core state machine: process a tagged event from the watcher.
    fn process_event(&mut self, tagged: TaggedEvent) {
        let TaggedEvent { session_id, event } = tagged;

        match event {
            SessionEvent::SessionStart { project, .. } => {
                if self.session_index_map.contains_key(&session_id) {
                    return; // Already tracked
                }
                let idx = self.sessions.len();
                self.sessions.push(Session {
                    session_id: session_id.clone(),
                    project_slug: project,
                    active: true,
                    kind: SessionKind::Code,
                    current_tool: None,
                    activity: "starting...".to_string(),
                    exited_at: None,
                    archived: false,
                });
                self.activity_logs.push(Vec::new());
                self.session_index_map.insert(session_id, idx);
            }
            SessionEvent::UserPrompt { text } => {
                if let Some(&idx) = self.session_index_map.get(&session_id) {
                    let session = &mut self.sessions[idx];
                    if session.exited_at.is_some() {
                        return; // Don't reactivate an exited session
                    }
                    session.active = true;
                    session.activity = truncate_str(&text, 200);
                    let ts = format_time_now();
                    self.activity_logs[idx].push(ActivityEntry {
                        timestamp: ts,
                        tool: "User".to_string(),
                        summary: truncate_str(&text, 80),
                        detail: text,
                        is_error: false,
                        category: ToolCategory::Unknown,
                    });
                }
            }
            SessionEvent::ToolStart {
                tool_name,
                tool_use_id,
                category,
                description,
            } => {
                if let Some(&idx) = self.session_index_map.get(&session_id) {
                    let session = &mut self.sessions[idx];
                    if session.exited_at.is_some() {
                        return; // Don't reactivate an exited session
                    }
                    session.active = true;
                    session.activity = format!("{tool_name}({description})");
                    session.current_tool = Some(ActiveTool {
                        tool_name: tool_name.clone(),
                        tool_use_id: tool_use_id.clone(),
                        category,
                        description: description.clone(),
                    });

                    // Determine timestamp from SystemTime
                    let ts = format_time_now();

                    self.activity_logs[idx].push(ActivityEntry {
                        timestamp: ts,
                        tool: tool_name,
                        summary: truncate_str(&description, 80),
                        detail: description,
                        is_error: false,
                        category,
                    });
                }
            }
            SessionEvent::ToolEnd {
                tool_use_id,
                is_error,
                error_message,
            } => {
                if let Some(&idx) = self.session_index_map.get(&session_id) {
                    let session = &mut self.sessions[idx];
                    // Clear current tool if it matches
                    if session
                        .current_tool
                        .as_ref()
                        .is_some_and(|t| t.tool_use_id == tool_use_id)
                    {
                        session.current_tool = None;
                    }

                    // Mark error on the last matching log entry
                    if is_error {
                        if let Some(entry) = self.activity_logs[idx].last_mut() {
                            entry.is_error = true;
                            if let Some(ref msg) = error_message {
                                entry.detail = msg.clone();
                            }
                        }
                    }
                }
            }
            SessionEvent::Thinking => {
                if let Some(&idx) = self.session_index_map.get(&session_id) {
                    let session = &mut self.sessions[idx];
                    session.active = true;
                    session.activity = "thinking...".to_string();
                    session.current_tool = Some(ActiveTool {
                        tool_name: "thinking".to_string(),
                        tool_use_id: String::new(),
                        category: ToolCategory::Thinking,
                        description: "thinking...".to_string(),
                    });
                }
            }
            SessionEvent::TurnComplete { .. } => {
                if let Some(&idx) = self.session_index_map.get(&session_id) {
                    let session = &mut self.sessions[idx];
                    session.current_tool = None;
                    // Don't set inactive — another turn may follow
                }
            }
            SessionEvent::AgentSpawned { description, .. } => {
                if let Some(&idx) = self.session_index_map.get(&session_id) {
                    let ts = format_time_now();
                    self.activity_logs[idx].push(ActivityEntry {
                        timestamp: ts,
                        tool: "Agent".to_string(),
                        summary: truncate_str(&description, 80),
                        detail: description,
                        is_error: false,
                        category: ToolCategory::Spawning,
                    });
                }
            }
            SessionEvent::ContextCompaction => {
                if let Some(&idx) = self.session_index_map.get(&session_id) {
                    let ts = format_time_now();
                    self.activity_logs[idx].push(ActivityEntry {
                        timestamp: ts,
                        tool: "System".to_string(),
                        summary: "context compaction".to_string(),
                        detail: "Context window compacted to free space".to_string(),
                        is_error: false,
                        category: ToolCategory::Unknown,
                    });
                }
            }
            SessionEvent::TokenUsage { .. } => {
                // Token usage tracked silently; could display in modal later
            }
            SessionEvent::SessionEnd => {
                if let Some(&idx) = self.session_index_map.get(&session_id) {
                    let session = &mut self.sessions[idx];
                    session.active = false;
                    session.current_tool = None;
                    session.activity = "session ended".to_string();
                    session.exited_at = Some(SystemTime::now());

                    let ts = format_time_now();
                    self.activity_logs[idx].push(ActivityEntry {
                        timestamp: ts,
                        tool: "System".to_string(),
                        summary: "session exited (/exit)".to_string(),
                        detail: "User ran /exit to end the session".to_string(),
                        is_error: false,
                        category: ToolCategory::Unknown,
                    });
                }
            }
        }
    }
}

/// Simple HH:MM:SS formatter for current time (UTC).
fn format_time_now() -> String {
    match std::time::SystemTime::now().duration_since(std::time::SystemTime::UNIX_EPOCH) {
        Ok(d) => {
            let total_secs = d.as_secs();
            let hours = (total_secs / 3600) % 24;
            let minutes = (total_secs / 60) % 60;
            let seconds = total_secs % 60;
            format!("{hours:02}:{minutes:02}:{seconds:02}")
        }
        Err(_) => "00:00:00".to_string(),
    }
}

// --- Demo data builder ---

fn create_demo_widget() -> ClaudeWidget {
    eprintln!("[dev-hud] demo claude: on (4 simulated sessions)");

    let sessions = vec![
        Session {
            session_id: "demo-0000-0000-0000-000000000001".to_string(),
            project_slug: "my-repo-1".to_string(),
            active: true,
            kind: SessionKind::Terminal,
            current_tool: Some(ActiveTool {
                tool_name: "Bash".to_string(),
                tool_use_id: "demo_bash_1".to_string(),
                category: ToolCategory::Running,
                description: "git log --oneline | grep '^fix' => /tmp/out".to_string(),
            }),
            activity: "Bash(git log --oneline | grep '^fix' => /tmp/out)".to_string(),
            exited_at: None,
            archived: false,
        },
        Session {
            session_id: "demo-0000-0000-0000-000000000002".to_string(),
            project_slug: "my-repo-2".to_string(),
            active: true,
            kind: SessionKind::Code,
            current_tool: Some(ActiveTool {
                tool_name: "Write".to_string(),
                tool_use_id: "demo_write_1".to_string(),
                category: ToolCategory::Writing,
                description: "items.filter(x => x != null && x.id >= 0 || x.flag === true)"
                    .to_string(),
            }),
            activity: "Write(items.filter(x => x != null && x.id >= 0 || x.flag === true))"
                .to_string(),
            exited_at: None,
            archived: false,
        },
        Session {
            session_id: "demo-0000-0000-0000-000000000003".to_string(),
            project_slug: "my-repo-3".to_string(),
            active: false,
            kind: SessionKind::Markdown,
            current_tool: None,
            activity: "Write(../appointment-view/README.md)".to_string(),
            exited_at: None,
            archived: false,
        },
        Session {
            session_id: "demo-0000-0000-0000-000000000004".to_string(),
            project_slug: "my-repo-4".to_string(),
            active: true,
            kind: SessionKind::Code,
            current_tool: Some(ActiveTool {
                tool_name: "Edit".to_string(),
                tool_use_id: "demo_edit_1".to_string(),
                category: ToolCategory::Writing,
                description: "fn run() -> Result<(), Error> { let val <= 0xff; www ==> ok }"
                    .to_string(),
            }),
            activity: "Edit(fn run() -> Result<(), Error> { let val <= 0xff; www ==> ok })"
                .to_string(),
            exited_at: None,
            archived: false,
        },
    ];

    let mut session_index_map = HashMap::new();
    for (i, s) in sessions.iter().enumerate() {
        session_index_map.insert(s.session_id.clone(), i);
    }

    // --- Demo activity logs ---
    let activity_logs = vec![
        // my-repo-1 (Terminal): debugging a Go service
        vec![
            ActivityEntry { timestamp: "14:30:01".into(), tool: "Bash".into(), summary: "git log --oneline | head -20".into(), detail: "a1b2c3d fix: handle nil pointer in event handler\ne4f5g6h feat: add retry logic for API calls\ni7j8k9l refactor: extract validation into separate fn".into(), is_error: false, category: ToolCategory::Running },
            ActivityEntry { timestamp: "14:30:08".into(), tool: "Read".into(), summary: "main.go (lines 1-85)".into(), detail: "Read 85 lines from main.go\nEntry point sets up HTTP server on :8080\nUses handler package for route registration".into(), is_error: false, category: ToolCategory::Reading },
            ActivityEntry { timestamp: "14:30:15".into(), tool: "Read".into(), summary: "handler/event.go (lines 1-120)".into(), detail: "Read 120 lines from handler/event.go\nFound handleRequest function at line 42\nIdentified potential nil dereference at line 87".into(), is_error: false, category: ToolCategory::Reading },
            ActivityEntry { timestamp: "14:30:22".into(), tool: "Grep".into(), summary: "\"handleRequest\" across codebase".into(), detail: "handler/event.go:42:  func handleRequest(e *Event) (*Result, error) {\nhandler/event_test.go:23:  result, err := handleRequest(testEvent)\nhandler/queue.go:67:    res, err := handleRequest(evt)".into(), is_error: false, category: ToolCategory::Reading },
            ActivityEntry { timestamp: "14:30:35".into(), tool: "Bash".into(), summary: "go test ./handler/... (FAIL)".into(), detail: "--- FAIL: TestHandleRequest_Nil (0.00s)\npanic: runtime error: invalid memory address or nil pointer dereference\n[signal SIGSEGV: segmentation violation]".into(), is_error: true, category: ToolCategory::Running },
            ActivityEntry { timestamp: "14:30:50".into(), tool: "Edit".into(), summary: "add nil guard in middleware".into(), detail: "handler/middleware.go:31\n+ if req == nil {\n+     http.Error(w, \"nil request\", http.StatusBadRequest)\n+     return\n+ }".into(), is_error: false, category: ToolCategory::Writing },
            ActivityEntry { timestamp: "14:31:12".into(), tool: "Edit".into(), summary: "fix nil pointer in handleRequest".into(), detail: "handler/event.go:80-81\n+ if e == nil {\n+     return nil, fmt.Errorf(\"handleRequest: nil event\")\n+ }".into(), is_error: false, category: ToolCategory::Writing },
            ActivityEntry { timestamp: "14:31:18".into(), tool: "Bash".into(), summary: "go test ./handler/... (PASS)".into(), detail: "ok\tgithub.com/example/my-repo-1/handler\t0.028s".into(), is_error: false, category: ToolCategory::Running },
            ActivityEntry { timestamp: "14:31:42".into(), tool: "Bash".into(), summary: "golangci-lint run ./...".into(), detail: "handler/queue.go:45:12: error return value not checked (errcheck)\n\tconn.Close()\nFound 1 issue(s)".into(), is_error: false, category: ToolCategory::Running },
            ActivityEntry { timestamp: "14:31:48".into(), tool: "Edit".into(), summary: "fix errcheck lint: defer conn.Close()".into(), detail: "handler/queue.go:45\n- conn.Close()\n+ if err := conn.Close(); err != nil {\n+     logger.Warn(\"failed to close conn\", \"err\", err)\n+ }".into(), is_error: false, category: ToolCategory::Writing },
            ActivityEntry { timestamp: "14:31:55".into(), tool: "Bash".into(), summary: "go test -race ./...".into(), detail: "ok\tgithub.com/example/my-repo-1/handler\t0.031s\nok\tgithub.com/example/my-repo-1/queue\t0.045s\nok\tgithub.com/example/my-repo-1/server\t0.022s".into(), is_error: false, category: ToolCategory::Running },
            ActivityEntry { timestamp: "14:32:08".into(), tool: "Bash".into(), summary: "git diff --stat".into(), detail: " handler/event.go      | 5 ++++-\n handler/middleware.go  | 4 ++++\n handler/queue.go      | 4 +++-\n handler/response.go   | 2 +-\n 4 files changed, 12 insertions(+), 3 deletions(-)".into(), is_error: false, category: ToolCategory::Running },
            ActivityEntry { timestamp: "14:33:55".into(), tool: "Bash".into(), summary: "\u{f071} BLOCKED: rm -rf /* (guardrail)".into(), detail: "\u{2718} Command rejected by safety guardrail\n\nAttempted: rm -rf /tmp/build/../../../*\nResolved path: rm -rf /*\n\nReason: path traversal detected \u{2014} resolved target\nis outside allowed working directory.".into(), is_error: true, category: ToolCategory::Running },
            ActivityEntry { timestamp: "14:33:57".into(), tool: "Bash".into(), summary: "rm -rf ./build/output (safe cleanup)".into(), detail: "# removed build artifacts safely\n# 12 files deleted, 3 directories removed".into(), is_error: false, category: ToolCategory::Running },
            ActivityEntry { timestamp: "14:34:42".into(), tool: "Bash".into(), summary: "git commit -m 'fix: nil pointer + integration tests'".into(), detail: "[main a1b2c3d] fix: nil pointer + integration tests\n 7 files changed, 46 insertions(+), 5 deletions(-)".into(), is_error: false, category: ToolCategory::Running },
        ],
        // my-repo-2 (Code): React component refactoring
        vec![
            ActivityEntry { timestamp: "14:28:01".into(), tool: "Read".into(), summary: "src/components/ItemList.tsx".into(), detail: "Read 85 lines from ItemList.tsx\nComponent renders unfiltered items array directly\nNo null checks on item properties".into(), is_error: false, category: ToolCategory::Reading },
            ActivityEntry { timestamp: "14:28:22".into(), tool: "Grep".into(), summary: "\"ItemList\" usage across src/".into(), detail: "src/components/ItemList.tsx:8:  export const ItemList: FC<Props>\nsrc/pages/Dashboard.tsx:5:  import { ItemList } from '../components'\nsrc/pages/Search.tsx:7:  import { ItemList } from '../components'".into(), is_error: false, category: ToolCategory::Reading },
            ActivityEntry { timestamp: "14:28:45".into(), tool: "Edit".into(), summary: "add null-safe filter in useItems hook".into(), detail: "src/hooks/useItems.ts:22\n- return { data: response.data, isLoading, error };\n+ const safeItems = (response.data ?? []).filter(\n+   (item): item is Item => item != null && item.id != null\n+ );\n+ return { data: safeItems, isLoading, error };".into(), is_error: false, category: ToolCategory::Writing },
            ActivityEntry { timestamp: "14:28:52".into(), tool: "Write".into(), summary: "src/components/ItemFilter.tsx (new)".into(), detail: "Created new component ItemFilter (45 lines)\nProps: { categories: string[]; selected: string; onChange }".into(), is_error: false, category: ToolCategory::Writing },
            ActivityEntry { timestamp: "14:29:00".into(), tool: "Edit".into(), summary: "update Dashboard to use ItemFilter".into(), detail: "src/pages/Dashboard.tsx:5\n- import { ItemList } from '../components'\n+ import { ItemList, ItemFilter } from '../components'".into(), is_error: false, category: ToolCategory::Writing },
            ActivityEntry { timestamp: "14:29:15".into(), tool: "Bash".into(), summary: "npx tsc --noEmit".into(), detail: "src/components/ItemFilter.tsx(12,5): error TS2322:\nType 'string | undefined' is not assignable to type 'string'.".into(), is_error: true, category: ToolCategory::Running },
            ActivityEntry { timestamp: "14:29:22".into(), tool: "Edit".into(), summary: "fix type error in ItemFilter onChange".into(), detail: "src/components/ItemFilter.tsx:12\n- onChange={(e) => onChange(e.target.value)}\n+ onChange={(e) => onChange(e.target.value ?? '')}".into(), is_error: false, category: ToolCategory::Writing },
            ActivityEntry { timestamp: "14:29:30".into(), tool: "Bash".into(), summary: "npx tsc --noEmit (OK)".into(), detail: "# no type errors".into(), is_error: false, category: ToolCategory::Running },
            ActivityEntry { timestamp: "14:29:52".into(), tool: "Bash".into(), summary: "npm test -- --watchAll=false (PASS)".into(), detail: "PASS  src/components/ItemList.test.tsx\nPASS  src/hooks/useItems.test.ts\nPASS  src/pages/Dashboard.test.tsx\n\nTest Suites: 3 passed, 3 total\nTests:       5 passed, 5 total".into(), is_error: false, category: ToolCategory::Running },
            ActivityEntry { timestamp: "14:29:58".into(), tool: "Bash".into(), summary: "\u{f071} BLOCKED: rm -rf node_modules/ dist/".into(), detail: "\u{2718} Command rejected by safety guardrail\n\nAttempted: rm -rf node_modules/ dist/ .next/\n\nReason: bulk recursive deletion of multiple\ndirectories requires explicit user approval.".into(), is_error: true, category: ToolCategory::Running },
        ],
        // my-repo-3 (Markdown): documentation overhaul
        vec![
            ActivityEntry { timestamp: "14:25:01".into(), tool: "Read".into(), summary: "README.md".into(), detail: "Read 45 lines from README.md\nTitle: Appointment View Service\nReferences deprecated /v1/appointments endpoint".into(), is_error: false, category: ToolCategory::Reading },
            ActivityEntry { timestamp: "14:25:08".into(), tool: "Read".into(), summary: "docs/api.md".into(), detail: "Read 120 lines from docs/api.md\nDocuments 8 REST endpoints\nAll use /v1/ prefix \u{2014} should be /v2/".into(), is_error: false, category: ToolCategory::Reading },
            ActivityEntry { timestamp: "14:25:22".into(), tool: "Grep".into(), summary: "\"/v1/\" across docs/".into(), detail: "docs/api.md:12:  POST /v1/appointments\ndocs/api.md:28:  GET  /v1/appointments/:id\nREADME.md:18:  curl http://localhost:3000/v1/appointments".into(), is_error: false, category: ToolCategory::Reading },
            ActivityEntry { timestamp: "14:25:30".into(), tool: "Write".into(), summary: "README.md (full rewrite)".into(), detail: "appointment-view/README.md (full rewrite, 62 lines)\n- Updated title and description\n- Fixed badge URLs to new CI\n- Updated endpoint from /v1/ to /v2/".into(), is_error: false, category: ToolCategory::Writing },
            ActivityEntry { timestamp: "14:25:42".into(), tool: "Write".into(), summary: "docs/api.md (update all endpoints)".into(), detail: "docs/api.md (rewrite, 145 lines)\n- Updated all 8 endpoints from /v1/ to /v2/\n- Added rate limiting section".into(), is_error: false, category: ToolCategory::Writing },
            ActivityEntry { timestamp: "14:26:25".into(), tool: "Bash".into(), summary: "npx markdownlint docs/ README.md".into(), detail: "docs/api.md:45 MD009 Trailing spaces\nFound 4 issues in 3 files".into(), is_error: false, category: ToolCategory::Running },
            ActivityEntry { timestamp: "14:26:32".into(), tool: "Edit".into(), summary: "fix markdownlint warnings".into(), detail: "docs/api.md: removed trailing spaces at lines 45, 88\nREADME.md: removed trailing space at line 38".into(), is_error: false, category: ToolCategory::Writing },
            ActivityEntry { timestamp: "14:26:38".into(), tool: "Bash".into(), summary: "npx markdownlint docs/ README.md (PASS)".into(), detail: "# no issues found".into(), is_error: false, category: ToolCategory::Running },
        ],
        // my-repo-4 (Code): Rust bug fix and optimization
        vec![
            ActivityEntry { timestamp: "14:33:01".into(), tool: "Read".into(), summary: "src/lib.rs (lines 1-200)".into(), detail: "Read 200 lines from lib.rs\nPublic API: run(), Config, Pipeline\nrun() declared as Result<(), Error> but line 68 returns String".into(), is_error: false, category: ToolCategory::Reading },
            ActivityEntry { timestamp: "14:33:15".into(), tool: "Grep".into(), summary: "\"process_data\" across src/".into(), detail: "src/lib.rs:112:  fn process_data(&mut self) -> Result<(), Error> {\nsrc/pipeline.rs:34: pub fn process_data(&mut self, input: &[u8]) -> Vec<u8>".into(), is_error: false, category: ToolCategory::Reading },
            ActivityEntry { timestamp: "14:33:30".into(), tool: "Bash".into(), summary: "cargo check".into(), detail: "error[E0308]: mismatched types\n  --> src/lib.rs:68:16\nerror[E0502]: cannot borrow `*self` as mutable\n  --> src/lib.rs:115:9\nerror: aborting due to 2 previous errors".into(), is_error: true, category: ToolCategory::Running },
            ActivityEntry { timestamp: "14:33:38".into(), tool: "Edit".into(), summary: "fix return type: use Box<dyn Error>".into(), detail: "src/lib.rs:12\n- pub fn run(config: Config) -> Result<(), Error> {\n+ pub fn run(config: Config) -> Result<(), Box<dyn std::error::Error>> {".into(), is_error: false, category: ToolCategory::Writing },
            ActivityEntry { timestamp: "14:33:52".into(), tool: "Edit".into(), summary: "fix borrow checker: clone buffer".into(), detail: "src/lib.rs:114-115\n- let data = &self.buffer;\n- self.transform(data);\n+ let data = self.buffer.clone();\n+ self.transform(&data);".into(), is_error: false, category: ToolCategory::Writing },
            ActivityEntry { timestamp: "14:33:58".into(), tool: "Bash".into(), summary: "cargo check (OK)".into(), detail: "    Checking my-repo-4 v0.1.0\n    Finished dev [unoptimized + debuginfo] in 1.42s".into(), is_error: false, category: ToolCategory::Running },
            ActivityEntry { timestamp: "14:34:12".into(), tool: "Bash".into(), summary: "cargo test".into(), detail: "running 8 tests\ntest tests::test_error_propagation ... FAILED\ntest result: FAILED. 7 passed; 1 failed".into(), is_error: true, category: ToolCategory::Running },
            ActivityEntry { timestamp: "14:34:20".into(), tool: "Edit".into(), summary: "fix test assertion for new error type".into(), detail: "tests/unit.rs:48\n- assert!(matches!(result, Err(Error::Config(_))));\n+ assert!(result.is_err());\n+ assert!(result.unwrap_err().to_string().contains(\"invalid config\"));".into(), is_error: false, category: ToolCategory::Writing },
            ActivityEntry { timestamp: "14:34:28".into(), tool: "Bash".into(), summary: "cargo test (PASS)".into(), detail: "running 8 tests\ntest result: ok. 8 passed; 0 failed".into(), is_error: false, category: ToolCategory::Running },
            ActivityEntry { timestamp: "14:34:42".into(), tool: "Edit".into(), summary: "optimize: reuse buffers with double-buffer swap".into(), detail: "src/pipeline.rs:34-40 \u{2014} replaced per-stage allocation with double-buffer swap\n~22% improvement on 1MB benchmark".into(), is_error: false, category: ToolCategory::Writing },
            ActivityEntry { timestamp: "14:35:10".into(), tool: "Bash".into(), summary: "\u{f071} BLOCKED: git push --force origin main".into(), detail: "\u{2718} Command rejected by safety guardrail\n\nAttempted: git push --force origin main\n\nReason: force-pushing to main/master is unconditionally blocked.".into(), is_error: true, category: ToolCategory::Running },
            ActivityEntry { timestamp: "14:35:14".into(), tool: "Bash".into(), summary: "git push origin main".into(), detail: "Enumerating objects: 12, done.\nTo github.com:example/my-repo-4.git\n   b4c5d6e..f7g8h9i  main -> main".into(), is_error: false, category: ToolCategory::Running },
        ],
    ];

    let mut session_index_map = HashMap::new();
    for (i, s) in sessions.iter().enumerate() {
        session_index_map.insert(s.session_id.clone(), i);
    }

    ClaudeWidget {
        sessions,
        activity_logs,
        spinner_frame: 0,
        session_index_map,
    }
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
    demo_claude: Option<ClaudeWidget>,
    claude: Option<ClaudeWidget>,
    modal: Option<ModalState>,
    archive_modal: Option<ArchiveModalState>,
    hovered_session: Option<usize>,
    hovered_archive: bool,
    theme_mode: ThemeMode,
    colors: ThemeColors,
    backdrop: bool,
    target_output: Option<String>,
}

impl Hud {
    fn current_font(&self) -> Font {
        FONT_OPTIONS[self.font_index].1
    }

    fn current_font_label(&self) -> &'static str {
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
    fn active_claude(&self) -> Option<&ClaudeWidget> {
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
enum Message {
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
                    "modal-close" => Some(Message::CloseModal),
                    "claude-live" => Some(Message::ClaudeLiveToggle),
                    "theme dark" => Some(Message::ThemeSet(ThemeMode::Dark)),
                    "theme light" => Some(Message::ThemeSet(ThemeMode::Light)),
                    "theme auto" => Some(Message::ThemeSet(ThemeMode::Auto)),
                    "theme adaptive" => Some(Message::ThemeSet(ThemeMode::Adaptive)),
                    "theme-toggle" => Some(Message::ThemeToggle),
                    "bg-toggle" => Some(Message::BackdropToggle),
                    "archive-show" => Some(Message::OpenArchiveModal),
                    "archive-close" => Some(Message::CloseArchiveModal),
                    "screen" => Some(Message::ScreenCycle),
                    cmd if cmd.starts_with("screen ") => {
                        Some(Message::ScreenSet(cmd[7..].trim().to_string()))
                    }
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

fn theme_refresh_stream() -> impl futures::Stream<Item = Message> {
    let (tx, rx) = mpsc::unbounded();
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_secs(5));
        if tx.unbounded_send(Message::ThemeRefresh).is_err() {
            break;
        }
    });
    rx
}

// --- Watcher subscription bridge ---

fn watcher_stream() -> impl futures::Stream<Item = Message> {
    let (tx, rx) = futures::channel::mpsc::unbounded();
    std::thread::spawn(move || {
        let projects_dir = match dirs::home_dir() {
            Some(h) => h.join(".claude/projects"),
            None => {
                eprintln!("[dev-hud] cannot determine home directory");
                return;
            }
        };
        let handle = match MultiWatcherHandle::spawn(projects_dir) {
            Ok(h) => h,
            Err(e) => {
                eprintln!("[dev-hud] watcher error: {e}");
                return;
            }
        };
        loop {
            for tagged in handle.drain_events() {
                if tx
                    .unbounded_send(Message::WatcherEvent(tagged))
                    .is_err()
                {
                    return;
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    });
    rx
}

// --- Layer Shell Settings ---

fn make_output_option(output: Option<&str>) -> iced_layershell::reexport::OutputOption {
    match output {
        Some(name) => iced_layershell::reexport::OutputOption::OutputName(name.to_string()),
        None => iced_layershell::reexport::OutputOption::None,
    }
}

fn visible_settings(output: Option<&str>) -> NewLayerShellSettings {
    NewLayerShellSettings {
        layer: Layer::Overlay,
        anchor: Anchor::Top | Anchor::Bottom | Anchor::Left | Anchor::Right,
        keyboard_interactivity: KeyboardInteractivity::None,
        exclusive_zone: Some(-1),
        size: Some((0, 0)),
        events_transparent: true,
        output_option: make_output_option(output),
        ..Default::default()
    }
}

fn focused_settings(output: Option<&str>) -> NewLayerShellSettings {
    NewLayerShellSettings {
        layer: Layer::Overlay,
        anchor: Anchor::Top | Anchor::Bottom | Anchor::Left | Anchor::Right,
        keyboard_interactivity: KeyboardInteractivity::OnDemand,
        exclusive_zone: Some(-1),
        size: Some((0, 0)),
        events_transparent: false,
        output_option: make_output_option(output),
        ..Default::default()
    }
}

fn modal_settings(output: Option<&str>) -> NewLayerShellSettings {
    NewLayerShellSettings {
        layer: Layer::Overlay,
        anchor: Anchor::Top | Anchor::Bottom | Anchor::Left | Anchor::Right,
        keyboard_interactivity: KeyboardInteractivity::OnDemand,
        exclusive_zone: Some(-1),
        size: Some((0, 0)),
        margin: Some((50, 50, 50, 50)),
        events_transparent: false,
        output_option: make_output_option(output),
        ..Default::default()
    }
}

/// Strip ANSI escape sequences from a string.
fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            // Skip until we hit a letter (end of escape sequence)
            for esc in chars.by_ref() {
                if esc.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Query available Wayland outputs. Tries cosmic-randr first, then wlr-randr.
fn enumerate_outputs() -> Vec<String> {
    let result = std::process::Command::new("cosmic-randr")
        .arg("list")
        .output()
        .ok()
        .filter(|o| o.status.success())
        .or_else(|| {
            std::process::Command::new("wlr-randr")
                .output()
                .ok()
                .filter(|o| o.status.success())
        });
    let result = match result {
        Some(o) => o,
        None => return Vec::new(),
    };
    let stdout = String::from_utf8_lossy(&result.stdout);
    stdout
        .lines()
        .map(|line| strip_ansi(line))
        .filter(|line| !line.starts_with(' ') && !line.starts_with('\t') && !line.is_empty())
        .filter_map(|line| line.split_whitespace().next().map(String::from))
        .collect()
}

// --- Hud Implementation ---

fn main() -> Result<(), iced_layershell::Error> {
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

    fn view_hud(&self) -> Element<'_, Message> {
        let mono = self.current_font();
        let shaped = Shaping::Advanced;
        let colors = &self.colors;
        let marker = || text("+").size(colors.marker_size).color(colors.marker);

        // Top row: corner markers only
        let top_row = row![marker(), space::horizontal(), marker()];

        // Build bottom row (with optional loader widget)
        let bottom_row = if let Some(loader) = &self.demo_loader {
            let label: Element<'_, Message> = text(format!(" {}", loader.style.label()))
                .size(colors.info_text)
                .color(colors.muted)
                .into();

            let widget: Element<'_, Message> = match loader.style {
                LoaderStyle::Braille | LoaderStyle::Bounce | LoaderStyle::Pipe => {
                    let frames = loader.style.text_frames();
                    let ch = frames[loader.frame % frames.len()];
                    text(format!(" {ch}"))
                        .size(colors.label_text)
                        .color(colors.marker)
                        .font(mono)
                        .shaping(shaped)
                        .into()
                }
                LoaderStyle::Gif => {
                    if loader.gif_frames.is_empty() {
                        text(" ?").size(colors.label_text).color(colors.marker).into()
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
                        text(" ?").size(colors.label_text).color(colors.marker).into()
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
        // Live takes precedence over demo
        if let Some(claude) = self.active_claude() {
            let focused = self.mode == HudMode::Focused;
            let max_chars: usize = if focused { 512 } else { 64 };

            // Collect non-archived session indices (preserves original Vec indices for modal)
            let visible: Vec<usize> = claude
                .sessions
                .iter()
                .enumerate()
                .filter(|(_, s)| !s.archived)
                .map(|(i, _)| i)
                .collect();
            let show_from = visible.len().saturating_sub(MAX_VISIBLE_SESSIONS);
            let display = &visible[show_from..];

            let mut session_col = column![];

            for &i in display {
                let session = &claude.sessions[i];

                // Dim sessions in grace period (exited but not yet archived)
                let in_grace_period = session.exited_at.is_some() && !session.archived;

                let icon_str = if session.exited_at.is_some() {
                    "\u{f04d}" // nf-fa-stop
                } else {
                    match &session.current_tool {
                        Some(tool) if session.active => {
                            let frames = tool_state_frames(tool.category);
                            frames[(claude.spinner_frame / 4) % frames.len()]
                        }
                        _ => {
                            if session.active {
                                claude.spinner_char()
                            } else {
                                session.kind.icon(focused)
                            }
                        }
                    }
                };

                let is_error = session
                    .current_tool
                    .as_ref()
                    .is_some_and(|_| false); // errors are on entries, not on active tool
                let _ = is_error; // reserved for future use

                let is_hovered = focused && self.hovered_session == Some(i);
                let fg = if in_grace_period {
                    colors.muted
                } else if is_hovered {
                    colors.hover_text
                } else {
                    colors.marker
                };
                let dim = if in_grace_period {
                    colors.muted
                } else if is_hovered {
                    colors.hover_text
                } else {
                    colors.muted
                };

                let activity = truncate_str(&session.activity, max_chars);

                let mut srow = row![];

                srow = srow.push(
                    text(format!("{icon_str} "))
                        .size(colors.widget_text)
                        .color(fg)
                        .font(mono)
                        .shaping(shaped),
                );

                // Show project slug prefix in focused mode always,
                // and in non-focus mode for exited sessions (static text won't clip badly)
                if focused || session.exited_at.is_some() {
                    let slug = util::shorten_project(&session.project_slug);
                    srow = srow.push(
                        text(format!("{slug} "))
                            .size(colors.widget_text)
                            .color(dim)
                            .font(mono)
                            .shaping(shaped),
                    );
                }

                srow = srow.push(
                    text(activity)
                        .size(colors.widget_text)
                        .color(fg)
                        .font(mono)
                        .shaping(shaped),
                );

                let session_element: Element<'_, Message> = srow.into();
                if focused {
                    let wrapped: Element<'_, Message> = if is_hovered {
                        container(session_element)
                            .style(colors.hover_style())
                            .into()
                    } else {
                        session_element
                    };
                    session_col = session_col.push(
                        mouse_area(wrapped)
                            .on_press(Message::OpenSessionModal(i))
                            .on_enter(Message::HoverSession(i))
                            .on_exit(Message::UnhoverSession(i))
                            .interaction(mouse::Interaction::Pointer),
                    );
                } else {
                    session_col = session_col.push(session_element);
                }
            }

            // Archive pill: show count of archived sessions
            let archived_count = claude.sessions.iter().filter(|s| s.archived).count();
            if archived_count > 0 && focused {
                let pill_fg = if self.hovered_archive {
                    colors.hover_text
                } else {
                    colors.muted
                };
                let pill_text = text(format!(" Archived ({archived_count})"))
                    .size(colors.widget_text * 0.9)
                    .color(pill_fg)
                    .font(mono)
                    .shaping(shaped);
                let pill_element: Element<'_, Message> = if self.hovered_archive {
                    container(pill_text).style(colors.hover_style()).into()
                } else {
                    pill_text.into()
                };
                session_col = session_col.push(
                    mouse_area(pill_element)
                        .on_press(Message::OpenArchiveModal)
                        .on_enter(Message::HoverArchive)
                        .on_exit(Message::UnhoverArchive)
                        .interaction(mouse::Interaction::Pointer),
                );
            }

            let session_widget: Element<'_, Message> = if self.backdrop {
                container(session_col)
                    .style(colors.hud_backdrop_style())
                    .padding(6)
                    .into()
            } else {
                session_col.into()
            };
            main_col = main_col.push(session_widget);
            main_col = main_col.push(space::Space::new().height(4));
        }

        main_col = main_col.push(bottom_row);

        // Info line: version, commit, font — below the marker rectangle
        let info_size = colors.info_text;
        let info_row = row![
            space::horizontal(),
            text(format!(
                "v{} {} {}",
                env!("DEV_HUD_VERSION"),
                env!("DEV_HUD_COMMIT"),
                self.current_font_label()
            ))
            .size(info_size)
            .color(colors.muted)
            .font(mono)
            .shaping(shaped)
        ];

        let outer = column![
            container(main_col)
                .padding(EDGE_MARGIN)
                .width(Length::Fill)
                .height(Length::Fill),
            container(info_row)
                .padding(iced::Padding {
                    top: 0.0,
                    right: EDGE_MARGIN as f32,
                    bottom: 8.0,
                    left: 0.0,
                })
                .width(Length::Fill),
        ]
        .width(Length::Fill)
        .height(Length::Fill);

        outer.into()
    }

    fn view_modal(&self, modal: &ModalState) -> Element<'_, Message> {
        let mono = self.current_font();
        let shaped = Shaping::Advanced;
        let colors = &self.colors;

        let claude = match self.active_claude() {
            Some(c) => c,
            None => {
                return container(text("No data"))
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .style(colors.modal_bg_style())
                    .into();
            }
        };

        let session = &claude.sessions[modal.session_index];
        let entries = &claude.activity_logs[modal.session_index];

        // Title row
        let title = text(format!(
            "{} {} \u{2014} Activity Log",
            session.kind.icon(true),
            util::shorten_project(&session.project_slug)
        ))
        .size(colors.modal_title)
        .color(colors.marker)
        .font(mono)
        .shaping(shaped);

        let entry_count = text(format!("{} entries", entries.len()))
            .size(colors.modal_text)
            .color(colors.muted)
            .font(mono)
            .shaping(shaped);

        let close_btn = mouse_area(
            text("\u{f00d}")
                .size(colors.modal_title)
                .color(colors.marker)
                .font(mono)
                .shaping(shaped),
        )
        .on_press(Message::CloseModal)
        .interaction(mouse::Interaction::Pointer);

        // Live-mode pulse indicator — reads spinner_frame so view_modal depends on it,
        // causing iced to re-render the modal surface on every Tick and pick up new
        // WatcherEvent entries in real time.
        let live_badge: Element<'_, Message> = if self.claude.is_some() {
            let frames = &["◉", "◎", "○", "◎"];
            let pulse = frames[(claude.spinner_frame / 8) % frames.len()];
            text(format!("  {pulse} live"))
                .size(colors.modal_text)
                .color(colors.hover_text)
                .font(mono)
                .shaping(shaped)
                .into()
        } else {
            space::horizontal().into()
        };

        let title_row = row![title, live_badge, text("  "), entry_count, space::horizontal(), close_btn];

        // UUID subtitle row with copy button
        let uuid_text = text(format!("  {}", session.session_id))
            .size(colors.modal_text * 0.9)
            .color(colors.muted)
            .font(mono)
            .shaping(shaped);

        let copy_btn = mouse_area(
            text("\u{f0c5}") // nf-fa-copy
                .size(colors.modal_text)
                .color(colors.muted)
                .font(mono)
                .shaping(shaped),
        )
        .on_press(Message::CopySessionId(session.session_id.clone()))
        .interaction(mouse::Interaction::Pointer);

        let uuid_row = row![uuid_text, text(" "), copy_btn];

        // --- Left panel: scrollable entry list ---
        let mut entries_col = column![].spacing(2);

        for (i, entry) in entries.iter().enumerate() {
            let is_selected = modal.selected_entry == Some(i);
            let is_hovered = !is_selected && modal.hovered_entry == Some(i);

            // Guardrail-blocked entries carry the \u{f071} warning triangle in their summary.
            // Distinguish them from genuine tool failures so each gets its own accent.
            let is_guardrail = entry.is_error && entry.summary.contains('\u{f071}');
            let is_genuine_error = entry.is_error && !is_guardrail;
            // Last entry while a tool is actively running = awaiting approval / in progress.
            let is_active =
                !entry.is_error && session.current_tool.is_some() && i == entries.len() - 1;

            let accent = if is_genuine_error {
                Some(colors.error)
            } else if is_guardrail || is_active {
                Some(colors.approval)
            } else {
                None
            };

            let fg = match (accent, is_hovered) {
                (Some(c), _) => c,
                (None, true) => colors.hover_text,
                (None, false) => colors.marker,
            };
            let dim = match (accent, is_hovered) {
                (Some(c), _) => c,
                (None, true) => colors.hover_text,
                (None, false) => colors.muted,
            };

            // Genuine errors get ✘ prefix; guardrail blocks already carry \u{f071} in summary;
            // active/in-progress entries get a subtle ⋯ indicator.
            let icon_prefix = if is_genuine_error {
                "✘ "
            } else if is_active {
                "⋯ "
            } else {
                ""
            };

            let entry_row = row![
                text(format!("{} ", entry.timestamp))
                    .size(colors.modal_text)
                    .color(dim)
                    .font(mono)
                    .shaping(shaped),
                text(format!("{icon_prefix}{:<5} ", entry.tool))
                    .size(colors.modal_text)
                    .color(fg)
                    .font(mono)
                    .shaping(shaped),
                text(truncate_str(&entry.summary, 48))
                    .size(colors.modal_text)
                    .color(if is_selected { colors.marker } else { dim })
                    .font(mono)
                    .shaping(shaped),
            ];

            let entry_element: Element<'_, Message> = if is_selected {
                container(entry_row)
                    .style(colors.selected_style())
                    .padding(iced::Padding::ZERO.top(2).bottom(2))
                    .into()
            } else if is_hovered {
                container(entry_row)
                    .style(colors.hover_style())
                    .padding(iced::Padding::ZERO.top(2).bottom(2))
                    .into()
            } else {
                container(entry_row)
                    .padding(iced::Padding::ZERO.top(2).bottom(2))
                    .into()
            };

            entries_col = entries_col.push(
                mouse_area(entry_element)
                    .on_press(Message::SelectActivity(i))
                    .on_enter(Message::HoverEntry(i))
                    .on_exit(Message::UnhoverEntry(i))
                    .interaction(mouse::Interaction::Pointer),
            );
        }

        let left_panel = scrollable(entries_col)
            .width(Length::FillPortion(2))
            .height(Length::Fill);

        // --- Right panel: detail view ---
        let right_panel: Element<'_, Message> = if let Some(idx) = modal.selected_entry {
            let entry = &entries[idx];

            let detail_is_guardrail =
                entry.is_error && entry.summary.contains('\u{f071}');
            let detail_accent = if entry.is_error && !detail_is_guardrail {
                colors.error
            } else if detail_is_guardrail {
                colors.approval
            } else {
                colors.marker
            };

            let header = row![
                text(&entry.tool)
                    .size(colors.modal_title)
                    .color(detail_accent)
                    .font(mono)
                    .shaping(shaped),
                text(format!("  {}", entry.timestamp))
                    .size(colors.modal_text)
                    .color(colors.muted)
                    .font(mono)
                    .shaping(shaped),
            ];

            let summary = text(&entry.summary)
                .size(colors.modal_text)
                .color(detail_accent)
                .font(mono)
                .shaping(shaped);

            let separator = text("\u{2500}".repeat(40))
                .size(colors.modal_text * 0.8)
                .color(colors.muted)
                .font(mono)
                .shaping(shaped);

            let detail = text(&entry.detail)
                .size(colors.modal_text)
                .color(colors.muted)
                .font(mono)
                .shaping(shaped);

            let detail_col = column![header, summary, separator, detail].spacing(8);

            container(
                scrollable(detail_col)
                    .width(Length::Fill)
                    .height(Length::Fill),
            )
            .padding(16)
            .width(Length::FillPortion(3))
            .height(Length::Fill)
            .style(colors.detail_bg_style())
            .into()
        } else {
            container(
                text("Select an entry to view details")
                    .size(colors.modal_text)
                    .color(colors.muted)
                    .font(mono)
                    .shaping(shaped),
            )
            .center_x(Length::FillPortion(3))
            .center_y(Length::Fill)
            .style(colors.detail_bg_style())
            .into()
        };

        // --- Compose layout ---
        let body = row![left_panel, right_panel]
            .spacing(12)
            .width(Length::Fill)
            .height(Length::Fill);

        let content = column![title_row, uuid_row, body]
            .spacing(12)
            .width(Length::Fill)
            .height(Length::Fill);

        container(content)
            .padding(24)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(colors.modal_bg_style())
            .into()
    }

    fn view_archive_modal(&self, archive: &ArchiveModalState) -> Element<'_, Message> {
        let mono = self.current_font();
        let shaped = Shaping::Advanced;
        let colors = &self.colors;

        let claude = match self.active_claude() {
            Some(c) => c,
            None => {
                return container(text("No data"))
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .style(colors.modal_bg_style())
                    .into();
            }
        };

        // Collect archived session indices
        let archived_indices: Vec<usize> = claude
            .sessions
            .iter()
            .enumerate()
            .filter(|(_, s)| s.archived)
            .map(|(i, _)| i)
            .collect();

        // Title row
        let title = text(format!(
            "\u{f187} Archived Sessions ({} total)",
            archived_indices.len()
        ))
        .size(colors.modal_title)
        .color(colors.marker)
        .font(mono)
        .shaping(shaped);

        let close_btn = mouse_area(
            text("\u{f00d}")
                .size(colors.modal_title)
                .color(colors.marker)
                .font(mono)
                .shaping(shaped),
        )
        .on_press(Message::CloseArchiveModal)
        .interaction(mouse::Interaction::Pointer);

        let title_row = row![title, space::horizontal(), close_btn];

        // --- Left column: list of archived sessions ---
        let mut sessions_col = column![].spacing(4);

        for (list_idx, &session_idx) in archived_indices.iter().enumerate() {
            let session = &claude.sessions[session_idx];
            let is_selected = archive.selected_session == Some(list_idx);
            let is_hovered = !is_selected && archive.hovered_session == Some(list_idx);

            let fg = if is_selected {
                colors.marker
            } else if is_hovered {
                colors.hover_text
            } else {
                colors.muted
            };

            let slug = util::shorten_project(&session.project_slug);
            let id_snippet = if session.session_id.len() > 8 {
                &session.session_id[..8]
            } else {
                &session.session_id
            };

            let exit_label = if let Some(exited_at) = session.exited_at {
                match exited_at.duration_since(std::time::UNIX_EPOCH) {
                    Ok(d) => {
                        let total_secs = d.as_secs();
                        let hours = (total_secs / 3600) % 24;
                        let minutes = (total_secs / 60) % 60;
                        format!("exited {hours:02}:{minutes:02}")
                    }
                    Err(_) => "exited".to_string(),
                }
            } else {
                "archived".to_string()
            };

            let session_row = row![
                text(format!("{slug} "))
                    .size(colors.modal_text)
                    .color(fg)
                    .font(mono)
                    .shaping(shaped),
                text(format!("{id_snippet}.. "))
                    .size(colors.modal_text * 0.85)
                    .color(colors.muted)
                    .font(mono)
                    .shaping(shaped),
                text(exit_label)
                    .size(colors.modal_text * 0.85)
                    .color(colors.muted)
                    .font(mono)
                    .shaping(shaped),
            ];

            let session_element: Element<'_, Message> = if is_selected {
                container(session_row)
                    .style(colors.selected_style())
                    .padding(iced::Padding::ZERO.top(2).bottom(2))
                    .into()
            } else if is_hovered {
                container(session_row)
                    .style(colors.hover_style())
                    .padding(iced::Padding::ZERO.top(2).bottom(2))
                    .into()
            } else {
                container(session_row)
                    .padding(iced::Padding::ZERO.top(2).bottom(2))
                    .into()
            };

            sessions_col = sessions_col.push(
                mouse_area(session_element)
                    .on_press(Message::SelectArchivedSession(list_idx))
                    .on_enter(Message::HoverArchivedSession(list_idx))
                    .on_exit(Message::UnhoverArchivedSession(list_idx))
                    .interaction(mouse::Interaction::Pointer),
            );
        }

        let left_panel = scrollable(sessions_col)
            .width(Length::FillPortion(1))
            .height(Length::Fill);

        // --- Middle column: activity log for selected archived session ---
        let (middle_panel, right_panel): (Element<'_, Message>, Element<'_, Message>) =
            if let Some(list_idx) = archive.selected_session {
                if let Some(&session_idx) = archived_indices.get(list_idx) {
                    let entries = &claude.activity_logs[session_idx];

                    let mut entries_col = column![].spacing(2);
                    for (i, entry) in entries.iter().enumerate() {
                        let is_selected = archive.selected_entry == Some(i);
                        let is_hovered = !is_selected && archive.hovered_entry == Some(i);

                        let is_guardrail = entry.is_error && entry.summary.contains('\u{f071}');
                        let is_genuine_error = entry.is_error && !is_guardrail;

                        let accent = if is_genuine_error {
                            Some(colors.error)
                        } else if is_guardrail {
                            Some(colors.approval)
                        } else {
                            None
                        };

                        let fg = match (accent, is_hovered) {
                            (Some(c), _) => c,
                            (None, true) => colors.hover_text,
                            (None, false) => colors.marker,
                        };
                        let dim = match (accent, is_hovered) {
                            (Some(c), _) => c,
                            (None, true) => colors.hover_text,
                            (None, false) => colors.muted,
                        };

                        let icon_prefix = if is_genuine_error { "✘ " } else { "" };

                        let entry_row = row![
                            text(format!("{} ", entry.timestamp))
                                .size(colors.modal_text)
                                .color(dim)
                                .font(mono)
                                .shaping(shaped),
                            text(format!("{icon_prefix}{:<5} ", entry.tool))
                                .size(colors.modal_text)
                                .color(fg)
                                .font(mono)
                                .shaping(shaped),
                            text(truncate_str(&entry.summary, 48))
                                .size(colors.modal_text)
                                .color(if is_selected { colors.marker } else { dim })
                                .font(mono)
                                .shaping(shaped),
                        ];

                        let entry_element: Element<'_, Message> = if is_selected {
                            container(entry_row)
                                .style(colors.selected_style())
                                .padding(iced::Padding::ZERO.top(2).bottom(2))
                                .into()
                        } else if is_hovered {
                            container(entry_row)
                                .style(colors.hover_style())
                                .padding(iced::Padding::ZERO.top(2).bottom(2))
                                .into()
                        } else {
                            container(entry_row)
                                .padding(iced::Padding::ZERO.top(2).bottom(2))
                                .into()
                        };

                        entries_col = entries_col.push(
                            mouse_area(entry_element)
                                .on_press(Message::SelectArchivedEntry(i))
                                .on_enter(Message::HoverArchivedEntry(i))
                                .on_exit(Message::UnhoverArchivedEntry(i))
                                .interaction(mouse::Interaction::Pointer),
                        );
                    }

                    let mid: Element<'_, Message> = scrollable(entries_col)
                        .width(Length::FillPortion(2))
                        .height(Length::Fill)
                        .into();

                    // Right panel: detail for selected entry
                    let right: Element<'_, Message> =
                        if let Some(entry_idx) = archive.selected_entry {
                            if let Some(entry) = entries.get(entry_idx) {
                                let detail_is_guardrail =
                                    entry.is_error && entry.summary.contains('\u{f071}');
                                let detail_accent = if entry.is_error && !detail_is_guardrail {
                                    colors.error
                                } else if detail_is_guardrail {
                                    colors.approval
                                } else {
                                    colors.marker
                                };

                                let header = row![
                                    text(&entry.tool)
                                        .size(colors.modal_title)
                                        .color(detail_accent)
                                        .font(mono)
                                        .shaping(shaped),
                                    text(format!("  {}", entry.timestamp))
                                        .size(colors.modal_text)
                                        .color(colors.muted)
                                        .font(mono)
                                        .shaping(shaped),
                                ];

                                let summary = text(&entry.summary)
                                    .size(colors.modal_text)
                                    .color(detail_accent)
                                    .font(mono)
                                    .shaping(shaped);

                                let separator = text("\u{2500}".repeat(40))
                                    .size(colors.modal_text * 0.8)
                                    .color(colors.muted)
                                    .font(mono)
                                    .shaping(shaped);

                                let detail = text(&entry.detail)
                                    .size(colors.modal_text)
                                    .color(colors.muted)
                                    .font(mono)
                                    .shaping(shaped);

                                let detail_col =
                                    column![header, summary, separator, detail].spacing(8);

                                container(
                                    scrollable(detail_col)
                                        .width(Length::Fill)
                                        .height(Length::Fill),
                                )
                                .padding(16)
                                .width(Length::FillPortion(2))
                                .height(Length::Fill)
                                .style(colors.detail_bg_style())
                                .into()
                            } else {
                                container(
                                    text("Select an entry to view details")
                                        .size(colors.modal_text)
                                        .color(colors.muted)
                                        .font(mono)
                                        .shaping(shaped),
                                )
                                .center_x(Length::FillPortion(2))
                                .center_y(Length::Fill)
                                .style(colors.detail_bg_style())
                                .into()
                            }
                        } else {
                            container(
                                text("Select an entry to view details")
                                    .size(colors.modal_text)
                                    .color(colors.muted)
                                    .font(mono)
                                    .shaping(shaped),
                            )
                            .center_x(Length::FillPortion(2))
                            .center_y(Length::Fill)
                            .style(colors.detail_bg_style())
                            .into()
                        };

                    (mid, right)
                } else {
                    let mid: Element<'_, Message> = container(
                        text("Session not found")
                            .size(colors.modal_text)
                            .color(colors.muted)
                            .font(mono)
                            .shaping(shaped),
                    )
                    .center_x(Length::FillPortion(2))
                    .center_y(Length::Fill)
                    .into();
                    let right: Element<'_, Message> = container(space::horizontal())
                        .width(Length::FillPortion(2))
                        .height(Length::Fill)
                        .style(colors.detail_bg_style())
                        .into();
                    (mid, right)
                }
            } else {
                let mid: Element<'_, Message> = container(
                    text("Select an archived session")
                        .size(colors.modal_text)
                        .color(colors.muted)
                        .font(mono)
                        .shaping(shaped),
                )
                .center_x(Length::FillPortion(2))
                .center_y(Length::Fill)
                .into();
                let right: Element<'_, Message> = container(space::horizontal())
                    .width(Length::FillPortion(2))
                    .height(Length::Fill)
                    .style(colors.detail_bg_style())
                    .into();
                (mid, right)
            };

        // --- Compose layout ---
        let body = row![left_panel, middle_panel, right_panel]
            .spacing(12)
            .width(Length::Fill)
            .height(Length::Fill);

        let content = column![title_row, body]
            .spacing(12)
            .width(Length::Fill)
            .height(Length::Fill);

        container(content)
            .padding(24)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(colors.modal_bg_style())
            .into()
    }

    fn subscription(state: &Self) -> Subscription<Message> {
        let socket = Subscription::run(socket_listener);
        let needs_tick = (state.demo_loader.is_some()
            || state.demo_claude.is_some()
            || state.claude.is_some())
            && state.mode != HudMode::Hidden;

        let mut subs = vec![socket];

        if needs_tick {
            subs.push(Subscription::run_with(TICK_MS, tick_stream));
        }

        if state.claude.is_some() {
            subs.push(Subscription::run(watcher_stream));
        }

        // Theme refresh for auto/adaptive modes (5s interval)
        if matches!(state.theme_mode, ThemeMode::Auto | ThemeMode::Adaptive) {
            subs.push(Subscription::run(theme_refresh_stream));
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
