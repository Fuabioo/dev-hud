use std::collections::HashMap;
use std::fmt;
use std::fs;
use std::io::{BufRead, BufReader, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::events::SessionEvent;
use crate::watcher::parser::Parser;

/// 30 minutes — sessions modified more recently than this are considered active.
const ACTIVE_THRESHOLD_SECS: u64 = 1800;

/// Errors that can occur during scanning.
#[derive(Debug)]
#[allow(dead_code)]
pub enum ScannerError {
    Io(std::io::Error),
    NoSessions(String),
}

impl fmt::Display for ScannerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ScannerError::Io(e) => write!(f, "I/O error: {e}"),
            ScannerError::NoSessions(msg) => write!(f, "No sessions found: {msg}"),
        }
    }
}

impl From<std::io::Error> for ScannerError {
    fn from(e: std::io::Error) -> Self {
        ScannerError::Io(e)
    }
}

/// Metadata about a discovered session.
#[derive(Debug, Clone)]
pub struct SessionInfo {
    pub path: PathBuf,
    pub session_id: String,
    pub project_slug: String,
    pub modified: SystemTime,
}

/// Tracks byte offset for incremental reading of a single JSONL file.
struct TrackedFile {
    path: PathBuf,
    offset: u64,
}

/// Discovers sessions and incrementally reads JSONL files.
#[allow(dead_code)]
pub struct Scanner {
    parser: Parser,
    /// Main session file.
    main_file: Option<TrackedFile>,
    /// Subagent files keyed by filename.
    subagent_files: HashMap<String, TrackedFile>,
    /// Path to the subagents directory (if any).
    subagents_dir: Option<PathBuf>,
    /// Session ID being watched.
    session_id: String,
    /// Project slug being watched.
    project_slug: String,
}

impl Scanner {
    /// Create a scanner from a SessionInfo (used by multi-session watcher).
    pub fn from_session_info(info: &SessionInfo) -> Result<Self, ScannerError> {
        Self::create_for_session(
            info.path.clone(),
            info.session_id.clone(),
            info.project_slug.clone(),
        )
    }

    fn create_for_session(
        path: PathBuf,
        session_id: String,
        project_slug: String,
    ) -> Result<Self, ScannerError> {
        // Always read from the start so the modal can show full session history.
        // The file size check in read_new_lines guards against re-reading unchanged data.
        let offset = 0;

        // Check for subagents directory
        let parent = path.parent().unwrap_or_else(|| Path::new("."));
        let subagents_dir = parent.join(&session_id).join("subagents");
        let subagents_dir = if subagents_dir.is_dir() {
            Some(subagents_dir)
        } else {
            None
        };

        eprintln!("[scanner] watching session {session_id} in project {project_slug}");

        Ok(Scanner {
            parser: Parser::new(),
            main_file: Some(TrackedFile { path, offset }),
            subagent_files: HashMap::new(),
            subagents_dir,
            session_id,
            project_slug,
        })
    }

    /// Read new lines from all tracked files and return parsed events.
    pub fn poll(&mut self) -> Vec<SessionEvent> {
        let mut events = Vec::new();

        // Read main session file
        if let Some(ref mut tracked) = self.main_file {
            if let Err(e) = read_new_lines(tracked, &mut self.parser, &mut events) {
                eprintln!("[scanner] error reading main file: {e}");
            }
        }

        // Scan for new subagent files
        self.discover_subagent_files();

        // Read all subagent files
        let keys: Vec<String> = self.subagent_files.keys().cloned().collect();
        for key in keys {
            if let Some(tracked) = self.subagent_files.get_mut(&key) {
                if let Err(e) = read_new_lines(tracked, &mut self.parser, &mut events) {
                    eprintln!("[scanner] error reading subagent file {key}: {e}");
                }
            }
        }

        events
    }

