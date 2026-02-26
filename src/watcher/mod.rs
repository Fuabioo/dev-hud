pub mod parser;
pub mod scanner;

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, SystemTime};

use crate::events::{EventSource, SessionEvent, TaggedEvent};
use crate::watcher::scanner::{discover_active_sessions, Scanner};

const POLL_INTERVAL_MS: u64 = 500;
const RESCAN_INTERVAL_POLLS: u32 = 10; // ~5s

/// Format a SystemTime as HH:MM:SS (local time via elapsed from midnight).
fn format_time_hms(time: SystemTime) -> String {
    match time.duration_since(SystemTime::UNIX_EPOCH) {
        Ok(d) => {
            // Get local timezone offset from /etc/localtime would be complex;
            // use a simpler approach: compute from epoch with UTC offset.
            // For simplicity, we'll just format UTC. If the user wants local
            // time we'd need chrono, but the plan says no chrono.
            let total_secs = d.as_secs();
            let hours = (total_secs / 3600) % 24;
            let minutes = (total_secs / 60) % 60;
            let seconds = total_secs % 60;
            format!("{hours:02}:{minutes:02}:{seconds:02}")
        }
        Err(_) => "00:00:00".to_string(),
    }
}

/// Handle to the background multi-session watcher thread.
pub struct MultiWatcherHandle {
    receiver: mpsc::Receiver<TaggedEvent>,
}

impl MultiWatcherHandle {
    /// Spawn a watcher that monitors all active sessions under `projects_dir`.
    pub fn spawn(projects_dir: PathBuf) -> Result<Self, String> {
        let (tx, rx) = mpsc::channel();

        if !projects_dir.exists() {
            return Err(format!(
                "Projects dir not found: {}",
                projects_dir.display()
            ));
        }

        let initial_sessions = discover_active_sessions(&projects_dir);
        eprintln!(
            "[watcher] discovered {} active session(s)",
            initial_sessions.len()
        );

        // Build initial scanners
        struct ScannerSlot {
            session_id: String,
            scanner: Scanner,
        }

        let mut slots: Vec<ScannerSlot> = Vec::new();
        let mut known_ids: HashSet<String> = HashSet::new();

        let now_str = format_time_hms(SystemTime::now());

        for info in &initial_sessions {
            match Scanner::from_session_info(info) {
                Ok(scanner) => {
                    known_ids.insert(info.session_id.clone());
                    // Send initial SessionStart (always from Main source)
                    if let Err(e) = tx.send(TaggedEvent {
                        session_id: info.session_id.clone(),
                        event: SessionEvent::SessionStart {
                            session_id: info.session_id.clone(),
                            project: info.project_slug.clone(),
                            timestamp: now_str.clone(),
                        },
                        source: EventSource::Main,
                    }) {
                        return Err(format!("Failed to send initial event: {e}"));
                    }
                    slots.push(ScannerSlot {
                        session_id: info.session_id.clone(),
                        scanner,
                    });
                }
                Err(e) => {
                    eprintln!(
                        "[watcher] skipping session {}: {e}",
                        info.session_id
                    );
                }
            }
        }

        thread::spawn(move || {
            let poll_interval = Duration::from_millis(POLL_INTERVAL_MS);
            let mut poll_count: u32 = 0;

            loop {
                // Poll all existing scanners
                for slot in &mut slots {
                    let sourced_events = slot.scanner.poll();
                    for sourced in sourced_events {
                        if tx
                            .send(TaggedEvent {
                                session_id: slot.session_id.clone(),
                                event: sourced.event,
                                source: sourced.source,
                            })
                            .is_err()
                        {
                            return;
                        }
                    }
                }

                // Periodic re-scan for new sessions
                poll_count += 1;
                if poll_count % RESCAN_INTERVAL_POLLS == 0 {
                    let fresh = discover_active_sessions(&projects_dir);
                    for info in &fresh {
                        if known_ids.contains(&info.session_id) {
                            continue;
                        }
                        match Scanner::from_session_info(info) {
                            Ok(scanner) => {
                                eprintln!(
                                    "[watcher] new session discovered: {} in {}",
                                    info.session_id, info.project_slug
                                );
                                known_ids.insert(info.session_id.clone());
                                let ts = format_time_hms(SystemTime::now());
                                if tx
                                    .send(TaggedEvent {
                                        session_id: info.session_id.clone(),
                                        event: SessionEvent::SessionStart {
                                            session_id: info.session_id.clone(),
                                            project: info.project_slug.clone(),
                                            timestamp: ts,
                                        },
                                        source: EventSource::Main,
                                    })
                                    .is_err()
                                {
                                    return;
                                }
                                slots.push(ScannerSlot {
                                    session_id: info.session_id.clone(),
                                    scanner,
                                });
                            }
                            Err(e) => {
                                eprintln!(
                                    "[watcher] failed to watch new session {}: {e}",
                                    info.session_id
                                );
                            }
                        }
                    }
                }

                thread::sleep(poll_interval);
            }
        });

        Ok(MultiWatcherHandle { receiver: rx })
    }

    /// Drain all pending tagged events from the channel (non-blocking).
    pub fn drain_events(&self) -> Vec<TaggedEvent> {
        let mut events = Vec::new();
        loop {
            match self.receiver.try_recv() {
                Ok(event) => events.push(event),
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => break,
            }
        }
        events
    }
}
