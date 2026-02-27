use std::collections::HashMap;

use crate::events::ToolCategory;
use crate::session::*;

pub(crate) fn create_demo_widget() -> ClaudeWidget {
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
            last_event_time: None,
            needs_attention: false,
            subagents: Vec::new(),
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
            last_event_time: None,
            needs_attention: false,
            subagents: Vec::new(),
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
            last_event_time: None,
            needs_attention: false,
            subagents: Vec::new(),
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
            last_event_time: None,
            needs_attention: false,
            subagents: Vec::new(),
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
