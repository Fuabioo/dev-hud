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
}
