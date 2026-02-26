pub mod config;

use std::collections::VecDeque;
use std::io::BufRead;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant, SystemTime};

use config::{ShellConfig, ShellMode};

/// Maximum lines kept in the ring buffer per instance.
const MAX_BUFFER_LINES: usize = 256;

/// How quickly we poll for new output (ms).
const POLL_INTERVAL_MS: u64 = 50;

/// Config file mtime check interval (polls).
const CONFIG_CHECK_POLLS: u64 = 40; // ~2s at 50ms

/// If mode is auto-detect and process exits within this duration, treat as oneshot.
const ONESHOT_DETECT_SECS: u64 = 3;

/// A running shell widget instance.
pub struct ShellInstance {
    pub config: ShellConfig,
    pub buffer: VecDeque<String>,
    pub exit_code: Option<i32>,
    pub last_update: SystemTime,
    pub error: Option<String>,
    /// Resolved mode (after auto-detection).
    pub resolved_mode: ShellMode,
    /// When the process was spawned (for oneshot auto-detection).
    spawned_at: Instant,
}

/// Top-level state for the shell output widget system.
pub struct ShellState {
    pub instances: Vec<ShellInstance>,
    pub most_recent: Option<usize>,
}

impl Default for ShellState {
    fn default() -> Self {
        Self {
            instances: Vec::new(),
            most_recent: None,
        }
    }
}

/// Events sent from the shell background thread to the UI.
#[derive(Debug, Clone)]
pub enum ShellEvent {
    /// New output lines for a shell instance (identified by label).
    Output { label: String, lines: Vec<String> },
    /// A shell process exited.
    Exited { label: String, exit_code: Option<i32> },
    /// A shell process failed to spawn.
    Error { label: String, error: String },
    /// Initial config loaded — list of configs to create instances for.
    ConfigLoaded(Vec<ShellConfig>),
    /// Config file changed — new list of configs (UI should reconcile).
    ConfigReloaded(Vec<ShellConfig>),
}

/// Return the config file path (re-exported for convenience).
pub fn config_file_path() -> std::path::PathBuf {
    config::config_file_path()
}

/// The shell subscription stream. Spawns a background thread that:
/// 1. Reads and parses the config file
/// 2. Spawns child processes
/// 3. Reads their output via per-process reader threads
/// 4. Watches the config file for changes and reconciles
pub fn shell_stream() -> impl futures::Stream<Item = ShellEvent> {
    let (tx, rx) = futures::channel::mpsc::unbounded();
    std::thread::spawn(move || {
        if let Err(e) = shell_thread(tx) {
            eprintln!("[dev-hud] shell thread error: {e}");
        }
    });
    rx
}

/// Internal: a managed child process with its reader channel.
struct ManagedProcess {
    label: String,
    #[allow(dead_code)]
    config: ShellConfig,
    child: Child,
    line_rx: mpsc::Receiver<String>,
    #[allow(dead_code)]
    spawned_at: Instant,
}

