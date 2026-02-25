use std::io::Write;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::process;

fn socket_path() -> PathBuf {
    let runtime_dir = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".to_string());
    PathBuf::from(runtime_dir).join("dev-hud.sock")
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        usage();
        process::exit(1);
    }

    let cmd = args.join(" ");
    match cmd.as_str() {
        "toggle" | "focus" | "demo loader-toggle" | "demo loader-change"
        | "demo claude-toggle" | "demo font-change" | "modal-close" => {}
        _ => {
            eprintln!("unknown command: {cmd}");
            usage();
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

fn usage() {
    eprintln!("usage: dev-hud-ctl <command>");
    eprintln!();
    eprintln!("commands:");
    eprintln!("  toggle              toggle HUD visibility");
    eprintln!("  focus               toggle HUD focus/interactivity");
    eprintln!("  demo loader-toggle  toggle demo loader widget");
    eprintln!("  demo loader-change  cycle demo loader animation style");
    eprintln!("  demo claude-toggle  toggle claude code visualizer demo");
    eprintln!("  demo font-change    cycle HUD font");
    eprintln!("  modal-close         close activity log modal");
}