    fn discover_subagent_files(&mut self) {
        let subagents_dir = match &self.subagents_dir {
            Some(d) => d.clone(),
            None => {
                // Check if it was created since we started
                if let Some(ref tracked) = self.main_file {
                    let parent = tracked
                        .path
                        .parent()
                        .unwrap_or_else(|| Path::new("."));
                    let candidate = parent.join(&self.session_id).join("subagents");
                    if candidate.is_dir() {
                        self.subagents_dir = Some(candidate.clone());
                        candidate
                    } else {
                        return;
                    }
                } else {
                    return;
                }
            }
        };

        let entries = match fs::read_dir(&subagents_dir) {
            Ok(e) => e,
            Err(_) => return,
        };

        for entry in entries {
            let entry = match entry {
                Ok(e) => e,
                Err(_) => continue,
            };
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
                continue;
            }
            let filename = entry.file_name().to_string_lossy().to_string();
            if self.subagent_files.contains_key(&filename) {
                continue;
            }
            self.subagent_files.insert(
                filename,
                TrackedFile {
                    path,
                    offset: 0, // Always read subagent files from the start
                },
            );
        }
    }

    #[allow(dead_code)]
    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    #[allow(dead_code)]
    pub fn project_slug(&self) -> &str {
        &self.project_slug
    }
}

/// Scan all projects for recently-active sessions (modified within threshold).
pub fn discover_active_sessions(projects_dir: &Path) -> Vec<SessionInfo> {
    let mut sessions = Vec::new();

    let now = match SystemTime::now().duration_since(SystemTime::UNIX_EPOCH) {
        Ok(d) => d,
        Err(_) => return sessions,
    };

    let entries = match fs::read_dir(projects_dir) {
        Ok(e) => e,
        Err(_) => return sessions,
    };

    for entry in entries {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        let project_path = entry.path();
        if !project_path.is_dir() {
            continue;
        }
        let project_slug = entry.file_name().to_string_lossy().to_string();

        let files = match fs::read_dir(&project_path) {
            Ok(f) => f,
            Err(_) => continue,
        };

        for file_entry in files {
            let file_entry = match file_entry {
                Ok(f) => f,
                Err(_) => continue,
            };
            let path = file_entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
                continue;
            }
            let meta = match fs::metadata(&path) {
                Ok(m) => m,
                Err(_) => continue,
            };
            let modified = match meta.modified() {
                Ok(t) => t,
                Err(_) => continue,
            };

            // Check if within activity threshold
            let file_epoch = match modified.duration_since(SystemTime::UNIX_EPOCH) {
                Ok(d) => d,
                Err(_) => continue,
            };
            let age_secs = now.as_secs().saturating_sub(file_epoch.as_secs());
            if age_secs > ACTIVE_THRESHOLD_SECS {
                continue;
            }

            let session_id = match path.file_stem().and_then(|s| s.to_str()) {
                Some(s) => s.to_string(),
                None => continue,
            };

            sessions.push(SessionInfo {
                path,
                session_id,
                project_slug: project_slug.clone(),
                modified,
            });
        }
    }

    // Sort by modification time, most recent first
    sessions.sort_by(|a, b| b.modified.cmp(&a.modified));
    sessions
}

/// Read new lines from a tracked file starting at the stored offset.
fn read_new_lines(
    tracked: &mut TrackedFile,
    parser: &mut Parser,
    events: &mut Vec<SessionEvent>,
) -> Result<(), ScannerError> {
    let file = match fs::File::open(&tracked.path) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(ScannerError::Io(e)),
    };

    let meta = file.metadata()?;
    if meta.len() <= tracked.offset {
        return Ok(()); // No new data
    }

    let mut reader = BufReader::new(file);
    reader.seek(SeekFrom::Start(tracked.offset))?;

    let mut line = String::new();
    loop {
        line.clear();
        let bytes_read = match reader.read_line(&mut line) {
            Ok(n) => n,
            Err(e) => {
                eprintln!("[scanner] read error: {e}");
                break;
            }
        };
        if bytes_read == 0 {
            break; // EOF
        }
        tracked.offset += bytes_read as u64;

        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let parsed = parser.parse_line(trimmed);
        events.extend(parsed);
    }

    Ok(())
}
