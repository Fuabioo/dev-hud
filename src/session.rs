use std::collections::HashMap;
use std::time::{Duration, Instant, SystemTime};

use crate::events::{EventSource, SessionEvent, TaggedEvent, ToolCategory};
use crate::loader::LoaderStyle;
use crate::util::truncate_str;

pub(crate) type IcedId = iced_layershell::reexport::IcedId;

pub(crate) const MAX_VISIBLE_SESSIONS: usize = 6;
pub(crate) const ARCHIVE_GRACE_SECS: u64 = 300; // 5 minutes
const ATTENTION_THRESHOLD_SECS: u64 = 12;
const SUBAGENT_CLEANUP_SECS: u64 = 60;

#[derive(Debug, Clone, Copy)]
pub(crate) enum SessionKind {
    Terminal,
    Code,
    Markdown,
}

impl SessionKind {
    pub(crate) fn icon(self, focused: bool) -> &'static str {
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

pub(crate) struct SubAgent {
    pub(crate) agent_id: String,
    pub(crate) description: String,
    pub(crate) active: bool,
    pub(crate) current_tool: Option<ActiveTool>,
    pub(crate) activity: String,
    pub(crate) last_event_time: Option<Instant>,
    pub(crate) needs_attention: bool,
}

pub(crate) struct Session {
    pub(crate) session_id: String,
    pub(crate) project_slug: String,
    pub(crate) active: bool,
    pub(crate) kind: SessionKind,
    pub(crate) current_tool: Option<ActiveTool>,
    pub(crate) activity: String,
    pub(crate) exited_at: Option<SystemTime>,
    pub(crate) archived: bool,
    pub(crate) last_event_time: Option<Instant>,
    pub(crate) needs_attention: bool,
    pub(crate) subagents: Vec<SubAgent>,
}

pub(crate) struct ActiveTool {
    #[allow(dead_code)]
    pub(crate) tool_name: String,
    pub(crate) tool_use_id: String,
    pub(crate) category: ToolCategory,
    #[allow(dead_code)]
    pub(crate) description: String,
}

#[allow(dead_code)]
pub(crate) struct ActivityEntry {
    pub(crate) timestamp: String,
    pub(crate) tool: String,
    pub(crate) summary: String,
    pub(crate) detail: String,
    pub(crate) is_error: bool,
    pub(crate) category: ToolCategory,
}

pub(crate) struct ModalState {
    pub(crate) surface_id: IcedId,
    pub(crate) session_index: usize,
    pub(crate) selected_entry: Option<usize>,
    pub(crate) hovered_entry: Option<usize>,
}

pub(crate) struct ArchiveModalState {
    pub(crate) surface_id: IcedId,
    pub(crate) selected_session: Option<usize>,
    pub(crate) selected_entry: Option<usize>,
    pub(crate) hovered_session: Option<usize>,
    pub(crate) hovered_entry: Option<usize>,
}

pub(crate) struct ClaudeWidget {
    pub(crate) sessions: Vec<Session>,
    pub(crate) activity_logs: Vec<Vec<ActivityEntry>>,
    pub(crate) spinner_frame: usize,
    pub(crate) session_index_map: HashMap<String, usize>,
}

impl ClaudeWidget {
    pub(crate) fn new() -> Self {
        Self {
            sessions: Vec::new(),
            activity_logs: Vec::new(),
            spinner_frame: 0,
            session_index_map: HashMap::new(),
        }
    }

    pub(crate) fn tick(&mut self) {
        self.spinner_frame = self.spinner_frame.wrapping_add(1);
        let now_sys = SystemTime::now();
        let now = Instant::now();
        for session in &mut self.sessions {
            // Archive grace period
            if let Some(exited_at) = session.exited_at {
                if !session.archived {
                    if let Ok(elapsed) = now_sys.duration_since(exited_at) {
                        if elapsed >= Duration::from_secs(ARCHIVE_GRACE_SECS) {
                            session.archived = true;
                        }
                    }
                }
            }

            // Staleness → needs_attention detection for parent session.
            // Skip if already flagged, no active tool, or Thinking category.
            if !session.needs_attention {
                if let Some(ref tool) = session.current_tool {
                    if tool.category != ToolCategory::Thinking {
                        if let Some(last) = session.last_event_time {
                            if now.duration_since(last).as_secs() >= ATTENTION_THRESHOLD_SECS {
                                session.needs_attention = true;
                            }
                        }
                    }
                }
            }

            // Staleness detection for subagents
            for sub in &mut session.subagents {
                if sub.active && !sub.needs_attention {
                    if let Some(ref tool) = sub.current_tool {
                        if tool.category != ToolCategory::Thinking {
                            if let Some(last) = sub.last_event_time {
                                if now.duration_since(last).as_secs() >= ATTENTION_THRESHOLD_SECS {
                                    sub.needs_attention = true;
                                }
                            }
                        }
                    }
                }
            }

            // Evict inactive subagents after SUBAGENT_CLEANUP_SECS
            session.subagents.retain(|sub| {
                if sub.active || sub.needs_attention {
                    return true;
                }
                match sub.last_event_time {
                    Some(last) => now.duration_since(last).as_secs() < SUBAGENT_CLEANUP_SECS,
                    None => true,
                }
            });
        }
    }

