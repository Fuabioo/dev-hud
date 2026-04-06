use std::io::BufRead;
use std::os::unix::net::UnixListener;
use std::path::PathBuf;
use std::time::Duration;

use futures::channel::mpsc;

use crate::app::Message;
use crate::shell;
use crate::theme::ThemeMode;

pub(crate) fn socket_path() -> PathBuf {
    let runtime_dir = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".to_string());
    PathBuf::from(runtime_dir).join("dev-hud.sock")
}

pub(crate) fn socket_listener() -> impl futures::Stream<Item = Message> {
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
                    "demo font-change" => Some(Message::FontChange),
                    "theme dark" => Some(Message::ThemeSet(ThemeMode::Dark)),
                    "theme light" => Some(Message::ThemeSet(ThemeMode::Light)),
                    "theme auto" => Some(Message::ThemeSet(ThemeMode::Auto)),
                    "theme adaptive" => Some(Message::ThemeSet(ThemeMode::Adaptive)),
                    "theme-toggle" => Some(Message::ThemeToggle),
                    "bg-toggle" => Some(Message::BackdropToggle),
                    "shell-toggle" => Some(Message::ShellToggle),
                    "screen" => Some(Message::ScreenCycle),
                    cmd if cmd.starts_with("screen ") => {
                        Some(Message::ScreenSet(cmd[7..].trim().to_string()))
                    }
                    other => {
                        eprintln!("[dev-hud] unknown command: {other:?}");
                        None
                    }
                };
                if let Some(msg) = msg
                    && tx.unbounded_send(msg).is_err()
                {
                    break;
                }
            }
        }
    });
    rx
}

pub(crate) fn tick_stream(ms: &u64) -> mpsc::UnboundedReceiver<Message> {
    let ms = *ms;
    let (tx, rx) = mpsc::unbounded();
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(Duration::from_millis(ms));
            if tx.unbounded_send(Message::Tick).is_err() {
                break;
            }
        }
    });
    rx
}

pub(crate) fn theme_refresh_stream() -> impl futures::Stream<Item = Message> {
    let (tx, rx) = mpsc::unbounded();
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(Duration::from_secs(5));
            if tx.unbounded_send(Message::ThemeRefresh).is_err() {
                break;
            }
        }
    });
    rx
}

// --- Shell subscription bridge ---

pub(crate) fn shell_event_stream() -> impl futures::Stream<Item = Message> {
    use futures::StreamExt;
    shell::shell_stream().map(Message::ShellEvent)
}
