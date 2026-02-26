use std::path::PathBuf;

/// Shell execution mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellMode {
    /// Long-running command (e.g. `tail -f`). Stays alive, streams output.
    Stream,
    /// One-shot command (e.g. `date`). Runs, captures output, exits.
    Oneshot,
}

/// Parsed configuration for a single shell widget.
#[derive(Debug, Clone)]
pub struct ShellConfig {
    pub label: String,
    pub command: String,
    pub mode: Option<ShellMode>,
    pub lines: usize,
}

/// What changed between two config snapshots.
pub struct ConfigDiff {
    /// Labels that were added (new instances to spawn).
    pub added: Vec<ShellConfig>,
    /// Labels that were removed (processes to kill).
    pub removed: Vec<String>,
    /// Labels whose command/mode/lines changed (kill old, spawn new).
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
/// ```
///
/// Only `# heading` and `- command:` are required. Mode defaults to auto-detect,
/// lines defaults to 16.
pub fn parse_config(content: &str) -> Vec<ShellConfig> {
    let mut configs = Vec::new();
    let mut current_label: Option<String> = None;
    let mut current_command: Option<String> = None;
    let mut current_mode: Option<ShellMode> = None;
    let mut current_lines: usize = 16;

    for line in content.lines() {
        let trimmed = line.trim();

        if let Some(heading) = trimmed.strip_prefix("# ") {
            // Flush previous instance
            if let (Some(label), Some(command)) = (current_label.take(), current_command.take()) {
                configs.push(ShellConfig {
                    label,
                    command,
                    mode: current_mode.take(),
                    lines: current_lines,
                });
            } else {
                current_command = None;
                current_mode = None;
            }
            current_label = Some(heading.trim().to_string());
            current_lines = 16;
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
                _ => None,
            };
        } else if let Some(rest) = trimmed.strip_prefix("- lines:") {
            if let Ok(n) = rest.trim().parse::<usize>() {
                current_lines = n.clamp(1, 64);
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

        assert_eq!(configs[1].label, "uptime");
        assert_eq!(configs[1].command, "uptime");
        assert_eq!(configs[1].mode, Some(ShellMode::Oneshot));
        assert_eq!(configs[1].lines, 16); // default
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
    fn reconcile_detects_changes() {
        let old = vec![
            ShellConfig {
                label: "a".into(),
                command: "echo a".into(),
                mode: None,
                lines: 16,
            },
            ShellConfig {
                label: "b".into(),
                command: "echo b".into(),
                mode: None,
                lines: 16,
            },
        ];
        let new = vec![
            ShellConfig {
                label: "a".into(),
                command: "echo a-v2".into(),
                mode: None,
                lines: 16,
            },
            ShellConfig {
                label: "c".into(),
                command: "echo c".into(),
                mode: None,
                lines: 16,
            },
        ];
        let diff = reconcile(&old, &new);
        assert_eq!(diff.added.len(), 1);
        assert_eq!(diff.added[0].label, "c");
        assert_eq!(diff.removed.len(), 1);
        assert_eq!(diff.removed[0], "b");
        assert_eq!(diff.changed.len(), 1);
        assert_eq!(diff.changed[0].label, "a");
    }
}
