use std::io::Write;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::process;

fn socket_path() -> PathBuf {
    let runtime_dir = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".to_string());
    PathBuf::from(runtime_dir).join("dev-hud.sock")
}

fn main() {
    let cmd = match std::env::args().nth(1) {
        Some(c) => c,
        None => {
            eprintln!("usage: dev-hud-ctl <toggle|focus>");
            process::exit(1);
        }
    };

    match cmd.as_str() {
        "toggle" | "focus" => {}
        _ => {
            eprintln!("unknown command: {cmd}");
            eprintln!("usage: dev-hud-ctl <toggle|focus>");
            process::exit(1);
        }
    }

    let path = socket_path();
    let mut stream = match UnixStream::connect(&path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("dev-hud not running ({path:?}): {e}");
            process::exit(1);
        }
    };

    if let Err(e) = writeln!(stream, "{cmd}") {
        eprintln!("failed to send command: {e}");
        process::exit(1);
    }
}
