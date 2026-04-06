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
        "toggle" | "focus" | "demo loader-toggle" | "demo loader-change" | "demo font-change"
        | "theme dark" | "theme light" | "theme auto" | "theme adaptive" | "theme-toggle"
        | "bg-toggle" | "shell-toggle" | "screen" => {}
        _ if cmd.starts_with("screen ") => {}
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
    eprintln!("  demo font-change    cycle HUD font");
    eprintln!("  theme dark          force dark theme");
    eprintln!("  theme light         force light theme");
    eprintln!("  theme auto          follow DE system theme (updates dynamically)");
    eprintln!("  theme adaptive      sample screen under HUD to pick theme automatically");
    eprintln!("  theme-toggle        cycle between dark and light themes");
    eprintln!("  bg-toggle           toggle semi-transparent backdrop behind widgets");
    eprintln!("  shell-toggle        toggle shell output widgets");
    eprintln!("  screen              cycle HUD to next monitor");
    eprintln!("  screen <name>       move HUD to specific output (e.g. DP-1, HDMI-A-1)");
}
