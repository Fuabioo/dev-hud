/// Tool categories for Claude Code sessions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ToolCategory {
    /// Read, Glob, Grep, ListMcpResourcesTool, ReadMcpResourceTool
    Reading,
    /// Edit, Write, NotebookEdit, MultiEdit
    Writing,
    /// Bash
    Running,
    /// Task, TaskCreate, SendMessage, Team*, EnterWorktree
    Spawning,
    /// WebSearch, WebFetch
    Web,
    /// mcp__* tools
    Mcp,
    /// AskUserQuestion
    Awaiting,
    /// Thinking blocks
    Thinking,
    /// Everything else (EnterPlanMode, Skill, TodoRead, etc.)
    Unknown,
}

impl ToolCategory {
    pub fn from_tool_name(name: &str) -> Self {
        match name {
            "Read" | "Glob" | "Grep" | "ListMcpResourcesTool" | "ReadMcpResourceTool"
            | "ToolSearch" => ToolCategory::Reading,
            "Edit" | "Write" | "NotebookEdit" | "MultiEdit" => ToolCategory::Writing,
            "Bash" => ToolCategory::Running,
            "Task" | "TaskCreate" | "TaskUpdate" | "TaskList" | "TaskGet" | "TaskOutput"
            | "TaskStop" | "SendMessage" | "TeamCreate" | "TeamDelete" | "EnterWorktree" => {
                ToolCategory::Spawning
            }
            "WebSearch" | "WebFetch" => ToolCategory::Web,
            "AskUserQuestion" => ToolCategory::Awaiting,
            _ if name.starts_with("mcp__") => ToolCategory::Mcp,
            _ => ToolCategory::Unknown,
        }
    }
}

/// Identifies where an event originated: the main session file or a subagent file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EventSource {
    /// Event from the main session JSONL file.
    Main,
    /// Event from a subagent JSONL file.
    SubAgent { agent_id: String },
}

/// A SessionEvent tagged with the session it belongs to and its source.
#[derive(Debug, Clone)]
pub struct TaggedEvent {
    pub session_id: String,
    pub event: SessionEvent,
    pub source: EventSource,
}

