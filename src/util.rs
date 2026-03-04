use std::path::PathBuf;

/// UTF-8 safe string truncation by character count.
/// If the string exceeds `max_chars`, truncates and appends "...".
/// When `max_chars` is 3 or less, returns exactly `max_chars` characters
/// without ellipsis (no room for the "..." suffix).
pub fn truncate_str(s: &str, max_chars: usize) -> String {
    let char_count = s.chars().count();
    if char_count <= max_chars {
        s.to_string()
    } else if max_chars <= 3 {
        // Not enough room for "..." — just hard-truncate
        s.chars().take(max_chars).collect()
    } else {
        let end = s
            .char_indices()
            .nth(max_chars.saturating_sub(3))
            .map_or(s.len(), |(i, _)| i);
        format!("{}...", &s[..end])
    }
}

/// Resolve a Claude Code project slug back to a filesystem path, then return
/// the last directory component. Falls back to the last two dash-separated
/// parts if the path can't be resolved.
///
/// Slug format: absolute path with `/` replaced by `-`, e.g.
/// `/home/fuabioo/hulilabs/ai` → `-home-fuabioo-hulilabs-ai`.
///
/// Since `-` is ambiguous (path separator vs literal hyphen), we greedily
/// match against existing directories starting from `/`.
pub fn shorten_project(slug: &str) -> String {
    if let Some(name) = resolve_slug_to_dirname(slug) {
        return name;
    }
    // Fallback: last two dash-separated parts
    let parts: Vec<&str> = slug.split('-').filter(|s| !s.is_empty()).collect();
    if parts.len() <= 2 {
        slug.to_string()
    } else {
        parts[parts.len() - 2..].join("-")
    }
}

/// Shorten a project slug to just its last dash-separated component.
/// Used for compact displays (e.g. session pills) where space is limited.
#[allow(dead_code)]
pub fn shorten_project_short(slug: &str) -> String {
    if let Some(name) = resolve_slug_to_dirname(slug) {
        return name;
    }
    let parts: Vec<&str> = slug.split('-').filter(|s| !s.is_empty()).collect();
    if parts.is_empty() {
        slug.to_string()
    } else {
        parts[parts.len() - 1].to_string()
    }
}

/// Try to resolve a slug to a real path by greedily matching directory
/// components against the filesystem. Returns the last path component
/// (directory name) on success.
fn resolve_slug_to_dirname(slug: &str) -> Option<String> {
    // Strip leading `-` to get "home-user-dir-project"
    let stripped = slug.strip_prefix('-').unwrap_or(slug);
    if stripped.is_empty() {
        return None;
    }

    let parts: Vec<&str> = stripped.split('-').collect();
    if parts.is_empty() {
        return None;
    }

    // Greedy resolution: starting from `/`, try to match each part.
    // When a single part doesn't match, try joining it with the next part(s)
    // using `-` (to handle dir names containing hyphens like `dev-hud`).
    let mut path = PathBuf::from("/");
    let mut i = 0;
    let mut resolved_any = false;

    while i < parts.len() {
        // Try increasingly longer hyphenated names
        let mut matched = false;
        for end in (i + 1..=parts.len()).rev() {
            let candidate = parts[i..end].join("-");
            let try_path = path.join(&candidate);
            if try_path.is_dir() {
                path = try_path;
                i = end;
                matched = true;
                resolved_any = true;
                break;
            }
        }
        if !matched {
            // Can't resolve further — use remaining parts as last component
            let remainder = parts[i..].join("-");
            path = path.join(&remainder);
            break;
        }
    }

    if !resolved_any {
        return None;
    }

    // Return the last component of the resolved path
    path.file_name()
        .and_then(|n| n.to_str())
        .map(|s| s.to_string())
}

/// Strip ANSI escape sequences from a string.
pub fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            // Skip until we hit a letter (end of escape sequence)
            for esc in chars.by_ref() {
                if esc.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // truncate_str
    // -----------------------------------------------------------------------

    #[test]
    fn truncate_str_short_string_unchanged() {
        assert_eq!(truncate_str("hello", 10), "hello");
    }

    #[test]
    fn truncate_str_at_exact_limit() {
        assert_eq!(truncate_str("hello", 5), "hello");
    }

    #[test]
    fn truncate_str_long_string_truncated() {
        let result = truncate_str("hello world this is long", 10);
        assert!(result.ends_with("..."));
        assert!(result.chars().count() <= 10);
    }

    #[test]
    fn truncate_str_multibyte_utf8_no_panic() {
        let s = "こんにちは世界テスト文字列";
        let result = truncate_str(s, 5);
        assert!(result.ends_with("..."));
    }

    #[test]
    fn truncate_str_emoji_no_panic() {
        let s = "🎮🗡️🛡️🏰🐉💀⚔️🔮";
        let result = truncate_str(s, 4);
        assert!(result.ends_with("..."));
    }

    #[test]
    fn truncate_str_mixed_utf8_no_panic() {
        let s = "path — with em-dash and 日本語";
        let result = truncate_str(s, 15);
        assert!(result.ends_with("..."));
    }

    #[test]
    fn truncate_str_max_chars_zero() {
        assert_eq!(truncate_str("hello", 0), "");
    }

    #[test]
    fn truncate_str_max_chars_one() {
        assert_eq!(truncate_str("hello", 1), "h");
    }

    #[test]
    fn truncate_str_max_chars_two() {
        assert_eq!(truncate_str("hello", 2), "he");
    }

    #[test]
    fn truncate_str_max_chars_three() {
        assert_eq!(truncate_str("hello", 3), "hel");
    }

    #[test]
    fn truncate_str_max_chars_four_uses_ellipsis() {
        let result = truncate_str("hello world", 4);
        assert_eq!(result, "h...");
        assert_eq!(result.chars().count(), 4);
    }

    #[test]
    fn truncate_str_max_chars_three_multibyte() {
        let result = truncate_str("こんにちは", 3);
        assert_eq!(result, "こんに");
    }

    // -----------------------------------------------------------------------
    // resolve_slug_to_dirname
    // -----------------------------------------------------------------------

    #[test]
    fn resolve_empty_slug() {
        assert_eq!(resolve_slug_to_dirname(""), None);
        assert_eq!(resolve_slug_to_dirname("-"), None);
    }

    // -----------------------------------------------------------------------
    // shorten_project (fallback behavior)
    // -----------------------------------------------------------------------

    #[test]
    fn shorten_project_short_slug() {
        // Can't resolve on filesystem, falls back to last-2 heuristic
        assert_eq!(shorten_project("my-app"), "my-app");
    }

    #[test]
    fn shorten_project_single_component() {
        assert_eq!(shorten_project("app"), "app");
    }

    #[test]
    fn shorten_project_empty_slug() {
        assert_eq!(shorten_project(""), "");
    }

    // -----------------------------------------------------------------------
    // shorten_project_short (fallback behavior)
    // -----------------------------------------------------------------------

    #[test]
    fn shorten_project_short_two_components() {
        assert_eq!(shorten_project_short("my-app"), "app");
    }

    #[test]
    fn shorten_project_short_single_component() {
        assert_eq!(shorten_project_short("app"), "app");
    }

    #[test]
    fn shorten_project_short_empty_slug() {
        assert_eq!(shorten_project_short(""), "");
    }
}
