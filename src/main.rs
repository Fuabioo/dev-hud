use std::io::BufRead;
use std::os::unix::net::UnixListener;
use std::path::PathBuf;
use std::time::Duration;

use futures::channel::mpsc;
use iced::widget::{
    column, container, image as iced_image, mouse_area, row, scrollable, space, svg, text,
};
use iced::widget::text::Shaping;
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

struct DemoActivityEntry {
    timestamp: &'static str,
    tool: &'static str,
    summary: &'static str,
    detail: &'static str,
}

struct ModalState {
    surface_id: IcedId,
    session_index: usize,
    selected_entry: Option<usize>,
    hovered_entry: Option<usize>,
}

struct DemoClaudeWidget {
    sessions: Vec<DemoSession>,
    activity_logs: Vec<Vec<DemoActivityEntry>>,
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
            activity_logs: vec![
                // my-repo-1 (Terminal): debugging a Go service — 20 entries
                vec![
                    DemoActivityEntry {
                        timestamp: "14:30:01",
                        tool: "Bash",
                        summary: "git log --oneline | head -20",
                        detail: "a1b2c3d fix: handle nil pointer in event handler\n\
                                 e4f5g6h feat: add retry logic for API calls\n\
                                 i7j8k9l refactor: extract validation into separate fn\n\
                                 m0n1o2p docs: update README with new env vars\n\
                                 q3r4s5t fix: race condition in worker pool\n\
                                 u6v7w8x feat: add health check endpoint\n\
                                 y9z0a1b chore: bump dependencies\n\
                                 c2d3e4f fix: incorrect error wrapping in middleware\n\
                                 g5h6i7j test: add integration tests for auth flow\n\
                                 k8l9m0n feat: implement graceful shutdown\n\
                                 ...(10 more lines)",
                    },
                    DemoActivityEntry {
                        timestamp: "14:30:08",
                        tool: "Read",
                        summary: "main.go (lines 1-85)",
                        detail: "Read 85 lines from main.go\n\
                                 Entry point sets up HTTP server on :8080\n\
                                 Uses handler package for route registration\n\
                                 Graceful shutdown via signal.NotifyContext",
                    },
                    DemoActivityEntry {
                        timestamp: "14:30:15",
                        tool: "Read",
                        summary: "handler/event.go (lines 1-120)",
                        detail: "Read 120 lines from handler/event.go\n\
                                 Found handleRequest function at line 42\n\
                                 Identified potential nil dereference at line 87\n\
                                 Function receives *Event which may be nil from queue",
                    },
                    DemoActivityEntry {
                        timestamp: "14:30:22",
                        tool: "Grep",
                        summary: "\"handleRequest\" across codebase",
                        detail: "handler/event.go:42:  func handleRequest(e *Event) (*Result, error) {\n\
                                 handler/event.go:98:  // handleRequest is called from processQueue\n\
                                 handler/event_test.go:23:  result, err := handleRequest(testEvent)\n\
                                 handler/event_test.go:55:  result, err := handleRequest(nil)\n\
                                 handler/queue.go:67:    res, err := handleRequest(evt)",
                    },
                    DemoActivityEntry {
                        timestamp: "14:30:28",
                        tool: "Read",
                        summary: "handler/event_test.go",
                        detail: "Read 89 lines from handler/event_test.go\n\
                                 TestHandleRequest_Valid passes correctly\n\
                                 TestHandleRequest_Nil tests nil input — currently panics\n\
                                 Missing test for empty Event struct",
                    },
                    DemoActivityEntry {
                        timestamp: "14:30:35",
                        tool: "Bash",
                        summary: "go test ./handler/... (FAIL)",
                        detail: "--- FAIL: TestHandleRequest_Nil (0.00s)\n\
                                 panic: runtime error: invalid memory address or nil pointer dereference\n\
                                 [signal SIGSEGV: segmentation violation]\n\n\
                                 goroutine 7 [running]:\n\
                                 github.com/example/my-repo-1/handler.handleRequest(0x0)\n\
                                 \thandler/event.go:87 +0x1a\n\
                                 FAIL\tgithub.com/example/my-repo-1/handler\t0.034s",
                    },
                    DemoActivityEntry {
                        timestamp: "14:30:42",
                        tool: "Read",
                        summary: "handler/middleware.go",
                        detail: "Read 74 lines from handler/middleware.go\n\
                                 LoggingMiddleware wraps handlers with request logging\n\
                                 RecoveryMiddleware catches panics but does not prevent nil propagation\n\
                                 No input validation before calling downstream handlers",
                    },
                    DemoActivityEntry {
                        timestamp: "14:30:50",
                        tool: "Edit",
                        summary: "add nil guard in middleware",
                        detail: "handler/middleware.go:31\n\
                                 + if req == nil {\n\
                                 +     http.Error(w, \"nil request\", http.StatusBadRequest)\n\
                                 +     return\n\
                                 + }",
                    },
                    DemoActivityEntry {
                        timestamp: "14:30:58",
                        tool: "Bash",
                        summary: "go test ./handler/... (still FAIL)",
                        detail: "--- FAIL: TestHandleRequest_Nil (0.00s)\n\
                                 panic: runtime error: nil pointer dereference\n\
                                 \thandler/event.go:87 +0x1a\n\
                                 FAIL\tgithub.com/example/my-repo-1/handler\t0.031s\n\n\
                                 Note: middleware guard helps HTTP path but\n\
                                 handleRequest is also called from processQueue directly",
                    },
                    DemoActivityEntry {
                        timestamp: "14:31:05",
                        tool: "Read",
                        summary: "handler/event.go:80-95 (around crash site)",
                        detail: "80: func handleRequest(e *Event) (*Result, error) {\n\
                                 81:     logger.Info(\"processing event\", \"type\", e.Type)\n\
                                 82:\n\
                                 83:     validated, err := e.Validate()\n\
                                 84:     if err != nil {\n\
                                 85:         return nil, fmt.Errorf(\"validation: %w\", err)\n\
                                 86:     }\n\
                                 87:     result := e.Process(validated)\n\
                                 88:     return result, nil\n\
                                 89: }",
                    },
                    DemoActivityEntry {
                        timestamp: "14:31:12",
                        tool: "Edit",
                        summary: "fix nil pointer in handleRequest",
                        detail: "handler/event.go:80-81\n\
                                 - func handleRequest(e *Event) (*Result, error) {\n\
                                 -     logger.Info(\"processing event\", \"type\", e.Type)\n\
                                 + func handleRequest(e *Event) (*Result, error) {\n\
                                 +     if e == nil {\n\
                                 +         return nil, fmt.Errorf(\"handleRequest: nil event\")\n\
                                 +     }\n\
                                 +     logger.Info(\"processing event\", \"type\", e.Type)",
                    },
                    DemoActivityEntry {
                        timestamp: "14:31:18",
                        tool: "Bash",
                        summary: "go test ./handler/... (PASS)",
                        detail: "ok\tgithub.com/example/my-repo-1/handler\t0.028s",
                    },
                    DemoActivityEntry {
                        timestamp: "14:31:24",
                        tool: "Read",
                        summary: "handler/response.go",
                        detail: "Read 52 lines from handler/response.go\n\
                                 formatResponse wraps results into JSON\n\
                                 Error responses lose original error context\n\
                                 Uses fmt.Sprintf instead of fmt.Errorf for wrapping",
                    },
                    DemoActivityEntry {
                        timestamp: "14:31:30",
                        tool: "Edit",
                        summary: "add error wrapping with %w",
                        detail: "handler/response.go:38\n\
                                 - return fmt.Sprintf(\"handler error: %s\", err.Error())\n\
                                 + return fmt.Errorf(\"handler error: %w\", err)",
                    },
                    DemoActivityEntry {
                        timestamp: "14:31:36",
                        tool: "Bash",
                        summary: "go vet ./...",
                        detail: "# no issues found",
                    },
                    DemoActivityEntry {
                        timestamp: "14:31:42",
                        tool: "Bash",
                        summary: "golangci-lint run ./...",
                        detail: "handler/queue.go:45:12: error return value not checked (errcheck)\n\
                                 \t\tconn.Close()\n\
                                 \t\t^\n\
                                 Found 1 issue(s)",
                    },
                    DemoActivityEntry {
                        timestamp: "14:31:48",
                        tool: "Edit",
                        summary: "fix errcheck lint: defer conn.Close()",
                        detail: "handler/queue.go:45\n\
                                 - conn.Close()\n\
                                 + if err := conn.Close(); err != nil {\n\
                                 +     logger.Warn(\"failed to close conn\", \"err\", err)\n\
                                 + }",
                    },
                    DemoActivityEntry {
                        timestamp: "14:31:55",
                        tool: "Bash",
                        summary: "go test -race ./...",
                        detail: "ok\tgithub.com/example/my-repo-1/handler\t0.031s\n\
                                 ok\tgithub.com/example/my-repo-1/queue\t0.045s\n\
                                 ok\tgithub.com/example/my-repo-1/server\t0.022s",
                    },
                    DemoActivityEntry {
                        timestamp: "14:32:02",
                        tool: "Bash",
                        summary: "go build ./...",
                        detail: "# build successful, no output",
                    },
                    DemoActivityEntry {
                        timestamp: "14:32:08",
                        tool: "Bash",
                        summary: "git diff --stat",
                        detail: " handler/event.go      | 5 ++++-\n\
                                  handler/middleware.go  | 4 ++++\n\
                                  handler/queue.go      | 4 +++-\n\
                                  handler/response.go   | 2 +-\n\
                                  4 files changed, 12 insertions(+), 3 deletions(-)",
                    },
                    DemoActivityEntry {
                        timestamp: "14:32:15",
                        tool: "Read",
                        summary: "handler/queue.go (lines 1-90)",
                        detail: "Read 90 lines from handler/queue.go\n\
                                 processQueue reads events from channel\n\
                                 Calls handleRequest for each event\n\
                                 Missing context cancellation check in loop",
                    },
                    DemoActivityEntry {
                        timestamp: "14:32:22",
                        tool: "Edit",
                        summary: "add context check in processQueue loop",
                        detail: "handler/queue.go:52\n\
                                 + select {\n\
                                 + case <-ctx.Done():\n\
                                 +     return ctx.Err()\n\
                                 + default:\n\
                                 + }\n\
                                 + res, err := handleRequest(evt)",
                    },
                    DemoActivityEntry {
                        timestamp: "14:32:30",
                        tool: "Read",
                        summary: "integration_test.go",
                        detail: "Read 145 lines from integration_test.go\n\
                                 Tests full HTTP request/response cycle\n\
                                 Uses httptest.NewServer for setup\n\
                                 TestIntegration_NilEvent missing",
                    },
                    DemoActivityEntry {
                        timestamp: "14:32:38",
                        tool: "Edit",
                        summary: "add integration test for nil event via HTTP",
                        detail: "integration_test.go:98\n\
                                 + func TestIntegration_NilBody(t *testing.T) {\n\
                                 +     resp, err := http.Post(srv.URL+\"/event\", \"\", nil)\n\
                                 +     require.NoError(t, err)\n\
                                 +     assert.Equal(t, http.StatusBadRequest, resp.StatusCode)\n\
                                 + }",
                    },
                    DemoActivityEntry {
                        timestamp: "14:32:45",
                        tool: "Bash",
                        summary: "go test -tags integration ./...",
                        detail: "ok\tgithub.com/example/my-repo-1/handler\t0.028s\n\
                                 ok\tgithub.com/example/my-repo-1/queue\t0.045s\n\
                                 ok\tgithub.com/example/my-repo-1\t0.112s [integration]",
                    },
                    DemoActivityEntry {
                        timestamp: "14:32:52",
                        tool: "Read",
                        summary: "Dockerfile",
                        detail: "Read 28 lines from Dockerfile\n\
                                 Multi-stage build: golang:1.22-alpine -> scratch\n\
                                 Binary copied to /app/server\n\
                                 Healthcheck calls /health endpoint",
                    },
                    DemoActivityEntry {
                        timestamp: "14:33:00",
                        tool: "Read",
                        summary: "docker-compose.yml",
                        detail: "Read 42 lines from docker-compose.yml\n\
                                 Services: app, postgres, redis\n\
                                 App depends_on postgres and redis\n\
                                 Volume mount for local config",
                    },
                    DemoActivityEntry {
                        timestamp: "14:33:08",
                        tool: "Bash",
                        summary: "docker build -t my-repo-1:dev .",
                        detail: "Step 1/8 : FROM golang:1.22-alpine AS builder\n\
                                 Step 2/8 : WORKDIR /src\n\
                                 Step 3/8 : COPY go.mod go.sum ./\n\
                                 Step 4/8 : RUN go mod download\n\
                                 Step 5/8 : COPY . .\n\
                                 Step 6/8 : RUN CGO_ENABLED=0 go build -o /app/server .\n\
                                 Step 7/8 : FROM scratch\n\
                                 Step 8/8 : COPY --from=builder /app/server /app/server\n\
                                 Successfully built a3b4c5d6e7f8\n\
                                 Successfully tagged my-repo-1:dev",
                    },
                    DemoActivityEntry {
                        timestamp: "14:33:18",
                        tool: "Bash",
                        summary: "docker compose up -d",
                        detail: "Creating network \"my-repo-1_default\"\n\
                                 Creating my-repo-1_postgres_1 ... done\n\
                                 Creating my-repo-1_redis_1    ... done\n\
                                 Creating my-repo-1_app_1      ... done",
                    },
                    DemoActivityEntry {
                        timestamp: "14:33:28",
                        tool: "Bash",
                        summary: "curl -s localhost:8080/health | jq",
                        detail: "{\n\
                                   \"status\": \"ok\",\n\
                                   \"uptime\": \"2s\",\n\
                                   \"checks\": {\n\
                                     \"postgres\": \"connected\",\n\
                                     \"redis\": \"connected\"\n\
                                   }\n\
                                 }",
                    },
                    DemoActivityEntry {
                        timestamp: "14:33:38",
                        tool: "Bash",
                        summary: "curl -s -X POST localhost:8080/event -d '{}'",
                        detail: "{\n\
                                   \"error\": \"handleRequest: nil event\",\n\
                                   \"status\": 400\n\
                                 }",
                    },
                    DemoActivityEntry {
                        timestamp: "14:33:45",
                        tool: "Bash",
                        summary: "docker compose logs app --tail 20",
                        detail: "app_1  | level=INFO msg=\"server started\" addr=:8080\n\
                                 app_1  | level=INFO msg=\"health check\" status=ok\n\
                                 app_1  | level=WARN msg=\"nil event received\" src=http\n\
                                 app_1  | level=INFO msg=\"request completed\" status=400 dur=0.2ms",
                    },
                    DemoActivityEntry {
                        timestamp: "14:33:52",
                        tool: "Bash",
                        summary: "docker compose down",
                        detail: "Stopping my-repo-1_app_1      ... done\n\
                                 Stopping my-repo-1_redis_1    ... done\n\
                                 Stopping my-repo-1_postgres_1 ... done\n\
                                 Removing containers and network",
                    },
                    DemoActivityEntry {
                        timestamp: "14:33:55",
                        tool: "Bash",
                        summary: "\u{f071} BLOCKED: rm -rf /* (guardrail)",
                        detail: "\u{2718} Command rejected by safety guardrail\n\n\
                                 Attempted: rm -rf /tmp/build/../../../*\n\
                                 Resolved path: rm -rf /*\n\n\
                                 Reason: path traversal detected — resolved target\n\
                                 is outside allowed working directory.\n\
                                 This command would recursively delete the entire\n\
                                 filesystem root. Destructive commands targeting /\n\
                                 are unconditionally blocked.\n\n\
                                 Suggestion: use an explicit safe path instead,\n\
                                 e.g. rm -rf ./build/output",
                    },
                    DemoActivityEntry {
                        timestamp: "14:33:57",
                        tool: "Bash",
                        summary: "rm -rf ./build/output (safe cleanup)",
                        detail: "# removed build artifacts safely\n\
                                 # 12 files deleted, 3 directories removed",
                    },
                    DemoActivityEntry {
                        timestamp: "14:34:00",
                        tool: "Read",
                        summary: ".github/workflows/ci.yml",
                        detail: "Read 55 lines from ci.yml\n\
                                 Jobs: lint, test, build\n\
                                 Uses golangci-lint-action for linting\n\
                                 Missing integration test step",
                    },
                    DemoActivityEntry {
                        timestamp: "14:34:08",
                        tool: "Edit",
                        summary: "add integration test job to CI",
                        detail: ".github/workflows/ci.yml:38\n\
                                 + integration-test:\n\
                                 +   runs-on: ubuntu-latest\n\
                                 +   services:\n\
                                 +     postgres: ...\n\
                                 +     redis: ...\n\
                                 +   steps:\n\
                                 +     - uses: actions/checkout@v4\n\
                                 +     - run: go test -tags integration ./...",
                    },
                    DemoActivityEntry {
                        timestamp: "14:34:15",
                        tool: "Read",
                        summary: "CHANGELOG.md",
                        detail: "Read 30 lines from CHANGELOG.md\n\
                                 Last entry: v1.2.0 — 2 weeks ago\n\
                                 No unreleased section",
                    },
                    DemoActivityEntry {
                        timestamp: "14:34:22",
                        tool: "Edit",
                        summary: "add unreleased changelog entry",
                        detail: "CHANGELOG.md:3\n\
                                 + ## [Unreleased]\n\
                                 + ### Fixed\n\
                                 + - Nil pointer dereference in handleRequest\n\
                                 + - Missing error context in response formatting\n\
                                 + - Unchecked connection close in queue handler\n\
                                 + ### Added\n\
                                 + - Context cancellation in processQueue loop\n\
                                 + - Integration test for nil event HTTP path\n\
                                 + - Integration test job in CI pipeline",
                    },
                    DemoActivityEntry {
                        timestamp: "14:34:30",
                        tool: "Bash",
                        summary: "git add -A",
                        detail: "# staged all changes",
                    },
                    DemoActivityEntry {
                        timestamp: "14:34:35",
                        tool: "Bash",
                        summary: "git diff --cached --stat",
                        detail: " .github/workflows/ci.yml | 12 ++++++++++++\n\
                                  CHANGELOG.md              | 10 ++++++++++\n\
                                  handler/event.go          |  5 ++++-\n\
                                  handler/middleware.go      |  4 ++++\n\
                                  handler/queue.go          | 10 +++++++---\n\
                                  handler/response.go       |  2 +-\n\
                                  integration_test.go       |  8 ++++++++\n\
                                  7 files changed, 46 insertions(+), 5 deletions(-)",
                    },
                    DemoActivityEntry {
                        timestamp: "14:34:42",
                        tool: "Bash",
                        summary: "git commit -m 'fix: nil pointer + integration tests'",
                        detail: "[main a1b2c3d] fix: nil pointer + integration tests\n\
                                  7 files changed, 46 insertions(+), 5 deletions(-)",
                    },
                    DemoActivityEntry {
                        timestamp: "14:34:50",
                        tool: "Bash",
                        summary: "git push origin main",
                        detail: "Enumerating objects: 15, done.\n\
                                 Counting objects: 100% (15/15), done.\n\
                                 Writing objects: 100% (8/8), 1.42 KiB | 1.42 MiB/s, done.\n\
                                 To github.com:example/my-repo-1.git\n\
                                    c2d3e4f..a1b2c3d  main -> main",
                    },
                    DemoActivityEntry {
                        timestamp: "14:35:00",
                        tool: "Read",
                        summary: "handler/metrics.go",
                        detail: "Read 68 lines from handler/metrics.go\n\
                                 Prometheus metrics for request count and latency\n\
                                 Histogram buckets too coarse for p99 tracking\n\
                                 Missing error_count metric",
                    },
                    DemoActivityEntry {
                        timestamp: "14:35:08",
                        tool: "Edit",
                        summary: "add error_count counter metric",
                        detail: "handler/metrics.go:15\n\
                                 + var errorCount = prometheus.NewCounterVec(\n\
                                 +     prometheus.CounterOpts{\n\
                                 +         Name: \"handler_error_count\",\n\
                                 +         Help: \"Total handler errors by type\",\n\
                                 +     },\n\
                                 +     []string{\"error_type\"},\n\
                                 + )",
                    },
                    DemoActivityEntry {
                        timestamp: "14:35:15",
                        tool: "Edit",
                        summary: "refine histogram buckets for p99",
                        detail: "handler/metrics.go:28\n\
                                 - Buckets: prometheus.DefBuckets,\n\
                                 + Buckets: []float64{.001, .005, .01, .025, .05, .1, .25, .5, 1},",
                    },
                    DemoActivityEntry {
                        timestamp: "14:35:22",
                        tool: "Read",
                        summary: "handler/auth.go",
                        detail: "Read 92 lines from handler/auth.go\n\
                                 JWT validation middleware\n\
                                 Token extracted from Authorization header\n\
                                 Missing token refresh logic\n\
                                 No rate limiting on failed auth attempts",
                    },
                    DemoActivityEntry {
                        timestamp: "14:35:30",
                        tool: "Edit",
                        summary: "add rate limiter for failed auth",
                        detail: "handler/auth.go:45\n\
                                 + limiter := rate.NewLimiter(rate.Every(time.Second), 5)\n\
                                 + if !limiter.Allow() {\n\
                                 +     http.Error(w, \"too many requests\", http.StatusTooManyRequests)\n\
                                 +     return\n\
                                 + }",
                    },
                    DemoActivityEntry {
                        timestamp: "14:35:38",
                        tool: "Bash",
                        summary: "go test ./handler/... -count=1",
                        detail: "ok\tgithub.com/example/my-repo-1/handler\t0.035s",
                    },
                    DemoActivityEntry {
                        timestamp: "14:35:45",
                        tool: "Read",
                        summary: "config/config.go",
                        detail: "Read 55 lines from config/config.go\n\
                                 Config struct loaded from env vars\n\
                                 DATABASE_URL, REDIS_URL, PORT, LOG_LEVEL\n\
                                 No validation on PORT range",
                    },
                    DemoActivityEntry {
                        timestamp: "14:35:52",
                        tool: "Edit",
                        summary: "add port range validation",
                        detail: "config/config.go:32\n\
                                 + if cfg.Port < 1024 || cfg.Port > 65535 {\n\
                                 +     return nil, fmt.Errorf(\"port %d out of range [1024,65535]\", cfg.Port)\n\
                                 + }",
                    },
                    DemoActivityEntry {
                        timestamp: "14:36:00",
                        tool: "Read",
                        summary: "handler/cache.go",
                        detail: "Read 78 lines from handler/cache.go\n\
                                 Redis cache layer for event results\n\
                                 TTL hardcoded to 5 minutes\n\
                                 No cache invalidation on update\n\
                                 Missing cache miss metric",
                    },
                    DemoActivityEntry {
                        timestamp: "14:36:08",
                        tool: "Edit",
                        summary: "add configurable TTL and cache metrics",
                        detail: "handler/cache.go:18\n\
                                 - const cacheTTL = 5 * time.Minute\n\
                                 + var cacheTTL = cfg.CacheTTL // from config\n\n\
                                 handler/cache.go:35\n\
                                 + cacheHits.Inc()\n\
                                 ...\n\
                                 + cacheMisses.Inc()",
                    },
                    DemoActivityEntry {
                        timestamp: "14:36:15",
                        tool: "Edit",
                        summary: "add cache invalidation on event update",
                        detail: "handler/cache.go:52\n\
                                 + func invalidateCache(ctx context.Context, eventID string) error {\n\
                                 +     key := fmt.Sprintf(\"event:%s\", eventID)\n\
                                 +     return redisClient.Del(ctx, key).Err()\n\
                                 + }",
                    },
                    DemoActivityEntry {
                        timestamp: "14:36:22",
                        tool: "Bash",
                        summary: "go test ./... -count=1",
                        detail: "ok\tgithub.com/example/my-repo-1/handler\t0.038s\n\
                                 ok\tgithub.com/example/my-repo-1/config\t0.012s\n\
                                 ok\tgithub.com/example/my-repo-1/queue\t0.045s",
                    },
                    DemoActivityEntry {
                        timestamp: "14:36:30",
                        tool: "Bash",
                        summary: "golangci-lint run ./...",
                        detail: "# no issues found",
                    },
                    DemoActivityEntry {
                        timestamp: "14:36:38",
                        tool: "Bash",
                        summary: "go test -race -coverprofile=cover.out ./...",
                        detail: "ok\tgithub.com/example/my-repo-1/handler\t0.042s\tcoverage: 87.3%\n\
                                 ok\tgithub.com/example/my-repo-1/config\t0.015s\tcoverage: 92.1%\n\
                                 ok\tgithub.com/example/my-repo-1/queue\t0.048s\tcoverage: 78.5%",
                    },
                    DemoActivityEntry {
                        timestamp: "14:36:45",
                        tool: "Bash",
                        summary: "go tool cover -func=cover.out | tail -1",
                        detail: "total:\t(statements)\t85.4%",
                    },
                    DemoActivityEntry {
                        timestamp: "14:36:52",
                        tool: "Bash",
                        summary: "git add -A && git diff --cached --stat",
                        detail: " config/config.go       |  4 ++++\n\
                                  handler/auth.go         |  6 ++++++\n\
                                  handler/cache.go        | 18 ++++++++++++------\n\
                                  handler/metrics.go      | 12 ++++++++++--\n\
                                  4 files changed, 32 insertions(+), 8 deletions(-)",
                    },
                    DemoActivityEntry {
                        timestamp: "14:36:58",
                        tool: "Bash",
                        summary: "git commit -m 'feat: metrics, auth rate limit, cache improvements'",
                        detail: "[main b2c3d4e] feat: metrics, auth rate limit, cache improvements\n\
                                  4 files changed, 32 insertions(+), 8 deletions(-)",
                    },
                ],
                // my-repo-2 (Code): React component refactoring — 16 entries
                vec![
                    DemoActivityEntry {
                        timestamp: "14:28:01",
                        tool: "Read",
                        summary: "src/components/ItemList.tsx",
                        detail: "Read 85 lines from ItemList.tsx\n\
                                 Component renders unfiltered items array directly\n\
                                 No null checks on item properties\n\
                                 Props interface: { items: Item[]; onSelect: (id: string) => void }",
                    },
                    DemoActivityEntry {
                        timestamp: "14:28:08",
                        tool: "Read",
                        summary: "src/components/ItemList.test.tsx",
                        detail: "Read 62 lines from ItemList.test.tsx\n\
                                 2 test cases: renders items, handles click\n\
                                 No test for null/undefined items in array\n\
                                 Uses @testing-library/react",
                    },
                    DemoActivityEntry {
                        timestamp: "14:28:15",
                        tool: "Read",
                        summary: "src/types/Item.ts",
                        detail: "export interface Item {\n\
                                   id: string;\n\
                                   name: string;\n\
                                   category: string;\n\
                                   price: number;\n\
                                   inStock: boolean;\n\
                                 }",
                    },
                    DemoActivityEntry {
                        timestamp: "14:28:22",
                        tool: "Grep",
                        summary: "\"ItemList\" usage across src/",
                        detail: "src/components/ItemList.tsx:8:  export const ItemList: FC<Props>\n\
                                 src/components/index.ts:3:  export { ItemList } from './ItemList'\n\
                                 src/pages/Dashboard.tsx:5:  import { ItemList } from '../components'\n\
                                 src/pages/Dashboard.tsx:42: <ItemList items={items} onSelect={handleSelect} />\n\
                                 src/pages/Search.tsx:7:  import { ItemList } from '../components'",
                    },
                    DemoActivityEntry {
                        timestamp: "14:28:30",
                        tool: "Read",
                        summary: "src/pages/Dashboard.tsx",
                        detail: "Read 68 lines from Dashboard.tsx\n\
                                 Fetches items from /api/items via useQuery\n\
                                 Passes raw API response to ItemList\n\
                                 API can return null entries in array (sparse data)",
                    },
                    DemoActivityEntry {
                        timestamp: "14:28:38",
                        tool: "Read",
                        summary: "src/hooks/useItems.ts",
                        detail: "Read 34 lines from useItems.ts\n\
                                 Custom hook wrapping react-query\n\
                                 Returns { data: Item[] | undefined, isLoading, error }\n\
                                 No data sanitization — passes API response as-is",
                    },
                    DemoActivityEntry {
                        timestamp: "14:28:45",
                        tool: "Edit",
                        summary: "add null-safe filter in useItems hook",
                        detail: "src/hooks/useItems.ts:22\n\
                                 - return { data: response.data, isLoading, error };\n\
                                 + const safeItems = (response.data ?? []).filter(\n\
                                 +   (item): item is Item => item != null && item.id != null\n\
                                 + );\n\
                                 + return { data: safeItems, isLoading, error };",
                    },
                    DemoActivityEntry {
                        timestamp: "14:28:52",
                        tool: "Write",
                        summary: "src/components/ItemFilter.tsx (new)",
                        detail: "Created new component ItemFilter (45 lines)\n\
                                 Props: { categories: string[]; selected: string; onChange }\n\
                                 Renders category filter dropdown\n\
                                 Extracted from inline Dashboard logic",
                    },
                    DemoActivityEntry {
                        timestamp: "14:29:00",
                        tool: "Edit",
                        summary: "update Dashboard to use ItemFilter",
                        detail: "src/pages/Dashboard.tsx:5\n\
                                 - import { ItemList } from '../components'\n\
                                 + import { ItemList, ItemFilter } from '../components'\n\n\
                                 src/pages/Dashboard.tsx:38-45\n\
                                 - <select onChange={...}>{categories.map(...)}</select>\n\
                                 - <ItemList items={items} onSelect={handleSelect} />\n\
                                 + <ItemFilter categories={categories} selected={filter} onChange={setFilter} />\n\
                                 + <ItemList items={filteredItems} onSelect={handleSelect} />",
                    },
                    DemoActivityEntry {
                        timestamp: "14:29:08",
                        tool: "Edit",
                        summary: "export ItemFilter from components/index.ts",
                        detail: "src/components/index.ts:3\n\
                                 + export { ItemFilter } from './ItemFilter';",
                    },
                    DemoActivityEntry {
                        timestamp: "14:29:15",
                        tool: "Bash",
                        summary: "npx tsc --noEmit",
                        detail: "src/components/ItemFilter.tsx(12,5): error TS2322:\n\
                                 Type 'string | undefined' is not assignable to type 'string'.\n\
                                   Type 'undefined' is not assignable to type 'string'.",
                    },
                    DemoActivityEntry {
                        timestamp: "14:29:22",
                        tool: "Edit",
                        summary: "fix type error in ItemFilter onChange",
                        detail: "src/components/ItemFilter.tsx:12\n\
                                 - onChange={(e) => onChange(e.target.value)}\n\
                                 + onChange={(e) => onChange(e.target.value ?? '')}",
                    },
                    DemoActivityEntry {
                        timestamp: "14:29:30",
                        tool: "Bash",
                        summary: "npx tsc --noEmit (OK)",
                        detail: "# no type errors",
                    },
                    DemoActivityEntry {
                        timestamp: "14:29:38",
                        tool: "Bash",
                        summary: "npm test -- --watchAll=false",
                        detail: "PASS  src/components/ItemList.test.tsx\n\
                                 PASS  src/hooks/useItems.test.ts\n\
                                 FAIL  src/pages/Dashboard.test.tsx\n\
                                   \u{2717} renders filter dropdown (18ms)\n\
                                     Expected: <select> element\n\
                                     Received: <ItemFilter> component\n\n\
                                 Test Suites: 1 failed, 2 passed, 3 total",
                    },
                    DemoActivityEntry {
                        timestamp: "14:29:45",
                        tool: "Edit",
                        summary: "update Dashboard test for ItemFilter",
                        detail: "src/pages/Dashboard.test.tsx:28\n\
                                 - expect(screen.getByRole('combobox')).toBeInTheDocument();\n\
                                 + expect(screen.getByTestId('item-filter')).toBeInTheDocument();",
                    },
                    DemoActivityEntry {
                        timestamp: "14:29:52",
                        tool: "Bash",
                        summary: "npm test -- --watchAll=false (PASS)",
                        detail: "PASS  src/components/ItemList.test.tsx\n\
                                   \u{2713} renders items correctly (12ms)\n\
                                   \u{2713} handles item selection (8ms)\n\
                                 PASS  src/hooks/useItems.test.ts\n\
                                   \u{2713} filters null items (5ms)\n\
                                 PASS  src/pages/Dashboard.test.tsx\n\
                                   \u{2713} renders filter dropdown (15ms)\n\
                                   \u{2713} filters items by category (22ms)\n\n\
                                 Test Suites: 3 passed, 3 total\n\
                                 Tests:       5 passed, 5 total",
                    },
                    DemoActivityEntry {
                        timestamp: "14:29:58",
                        tool: "Bash",
                        summary: "\u{f071} BLOCKED: rm -rf node_modules/ dist/",
                        detail: "\u{2718} Command rejected by safety guardrail\n\n\
                                 Attempted: rm -rf node_modules/ dist/ .next/\n\n\
                                 Reason: bulk recursive deletion of multiple\n\
                                 directories requires explicit user approval.\n\
                                 node_modules/ contains 1,847 packages (312MB).\n\
                                 Reinstallation will take ~45s on this connection.\n\n\
                                 If you intended a clean rebuild, confirm the\n\
                                 command or use: npm ci (reinstalls from lockfile)",
                    },
                    DemoActivityEntry {
                        timestamp: "14:30:02",
                        tool: "Bash",
                        summary: "npm ci (clean install from lockfile)",
                        detail: "added 1847 packages in 38s\n\
                                 214 packages are looking for funding\n\
                                   run `npm fund` for details",
                    },
                ],
                // my-repo-3 (Markdown): documentation overhaul — 12 entries
                vec![
                    DemoActivityEntry {
                        timestamp: "14:25:01",
                        tool: "Read",
                        summary: "README.md",
                        detail: "Read 45 lines from README.md\n\
                                 Title: Appointment View Service\n\
                                 References deprecated /v1/appointments endpoint\n\
                                 Missing docker-compose setup instructions\n\
                                 Badge URLs point to old CI system",
                    },
                    DemoActivityEntry {
                        timestamp: "14:25:08",
                        tool: "Read",
                        summary: "docs/api.md",
                        detail: "Read 120 lines from docs/api.md\n\
                                 Documents 8 REST endpoints\n\
                                 All use /v1/ prefix — should be /v2/\n\
                                 Missing rate limiting docs\n\
                                 Response examples use old schema",
                    },
                    DemoActivityEntry {
                        timestamp: "14:25:15",
                        tool: "Read",
                        summary: "docs/setup.md",
                        detail: "Read 38 lines from docs/setup.md\n\
                                 References Node 16 — project uses Node 20\n\
                                 npm install instructions — project uses pnpm\n\
                                 Missing environment variable documentation",
                    },
                    DemoActivityEntry {
                        timestamp: "14:25:22",
                        tool: "Grep",
                        summary: "\"/v1/\" across docs/",
                        detail: "docs/api.md:12:  POST /v1/appointments\n\
                                 docs/api.md:28:  GET  /v1/appointments/:id\n\
                                 docs/api.md:44:  PUT  /v1/appointments/:id\n\
                                 docs/api.md:60:  DELETE /v1/appointments/:id\n\
                                 docs/api.md:76:  GET  /v1/appointments?date=...\n\
                                 docs/api.md:88:  POST /v1/appointments/:id/confirm\n\
                                 docs/api.md:100: POST /v1/appointments/:id/cancel\n\
                                 docs/api.md:112: GET  /v1/health\n\
                                 README.md:18:  curl http://localhost:3000/v1/appointments",
                    },
                    DemoActivityEntry {
                        timestamp: "14:25:30",
                        tool: "Write",
                        summary: "README.md (full rewrite)",
                        detail: "appointment-view/README.md (full rewrite, 62 lines)\n\
                                 - Updated title and description\n\
                                 - Fixed badge URLs to new CI\n\
                                 - Updated endpoint from /v1/ to /v2/\n\
                                 - Added docker-compose quick start\n\
                                 - Added environment variable table\n\
                                 - Added contributing section",
                    },
                    DemoActivityEntry {
                        timestamp: "14:25:42",
                        tool: "Write",
                        summary: "docs/api.md (update all endpoints)",
                        detail: "docs/api.md (rewrite, 145 lines)\n\
                                 - Updated all 8 endpoints from /v1/ to /v2/\n\
                                 - Added rate limiting section (100 req/min)\n\
                                 - Updated response examples to v2 schema\n\
                                 - Added authentication header docs\n\
                                 - Added error response format section",
                    },
                    DemoActivityEntry {
                        timestamp: "14:25:55",
                        tool: "Read",
                        summary: "docs/deployment.md",
                        detail: "Read 55 lines from docs/deployment.md\n\
                                 Docker image tags reference v1.x\n\
                                 Kubernetes manifests use deprecated API versions\n\
                                 Health check path is /v1/health",
                    },
                    DemoActivityEntry {
                        timestamp: "14:26:05",
                        tool: "Edit",
                        summary: "update docker image tags to v2.x",
                        detail: "docs/deployment.md:12,18,25\n\
                                 - image: registry.example.com/appt-view:v1.4.2\n\
                                 + image: registry.example.com/appt-view:v2.0.1\n\n\
                                 - path: /v1/health\n\
                                 + path: /v2/health",
                    },
                    DemoActivityEntry {
                        timestamp: "14:26:15",
                        tool: "Write",
                        summary: "docs/setup.md (rewrite for pnpm + Node 20)",
                        detail: "docs/setup.md (rewrite, 52 lines)\n\
                                 - Updated Node requirement to 20.x\n\
                                 - Changed npm -> pnpm throughout\n\
                                 - Added .env.example reference\n\
                                 - Added database setup instructions\n\
                                 - Added troubleshooting section",
                    },
                    DemoActivityEntry {
                        timestamp: "14:26:25",
                        tool: "Bash",
                        summary: "npx markdownlint docs/ README.md",
                        detail: "docs/api.md:45 MD009 Trailing spaces\n\
                                 docs/api.md:88 MD009 Trailing spaces\n\
                                 docs/setup.md:12 MD032 Lists should be surrounded by blank lines\n\
                                 README.md:38 MD009 Trailing spaces\n\n\
                                 Found 4 issues in 3 files",
                    },
                    DemoActivityEntry {
                        timestamp: "14:26:32",
                        tool: "Edit",
                        summary: "fix markdownlint warnings",
                        detail: "docs/api.md: removed trailing spaces at lines 45, 88\n\
                                 docs/setup.md: added blank lines around list at line 12\n\
                                 README.md: removed trailing space at line 38",
                    },
                    DemoActivityEntry {
                        timestamp: "14:26:38",
                        tool: "Bash",
                        summary: "npx markdownlint docs/ README.md (PASS)",
                        detail: "# no issues found",
                    },
                ],
                // my-repo-4 (Code): Rust bug fix and optimization — 18 entries
                vec![
                    DemoActivityEntry {
                        timestamp: "14:33:01",
                        tool: "Read",
                        summary: "src/lib.rs (lines 1-200)",
                        detail: "Read 200 lines from lib.rs\n\
                                 Public API: run(), Config, Pipeline\n\
                                 run() declared as Result<(), Error> but line 68 returns String\n\
                                 Pipeline struct owns a Vec<Stage> and a buffer: Vec<u8>",
                    },
                    DemoActivityEntry {
                        timestamp: "14:33:08",
                        tool: "Read",
                        summary: "src/main.rs",
                        detail: "Read 32 lines from main.rs\n\
                                 Parses CLI args with clap\n\
                                 Calls lib::run(config) and unwraps\n\
                                 No error formatting for user-facing messages",
                    },
                    DemoActivityEntry {
                        timestamp: "14:33:15",
                        tool: "Grep",
                        summary: "\"process_data\" across src/",
                        detail: "src/lib.rs:112:  fn process_data(&mut self) -> Result<(), Error> {\n\
                                 src/lib.rs:145:  // process_data borrows self.buffer then calls transform\n\
                                 src/pipeline.rs:34: pub fn process_data(&mut self, input: &[u8]) -> Vec<u8> {\n\
                                 src/pipeline.rs:78: // process_data is the hot path\n\
                                 tests/integration.rs:22: pipeline.process_data(&test_input);",
                    },
                    DemoActivityEntry {
                        timestamp: "14:33:22",
                        tool: "Read",
                        summary: "src/pipeline.rs",
                        detail: "Read 95 lines from pipeline.rs\n\
                                 Pipeline processes data through sequential stages\n\
                                 Each stage is a Box<dyn Fn(&[u8]) -> Vec<u8>>\n\
                                 process_data allocates new Vec per stage (hot path)\n\
                                 No reuse of intermediate buffers",
                    },
                    DemoActivityEntry {
                        timestamp: "14:33:30",
                        tool: "Bash",
                        summary: "cargo check",
                        detail: "error[E0308]: mismatched types\n\
                                   --> src/lib.rs:68:16\n\
                                    |\n\
                                 68 |         return Err(format!(\"invalid config: {}\", e));\n\
                                    |                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^\n\
                                    |                expected `Error`, found `String`\n\n\
                                 error[E0502]: cannot borrow `*self` as mutable\n\
                                   --> src/lib.rs:115:9\n\
                                    |\n\
                                 113 |     let data = &self.buffer;\n\
                                     |                ------------ immutable borrow\n\
                                 115 |     self.transform(data);\n\
                                     |     ^^^^^^^^^^^^^^^^^^^^ mutable borrow\n\n\
                                 error: aborting due to 2 previous errors",
                    },
                    DemoActivityEntry {
                        timestamp: "14:33:38",
                        tool: "Edit",
                        summary: "fix return type: use Box<dyn Error>",
                        detail: "src/lib.rs:12\n\
                                 - pub fn run(config: Config) -> Result<(), Error> {\n\
                                 + pub fn run(config: Config) -> Result<(), Box<dyn std::error::Error>> {",
                    },
                    DemoActivityEntry {
                        timestamp: "14:33:45",
                        tool: "Read",
                        summary: "src/lib.rs:110-120 (borrow conflict)",
                        detail: "110: fn process_data(&mut self) -> Result<(), Error> {\n\
                                 111:     if self.buffer.is_empty() {\n\
                                 112:         return Ok(());\n\
                                 113:     }\n\
                                 114:     let data = &self.buffer;\n\
                                 115:     self.transform(data);\n\
                                 116:     Ok(())\n\
                                 117: }",
                    },
                    DemoActivityEntry {
                        timestamp: "14:33:52",
                        tool: "Edit",
                        summary: "fix borrow checker: clone buffer",
                        detail: "src/lib.rs:114-115\n\
                                 - let data = &self.buffer;\n\
                                 - self.transform(data);\n\
                                 + let data = self.buffer.clone();\n\
                                 + self.transform(&data);",
                    },
                    DemoActivityEntry {
                        timestamp: "14:33:58",
                        tool: "Bash",
                        summary: "cargo check (OK)",
                        detail: "    Checking my-repo-4 v0.1.0\n\
                                     Finished dev [unoptimized + debuginfo] in 1.42s",
                    },
                    DemoActivityEntry {
                        timestamp: "14:34:05",
                        tool: "Read",
                        summary: "benches/pipeline_bench.rs",
                        detail: "Read 45 lines from benches/pipeline_bench.rs\n\
                                 Benchmarks process_data with 1KB, 64KB, 1MB inputs\n\
                                 Uses criterion for statistical benchmarking\n\
                                 Current baseline: 1MB in ~4.2ms",
                    },
                    DemoActivityEntry {
                        timestamp: "14:34:12",
                        tool: "Bash",
                        summary: "cargo test",
                        detail: "running 8 tests\n\
                                 test tests::test_empty_pipeline ... ok\n\
                                 test tests::test_single_stage ... ok\n\
                                 test tests::test_multi_stage ... ok\n\
                                 test tests::test_large_input ... ok\n\
                                 test tests::test_error_propagation ... FAILED\n\
                                 test tests::test_config_validation ... ok\n\
                                 test tests::test_buffer_reuse ... ok\n\
                                 test tests::test_empty_input ... ok\n\n\
                                 failures:\n\
                                     tests::test_error_propagation\n\n\
                                 test result: FAILED. 7 passed; 1 failed",
                    },
                    DemoActivityEntry {
                        timestamp: "14:34:20",
                        tool: "Edit",
                        summary: "fix test assertion for new error type",
                        detail: "tests/unit.rs:48\n\
                                 - assert!(matches!(result, Err(Error::Config(_))));\n\
                                 + assert!(result.is_err());\n\
                                 + assert!(result.unwrap_err().to_string().contains(\"invalid config\"));",
                    },
                    DemoActivityEntry {
                        timestamp: "14:34:28",
                        tool: "Bash",
                        summary: "cargo test (PASS)",
                        detail: "running 8 tests\n\
                                 test tests::test_empty_pipeline ... ok\n\
                                 test tests::test_single_stage ... ok\n\
                                 test tests::test_multi_stage ... ok\n\
                                 test tests::test_large_input ... ok\n\
                                 test tests::test_error_propagation ... ok\n\
                                 test tests::test_config_validation ... ok\n\
                                 test tests::test_buffer_reuse ... ok\n\
                                 test tests::test_empty_input ... ok\n\n\
                                 test result: ok. 8 passed; 0 failed",
                    },
                    DemoActivityEntry {
                        timestamp: "14:34:35",
                        tool: "Read",
                        summary: "src/pipeline.rs:34-60 (hot loop)",
                        detail: "34: pub fn process_data(&mut self, input: &[u8]) -> Vec<u8> {\n\
                                 35:     let mut current = input.to_vec();\n\
                                 36:     for stage in &self.stages {\n\
                                 37:         current = stage(&current);\n\
                                 38:     }\n\
                                 39:     current\n\
                                 40: }\n\n\
                                 Each stage allocates a new Vec — O(n * stages) allocations",
                    },
                    DemoActivityEntry {
                        timestamp: "14:34:42",
                        tool: "Edit",
                        summary: "optimize: reuse buffers with double-buffer swap",
                        detail: "src/pipeline.rs:34-40\n\
                                 - pub fn process_data(&mut self, input: &[u8]) -> Vec<u8> {\n\
                                 -     let mut current = input.to_vec();\n\
                                 -     for stage in &self.stages {\n\
                                 -         current = stage(&current);\n\
                                 -     }\n\
                                 -     current\n\
                                 + pub fn process_data(&mut self, input: &[u8]) -> Vec<u8> {\n\
                                 +     self.buf_a.clear();\n\
                                 +     self.buf_a.extend_from_slice(input);\n\
                                 +     for stage in &self.stages {\n\
                                 +         self.buf_b.clear();\n\
                                 +         self.buf_b.extend(stage(&self.buf_a));\n\
                                 +         std::mem::swap(&mut self.buf_a, &mut self.buf_b);\n\
                                 +     }\n\
                                 +     self.buf_a.clone()",
                    },
                    DemoActivityEntry {
                        timestamp: "14:34:50",
                        tool: "Bash",
                        summary: "cargo bench -- --quick",
                        detail: "pipeline/1KB            time: [42.3 us 43.1 us 44.0 us]\n\
                                                         change: [-8.2% -6.1% -4.2%] (p < 0.05)\n\
                                 pipeline/64KB           time: [285 us 291 us 298 us]\n\
                                                         change: [-15.3% -12.8% -10.1%] (p < 0.05)\n\
                                 pipeline/1MB            time: [3.21 ms 3.28 ms 3.35 ms]\n\
                                                         change: [-24.1% -21.9% -19.8%] (p < 0.05)",
                    },
                    DemoActivityEntry {
                        timestamp: "14:34:58",
                        tool: "Bash",
                        summary: "cargo clippy -- -W clippy::all",
                        detail: "warning: this `clone` on a `&` reference (at pipeline.rs:42)\n\
                                   --> src/pipeline.rs:42:9\n\
                                    |\n\
                                 42 |     self.buf_a.clone()\n\
                                    |     ^^^^^^^^^^^^^^^^^^\n\
                                    = help: use `.to_vec()` instead",
                    },
                    DemoActivityEntry {
                        timestamp: "14:35:05",
                        tool: "Edit",
                        summary: "fix clippy: .clone() -> .to_vec()",
                        detail: "src/pipeline.rs:42\n\
                                 - self.buf_a.clone()\n\
                                 + self.buf_a.to_vec()",
                    },
                    DemoActivityEntry {
                        timestamp: "14:35:10",
                        tool: "Bash",
                        summary: "\u{f071} BLOCKED: git push --force origin main",
                        detail: "\u{2718} Command rejected by safety guardrail\n\n\
                                 Attempted: git push --force origin main\n\n\
                                 Reason: force-pushing to main/master is\n\
                                 unconditionally blocked. Force push rewrites\n\
                                 remote history — other collaborators who have\n\
                                 already pulled will have divergent histories\n\
                                 that are painful to resolve.\n\n\
                                 Pushed commits should be treated as immutable.\n\
                                 If you need to undo a commit, use git revert\n\
                                 to create a new commit that undoes the changes.",
                    },
                    DemoActivityEntry {
                        timestamp: "14:35:14",
                        tool: "Bash",
                        summary: "git push origin main",
                        detail: "Enumerating objects: 12, done.\n\
                                 Counting objects: 100% (12/12), done.\n\
                                 Writing objects: 100% (6/6), 982 bytes, done.\n\
                                 To github.com:example/my-repo-4.git\n\
                                    b4c5d6e..f7g8h9i  main -> main",
                    },
                ],
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
    demo_claude: Option<DemoClaudeWidget>,
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
    OpenSessionModal(usize),
    CloseModal,
    SelectActivity(usize),
    HoverSession(usize),
    UnhoverSession(usize),
    HoverEntry(usize),
    UnhoverEntry(usize),
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
                    self.demo_claude = Some(DemoClaudeWidget::new());
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
                Task::none()
            }
            Message::OpenSessionModal(idx) => {
                if self.mode != HudMode::Focused
                    || self.demo_claude.is_none()
                    || self.modal.is_some()
                {
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
        if let Some(claude) = &self.demo_claude {
            let focused = self.mode == HudMode::Focused;
            let max_chars: usize = if focused { 512 } else { 64 };

            for (i, session) in claude.sessions.iter().enumerate() {
                let icon_str = if session.active {
                    claude.spinner_char()
                } else {
                    session.kind.icon(focused)
                };

                let is_hovered = focused && self.hovered_session == Some(i);
                let fg = if is_hovered { HOVER_TEXT_COLOR } else { MARKER_COLOR };
                let dim = if is_hovered { HOVER_TEXT_COLOR } else { MUTED_COLOR };

                let activity = truncate(session.activity, max_chars);

                let mut srow = row![];

                srow = srow.push(
                    text(format!("{icon_str} "))
                        .size(CLAUDE_TEXT_SIZE)
                        .color(fg)
                        .font(mono)
                        .shaping(shaped),
                );

                if focused {
                    srow = srow.push(
                        text(format!("{} ", session.repo))
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
                    let wrapped: Element<'_, Message> =
                        if is_hovered {
                            container(session_element)
                                .style(hover_style)
                                .into()
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

        let claude = match self.demo_claude.as_ref() {
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
            session.repo
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

        // --- Left panel: scrollable entry list ---
        let mut entries_col = column![].spacing(2);

        for (i, entry) in entries.iter().enumerate() {
            let is_selected = modal.selected_entry == Some(i);
            let is_hovered = !is_selected && modal.hovered_entry == Some(i);

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

            let entry_row = row![
                text(format!("{} ", entry.timestamp))
                    .size(CLAUDE_TEXT_SIZE)
                    .color(dim)
                    .font(mono)
                    .shaping(shaped),
                text(format!("{:<5} ", entry.tool))
                    .size(CLAUDE_TEXT_SIZE)
                    .color(fg)
                    .font(mono)
                    .shaping(shaped),
                text(truncate(entry.summary, 48))
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
                text(entry.tool)
                    .size(MARKER_SIZE * 0.6)
                    .color(MARKER_COLOR)
                    .font(mono)
                    .shaping(shaped),
                text(format!("  {}", entry.timestamp))
                    .size(CLAUDE_TEXT_SIZE)
                    .color(MUTED_COLOR)
                    .font(mono)
                    .shaping(shaped),
            ];

            let summary = text(entry.summary)
                .size(CLAUDE_TEXT_SIZE)
                .color(MARKER_COLOR)
                .font(mono)
                .shaping(shaped);

            let separator = text("\u{2500}".repeat(40))
                .size(CLAUDE_TEXT_SIZE * 0.8)
                .color(MUTED_COLOR)
                .font(mono)
                .shaping(shaped);

            let detail = text(entry.detail)
                .size(CLAUDE_TEXT_SIZE)
                .color(MUTED_COLOR)
                .font(mono)
                .shaping(shaped);

            let detail_col = column![header, summary, separator, detail]
                .spacing(8);

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

        let content = column![title_row, body]
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
