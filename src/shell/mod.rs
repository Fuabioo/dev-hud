pub mod config;

use std::collections::VecDeque;
use std::io::{BufRead, Read as _};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant, SystemTime};

pub use config::{ShellMode, Visibility};
use config::ShellConfig;

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
    /// TUI screen snapshot (only used when resolved_mode == Tui).
    pub tui_screen: Option<Vec<String>>,
    /// When the process was spawned (for oneshot auto-detection).
    spawned_at: Instant,
}

fn new_instance(cfg: &ShellConfig) -> ShellInstance {
    ShellInstance {
        resolved_mode: cfg.mode.unwrap_or(ShellMode::Stream),
        config: cfg.clone(),
        buffer: VecDeque::new(),
        exit_code: None,
        last_update: SystemTime::now(),
        error: None,
        tui_screen: None,
        spawned_at: Instant::now(),
    }
}

fn placeholder_instance(label: &str, error: String) -> ShellInstance {
    ShellInstance {
        config: ShellConfig {
            label: label.to_string(),
            command: String::new(),
            mode: None,
            lines: 16,
            visible: Visibility::Focus,
            cols: 120,
            rows: 24,
            font_size: None,
        },
        buffer: VecDeque::new(),
        exit_code: None,
        last_update: SystemTime::now(),
        error: Some(error),
        resolved_mode: ShellMode::Stream,
        tui_screen: None,
        spawned_at: Instant::now(),
    }
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
    /// Full TUI screen update (replaces the entire screen snapshot).
    TuiUpdate { label: String, rows: Vec<String> },
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
/// 2. Spawns child processes (regular or PTY-based for TUI mode)
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
    child: ManagedChild,
    line_rx: mpsc::Receiver<ProcessOutput>,
    #[allow(dead_code)]
    spawned_at: Instant,
}

/// Either a regular Child or a PTY-based child.
enum ManagedChild {
    Regular(Child),
    Pty {
        child: Box<dyn portable_pty::Child + Send>,
        _pair: portable_pty::PtyPair,
    },
}

impl ManagedChild {
    fn kill_and_wait(&mut self) {
        match self {
            ManagedChild::Regular(child) => {
                let _ = child.kill();
                let _ = child.wait();
            }
            ManagedChild::Pty { child, .. } => {
                let _ = child.kill();
                let _ = child.wait();
            }
        }
    }

    fn try_wait(&mut self) -> Result<Option<portable_pty::ExitStatus>, String> {
        match self {
            ManagedChild::Regular(child) => match child.try_wait() {
                Ok(Some(status)) => Ok(Some(if status.success() {
                    portable_pty::ExitStatus::with_exit_code(0)
                } else {
                    portable_pty::ExitStatus::with_exit_code(
                        status.code().unwrap_or(1) as u32,
                    )
                })),
                Ok(None) => Ok(None),
                Err(e) => Err(e.to_string()),
            },
            ManagedChild::Pty { child, .. } => match child.try_wait() {
                Ok(status) => Ok(status),
                Err(e) => Err(e.to_string()),
            },
        }
    }

    fn id_string(&self) -> String {
        match self {
            ManagedChild::Regular(child) => child.id().to_string(),
            ManagedChild::Pty { child, .. } => {
                child
                    .process_id()
                    .map(|id| id.to_string())
                    .unwrap_or_else(|| "pty".to_string())
            }
        }
    }
}

/// Output from a managed process reader thread.
enum ProcessOutput {
    /// A single line (for stream/oneshot modes).
    Line(String),
    /// A full TUI screen update (for tui mode).
    Screen(Vec<String>),
}

/// Spawn a shell command, returning the managed process.
fn spawn_shell(cfg: &ShellConfig) -> Result<ManagedProcess, String> {
    let is_tui = cfg.mode == Some(ShellMode::Tui);

    if is_tui {
        spawn_tui(cfg)
    } else {
        spawn_regular(cfg)
    }
}

/// Spawn a regular (non-PTY) shell command.
fn spawn_regular(cfg: &ShellConfig) -> Result<ManagedProcess, String> {
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
                    if line_tx.send(ProcessOutput::Line(l)).is_err() {
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
        child: ManagedChild::Regular(child),
        line_rx,
        spawned_at: Instant::now(),
    })
}

