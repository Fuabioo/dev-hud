use std::path::PathBuf;

/// Shell execution mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellMode {
    /// Long-running command (e.g. `tail -f`). Stays alive, streams output.
    Stream,
    /// One-shot command (e.g. `date`). Runs, captures output, exits.
    Oneshot,
    /// TUI program (e.g. `top`, `htop`). Runs in a PTY with terminal emulation.
    Tui,
}

/// When a shell widget is visible.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Visibility {
    /// Show only in focused mode (default for stream/oneshot).
    Focus,
    /// Show in both focused and unfocused modes.
    Always,
}

impl Default for Visibility {
    fn default() -> Self {
        Self::Focus
    }
}

/// Parsed configuration for a single shell widget.
#[derive(Debug, Clone)]
pub struct ShellConfig {
    pub label: String,
    pub command: String,
    pub mode: Option<ShellMode>,
    pub lines: usize,
    pub visible: Visibility,
    pub cols: usize,
    pub rows: usize,
    pub font_size: Option<f32>,
}

impl ShellConfig {
    fn defaults() -> ShellConfigDefaults {
        ShellConfigDefaults {
            lines: 16,
            visible: Visibility::Focus,
            cols: 120,
            rows: 24,
            font_size: None,
        }
    }
}

struct ShellConfigDefaults {
    lines: usize,
    visible: Visibility,
    cols: usize,
    rows: usize,
    font_size: Option<f32>,
}

/// What changed between two config snapshots.
pub struct ConfigDiff {
    /// Labels that were added (new instances to spawn).
    pub added: Vec<ShellConfig>,
    /// Labels that were removed (processes to kill).
    pub removed: Vec<String>,
    /// Labels whose config changed (kill old, spawn new).
    pub changed: Vec<ShellConfig>,
}

/// Return the path to the shells config file.
pub fn config_file_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join(".config/viz/shells.md")
}

/// Parse `~/.config/viz/shells.md` into a list of shell configs.
///
/// Format:
/// ```markdown
/// # label-name
/// - command: tail -f /var/log/syslog
/// - mode: stream
/// - lines: 16
/// - visible: always
/// - cols: 160
/// - rows: 40
/// - font_size: 5.0
/// ```
///
/// Only `# heading` and `- command:` are required. See `ShellConfig` fields for defaults.
pub fn parse_config(content: &str) -> Vec<ShellConfig> {
    let mut configs = Vec::new();
    let mut current_label: Option<String> = None;
    let mut current_command: Option<String> = None;
    let mut current_mode: Option<ShellMode> = None;
    let defaults = ShellConfig::defaults();
    let mut current_lines: usize = defaults.lines;
    let mut current_visible: Visibility = defaults.visible;
    let mut current_cols: usize = defaults.cols;
    let mut current_rows: usize = defaults.rows;
    let mut current_font_size: Option<f32> = defaults.font_size;

    let mut in_comment = false;

    for line in content.lines() {
        let trimmed = line.trim();

        // Skip HTML comment blocks: <!-- ... -->
        if !in_comment && trimmed.contains("<!--") {
            in_comment = true;
        }
        if in_comment {
            if trimmed.contains("-->") {
                in_comment = false;
            }
            continue;
        }

        if let Some(heading) = trimmed.strip_prefix("# ") {
            // Flush previous instance
            if let (Some(label), Some(command)) = (current_label.take(), current_command.take()) {
                configs.push(ShellConfig {
                    label,
                    command,
                    mode: current_mode.take(),
                    lines: current_lines,
                    visible: current_visible,
                    cols: current_cols,
                    rows: current_rows,
                    font_size: current_font_size,
                });
            } else {
                current_command = None;
                current_mode = None;
            }
            current_label = Some(heading.trim().to_string());
            current_lines = defaults.lines;
            current_visible = defaults.visible;
            current_cols = defaults.cols;
            current_rows = defaults.rows;
            current_font_size = defaults.font_size;
            continue;
        }

        if current_label.is_none() {
            continue;
        }

        if let Some(rest) = trimmed.strip_prefix("- command:") {
            let cmd = rest.trim();
            if !cmd.is_empty() {
                current_command = Some(cmd.to_string());
            }
        } else if let Some(rest) = trimmed.strip_prefix("- mode:") {
            let mode_str = rest.trim().to_lowercase();
            current_mode = match mode_str.as_str() {
                "stream" => Some(ShellMode::Stream),
                "oneshot" => Some(ShellMode::Oneshot),
                "tui" => Some(ShellMode::Tui),
                _ => None,
            };
        } else if let Some(rest) = trimmed.strip_prefix("- lines:") {
            if let Ok(n) = rest.trim().parse::<usize>() {
                current_lines = n.clamp(1, 64);
            }
        } else if let Some(rest) = trimmed.strip_prefix("- visible:") {
            let vis_str = rest.trim().to_lowercase();
            current_visible = match vis_str.as_str() {
                "always" => Visibility::Always,
                _ => Visibility::Focus,
            };
        } else if let Some(rest) = trimmed.strip_prefix("- cols:") {
            if let Ok(n) = rest.trim().parse::<usize>() {
                current_cols = n.clamp(40, 512);
            }
        } else if let Some(rest) = trimmed.strip_prefix("- rows:") {
            if let Ok(n) = rest.trim().parse::<usize>() {
                current_rows = n.clamp(4, 200);
            }
        } else if let Some(rest) = trimmed.strip_prefix("- font_size:") {
            if let Ok(f) = rest.trim().parse::<f32>() {
                current_font_size = Some(f.clamp(2.0, 32.0));
            }
        }
    }

    // Flush last instance
    if let (Some(label), Some(command)) = (current_label, current_command) {
        configs.push(ShellConfig {
            label,
            command,
            mode: current_mode,
            lines: current_lines,
            visible: current_visible,
            cols: current_cols,
            rows: current_rows,
            font_size: current_font_size,
        });
    }

    configs
}