/// Spawn a shell command, returning the managed process.
fn spawn_shell(cfg: &ShellConfig) -> Result<ManagedProcess, String> {
    let mut child = Command::new("sh")
        .args(["-c", &format!("{} 2>&1", cfg.command)])
        .stdout(Stdio::piped())
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("failed to spawn '{}': {e}", cfg.command))?;

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "failed to capture stdout".to_string())?;

    let (line_tx, line_rx) = mpsc::channel();
    let label = cfg.label.clone();

    // Per-process reader thread
    std::thread::spawn(move || {
        let reader = std::io::BufReader::new(stdout);
        for line in reader.lines() {
            match line {
                Ok(l) => {
                    if line_tx.send(l).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
        drop(line_tx);
        eprintln!("[dev-hud] shell reader done: {label}");
    });

    Ok(ManagedProcess {
        label: cfg.label.clone(),
        config: cfg.clone(),
        child,
        line_rx,
        spawned_at: Instant::now(),
    })
}

/// Main shell management thread.
fn shell_thread(
    tx: futures::channel::mpsc::UnboundedSender<ShellEvent>,
) -> Result<(), String> {
    let config_path = config::config_file_path();

    // Read initial config
    let content = std::fs::read_to_string(&config_path)
        .map_err(|e| format!("cannot read {}: {e}", config_path.display()))?;
    let configs = config::parse_config(&content);

    eprintln!(
        "[dev-hud] shell: loaded {} widget(s) from {}",
        configs.len(),
        config_path.display()
    );

    // Send initial config to UI
    if tx
        .unbounded_send(ShellEvent::ConfigLoaded(configs.clone()))
        .is_err()
    {
        return Ok(());
    }

    // Spawn initial processes
    let mut processes: Vec<ManagedProcess> = Vec::new();
    for cfg in &configs {
        match spawn_shell(cfg) {
            Ok(proc) => {
                eprintln!("[dev-hud] shell: spawned '{}' (pid {})", cfg.label, proc.child.id());
                processes.push(proc);
            }
            Err(e) => {
                eprintln!("[dev-hud] shell: {e}");
                let _ = tx.unbounded_send(ShellEvent::Error {
                    label: cfg.label.clone(),
                    error: e,
                });
            }
        }
    }

    let mut last_configs = configs;
    let mut last_mtime = std::fs::metadata(&config_path)
        .and_then(|m| m.modified())
        .ok();
    let mut poll_count: u64 = 0;

    loop {
        // Drain output from all processes
        for proc in &mut processes {
            let mut lines = Vec::new();
            loop {
                match proc.line_rx.try_recv() {
                    Ok(line) => {
                        let stripped = crate::util::strip_ansi(&line);
                        lines.push(stripped);
                    }
                    Err(mpsc::TryRecvError::Empty) => break,
                    Err(mpsc::TryRecvError::Disconnected) => break,
                }
            }
            if !lines.is_empty() {
                if tx
                    .unbounded_send(ShellEvent::Output {
                        label: proc.label.clone(),
                        lines,
                    })
                    .is_err()
                {
                    // UI gone, clean up
                    kill_all(&mut processes);
                    return Ok(());
                }
            }
        }

        // Check for exited processes
        let mut i = 0;
        while i < processes.len() {
            match processes[i].child.try_wait() {
                Ok(Some(status)) => {
                    let label = processes[i].label.clone();
                    let code = status.code();
                    eprintln!("[dev-hud] shell: '{}' exited (code {:?})", label, code);

                    // Drain any remaining output
                    let mut final_lines = Vec::new();
                    loop {
                        match processes[i].line_rx.try_recv() {
                            Ok(line) => final_lines.push(crate::util::strip_ansi(&line)),
                            Err(_) => break,
                        }
                    }
                    if !final_lines.is_empty() {
                        let _ = tx.unbounded_send(ShellEvent::Output {
                            label: label.clone(),
                            lines: final_lines,
                        });
                    }

                    if tx
                        .unbounded_send(ShellEvent::Exited {
                            label: label.clone(),
                            exit_code: code,
                        })
                        .is_err()
                    {
                        kill_all(&mut processes);
                        return Ok(());
                    }
                    processes.remove(i);
                }
                Ok(None) => {
                    i += 1;
                }
                Err(e) => {
                    eprintln!(
                        "[dev-hud] shell: error checking '{}': {e}",
                        processes[i].label
                    );
                    i += 1;
                }
            }
        }

        // Periodic config file check
        poll_count += 1;
        if poll_count % CONFIG_CHECK_POLLS == 0 {
            let current_mtime = std::fs::metadata(&config_path)
                .and_then(|m| m.modified())
                .ok();

            if current_mtime != last_mtime {
                last_mtime = current_mtime;
                if let Ok(content) = std::fs::read_to_string(&config_path) {
                    let new_configs = config::parse_config(&content);
                    let diff = config::reconcile(&last_configs, &new_configs);

                    // Kill removed processes
                    for label in &diff.removed {
                        if let Some(pos) = processes.iter().position(|p| &p.label == label) {
                            eprintln!("[dev-hud] shell: killing removed '{label}'");
                            let mut proc = processes.remove(pos);
                            let _ = proc.child.kill();
                            let _ = proc.child.wait();
                        }
                    }

                    // Kill changed processes (will be respawned)
                    for cfg in &diff.changed {
                        if let Some(pos) = processes.iter().position(|p| p.label == cfg.label) {
                            eprintln!("[dev-hud] shell: restarting changed '{}'", cfg.label);
                            let mut proc = processes.remove(pos);
                            let _ = proc.child.kill();
                            let _ = proc.child.wait();
                        }
                    }

                    // Spawn added + changed
                    for cfg in diff.added.iter().chain(diff.changed.iter()) {
                        match spawn_shell(cfg) {
                            Ok(proc) => {
                                eprintln!(
                                    "[dev-hud] shell: spawned '{}' (pid {})",
                                    cfg.label,
                                    proc.child.id()
                                );
                                processes.push(proc);
                            }
                            Err(e) => {
                                eprintln!("[dev-hud] shell: {e}");
                                let _ = tx.unbounded_send(ShellEvent::Error {
                                    label: cfg.label.clone(),
                                    error: e,
                                });
                            }
                        }
                    }

                    if !diff.added.is_empty()
                        || !diff.removed.is_empty()
                        || !diff.changed.is_empty()
                    {
                        eprintln!(
                            "[dev-hud] shell: config reloaded (+{} -{} ~{})",
                            diff.added.len(),
                            diff.removed.len(),
                            diff.changed.len()
                        );
                        let _ = tx.unbounded_send(ShellEvent::ConfigReloaded(new_configs.clone()));
                    }

                    last_configs = new_configs;
                }
            }
        }

        std::thread::sleep(Duration::from_millis(POLL_INTERVAL_MS));
    }
}

/// Kill all managed child processes.
fn kill_all(processes: &mut Vec<ManagedProcess>) {
    for proc in processes.iter_mut() {
        eprintln!("[dev-hud] shell: killing '{}' (pid {})", proc.label, proc.child.id());
        let _ = proc.child.kill();
        let _ = proc.child.wait();
    }
    processes.clear();
}

impl ShellState {
    /// Apply a ShellEvent to update UI state.
    pub fn apply_event(&mut self, event: &ShellEvent) {
        match event {
            ShellEvent::Output { label, lines } => {
                if let Some(idx) = self.instances.iter().position(|i| i.config.label == *label) {
                    let inst = &mut self.instances[idx];
                    for line in lines {
                        inst.buffer.push_back(line.clone());
                        while inst.buffer.len() > MAX_BUFFER_LINES {
                            inst.buffer.pop_front();
                        }
                    }
                    inst.last_update = SystemTime::now();
                    self.most_recent = Some(idx);
                }
            }
            ShellEvent::Exited { label, exit_code } => {
                if let Some(idx) = self.instances.iter().position(|i| i.config.label == *label) {
                    let inst = &mut self.instances[idx];
                    inst.exit_code = *exit_code;

                    // Auto-detect: if mode was unspecified and exited quickly, mark as oneshot
                    if inst.config.mode.is_none()
                        && inst.spawned_at.elapsed() < Duration::from_secs(ONESHOT_DETECT_SECS)
                    {
                        inst.resolved_mode = ShellMode::Oneshot;
                    }
                }
            }
            ShellEvent::Error { label, error } => {
                if let Some(idx) = self.instances.iter().position(|i| i.config.label == *label) {
                    self.instances[idx].error = Some(error.clone());
                } else {
                    // Instance might not exist yet (spawn failure before ConfigLoaded)
                    // Create a placeholder
                    self.instances.push(ShellInstance {
                        config: ShellConfig {
                            label: label.clone(),
                            command: String::new(),
                            mode: None,
                            lines: 16,
                        },
                        buffer: VecDeque::new(),
                        exit_code: None,
                        last_update: SystemTime::now(),
                        error: Some(error.clone()),
                        resolved_mode: ShellMode::Stream,
                        spawned_at: Instant::now(),
                    });
                }
            }
            ShellEvent::ConfigLoaded(configs) => {
                self.instances = configs
                    .iter()
                    .map(|cfg| ShellInstance {
                        resolved_mode: cfg.mode.unwrap_or(ShellMode::Stream),
                        config: cfg.clone(),
                        buffer: VecDeque::new(),
                        exit_code: None,
                        last_update: SystemTime::now(),
                        error: None,
                        spawned_at: Instant::now(),
                    })
                    .collect();
                self.most_recent = None;
            }
            ShellEvent::ConfigReloaded(configs) => {
                // Preserve existing buffers for unchanged instances
                let mut new_instances: Vec<ShellInstance> = Vec::new();
                for cfg in configs {
                    if let Some(existing) = self
                        .instances
                        .iter()
                        .find(|i| i.config.label == cfg.label && i.config.command == cfg.command)
                    {
                        // Keep existing buffer/state
                        new_instances.push(ShellInstance {
                            config: cfg.clone(),
                            buffer: existing.buffer.clone(),
                            exit_code: existing.exit_code,
                            last_update: existing.last_update,
                            error: existing.error.clone(),
                            resolved_mode: existing.resolved_mode,
                            spawned_at: existing.spawned_at,
                        });
                    } else {
                        new_instances.push(ShellInstance {
                            resolved_mode: cfg.mode.unwrap_or(ShellMode::Stream),
                            config: cfg.clone(),
                            buffer: VecDeque::new(),
                            exit_code: None,
                            last_update: SystemTime::now(),
                            error: None,
                            spawned_at: Instant::now(),
                        });
                    }
                }
                self.instances = new_instances;
                // Reset most_recent if it's out of bounds
                if let Some(idx) = self.most_recent {
                    if idx >= self.instances.len() {
                        self.most_recent = None;
                    }
                }
            }
        }
    }
}
