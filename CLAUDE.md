# dev-hud

Transparent Wayland overlay HUD. Built with iced + iced_layershell.

## Development workflow

After any code change, rebuild and restart the running service:

```bash
cargo build --release && systemctl --user restart dev-hud
```

If the changes is to the configuration file ~/.config/viz/shells.md there is no need to restart the service, it will be picked up automatically.

The HUD runs as a systemd user service (`dev-hud.service`). It must be restarted to pick up code changes. Use `./setup.sh install` to do a full rebuild + restart cycle.

## Key files

| File | Purpose |
|------|---------|
| `src/main.rs` | HUD state machine, iced views, IPC socket, layer shell settings |
| `src/theme.rs` | ThemeMode, ThemeColors (colors + font sizes), system detection, screen sampling |
| `src/events.rs` | Claude Code JSONL event types |
| `src/shell/config.rs` | Shell widget config parsing (`~/.config/viz/shells.md`), `ShellMode`, `Visibility`, `Position` enums |
| `src/shell/mod.rs` | Shell process management, PTY spawning (TUI mode), `ShellState`, `ShellEvent` |
| `src/util.rs` | String helpers (truncation, project slug shortening) |
| `src/watcher/mod.rs` | Multi-session file watcher |
| `src/watcher/scanner.rs` | JSONL directory scanner |
| `src/watcher/parser.rs` | Event stream parser |
| `src/bin/dev-hud-ctl.rs` | CLI client for the IPC socket |
| `dev-hud.service` | Systemd user unit (env vars like DEV_HUD_SCREEN live here) |
| `setup.sh` | Install/uninstall script (build, symlink, enable service) |

## Shell widgets

Shell widgets are configured in `~/.config/viz/shells.md` (hot-reloaded, no restart needed). Format:

```markdown
# label-name
- command: top -b -d 2
- mode: tui              # oneshot | stream | tui (auto-detect if omitted)
- visible: always        # focus (default) | always
- position: top-left     # top-left | top-right | bottom-left | bottom-right (default)
- rows: 17               # PTY rows for tui mode (default 24)
- cols: 120              # truncation width / PTY cols (default 120)
- lines: 8               # visible output lines for stream/oneshot (default 16)
- font_size: 6.5         # per-instance override (default: theme widget_text)
```

HTML comments (`<!-- ... -->`) can be used to disable entries.

Modes:
- **oneshot/stream**: spawned via `sh -c "cmd 2>&1"`, output read line-by-line
- **tui**: spawned in a PTY (`portable-pty`) with `TERM=xterm-256color`, output parsed by `vt100` into a character grid

Layout positions: the HUD has four quadrants. Claude sessions always render at bottom-left. Shell widgets default to bottom-right but can be placed in any quadrant via `position`.

## Screenshots

Use `cosmic-screenshot` to capture the screen (saves to `~/Pictures/`):

```bash
cosmic-screenshot
```

## Architecture notes

- The HUD is a multi-window iced_layershell daemon. Surfaces (main, modal, archive) are created via `Message::layershell_open()` with `NewLayerShellSettings`.
- Monitor targeting uses `OutputOption::OutputName(name)` in `NewLayerShellSettings.output_option`. The default output is set via `DEV_HUD_SCREEN` env var in the systemd service file.
- IPC is plaintext over a Unix socket (`$XDG_RUNTIME_DIR/dev-hud.sock`). Commands arrive as single lines.
- Font sizes and colors live together in `ThemeColors` (in `theme.rs`). Dark and light themes each define their own sizes (dark uses larger modals, light uses larger markers). Widgets should reference `colors.widget_text`, `colors.marker_size`, etc. rather than defining local constants.
- Exited sessions enter a 5-minute grace period (`ARCHIVE_GRACE_SECS`) before auto-archiving. Archived sessions are viewable in the archive modal (`archive-show`/`archive-close`).
- Output enumeration for screen cycling tries `cosmic-randr list` first, then `wlr-randr` as fallback.
- The `#[to_layer_message(multi)]` macro auto-generates `layershell_open()` and `RemoveWindow()` message variants.

## Conventions

- All IPC commands must be added in three places: `socket_listener()` match (main.rs), `dev-hud-ctl.rs` validation match, and `dev-hud-ctl.rs` usage text.
- Use `eprintln!("[dev-hud] ...")` for all log output. Logs are visible via `journalctl --user -u dev-hud -f`.
- Files starting with `ms.<filename>` do not exist in this repo.