/// Compute the diff between old and new config lists.
/// Matches by label (the `# heading` text).
pub fn reconcile(old: &[ShellConfig], new: &[ShellConfig]) -> ConfigDiff {
    use std::collections::HashMap;

    let old_map: HashMap<&str, &ShellConfig> = old.iter().map(|c| (c.label.as_str(), c)).collect();
    let new_map: HashMap<&str, &ShellConfig> = new.iter().map(|c| (c.label.as_str(), c)).collect();

    let mut added = Vec::new();
    let mut removed = Vec::new();
    let mut changed = Vec::new();

    for new_cfg in new {
        match old_map.get(new_cfg.label.as_str()) {
            None => added.push(new_cfg.clone()),
            Some(old_cfg) => {
                if old_cfg.command != new_cfg.command
                    || old_cfg.mode != new_cfg.mode
                    || old_cfg.lines != new_cfg.lines
                    || old_cfg.visible != new_cfg.visible
                    || old_cfg.cols != new_cfg.cols
                    || old_cfg.rows != new_cfg.rows
                    || old_cfg.font_size != new_cfg.font_size
                {
                    changed.push(new_cfg.clone());
                }
            }
        }
    }

    for old_cfg in old {
        if !new_map.contains_key(old_cfg.label.as_str()) {
            removed.push(old_cfg.label.clone());
        }
    }

    ConfigDiff {
        added,
        removed,
        changed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_config(label: &str, command: &str) -> ShellConfig {
        ShellConfig {
            label: label.into(),
            command: command.into(),
            mode: None,
            lines: 16,
            visible: Visibility::Focus,
            cols: 120,
            rows: 24,
            font_size: None,
        }
    }

    #[test]
    fn parse_basic_config() {
        let input = r#"
# syslog
- command: tail -f /var/log/syslog
- mode: stream
- lines: 20

# uptime
- command: uptime
- mode: oneshot
"#;
        let configs = parse_config(input);
        assert_eq!(configs.len(), 2);

        assert_eq!(configs[0].label, "syslog");
        assert_eq!(configs[0].command, "tail -f /var/log/syslog");
        assert_eq!(configs[0].mode, Some(ShellMode::Stream));
        assert_eq!(configs[0].lines, 20);
        assert_eq!(configs[0].visible, Visibility::Focus);
        assert_eq!(configs[0].cols, 120);

        assert_eq!(configs[1].label, "uptime");
        assert_eq!(configs[1].command, "uptime");
        assert_eq!(configs[1].mode, Some(ShellMode::Oneshot));
        assert_eq!(configs[1].lines, 16); // default
    }

    #[test]
    fn parse_new_fields() {
        let input = r#"
# dev-hud-logs
- command: journalctl --user -u dev-hud -f --no-pager
- mode: stream
- lines: 8
- visible: always
- cols: 160
- font_size: 6.0
"#;
        let configs = parse_config(input);
        assert_eq!(configs.len(), 1);
        assert_eq!(configs[0].visible, Visibility::Always);
        assert_eq!(configs[0].cols, 160);
        assert_eq!(configs[0].font_size, Some(6.0));
    }

    #[test]
    fn parse_tui_mode() {
        let input = r#"
# system-monitor
- command: top -b -d 2
- mode: tui
- rows: 40
- cols: 120
- font_size: 5.0
"#;
        let configs = parse_config(input);
        assert_eq!(configs.len(), 1);
        assert_eq!(configs[0].mode, Some(ShellMode::Tui));
        assert_eq!(configs[0].rows, 40);
        assert_eq!(configs[0].cols, 120);
        assert_eq!(configs[0].font_size, Some(5.0));
    }

    #[test]
    fn parse_missing_command_skips() {
        let input = r#"
# no-command
- mode: stream

# has-command
- command: echo hello
"#;
        let configs = parse_config(input);
        assert_eq!(configs.len(), 1);
        assert_eq!(configs[0].label, "has-command");
    }

    #[test]
    fn parse_auto_detect_mode() {
        let input = r#"
# auto
- command: date
"#;
        let configs = parse_config(input);
        assert_eq!(configs.len(), 1);
        assert_eq!(configs[0].mode, None);
    }

    #[test]
    fn parse_lines_clamped() {
        let input = r#"
# big
- command: echo hi
- lines: 999
"#;
        let configs = parse_config(input);
        assert_eq!(configs[0].lines, 64);
    }

    #[test]
    fn parse_cols_clamped() {
        let input = r#"
# wide
- command: echo hi
- cols: 9999
"#;
        let configs = parse_config(input);
        assert_eq!(configs[0].cols, 512);

        let input2 = r#"
# narrow
- command: echo hi
- cols: 5
"#;
        let configs2 = parse_config(input2);
        assert_eq!(configs2[0].cols, 40);
    }

    #[test]
    fn parse_rows_clamped() {
        let input = r#"
# huge
- command: top
- rows: 999
"#;
        let configs = parse_config(input);
        assert_eq!(configs[0].rows, 200);
    }

    #[test]
    fn parse_font_size_clamped() {
        let input = r#"
# tiny
- command: echo hi
- font_size: 0.5
"#;
        let configs = parse_config(input);
        assert_eq!(configs[0].font_size, Some(2.0));
    }

    #[test]
    fn parse_html_comments_skipped() {
        let input = r#"
# active
- command: echo hello

<!-- # commented-out
- command: echo secret
- mode: stream -->

# also-active
- command: echo world
"#;
        let configs = parse_config(input);
        assert_eq!(configs.len(), 2);
        assert_eq!(configs[0].label, "active");
        assert_eq!(configs[1].label, "also-active");
    }

    #[test]
    fn parse_html_comment_does_not_pollute_previous() {
        let input = r#"
# real
- command: ctop

<!-- # fake
- command: cmatrix
- mode: tui -->
"#;
        let configs = parse_config(input);
        assert_eq!(configs.len(), 1);
        assert_eq!(configs[0].label, "real");
        assert_eq!(configs[0].command, "ctop");
    }

    #[test]
    fn reconcile_detects_changes() {
        let old = vec![
            default_config("a", "echo a"),
            default_config("b", "echo b"),
        ];
        let new = vec![
            ShellConfig {
                command: "echo a-v2".into(),
                ..default_config("a", "echo a-v2")
            },
            default_config("c", "echo c"),
        ];
        let diff = reconcile(&old, &new);
        assert_eq!(diff.added.len(), 1);
        assert_eq!(diff.added[0].label, "c");
        assert_eq!(diff.removed.len(), 1);
        assert_eq!(diff.removed[0], "b");
        assert_eq!(diff.changed.len(), 1);
        assert_eq!(diff.changed[0].label, "a");
    }

    #[test]
    fn reconcile_detects_visibility_change() {
        let old = vec![default_config("a", "echo a")];
        let new = vec![ShellConfig {
            visible: Visibility::Always,
            ..default_config("a", "echo a")
        }];
        let diff = reconcile(&old, &new);
        assert_eq!(diff.changed.len(), 1);
        assert_eq!(diff.changed[0].visible, Visibility::Always);
    }

    #[test]
    fn reconcile_detects_cols_change() {
        let old = vec![default_config("a", "echo a")];
        let new = vec![ShellConfig {
            cols: 200,
            ..default_config("a", "echo a")
        }];
        let diff = reconcile(&old, &new);
        assert_eq!(diff.changed.len(), 1);
    }
}