/// Spawn a TUI command in a PTY with terminal emulation.
fn spawn_tui(cfg: &ShellConfig) -> Result<ManagedProcess, String> {
    use portable_pty::{CommandBuilder, PtySize, native_pty_system};

    let pty_system = native_pty_system();
    let pty_size = PtySize {
        rows: cfg.rows as u16,
        cols: cfg.cols as u16,
        pixel_width: 0,
        pixel_height: 0,
    };

    let pair = pty_system
        .openpty(pty_size)
        .map_err(|e| format!("failed to open pty: {e}"))?;

    let mut cmd = CommandBuilder::new("sh");
    cmd.args(["-c", &cfg.command]);
    cmd.env("TERM", "xterm-256color");

    let child = pair
        .slave
        .spawn_command(cmd)
        .map_err(|e| format!("failed to spawn tui '{}': {e}", cfg.command))?;

    let mut reader = pair
        .master
        .try_clone_reader()
        .map_err(|e| format!("failed to clone pty reader: {e}"))?;

    let (line_tx, line_rx) = mpsc::channel();
    let label = cfg.label.clone();
    let rows = cfg.rows;
    let cols = cfg.cols;

    // PTY reader thread: reads raw bytes, feeds to vt100 parser, extracts screen rows
    std::thread::spawn(move || {
        let mut parser = vt100::Parser::new(rows as u16, cols as u16, 0);
        let mut buf = [0u8; 4096];
        loop {
            match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    parser.process(&buf[..n]);
                    let screen = parser.screen();
                    let screen_rows: Vec<String> = (0..rows)
                        .map(|r| {
                            screen
                                .contents_between(r as u16, 0, r as u16, cols as u16)
                                .trim_end()
                                .to_string()
                        })
                        .collect();
                    if line_tx.send(ProcessOutput::Screen(screen_rows)).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
        drop(line_tx);
        eprintln!("[dev-hud] tui reader done: {label}");
    });

    Ok(ManagedProcess {
        label: cfg.label.clone(),
        config: cfg.clone(),
        child: ManagedChild::Pty {
            child: child,
            _pair: pair,
        },
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
                eprintln!(
                    "[dev-hud] shell: spawned '{}' (pid {})",
                    cfg.label,
                    proc.child.id_string()
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

    let mut last_configs = configs;
    let mut last_mtime = std::fs::metadata(&config_path)
        .and_then(|m| m.modified())
        .ok();
    let mut poll_count: u64 = 0;

    loop {
        // Drain output from all processes
        for proc in &mut processes {
            let mut lines = Vec::new();
            let mut tui_screen: Option<Vec<String>> = None;

            loop {
                match proc.line_rx.try_recv() {
                    Ok(ProcessOutput::Line(line)) => {
                        let stripped = crate::util::strip_ansi(&line);
                        lines.push(stripped);
                    }
                    Ok(ProcessOutput::Screen(screen)) => {
                        // For TUI, keep only the latest screen snapshot
                        tui_screen = Some(screen);
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
                    kill_all(&mut processes);
                    return Ok(());
                }
            }

            if let Some(rows) = tui_screen {
                if tx
                    .unbounded_send(ShellEvent::TuiUpdate {
                        label: proc.label.clone(),
                        rows,
                    })
                    .is_err()
                {
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
                    let code = if status.success() {
                        Some(0)
                    } else {
                        // portable_pty ExitStatus doesn't expose code directly for failure,
                        // but we can check success. Non-success = report as 1.
                        Some(1)
                    };
                    eprintln!("[dev-hud] shell: '{}' exited (code {:?})", label, code);

                    // Drain any remaining output
                    let mut final_lines = Vec::new();
                    loop {
                        match processes[i].line_rx.try_recv() {
                            Ok(ProcessOutput::Line(line)) => {
                                final_lines.push(crate::util::strip_ansi(&line));
                            }
                            Ok(ProcessOutput::Screen(_)) => {
                                // Ignore final screen updates on exit
                            }
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
                            proc.child.kill_and_wait();
                        }
                    }

                    // Kill changed processes (will be respawned)
                    for cfg in &diff.changed {
                        if let Some(pos) = processes.iter().position(|p| p.label == cfg.label) {
                            eprintln!("[dev-hud] shell: restarting changed '{}'", cfg.label);
                            let mut proc = processes.remove(pos);
                            proc.child.kill_and_wait();
                        }
                    }

                    // Spawn added + changed
                    for cfg in diff.added.iter().chain(diff.changed.iter()) {
                        match spawn_shell(cfg) {
                            Ok(proc) => {
                                eprintln!(
                                    "[dev-hud] shell: spawned '{}' (pid {})",
                                    cfg.label,
                                    proc.child.id_string()
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
        eprintln!(
            "[dev-hud] shell: killing '{}' (pid {})",
            proc.label,
            proc.child.id_string()
        );
        proc.child.kill_and_wait();
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
            ShellEvent::TuiUpdate { label, rows } => {
                if let Some(idx) = self.instances.iter().position(|i| i.config.label == *label) {
                    let inst = &mut self.instances[idx];
                    inst.tui_screen = Some(rows.clone());
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
                    self.instances.push(placeholder_instance(label, error.clone()));
                }
            }
            ShellEvent::ConfigLoaded(configs) => {
                self.instances = configs.iter().map(|cfg| new_instance(cfg)).collect();
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
                            tui_screen: existing.tui_screen.clone(),
                            spawned_at: existing.spawned_at,
                        });
                    } else {
                        new_instances.push(new_instance(cfg));
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
