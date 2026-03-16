//! Small utility functions for TUI rendering
//!
//! The AZURE constant defines the app's signature color (#007FFF) used
//! everywhere Cyan was previously used, aligning with the "Azureal" name.

/// Azure blue (#3399FF) — the app's signature accent color, replacing all
/// uses of ANSI Cyan for a cohesive visual identity matching the name "Azureal".
pub const AZURE: ratatui::style::Color = ratatui::style::Color::Rgb(51, 153, 255);

/// Git brand orange (#F05032) — used for Git Actions panel border and accents
pub const GIT_ORANGE: ratatui::style::Color = ratatui::style::Color::Rgb(240, 80, 50);

/// Git brown (#A0522D, sienna) — warm secondary color for Git panel text elements
/// (headers, key hints, separators, footer) instead of generic gray
pub const GIT_BROWN: ratatui::style::Color = ratatui::style::Color::Rgb(160, 82, 45);
//
// Re-exports commonly used items from submodules:
// - `colorize`: Output colorization (colorize_output, MessageType, etc.)
// - `markdown`: Markdown parsing (parse_markdown_spans, etc.)
// - `render_events`: Display event rendering
// - `render_tools`: Tool result rendering

// Re-export commonly used items
pub use super::colorize::{colorize_output, detect_message_type, MessageType};
pub use super::render_events::render_display_events;