    pub(crate) fn spinner_char(&self) -> &'static str {
        let frames = LoaderStyle::Braille.text_frames();
        frames[(self.spinner_frame / 4) % frames.len()]
    }

    /// Core state machine: process a tagged event from the watcher.
    pub(crate) fn process_event(&mut self, tagged: TaggedEvent) {
        let TaggedEvent {
            session_id,
            event,
            source,
        } = tagged;

        // Route subagent events to their own handler
        if let EventSource::SubAgent { ref agent_id } = source {
            if let Some(&idx) = self.session_index_map.get(&session_id) {
                self.process_subagent_event(idx, agent_id.clone(), event);
            }
            return;
        }

        // Main source events: update parent timestamp + clear attention
        if let Some(&idx) = self.session_index_map.get(&session_id) {
            let session = &mut self.sessions[idx];
            if session.exited_at.is_none() {
                session.last_event_time = Some(Instant::now());
                session.needs_attention = false;
            }
        }

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
                    last_event_time: Some(Instant::now()),
                    needs_attention: false,
                    subagents: Vec::new(),
                });
                self.activity_logs.push(Vec::new());
                self.session_index_map.insert(session_id, idx);
            }
            SessionEvent::UserPrompt { text } => {
                if let Some(&idx) = self.session_index_map.get(&session_id) {
                    let session = &mut self.sessions[idx];
                    if session.exited_at.is_some() {
                        return;
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
                        return;
                    }
                    session.active = true;
                    session.activity = format!("{tool_name}({description})");
                    session.current_tool = Some(ActiveTool {
                        tool_name: tool_name.clone(),
                        tool_use_id: tool_use_id.clone(),
                        category,
                        description: description.clone(),
                    });

                    // AskUserQuestion → immediate needs_attention
                    if category == ToolCategory::Awaiting {
                        session.needs_attention = true;
                    }

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
                    if session
                        .current_tool
                        .as_ref()
                        .is_some_and(|t| t.tool_use_id == tool_use_id)
                    {
                        session.current_tool = None;
                    }
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
                    session.active = false;
                    session.activity = "idle".to_string();
                    session.current_tool = None;
                    // Clean up finished subagents — they won't produce more events
                    session.subagents.retain(|sub| sub.active || sub.needs_attention);
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
                    let session = &mut self.sessions[idx];
                    session.active = true;
                    session.activity = "compacting context...".to_string();
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
            SessionEvent::ToolProgress => {
                // Pure heartbeat — timestamp already updated above
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

    /// Process an event from a subagent file.
    fn process_subagent_event(
        &mut self,
        session_idx: usize,
        agent_id: String,
        event: SessionEvent,
    ) {
        let session = &mut self.sessions[session_idx];

        // Find or create the SubAgent entry
        let sub_idx = session
            .subagents
            .iter()
            .position(|s| s.agent_id == agent_id);
        let sub_idx = match sub_idx {
            Some(i) => i,
            None => {
                session.subagents.push(SubAgent {
                    agent_id: agent_id.clone(),
                    description: String::new(),
                    active: true,
                    current_tool: None,
                    activity: "starting...".to_string(),
                    last_event_time: Some(Instant::now()),
                    needs_attention: false,
                });
                session.subagents.len() - 1
            }
        };

        let sub = &mut session.subagents[sub_idx];
        sub.last_event_time = Some(Instant::now());
        sub.needs_attention = false;

        match event {
            SessionEvent::UserPrompt { text } => {
                if sub.description.is_empty() {
                    sub.description = truncate_str(&text, 60);
                }
                sub.active = true;
                sub.activity = truncate_str(&text, 200);
            }
            SessionEvent::ToolStart {
                tool_name,
                tool_use_id,
                category,
                description,
            } => {
                sub.active = true;
                sub.activity = format!("{tool_name}({description})");
                sub.current_tool = Some(ActiveTool {
                    tool_name,
                    tool_use_id,
                    category,
                    description,
                });
                if category == ToolCategory::Awaiting {
                    sub.needs_attention = true;
                }
            }
            SessionEvent::ToolEnd { tool_use_id, .. } => {
                if sub
                    .current_tool
                    .as_ref()
                    .is_some_and(|t| t.tool_use_id == tool_use_id)
                {
                    sub.current_tool = None;
                }
            }
            SessionEvent::Thinking => {
                sub.active = true;
                sub.activity = "thinking...".to_string();
                sub.current_tool = Some(ActiveTool {
                    tool_name: "thinking".to_string(),
                    tool_use_id: String::new(),
                    category: ToolCategory::Thinking,
                    description: "thinking...".to_string(),
                });
            }
            SessionEvent::TurnComplete { .. } => {
                sub.active = false;
                sub.current_tool = None;
                sub.activity = "done".to_string();
            }
            SessionEvent::ToolProgress => {
                // Pure heartbeat — timestamp already updated above
            }
            _ => {}
        }
    }
}

/// Simple HH:MM:SS formatter for current time (UTC).
pub(crate) fn format_time_now() -> String {
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
