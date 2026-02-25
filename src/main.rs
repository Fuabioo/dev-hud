mod events;
mod util;
mod watcher;

use std::collections::HashMap;
use std::io::BufRead;
use std::os::unix::net::UnixListener;
use std::path::PathBuf;
use std::time::Duration;

use events::{SessionEvent, TaggedEvent, ToolCategory};
use util::truncate_str;
use watcher::MultiWatcherHandle;

use futures::channel::mpsc;
use iced::widget::text::Shaping;
use iced::widget::{
    column, container, image as iced_image, mouse_area, row, scrollable, space, svg, text,
};
use iced::{mouse, Background, Color, Element, Font, Length, Subscription, Task};
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
const MODAL_BG_COLOR: Color = Color {
    r: 0.05,
    g: 0.05,
    b: 0.08,
    a: 0.92,
};
const DETAIL_BG_COLOR: Color = Color {
    r: 0.08,
    g: 0.08,
    b: 0.12,
    a: 0.6,
};
const SELECTED_COLOR: Color = Color {
    r: 0.15,
    g: 0.15,
    b: 0.22,
    a: 0.8,
};
const HOVER_COLOR: Color = Color {
    r: 0.12,
    g: 0.12,
    b: 0.18,
    a: 0.6,
};
const HOVER_TEXT_COLOR: Color = Color {
    r: 1.0,
    g: 0.78,
    b: 0.0,
    a: 1.0,
};
const ERROR_COLOR: Color = Color {
    r: 0.9,
    g: 0.2,
    b: 0.2,
    a: 1.0,
};

const TICK_MS: u64 = 80;
const LOADER_TEXT_SIZE: f32 = MARKER_SIZE * 0.5;
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