/// Truncate a string to max length, adding ellipsis if needed
pub fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        format!("{}…", s.chars().take(max - 1).collect::<String>())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ═══════════════════════════════════════════════════════════════════
    // Constants
    // ═══════════════════════════════════════════════════════════════════

    #[test]
    fn azure_is_rgb_51_153_255() {
        assert_eq!(AZURE, ratatui::style::Color::Rgb(51, 153, 255));
    }

    #[test]
    fn git_orange_is_rgb_240_80_50() {
        assert_eq!(GIT_ORANGE, ratatui::style::Color::Rgb(240, 80, 50));
    }

    #[test]
    fn git_brown_is_rgb_160_82_45() {
        assert_eq!(GIT_BROWN, ratatui::style::Color::Rgb(160, 82, 45));
    }

    #[test]
    fn azure_not_cyan() {
        assert_ne!(AZURE, ratatui::style::Color::Cyan);
    }

    #[test]
    fn colors_all_distinct() {
        assert_ne!(AZURE, GIT_ORANGE);
        assert_ne!(AZURE, GIT_BROWN);
        assert_ne!(GIT_ORANGE, GIT_BROWN);
    }

    // ═══════════════════════════════════════════════════════════════════
    // truncate — fits without change
    // ═══════════════════════════════════════════════════════════════════

    #[test]
    fn truncate_empty() {
        assert_eq!(truncate("", 10), "");
    }

    #[test]
    fn truncate_short_unchanged() {
        assert_eq!(truncate("hello", 10), "hello");
    }

    #[test]
    fn truncate_exact_length() {
        assert_eq!(truncate("hello", 5), "hello");
    }

    #[test]
    fn truncate_single_char_fits() {
        assert_eq!(truncate("a", 1), "a");
    }

    #[test]
    fn truncate_large_max_no_change() {
        assert_eq!(truncate("short", 1000), "short");
    }

    // ═══════════════════════════════════════════════════════════════════
    // truncate — over limit
    // ═══════════════════════════════════════════════════════════════════

    #[test]
    fn truncate_one_over() {
        let result = truncate("hello!", 5);
        assert!(result.ends_with('…'));
        assert_eq!(result.chars().count(), 5);
    }

    #[test]
    fn truncate_long_string() {
        let result = truncate("a very long string indeed", 10);
        assert_eq!(result.chars().count(), 10);
        assert!(result.ends_with('…'));
    }

    #[test]
    fn truncate_max_1() {
        assert_eq!(truncate("hello", 1), "…");
    }

    #[test]
    fn truncate_max_2() {
        assert_eq!(truncate("hello", 2), "h…");
    }

    #[test]
    fn truncate_max_3() {
        assert_eq!(truncate("hello", 3), "he…");
    }

    #[test]
    fn truncate_100_chars_to_10() {
        let s = "a".repeat(100);
        let result = truncate(&s, 10);
        assert_eq!(result.chars().count(), 10);
        assert!(result.ends_with('…'));
    }

    // ═══════════════════════════════════════════════════════════════════
    // truncate — unicode
    // ═══════════════════════════════════════════════════════════════════

    #[test]
    fn truncate_unicode_fits() {
        assert_eq!(truncate("日本語", 3), "日本語");
    }

    #[test]
    fn truncate_unicode_over() {
        let result = truncate("日本語テスト", 4);
        assert_eq!(result.chars().count(), 4);
        assert!(result.ends_with('…'));
    }

    #[test]
    fn truncate_emoji_fits() {
        assert_eq!(truncate("🎉🎊", 2), "🎉🎊");
    }

    #[test]
    fn truncate_emoji_over() {
        let result = truncate("🎉🎊🎃", 2);
        assert_eq!(result.chars().count(), 2);
        assert!(result.ends_with('…'));
    }

    #[test]
    fn truncate_mixed_unicode_ascii() {
        let result = truncate("abc日本語def", 6);
        assert_eq!(result.chars().count(), 6);
        assert!(result.ends_with('…'));
    }

    // ═══════════════════════════════════════════════════════════════════
    // truncate — whitespace & special
    // ═══════════════════════════════════════════════════════════════════

    #[test]
    fn truncate_whitespace() {
        assert_eq!(truncate("     ", 3), "  …");
    }

    #[test]
    fn truncate_newlines() {
        let result = truncate("line1\nline2\nline3", 8);
        assert_eq!(result.chars().count(), 8);
    }

    #[test]
    fn truncate_tabs() {
        let result = truncate("\t\t\t\t\t", 3);
        assert_eq!(result.chars().count(), 3);
    }

    #[test]
    fn truncate_special_chars_fit() {
        assert_eq!(truncate("!@#$%", 5), "!@#$%");
    }

    #[test]
    fn truncate_special_chars_over() {
        let result = truncate("!@#$%^&*()", 5);
        assert_eq!(result.chars().count(), 5);
        assert!(result.ends_with('…'));
    }

    #[test]
    fn truncate_path() {
        let result = truncate("/Users/macbookpro/projects/azureal/src/main.rs", 20);
        assert_eq!(result.chars().count(), 20);
    }

    // ═══════════════════════════════════════════════════════════════════
    // truncate — consistency properties
    // ═══════════════════════════════════════════════════════════════════

    #[test]
    fn truncate_never_exceeds_max() {
        for max in 1..20 {
            let result = truncate("this is a test string for truncation", max);
            assert!(result.chars().count() <= max);
        }
    }

    #[test]
    fn truncate_idempotent_when_fits() {
        let s = "fits";
        assert_eq!(truncate(&truncate(s, 10), 10), s);
    }

    #[test]
    fn truncate_preserves_short_exactly() {
        for s in &["", "a", "ab", "abc"] {
            assert_eq!(truncate(s, 10), *s);
        }
    }

    #[test]
    fn truncate_empty_with_zero_max() {
        assert_eq!(truncate("", 0), "");
    }

    #[test]
    fn truncate_ellipsis_chars() {
        assert_eq!(truncate("……", 2), "……");
    }

    #[test]
    fn truncate_ellipsis_chars_over() {
        let result = truncate("………", 2);
        assert_eq!(result.chars().count(), 2);
    }

    // ═══════════════════════════════════════════════════════════════════
    // truncate — boundary values
    // ═══════════════════════════════════════════════════════════════════

    #[test]
    fn truncate_at_boundary_minus_1() {
        // 4 chars, max=3 -> should truncate
        let result = truncate("abcd", 3);
        assert_eq!(result, "ab…");
    }

    #[test]
    fn truncate_at_boundary_exact() {
        // 4 chars, max=4 -> unchanged
        assert_eq!(truncate("abcd", 4), "abcd");
    }

    #[test]
    fn truncate_at_boundary_plus_1() {
        // 4 chars, max=5 -> unchanged
        assert_eq!(truncate("abcd", 5), "abcd");
    }

    #[test]
    fn truncate_two_chars_max_1() {
        assert_eq!(truncate("ab", 1), "…");
    }

    #[test]
    fn truncate_three_chars_max_2() {
        assert_eq!(truncate("abc", 2), "a…");
    }

    #[test]
    fn truncate_url() {
        let result = truncate("https://github.com/user/repo/pull/123", 15);
        assert_eq!(result.chars().count(), 15);
        assert!(result.ends_with('…'));
    }

    #[test]
    fn truncate_preserves_content_prefix() {
        let result = truncate("abcdef", 4);
        assert_eq!(&result[..3], "abc"); // first 3 chars preserved
    }

    #[test]
    fn truncate_very_long_to_5() {
        let s = "x".repeat(10000);
        let result = truncate(&s, 5);
        assert_eq!(result.chars().count(), 5);
        assert_eq!(result, "xxxx…");
    }

    #[test]
    fn truncate_numeric_string() {
        assert_eq!(truncate("1234567890", 5), "1234…");
    }

    #[test]
    fn truncate_with_dots() {
        assert_eq!(truncate("a.b.c.d.e", 5), "a.b.…");
    }

    #[test]
    fn truncate_slash_path() {
        let result = truncate("a/b/c/d", 4);
        assert_eq!(result, "a/b…");
    }

    #[test]
    fn truncate_backslash() {
        let result = truncate("a\\b\\c\\d", 4);
        assert_eq!(result, "a\\b…");
    }

    #[test]
    fn truncate_mixed_emoji_text() {
        let result = truncate("hi🎉there", 5);
        assert_eq!(result.chars().count(), 5);
    }

    #[test]
    fn truncate_cjk_single() {
        assert_eq!(truncate("漢", 1), "漢");
    }

    #[test]
    fn truncate_cjk_two_max_one() {
        let result = truncate("漢字", 1);
        assert_eq!(result, "…");
    }

    #[test]
    fn truncate_repeated_ellipsis() {
        let result = truncate("…………………", 3);
        assert_eq!(result.chars().count(), 3);
    }

    #[test]
    fn truncate_control_chars() {
        let result = truncate("\x00\x01\x02\x03\x04", 3);
        assert_eq!(result.chars().count(), 3);
    }
}
