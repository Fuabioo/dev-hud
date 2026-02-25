use crate::events::{SessionEvent, ToolCategory};
use crate::util::truncate_str;
use serde_json::Value;
use std::collections::HashSet;

/// Tracks seen IDs to deduplicate streaming chunks.
pub struct Parser {
    seen_message_ids: HashSet<String>,
    seen_tool_use_ids: HashSet<String>,
}

impl Parser {
    pub fn new() -> Self {
        Parser {
            seen_message_ids: HashSet::new(),
            seen_tool_use_ids: HashSet::new(),
        }
    }

    /// Parse a single JSONL line into zero or more SessionEvents.
    pub fn parse_line(&mut self, line: &str) -> Vec<SessionEvent> {
        let mut events = Vec::new();

        let entry: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("[parser] invalid JSON: {e}");
                return events;
            }
        };

        let entry_type = entry.get("type").and_then(|v| v.as_str()).unwrap_or("");

        match entry_type {
            "user" => self.parse_user_entry(&entry, &mut events),
            "assistant" => self.parse_assistant_entry(&entry, &mut events),
            "system" => self.parse_system_entry(&entry, &mut events),
            "file-history-snapshot" | "progress" | "queue-operation" => {
                // Ignored entry types
            }
            _ => {
                // Check if it has a message with role (some entries have type at top level
                // but the real content is in .message)
                if let Some(role) = entry
                    .get("message")
                    .and_then(|m| m.get("role"))
                    .and_then(|r| r.as_str())
                {
                    match role {
                        "user" => self.parse_user_entry(&entry, &mut events),
                        "assistant" => self.parse_assistant_entry(&entry, &mut events),
                        _ => {}
                    }
                }
            }
        }

        events
    }

    fn parse_user_entry(&mut self, entry: &Value, events: &mut Vec<SessionEvent>) {
        let message = match entry.get("message") {
            Some(m) => m,
            None => return,
        };
        let content = match message.get("content") {
            Some(c) => c,
            None => return,
        };

        // Plain string = user prompt (full text preserved for overlay display)
        if let Some(text) = content.as_str() {
            events.push(SessionEvent::UserPrompt {
                text: text.to_string(),
            });
            return;
        }

        // Array of content blocks — look for tool_result
        if let Some(blocks) = content.as_array() {
            for block in blocks {
                if block.get("type").and_then(|t| t.as_str()) == Some("tool_result") {
                    let tool_use_id = block
                        .get("tool_use_id")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let is_error = block
                        .get("is_error")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false);
                    let error_message = if is_error {
                        extract_error_content(block)
                    } else {
                        None
                    };
                    if !tool_use_id.is_empty() {
                        events.push(SessionEvent::ToolEnd {
                            tool_use_id,
                            is_error,
                            error_message,
                        });
                    }
                }
            }
        }
    }

    fn parse_assistant_entry(&mut self, entry: &Value, events: &mut Vec<SessionEvent>) {
        let message = match entry.get("message") {
            Some(m) => m,
            None => return,
        };

        // Extract message.id for deduplication
        let msg_id = message
            .get("id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        // Extract token usage (only once per message.id)
        if !msg_id.is_empty() {
            if let Some(usage) = message.get("usage") {
                let input = usage
                    .get("input_tokens")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0);
                let output = usage
                    .get("output_tokens")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0);
                let cache_read = usage
                    .get("cache_read_input_tokens")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0);

                if self.seen_message_ids.insert(msg_id.clone()) && (input > 0 || output > 0) {
                    events.push(SessionEvent::TokenUsage {
                        input_tokens: input,
                        output_tokens: output,
                        cache_read_tokens: cache_read,
                    });
                }
            }
        }

        // Extract content blocks
        let content = match message.get("content").and_then(|c| c.as_array()) {
            Some(c) => c,
            None => return,
        };

        for block in content {
            let block_type = match block.get("type").and_then(|t| t.as_str()) {
                Some(t) => t,
                None => continue,
            };

            match block_type {
                "thinking" => {
                    events.push(SessionEvent::Thinking);
                }
                "tool_use" => {
                    let tool_id = block
                        .get("id")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();

                    // Deduplicate streaming tool_use blocks
                    if tool_id.is_empty() || !self.seen_tool_use_ids.insert(tool_id.clone()) {
                        continue;
                    }

                    let tool_name = block
                        .get("name")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown")
                        .to_string();

                    let category = ToolCategory::from_tool_name(&tool_name);
                    let description = extract_tool_description(
                        &tool_name,
                        block.get("input").unwrap_or(&Value::Null),
                    );

                    // Check if this is a Task/TaskCreate tool (agent spawned)
                    if tool_name == "Task" || tool_name == "TaskCreate" {
                        let agent_desc = block
                            .get("input")
                            .and_then(|inp| inp.get("description"))
                            .and_then(|d| d.as_str())
                            .unwrap_or("unnamed agent")
                            .to_string();
                        events.push(SessionEvent::AgentSpawned {
                            agent_id: tool_id.clone(),
                            description: agent_desc,
                        });
                    }

                    events.push(SessionEvent::ToolStart {
                        tool_name,
                        tool_use_id: tool_id,
                        category,
                        description,
                    });
                }
                _ => {}
            }
        }
    }

    fn parse_system_entry(&self, entry: &Value, events: &mut Vec<SessionEvent>) {
        let subtype = match entry.get("subtype").and_then(|v| v.as_str()) {
            Some(s) => s,
            None => return,
        };

        match subtype {
            "turn_duration" => {
                if let Some(ms) = entry.get("durationMs").and_then(|v| v.as_u64()) {
                    events.push(SessionEvent::TurnComplete { duration_ms: ms });
                }
            }
            "compact_boundary" => {
                events.push(SessionEvent::ContextCompaction);
            }
            _ => {}
        }
    }
}