/// Events produced by the JSONL watcher, consumed by the UI state machine.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum SessionEvent {
    /// A new session was detected.
    SessionStart {
        session_id: String,
        project: String,
        timestamp: String,
    },
    /// The user typed a prompt.
    UserPrompt { text: String },
    /// A tool invocation started.
    ToolStart {
        tool_name: String,
        tool_use_id: String,
        category: ToolCategory,
        description: String,
    },
    /// A tool invocation completed.
    ToolEnd {
        tool_use_id: String,
        is_error: bool,
        error_message: Option<String>,
    },
    /// A subagent was spawned (Task tool).
    AgentSpawned {
        agent_id: String,
        description: String,
    },
    /// Context compaction occurred.
    ContextCompaction,
    /// A turn completed with duration info.
    TurnComplete { duration_ms: u64 },
    /// Token usage update.
    TokenUsage {
        input_tokens: u64,
        output_tokens: u64,
        cache_read_tokens: u64,
    },
    /// The assistant is thinking.
    Thinking,
    /// Session ended (no more activity).
    SessionEnd,
    /// A progress heartbeat (tool is actively running).
    ToolProgress,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reading_tools() {
        assert_eq!(ToolCategory::from_tool_name("Read"), ToolCategory::Reading);
        assert_eq!(ToolCategory::from_tool_name("Glob"), ToolCategory::Reading);
        assert_eq!(ToolCategory::from_tool_name("Grep"), ToolCategory::Reading);
        assert_eq!(
            ToolCategory::from_tool_name("ListMcpResourcesTool"),
            ToolCategory::Reading
        );
        assert_eq!(
            ToolCategory::from_tool_name("ReadMcpResourceTool"),
            ToolCategory::Reading
        );
        assert_eq!(
            ToolCategory::from_tool_name("ToolSearch"),
            ToolCategory::Reading
        );
    }

    #[test]
    fn writing_tools() {
        assert_eq!(ToolCategory::from_tool_name("Edit"), ToolCategory::Writing);
        assert_eq!(ToolCategory::from_tool_name("Write"), ToolCategory::Writing);
        assert_eq!(
            ToolCategory::from_tool_name("NotebookEdit"),
            ToolCategory::Writing
        );
        assert_eq!(
            ToolCategory::from_tool_name("MultiEdit"),
            ToolCategory::Writing
        );
    }

    #[test]
    fn running_tools() {
        assert_eq!(ToolCategory::from_tool_name("Bash"), ToolCategory::Running);
    }

    #[test]
    fn spawning_tools() {
        assert_eq!(
            ToolCategory::from_tool_name("Task"),
            ToolCategory::Spawning
        );
        assert_eq!(
            ToolCategory::from_tool_name("TaskCreate"),
            ToolCategory::Spawning
        );
        assert_eq!(
            ToolCategory::from_tool_name("TaskUpdate"),
            ToolCategory::Spawning
        );
        assert_eq!(
            ToolCategory::from_tool_name("TaskList"),
            ToolCategory::Spawning
        );
        assert_eq!(
            ToolCategory::from_tool_name("TaskGet"),
            ToolCategory::Spawning
        );
        assert_eq!(
            ToolCategory::from_tool_name("TaskOutput"),
            ToolCategory::Spawning
        );
        assert_eq!(
            ToolCategory::from_tool_name("TaskStop"),
            ToolCategory::Spawning
        );
        assert_eq!(
            ToolCategory::from_tool_name("SendMessage"),
            ToolCategory::Spawning
        );
        assert_eq!(
            ToolCategory::from_tool_name("TeamCreate"),
            ToolCategory::Spawning
        );
        assert_eq!(
            ToolCategory::from_tool_name("TeamDelete"),
            ToolCategory::Spawning
        );
        assert_eq!(
            ToolCategory::from_tool_name("EnterWorktree"),
            ToolCategory::Spawning
        );
    }

    #[test]
    fn web_tools() {
        assert_eq!(
            ToolCategory::from_tool_name("WebSearch"),
            ToolCategory::Web
        );
        assert_eq!(
            ToolCategory::from_tool_name("WebFetch"),
            ToolCategory::Web
        );
    }

    #[test]
    fn awaiting_tools() {
        assert_eq!(
            ToolCategory::from_tool_name("AskUserQuestion"),
            ToolCategory::Awaiting
        );
    }

    #[test]
    fn mcp_tools() {
        assert_eq!(
            ToolCategory::from_tool_name("mcp__db__query"),
            ToolCategory::Mcp
        );
        assert_eq!(
            ToolCategory::from_tool_name("mcp__excel__read"),
            ToolCategory::Mcp
        );
        assert_eq!(
            ToolCategory::from_tool_name("mcp__confluence__get_page"),
            ToolCategory::Mcp
        );
    }

    #[test]
    fn unknown_tools() {
        assert_eq!(
            ToolCategory::from_tool_name("SomethingNew"),
            ToolCategory::Unknown
        );
        assert_eq!(ToolCategory::from_tool_name(""), ToolCategory::Unknown);
        assert_eq!(
            ToolCategory::from_tool_name("EnterPlanMode"),
            ToolCategory::Unknown
        );
        assert_eq!(
            ToolCategory::from_tool_name("Skill"),
            ToolCategory::Unknown
        );
    }

    #[test]
    fn event_source_main() {
        let source = EventSource::Main;
        assert_eq!(source, EventSource::Main);
    }

    #[test]
    fn event_source_subagent() {
        let source = EventSource::SubAgent {
            agent_id: "agent-abc123".to_string(),
        };
        match source {
            EventSource::SubAgent { ref agent_id } => {
                assert_eq!(agent_id, "agent-abc123");
            }
            _ => panic!("expected SubAgent"),
        }
    }
}