const CLAUDE_TEXT_SIZE: f32 = MARKER_SIZE * 0.45;
const MAX_VISIBLE_SESSIONS: usize = 6;

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
    }

    fn spinner_char(&self) -> &'static str {
        let frames = LoaderStyle::Braille.text_frames();
        frames[self.spinner_frame % frames.len()]
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
                });
                self.activity_logs.push(Vec::new());
                self.session_index_map.insert(session_id, idx);
            }
            SessionEvent::UserPrompt { text } => {
                if let Some(&idx) = self.session_index_map.get(&session_id) {
                    let session = &mut self.sessions[idx];
                    session.active = true;
                    session.activity = truncate_str(&text, 200);
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
        },
        Session {
            session_id: "demo-0000-0000-0000-000000000003".to_string(),
            project_slug: "my-repo-3".to_string(),
            active: false,
            kind: SessionKind::Markdown,
            current_tool: None,
            activity: "Write(../appointment-view/README.md)".to_string(),
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
            ActivityEntry { timestamp: "14:33:55".into(), tool: "Bash".into(), summary: "\u{f071} BLOCKED: rm -rf /* (guardrail)".into(), detail: "\u{2718} Command rejected by safety guardrail\n\nAttempted: rm -rf /tmp/build/../../../*\nResolved path: rm -rf /*\n\nReason: path traversal detected — resolved target\nis outside allowed working directory.".into(), is_error: true, category: ToolCategory::Running },
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
            ActivityEntry { timestamp: "14:25:08".into(), tool: "Read".into(), summary: "docs/api.md".into(), detail: "Read 120 lines from docs/api.md\nDocuments 8 REST endpoints\nAll use /v1/ prefix — should be /v2/".into(), is_error: false, category: ToolCategory::Reading },
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
            ActivityEntry { timestamp: "14:34:42".into(), tool: "Edit".into(), summary: "optimize: reuse buffers with double-buffer swap".into(), detail: "src/pipeline.rs:34-40 — replaced per-stage allocation with double-buffer swap\n~22% improvement on 1MB benchmark".into(), is_error: false, category: ToolCategory::Writing },
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

// --- Container style helpers ---

fn modal_bg_style(_theme: &iced::Theme) -> iced::widget::container::Style {
    iced::widget::container::Style {
        background: Some(Background::Color(MODAL_BG_COLOR)),
        ..Default::default()
    }
}

fn detail_bg_style(_theme: &iced::Theme) -> iced::widget::container::Style {
    iced::widget::container::Style {
        background: Some(Background::Color(DETAIL_BG_COLOR)),
        ..Default::default()
    }
}

fn selected_entry_style(_theme: &iced::Theme) -> iced::widget::container::Style {
    iced::widget::container::Style {
        background: Some(Background::Color(SELECTED_COLOR)),
        ..Default::default()
    }
}

fn hover_style(_theme: &iced::Theme) -> iced::widget::container::Style {
    iced::widget::container::Style {
        background: Some(Background::Color(HOVER_COLOR)),
        ..Default::default()
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
    hovered_session: Option<usize>,
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

    /// Returns the active claude widget: live takes precedence over demo.
    fn active_claude(&self) -> Option<&ClaudeWidget> {
        self.claude.as_ref().or(self.demo_claude.as_ref())
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

fn modal_settings() -> NewLayerShellSettings {
    NewLayerShellSettings {
        layer: Layer::Overlay,
        anchor: Anchor::Top | Anchor::Bottom | Anchor::Left | Anchor::Right,
        keyboard_interactivity: KeyboardInteractivity::OnDemand,
        exclusive_zone: Some(-1),
        size: Some((0, 0)),
        margin: Some((50, 50, 50, 50)),
        events_transparent: false,
        ..Default::default()
    }
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
        let (id, task) = Message::layershell_open(visible_settings());
        eprintln!("[dev-hud] booting -> Visible (surface {id})");
        (
            Self {
                mode: HudMode::Visible,
                surface_id: Some(id),
                font_index: 0,
                demo_loader: None,
                demo_claude: None,
                claude: None,
                modal: None,
                hovered_session: None,
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
                    let modal_task = self.close_modal_task();
                    let task = if let Some(id) = self.surface_id.take() {
                        Task::done(Message::RemoveWindow(id))
                    } else {
                        Task::none()
                    };
                    self.mode = HudMode::Hidden;
                    eprintln!("[dev-hud] {mode:?} -> Hidden");
                    Task::batch([modal_task, task])
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
                    let modal_task = self.close_modal_task();
                    let remove_task = if let Some(id) = self.surface_id.take() {
                        Task::done(Message::RemoveWindow(id))
                    } else {
                        Task::none()
                    };
                    let (id, open_task) = Message::layershell_open(visible_settings());
                    self.surface_id = Some(id);
                    self.mode = HudMode::Visible;
                    eprintln!("[dev-hud] Focused -> Visible");
                    Task::batch([modal_task, remove_task, open_task])
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
                    self.demo_claude = None;
                    eprintln!("[dev-hud] demo claude: off");
                    modal_task
                } else {
                    self.demo_claude = Some(create_demo_widget());
                    Task::none()
                }
            }
            Message::ClaudeLiveToggle => {
                if self.claude.is_some() {
                    let modal_task = self.close_modal_task();
                    self.claude = None;
                    eprintln!("[dev-hud] claude live: off");
                    modal_task
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
                self.hovered_session = None;
                let (id, task) = Message::layershell_open(modal_settings());
                self.modal = Some(ModalState {
                    surface_id: id,
                    session_index: idx,
                    selected_entry: None,
                    hovered_entry: None,
                });
                eprintln!("[dev-hud] modal opened for session {idx}");
                task
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
            _ => Task::none(),
        }
    }

    fn view(&self, window_id: IcedId) -> Element<'_, Message> {
        if let Some(ref modal) = self.modal {
            if window_id == modal.surface_id {
                return self.view_modal(modal);
            }
        }
        self.view_hud()
    }

    fn view_hud(&self) -> Element<'_, Message> {
        let mono = self.current_font();
        let shaped = Shaping::Advanced;
        let marker = || text("+").size(MARKER_SIZE).color(MARKER_COLOR);

        // Top row: corner markers only
        let top_row = row![marker(), space::horizontal(), marker()];

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
        // Live takes precedence over demo
        if let Some(claude) = self.active_claude() {
            let focused = self.mode == HudMode::Focused;
            let max_chars: usize = if focused { 512 } else { 64 };

            // Show only the last MAX_VISIBLE_SESSIONS, preserving original indices
            let total = claude.sessions.len();
            let skip = total.saturating_sub(MAX_VISIBLE_SESSIONS);

            for (i, session) in claude.sessions.iter().enumerate().skip(skip) {
                let icon_str = match &session.current_tool {
                    Some(tool) if session.active => {
                        let frames = tool_state_frames(tool.category);
                        frames[claude.spinner_frame % frames.len()]
                    }
                    _ => {
                        if session.active {
                            claude.spinner_char()
                        } else {
                            session.kind.icon(focused)
                        }
                    }
                };

                let is_error = session
                    .current_tool
                    .as_ref()
                    .is_some_and(|_| false); // errors are on entries, not on active tool
                let _ = is_error; // reserved for future use

                let is_hovered = focused && self.hovered_session == Some(i);
                let fg = if is_hovered {
                    HOVER_TEXT_COLOR
                } else {
                    MARKER_COLOR
                };
                let dim = if is_hovered {
                    HOVER_TEXT_COLOR
                } else {
                    MUTED_COLOR
                };

                let activity = truncate_str(&session.activity, max_chars);

                let mut srow = row![];

                srow = srow.push(
                    text(format!("{icon_str} "))
                        .size(CLAUDE_TEXT_SIZE)
                        .color(fg)
                        .font(mono)
                        .shaping(shaped),
                );

                if focused {
                    let slug = util::shorten_project(&session.project_slug);
                    srow = srow.push(
                        text(format!("{slug} "))
                            .size(CLAUDE_TEXT_SIZE)
                            .color(dim)
                            .font(mono)
                            .shaping(shaped),
                    );
                }

                srow = srow.push(
                    text(activity)
                        .size(CLAUDE_TEXT_SIZE)
                        .color(fg)
                        .font(mono)
                        .shaping(shaped),
                );

                let session_element: Element<'_, Message> = srow.into();
                if focused {
                    let wrapped: Element<'_, Message> = if is_hovered {
                        container(session_element).style(hover_style).into()
                    } else {
                        session_element
                    };
                    main_col = main_col.push(
                        mouse_area(wrapped)
                            .on_press(Message::OpenSessionModal(i))
                            .on_enter(Message::HoverSession(i))
                            .on_exit(Message::UnhoverSession(i))
                            .interaction(mouse::Interaction::Pointer),
                    );
                } else {
                    main_col = main_col.push(session_element);
                }
            }

            main_col = main_col.push(space::Space::new().height(4));
        }

        main_col = main_col.push(bottom_row);

        // Info line: version, commit, font — below the marker rectangle
        let info_size = LOADER_TEXT_SIZE * 0.6;
        let info_row = row![
            space::horizontal(),
            text(format!(
                "v{} {} {}",
                env!("DEV_HUD_VERSION"),
                env!("DEV_HUD_COMMIT"),
                self.current_font_label()
            ))
            .size(info_size)
            .color(MUTED_COLOR)
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

        let claude = match self.active_claude() {
            Some(c) => c,
            None => {
                return container(text("No data"))
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .style(modal_bg_style)
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
        .size(MARKER_SIZE * 0.7)
        .color(MARKER_COLOR)
        .font(mono)
        .shaping(shaped);

        let entry_count = text(format!("{} entries", entries.len()))
            .size(CLAUDE_TEXT_SIZE)
            .color(MUTED_COLOR)
            .font(mono)
            .shaping(shaped);

        let close_btn = mouse_area(
            text("\u{f00d}")
                .size(MARKER_SIZE * 0.7)
                .color(MARKER_COLOR)
                .font(mono)
                .shaping(shaped),
        )
        .on_press(Message::CloseModal)
        .interaction(mouse::Interaction::Pointer);

        let title_row = row![title, text("  "), entry_count, space::horizontal(), close_btn];

        // UUID subtitle row with copy button
        let uuid_text = text(format!("  {}", session.session_id))
            .size(CLAUDE_TEXT_SIZE * 0.9)
            .color(MUTED_COLOR)
            .font(mono)
            .shaping(shaped);

        let copy_btn = mouse_area(
            text("\u{f0c5}") // nf-fa-copy
                .size(CLAUDE_TEXT_SIZE)
                .color(MUTED_COLOR)
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

            let fg = if entry.is_error {
                ERROR_COLOR
            } else if is_hovered {
                HOVER_TEXT_COLOR
            } else {
                MARKER_COLOR
            };
            let dim = if entry.is_error {
                ERROR_COLOR
            } else if is_hovered {
                HOVER_TEXT_COLOR
            } else {
                MUTED_COLOR
            };

            // Error entries get static ✘ icon prefix
            let icon_prefix = if entry.is_error { "✘ " } else { "" };

            let entry_row = row![
                text(format!("{} ", entry.timestamp))
                    .size(CLAUDE_TEXT_SIZE)
                    .color(dim)
                    .font(mono)
                    .shaping(shaped),
                text(format!("{icon_prefix}{:<5} ", entry.tool))
                    .size(CLAUDE_TEXT_SIZE)
                    .color(fg)
                    .font(mono)
                    .shaping(shaped),
                text(truncate_str(&entry.summary, 48))
                    .size(CLAUDE_TEXT_SIZE)
                    .color(if is_selected { MARKER_COLOR } else { dim })
                    .font(mono)
                    .shaping(shaped),
            ];

            let entry_element: Element<'_, Message> = if is_selected {
                container(entry_row)
                    .style(selected_entry_style)
                    .padding(iced::Padding::ZERO.top(2).bottom(2))
                    .into()
            } else if is_hovered {
                container(entry_row)
                    .style(hover_style)
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

            let header = row![
                text(&entry.tool)
                    .size(MARKER_SIZE * 0.6)
                    .color(if entry.is_error {
                        ERROR_COLOR
                    } else {
                        MARKER_COLOR
                    })
                    .font(mono)
                    .shaping(shaped),
                text(format!("  {}", entry.timestamp))
                    .size(CLAUDE_TEXT_SIZE)
                    .color(MUTED_COLOR)
                    .font(mono)
                    .shaping(shaped),
            ];

            let summary = text(&entry.summary)
                .size(CLAUDE_TEXT_SIZE)
                .color(if entry.is_error {
                    ERROR_COLOR
                } else {
                    MARKER_COLOR
                })
                .font(mono)
                .shaping(shaped);

            let separator = text("\u{2500}".repeat(40))
                .size(CLAUDE_TEXT_SIZE * 0.8)
                .color(MUTED_COLOR)
                .font(mono)
                .shaping(shaped);

            let detail = text(&entry.detail)
                .size(CLAUDE_TEXT_SIZE)
                .color(MUTED_COLOR)
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
            .style(detail_bg_style)
            .into()
        } else {
            container(
                text("Select an entry to view details")
                    .size(CLAUDE_TEXT_SIZE)
                    .color(MUTED_COLOR)
                    .font(mono)
                    .shaping(shaped),
            )
            .center_x(Length::FillPortion(3))
            .center_y(Length::Fill)
            .style(detail_bg_style)
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
            .style(modal_bg_style)
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

        Subscription::batch(subs)
    }

    fn style(&self, _theme: &iced::Theme) -> iced::theme::Style {
        iced::theme::Style {
            background_color: Color::TRANSPARENT,
            text_color: MARKER_COLOR,
        }
    }
}