/// Extract a human-readable description from tool input.
fn extract_tool_description(tool_name: &str, input: &Value) -> String {
    match tool_name {
        "Read" => input
            .get("file_path")
            .and_then(|v| v.as_str())
            .map(shorten_path)
            .unwrap_or_else(|| "reading file".to_string()),
        "Edit" => input
            .get("file_path")
            .and_then(|v| v.as_str())
            .map(shorten_path)
            .unwrap_or_else(|| "editing file".to_string()),
        "Write" => input
            .get("file_path")
            .and_then(|v| v.as_str())
            .map(shorten_path)
            .unwrap_or_else(|| "writing file".to_string()),
        "NotebookEdit" | "MultiEdit" => input
            .get("file_path")
            .or_else(|| input.get("notebook_path"))
            .and_then(|v| v.as_str())
            .map(shorten_path)
            .unwrap_or_else(|| "editing file".to_string()),
        "AskUserQuestion" => "asking user".to_string(),
        "EnterPlanMode" | "ExitPlanMode" => "planning".to_string(),
        "Skill" => input
            .get("skill")
            .and_then(|v| v.as_str())
            .map(|s| format!("/{s}"))
            .unwrap_or_else(|| "skill".to_string()),
        "Glob" => input
            .get("pattern")
            .and_then(|v| v.as_str())
            .map(|s| truncate_str(s, 200))
            .unwrap_or_else(|| "glob search".to_string()),
        "Grep" => input
            .get("pattern")
            .and_then(|v| v.as_str())
            .map(|s| truncate_str(s, 200))
            .unwrap_or_else(|| "grep search".to_string()),
        "Bash" => input
            .get("command")
            .and_then(|v| v.as_str())
            .map(|s| truncate_str(s, 500))
            .unwrap_or_else(|| "running command".to_string()),
        "Task" | "TaskCreate" => input
            .get("description")
            .and_then(|v| v.as_str())
            .map(|s| truncate_str(s, 200))
            .unwrap_or_else(|| "spawning agent".to_string()),
        "WebSearch" => input
            .get("query")
            .and_then(|v| v.as_str())
            .map(|s| truncate_str(s, 200))
            .unwrap_or_else(|| "web search".to_string()),
        "WebFetch" => input
            .get("url")
            .and_then(|v| v.as_str())
            .map(|s| truncate_str(s, 300))
            .unwrap_or_else(|| "fetching URL".to_string()),
        _ => truncate_str(tool_name, 80),
    }
}

/// Extract error message text from a tool_result content block.
fn extract_error_content(block: &Value) -> Option<String> {
    let content = block.get("content")?;
    // Content can be a plain string
    if let Some(text) = content.as_str() {
        if !text.is_empty() {
            return Some(truncate_str(text, 500));
        }
    }
    // Or an array of content blocks with {type: "text", text: "..."}
    if let Some(blocks) = content.as_array() {
        for b in blocks {
            if let Some(text) = b.get("text").and_then(|t| t.as_str()) {
                if !text.is_empty() {
                    return Some(truncate_str(text, 500));
                }
            }
        }
    }
    None
}

fn shorten_path(path: &str) -> String {
    // Show just the filename or last two components
    let parts: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    if parts.len() <= 2 {
        path.to_string()
    } else {
        format!(".../{}", parts[parts.len() - 2..].join("/"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::SessionEvent;

    fn make_parser() -> Parser {
        Parser::new()
    }

    // -----------------------------------------------------------------------
    // shorten_path
    // -----------------------------------------------------------------------

    #[test]
    fn shorten_path_short_unchanged() {
        assert_eq!(shorten_path("src/main.rs"), "src/main.rs");
    }

    #[test]
    fn shorten_path_long_trimmed() {
        assert_eq!(
            shorten_path("/home/user/projects/my-app/src/main.rs"),
            ".../src/main.rs"
        );
    }

    #[test]
    fn shorten_path_single_component() {
        assert_eq!(shorten_path("main.rs"), "main.rs");
    }

    // -----------------------------------------------------------------------
    // extract_tool_description
    // -----------------------------------------------------------------------

    #[test]
    fn extract_description_notebook_edit_with_notebook_path() {
        let input: Value = serde_json::json!({"notebook_path": "/home/user/analysis.ipynb"});
        let desc = extract_tool_description("NotebookEdit", &input);
        assert!(desc.contains("analysis.ipynb"));
    }

    #[test]
    fn extract_description_ask_user_question() {
        let desc = extract_tool_description("AskUserQuestion", &Value::Null);
        assert_eq!(desc, "asking user");
    }

    #[test]
    fn extract_description_skill_with_name() {
        let input: Value = serde_json::json!({"skill": "commit"});
        let desc = extract_tool_description("Skill", &input);
        assert_eq!(desc, "/commit");
    }

    #[test]
    fn extract_description_skill_without_name() {
        let desc = extract_tool_description("Skill", &Value::Null);
        assert_eq!(desc, "skill");
    }

    #[test]
    fn extract_description_enter_plan_mode() {
        let desc = extract_tool_description("EnterPlanMode", &Value::Null);
        assert_eq!(desc, "planning");
    }

    #[test]
    fn extract_description_task_create_with_description() {
        let input: Value =
            serde_json::json!({"subject": "Fix bug", "description": "investigate login issue"});
        let desc = extract_tool_description("TaskCreate", &input);
        assert_eq!(desc, "investigate login issue");
    }

    #[test]
    fn extract_description_task_create_without_description() {
        let desc = extract_tool_description("TaskCreate", &Value::Null);
        assert_eq!(desc, "spawning agent");
    }

    // -----------------------------------------------------------------------
    // User prompt parsing
    // -----------------------------------------------------------------------

    #[test]
    fn parse_user_prompt_simple() {
        let mut parser = make_parser();
        let line = r#"{"type":"user","message":{"role":"user","content":"hello world"}}"#;
        let events = parser.parse_line(line);
        assert_eq!(events.len(), 1);
        match &events[0] {
            SessionEvent::UserPrompt { text } => {
                assert_eq!(text, "hello world");
            }
            other => panic!("expected UserPrompt, got {:?}", other),
        }
    }

    #[test]
    fn parse_user_prompt_preserves_full_text() {
        let mut parser = make_parser();
        let long_text = "a".repeat(200);
        let line = format!(
            r#"{{"type":"user","message":{{"role":"user","content":"{}"}}}}"#,
            long_text
        );
        let events = parser.parse_line(&line);
        assert_eq!(events.len(), 1);
        match &events[0] {
            SessionEvent::UserPrompt { text } => {
                assert_eq!(text.len(), 200);
                assert!(!text.ends_with("..."));
            }
            other => panic!("expected UserPrompt, got {:?}", other),
        }
    }

    // -----------------------------------------------------------------------
    // Tool result (ToolEnd)
    // -----------------------------------------------------------------------

    #[test]
    fn parse_tool_result_success() {
        let mut parser = make_parser();
        let line = r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_123","is_error":false}]}}"#;
        let events = parser.parse_line(line);
        assert_eq!(events.len(), 1);
        match &events[0] {
            SessionEvent::ToolEnd {
                tool_use_id,
                is_error,
                error_message,
            } => {
                assert_eq!(tool_use_id, "toolu_123");
                assert!(!is_error);
                assert!(error_message.is_none());
            }
            other => panic!("expected ToolEnd, got {:?}", other),
        }
    }

    #[test]
    fn parse_tool_result_error() {
        let mut parser = make_parser();
        let line = r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_456","is_error":true,"content":"EISDIR: illegal operation on a directory"}]}}"#;
        let events = parser.parse_line(line);
        assert_eq!(events.len(), 1);
        match &events[0] {
            SessionEvent::ToolEnd {
                tool_use_id,
                is_error,
                error_message,
            } => {
                assert_eq!(tool_use_id, "toolu_456");
                assert!(is_error);
                assert!(error_message.is_some());
                assert!(error_message.as_ref().unwrap().contains("EISDIR"));
            }
            other => panic!("expected ToolEnd, got {:?}", other),
        }
    }

    #[test]
    fn parse_tool_result_error_array_content() {
        let mut parser = make_parser();
        let line = r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_789","is_error":true,"content":[{"type":"text","text":"Command failed: exit code 1"}]}]}}"#;
        let events = parser.parse_line(line);
        assert_eq!(events.len(), 1);
        match &events[0] {
            SessionEvent::ToolEnd {
                is_error,
                error_message,
                ..
            } => {
                assert!(is_error);
                assert!(error_message.as_ref().unwrap().contains("exit code 1"));
            }
            other => panic!("expected ToolEnd, got {:?}", other),
        }
    }

    #[test]
    fn parse_tool_result_error_no_content() {
        let mut parser = make_parser();
        let line = r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_abc","is_error":true}]}}"#;
        let events = parser.parse_line(line);
        assert_eq!(events.len(), 1);
        match &events[0] {
            SessionEvent::ToolEnd {
                is_error,
                error_message,
                ..
            } => {
                assert!(is_error);
                assert!(error_message.is_none());
            }
            other => panic!("expected ToolEnd, got {:?}", other),
        }
    }

    // -----------------------------------------------------------------------
    // extract_error_content
    // -----------------------------------------------------------------------

    #[test]
    fn extract_error_string_content() {
        let block: Value = serde_json::json!({
            "type": "tool_result",
            "tool_use_id": "t1",
            "is_error": true,
            "content": "file not found"
        });
        assert_eq!(
            extract_error_content(&block),
            Some("file not found".to_string())
        );
    }

    #[test]
    fn extract_error_array_content() {
        let block: Value = serde_json::json!({
            "type": "tool_result",
            "tool_use_id": "t2",
            "is_error": true,
            "content": [{"type": "text", "text": "permission denied"}]
        });
        assert_eq!(
            extract_error_content(&block),
            Some("permission denied".to_string())
        );
    }

    #[test]
    fn extract_error_empty_content() {
        let block: Value = serde_json::json!({
            "type": "tool_result",
            "tool_use_id": "t3",
            "is_error": true,
            "content": ""
        });
        assert_eq!(extract_error_content(&block), None);
    }

    #[test]
    fn extract_error_no_content_field() {
        let block: Value = serde_json::json!({
            "type": "tool_result",
            "tool_use_id": "t4",
            "is_error": true
        });
        assert_eq!(extract_error_content(&block), None);
    }

    #[test]
    fn extract_error_truncates_long_message() {
        let long_msg = "x".repeat(600);
        let block: Value = serde_json::json!({
            "type": "tool_result",
            "tool_use_id": "t5",
            "is_error": true,
            "content": long_msg
        });
        let result = extract_error_content(&block).unwrap();
        assert!(result.len() <= 503); // 500 + "..."
        assert!(result.ends_with("..."));
    }

    // -----------------------------------------------------------------------
    // Tool use (ToolStart)
    // -----------------------------------------------------------------------

    #[test]
    fn parse_tool_use_read() {
        let mut parser = make_parser();
        let line = r#"{"type":"assistant","message":{"id":"msg_001","role":"assistant","content":[{"type":"tool_use","id":"toolu_read1","name":"Read","input":{"file_path":"/home/user/src/main.rs"}}],"usage":{"input_tokens":100,"output_tokens":50}}}"#;
        let events = parser.parse_line(line);

        let tool_starts: Vec<_> = events
            .iter()
            .filter(|e| matches!(e, SessionEvent::ToolStart { .. }))
            .collect();
        assert_eq!(tool_starts.len(), 1);

        match tool_starts[0] {
            SessionEvent::ToolStart {
                tool_name,
                category,
                description,
                ..
            } => {
                assert_eq!(tool_name, "Read");
                assert_eq!(*category, ToolCategory::Reading);
                assert!(description.contains("main.rs"));
            }
            _ => unreachable!(),
        }
    }

    #[test]
    fn parse_tool_use_bash() {
        let mut parser = make_parser();
        let line = r#"{"type":"assistant","message":{"id":"msg_002","role":"assistant","content":[{"type":"tool_use","id":"toolu_bash1","name":"Bash","input":{"command":"cargo build"}}],"usage":{"input_tokens":100,"output_tokens":50}}}"#;
        let events = parser.parse_line(line);

        let tool_starts: Vec<_> = events
            .iter()
            .filter(|e| matches!(e, SessionEvent::ToolStart { .. }))
            .collect();
        assert_eq!(tool_starts.len(), 1);

        match tool_starts[0] {
            SessionEvent::ToolStart {
                tool_name,
                category,
                description,
                ..
            } => {
                assert_eq!(tool_name, "Bash");
                assert_eq!(*category, ToolCategory::Running);
                assert_eq!(description, "cargo build");
            }
            _ => unreachable!(),
        }
    }

    // -----------------------------------------------------------------------
    // Deduplication
    // -----------------------------------------------------------------------

    #[test]
    fn deduplicate_tool_use_ids() {
        let mut parser = make_parser();
        let line = r#"{"type":"assistant","message":{"id":"msg_003","role":"assistant","content":[{"type":"tool_use","id":"toolu_dup","name":"Read","input":{"file_path":"a.rs"}}],"usage":{"input_tokens":10,"output_tokens":5}}}"#;

        let events1 = parser.parse_line(line);
        let tool_starts1: Vec<_> = events1
            .iter()
            .filter(|e| matches!(e, SessionEvent::ToolStart { .. }))
            .collect();
        assert_eq!(tool_starts1.len(), 1);

        // Same line again — tool_use should be deduplicated
        let events2 = parser.parse_line(line);
        let tool_starts2: Vec<_> = events2
            .iter()
            .filter(|e| matches!(e, SessionEvent::ToolStart { .. }))
            .collect();
        assert_eq!(tool_starts2.len(), 0);
    }

    #[test]
    fn deduplicate_message_ids_for_tokens() {
        let mut parser = make_parser();
        let line = r#"{"type":"assistant","message":{"id":"msg_004","role":"assistant","content":[],"usage":{"input_tokens":1000,"output_tokens":500}}}"#;

        let events1 = parser.parse_line(line);
        let token_events1: Vec<_> = events1
            .iter()
            .filter(|e| matches!(e, SessionEvent::TokenUsage { .. }))
            .collect();
        assert_eq!(token_events1.len(), 1);

        // Same message ID — no new token event
        let events2 = parser.parse_line(line);
        let token_events2: Vec<_> = events2
            .iter()
            .filter(|e| matches!(e, SessionEvent::TokenUsage { .. }))
            .collect();
        assert_eq!(token_events2.len(), 0);
    }

    // -----------------------------------------------------------------------
    // Agent spawning (Task tool)
    // -----------------------------------------------------------------------

    #[test]
    fn parse_task_tool_spawns_agent() {
        let mut parser = make_parser();
        let line = r#"{"type":"assistant","message":{"id":"msg_005","role":"assistant","content":[{"type":"tool_use","id":"toolu_task1","name":"Task","input":{"description":"research codebase","prompt":"find all uses"}}],"usage":{"input_tokens":10,"output_tokens":5}}}"#;

        let events = parser.parse_line(line);

        let agents: Vec<_> = events
            .iter()
            .filter(|e| matches!(e, SessionEvent::AgentSpawned { .. }))
            .collect();
        assert_eq!(agents.len(), 1);

        match agents[0] {
            SessionEvent::AgentSpawned { description, .. } => {
                assert_eq!(description, "research codebase");
            }
            _ => unreachable!(),
        }

        // Should also produce a ToolStart
        let tool_starts: Vec<_> = events
            .iter()
            .filter(|e| matches!(e, SessionEvent::ToolStart { .. }))
            .collect();
        assert_eq!(tool_starts.len(), 1);
    }

    #[test]
    fn parse_task_create_spawns_agent() {
        let mut parser = make_parser();
        let line = r#"{"type":"assistant","message":{"id":"msg_008","role":"assistant","content":[{"type":"tool_use","id":"toolu_tc1","name":"TaskCreate","input":{"subject":"Fix bug","description":"investigate the login issue"}}],"usage":{"input_tokens":10,"output_tokens":5}}}"#;

        let events = parser.parse_line(line);

        let agents: Vec<_> = events
            .iter()
            .filter(|e| matches!(e, SessionEvent::AgentSpawned { .. }))
            .collect();
        assert_eq!(agents.len(), 1);

        match agents[0] {
            SessionEvent::AgentSpawned { description, .. } => {
                assert_eq!(description, "investigate the login issue");
            }
            _ => unreachable!(),
        }
    }

    // -----------------------------------------------------------------------
    // System entries
    // -----------------------------------------------------------------------

    #[test]
    fn parse_turn_complete() {
        let mut parser = make_parser();
        let line = r#"{"type":"system","subtype":"turn_duration","durationMs":5432}"#;
        let events = parser.parse_line(line);
        assert_eq!(events.len(), 1);
        match &events[0] {
            SessionEvent::TurnComplete { duration_ms } => {
                assert_eq!(*duration_ms, 5432);
            }
            other => panic!("expected TurnComplete, got {:?}", other),
        }
    }

    #[test]
    fn parse_context_compaction() {
        let mut parser = make_parser();
        let line = r#"{"type":"system","subtype":"compact_boundary"}"#;
        let events = parser.parse_line(line);
        assert_eq!(events.len(), 1);
        assert!(matches!(events[0], SessionEvent::ContextCompaction));
    }

    // -----------------------------------------------------------------------
    // Thinking
    // -----------------------------------------------------------------------

    #[test]
    fn parse_thinking_event() {
        let mut parser = make_parser();
        let line = r#"{"type":"assistant","message":{"id":"msg_006","role":"assistant","content":[{"type":"thinking","thinking":"analyzing code"}],"usage":{"input_tokens":50,"output_tokens":10}}}"#;
        let events = parser.parse_line(line);

        let thinking: Vec<_> = events
            .iter()
            .filter(|e| matches!(e, SessionEvent::Thinking))
            .collect();
        assert_eq!(thinking.len(), 1);
    }

    // -----------------------------------------------------------------------
    // Edge cases
    // -----------------------------------------------------------------------

    #[test]
    fn parse_invalid_json_no_panic() {
        let mut parser = make_parser();
        let events = parser.parse_line("not valid json{{{");
        assert!(events.is_empty());
    }

    #[test]
    fn parse_empty_line_no_panic() {
        let mut parser = make_parser();
        let events = parser.parse_line("");
        assert!(events.is_empty());
    }

    #[test]
    fn parse_ignored_entry_types() {
        let mut parser = make_parser();
        let events = parser.parse_line(r#"{"type":"file-history-snapshot"}"#);
        assert!(events.is_empty());
        let events = parser.parse_line(r#"{"type":"progress"}"#);
        assert!(events.is_empty());
        let events = parser.parse_line(r#"{"type":"queue-operation"}"#);
        assert!(events.is_empty());
    }

    #[test]
    fn parse_missing_content_no_panic() {
        let mut parser = make_parser();
        let line = r#"{"type":"user","message":{"role":"user"}}"#;
        let events = parser.parse_line(line);
        assert!(events.is_empty());
    }

    #[test]
    fn parse_missing_message_no_panic() {
        let mut parser = make_parser();
        let line = r#"{"type":"user"}"#;
        let events = parser.parse_line(line);
        assert!(events.is_empty());
    }

    #[test]
    fn parse_tool_result_empty_id_skipped() {
        let mut parser = make_parser();
        let line = r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"","is_error":false}]}}"#;
        let events = parser.parse_line(line);
        assert!(events.is_empty());
    }

    #[test]
    fn parse_mcp_tool_categorized() {
        let mut parser = make_parser();
        let line = r#"{"type":"assistant","message":{"id":"msg_007","role":"assistant","content":[{"type":"tool_use","id":"toolu_mcp1","name":"mcp__db__query","input":{}}],"usage":{"input_tokens":10,"output_tokens":5}}}"#;
        let events = parser.parse_line(line);

        let tool_starts: Vec<_> = events
            .iter()
            .filter(|e| matches!(e, SessionEvent::ToolStart { .. }))
            .collect();
        assert_eq!(tool_starts.len(), 1);
        match tool_starts[0] {
            SessionEvent::ToolStart { category, .. } => {
                assert_eq!(*category, ToolCategory::Mcp);
            }
            _ => unreachable!(),
        }
    }
}
