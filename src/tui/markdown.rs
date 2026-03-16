//! Markdown parsing utilities for TUI rendering
//!
//! Handles inline markdown (bold, italic, code) and table formatting.

use ratatui::{
    style::{Color, Modifier, Style},
    text::Span,
};

/// Parse inline markdown (bold, italic, inline code) into styled spans
pub fn parse_markdown_spans(text: &str, base_style: Style) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    let mut chars = text.char_indices().peekable();
    let mut current_text = String::new();

    while let Some((i, c)) = chars.next() {
        match c {
            '`' => {
                if !current_text.is_empty() {
                    spans.push(Span::styled(current_text.clone(), base_style));
                    current_text.clear();
                }
                let mut code = String::new();
                for (_, ch) in chars.by_ref() {
                    if ch == '`' {
                        break;
                    }
                    code.push(ch);
                }
                if !code.is_empty() {
                    spans.push(Span::styled(
                        code,
                        Style::default()
                            .fg(Color::Yellow)
                            .bg(Color::Rgb(40, 40, 40)),
                    ));
                }
            }
            '*' => {
                if chars.peek().map(|(_, ch)| *ch == '*').unwrap_or(false) {
                    chars.next();
                    if !current_text.is_empty() {
                        spans.push(Span::styled(current_text.clone(), base_style));
                        current_text.clear();
                    }
                    let mut bold_text = String::new();
                    while let Some((_, ch)) = chars.next() {
                        if ch == '*' && chars.peek().map(|(_, c)| *c == '*').unwrap_or(false) {
                            chars.next();
                            break;
                        }
                        bold_text.push(ch);
                    }
                    if !bold_text.is_empty() {
                        spans.push(Span::styled(
                            bold_text,
                            base_style.add_modifier(Modifier::BOLD),
                        ));
                    }
                } else {
                    let rest: String = text[i + 1..]
                        .chars()
                        .take_while(|&ch| ch != ' ' && ch != '\n')
                        .collect();
                    if rest.contains('*') && !rest.starts_with(' ') {
                        if !current_text.is_empty() {
                            spans.push(Span::styled(current_text.clone(), base_style));
                            current_text.clear();
                        }
                        let mut italic_text = String::new();
                        for (_, ch) in chars.by_ref() {
                            if ch == '*' {
                                break;
                            }
                            italic_text.push(ch);
                        }
                        if !italic_text.is_empty() {
                            spans.push(Span::styled(
                                italic_text,
                                base_style.add_modifier(Modifier::ITALIC),
                            ));
                        }
                    } else {
                        current_text.push(c);
                    }
                }
            }
            _ => current_text.push(c),
        }
    }

    if !current_text.is_empty() {
        spans.push(Span::styled(current_text, base_style));
    }
    if spans.is_empty() {
        spans.push(Span::styled("", base_style));
    }
    spans
}

/// Check if a line is a markdown table separator
pub fn is_table_separator(line: &str) -> bool {
    let trimmed = line.trim();
    trimmed.contains('|')
        && trimmed
            .chars()
            .all(|c| c == '|' || c == '-' || c == ':' || c == ' ')
}

#[cfg(test)]
mod tests {
    use super::*;

    // ─── Helper ───────────────────────────────────────────────────────
    fn base() -> Style {
        Style::default()
    }
    fn code_style() -> Style {
        Style::default()
            .fg(Color::Yellow)
            .bg(Color::Rgb(40, 40, 40))
    }
    fn bold(s: Style) -> Style {
        s.add_modifier(Modifier::BOLD)
    }
    fn italic(s: Style) -> Style {
        s.add_modifier(Modifier::ITALIC)
    }

    fn texts<'a>(spans: &'a [Span<'a>]) -> Vec<&'a str> {
        spans.iter().map(|s| s.content.as_ref()).collect()
    }

    // ═══════════════════════════════════════════════════════════════════
    // parse_markdown_spans — Plain text (no markdown)
    // ═══════════════════════════════════════════════════════════════════

    #[test]
    fn plain_hello() {
        let spans = parse_markdown_spans("hello world", base());
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].content.as_ref(), "hello world");
        assert_eq!(spans[0].style, base());
    }

    #[test]
    fn plain_numbers() {
        let spans = parse_markdown_spans("12345", base());
        assert_eq!(texts(&spans), vec!["12345"]);
    }

    #[test]
    fn plain_punctuation() {
        let spans = parse_markdown_spans("a, b; c! d? e.", base());
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].content.as_ref(), "a, b; c! d? e.");
    }

    #[test]
    fn empty_string_returns_empty_span() {
        let spans = parse_markdown_spans("", base());
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].content.as_ref(), "");
    }

    #[test]
    fn whitespace_only() {
        let spans = parse_markdown_spans("   ", base());
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].content.as_ref(), "   ");
    }

    // ═══════════════════════════════════════════════════════════════════
    // parse_markdown_spans — Inline code (backticks)
    // ═══════════════════════════════════════════════════════════════════

    #[test]
    fn inline_code_simple() {
        let spans = parse_markdown_spans("run `cargo test` now", base());
        assert_eq!(texts(&spans), vec!["run ", "cargo test", " now"]);
        assert_eq!(spans[1].style, code_style());
    }

    #[test]
    fn inline_code_at_start() {
        let spans = parse_markdown_spans("`code` at start", base());
        assert_eq!(texts(&spans), vec!["code", " at start"]);
        assert_eq!(spans[0].style, code_style());
    }

    #[test]
    fn inline_code_at_end() {
        let spans = parse_markdown_spans("ends with `code`", base());
        assert_eq!(texts(&spans), vec!["ends with ", "code"]);
        assert_eq!(spans[1].style, code_style());
    }

    #[test]
    fn inline_code_multiple() {
        let spans = parse_markdown_spans("`a` and `b`", base());
        assert_eq!(texts(&spans), vec!["a", " and ", "b"]);
        assert_eq!(spans[0].style, code_style());
        assert_eq!(spans[2].style, code_style());
    }

    #[test]
    fn inline_code_with_spaces() {
        let spans = parse_markdown_spans("`hello world`", base());
        assert_eq!(texts(&spans), vec!["hello world"]);
        assert_eq!(spans[0].style, code_style());
    }

    #[test]
    fn inline_code_with_special_chars() {
        let spans = parse_markdown_spans("`fn main() {}`", base());
        assert_eq!(texts(&spans), vec!["fn main() {}"]);
        assert_eq!(spans[0].style, code_style());
    }

    // ═══════════════════════════════════════════════════════════════════
    // parse_markdown_spans — Bold (**double asterisks**)
    // ═══════════════════════════════════════════════════════════════════

    #[test]
    fn bold_simple() {
        let spans = parse_markdown_spans("this is **bold** text", base());
        assert_eq!(texts(&spans), vec!["this is ", "bold", " text"]);
        assert_eq!(spans[1].style, bold(base()));
    }

    #[test]
    fn bold_at_start() {
        let spans = parse_markdown_spans("**bold** start", base());
        assert_eq!(texts(&spans), vec!["bold", " start"]);
        assert_eq!(spans[0].style, bold(base()));
    }

    #[test]
    fn bold_at_end() {
        let spans = parse_markdown_spans("end **bold**", base());
        assert_eq!(texts(&spans), vec!["end ", "bold"]);
        assert_eq!(spans[1].style, bold(base()));
    }

    #[test]
    fn bold_whole_string() {
        let spans = parse_markdown_spans("**everything**", base());
        assert_eq!(texts(&spans), vec!["everything"]);
        assert_eq!(spans[0].style, bold(base()));
    }

    #[test]
    fn bold_multiple() {
        let spans = parse_markdown_spans("**one** and **two**", base());
        assert_eq!(texts(&spans), vec!["one", " and ", "two"]);
        assert_eq!(spans[0].style, bold(base()));
        assert_eq!(spans[2].style, bold(base()));
    }

    #[test]
    fn bold_with_spaces() {
        let spans = parse_markdown_spans("**hello world**", base());
        assert_eq!(texts(&spans), vec!["hello world"]);
        assert_eq!(spans[0].style, bold(base()));
    }

    // ═══════════════════════════════════════════════════════════════════
    // parse_markdown_spans — Italic (*single asterisks*)
    // ═══════════════════════════════════════════════════════════════════

    #[test]
    fn italic_simple() {
        let spans = parse_markdown_spans("this is *italic* text", base());
        assert_eq!(texts(&spans), vec!["this is ", "italic", " text"]);
        assert_eq!(spans[1].style, italic(base()));
    }

    #[test]
    fn italic_at_start() {
        let spans = parse_markdown_spans("*italic* start", base());
        assert_eq!(texts(&spans), vec!["italic", " start"]);
        assert_eq!(spans[0].style, italic(base()));
    }

    #[test]
    fn italic_at_end() {
        let spans = parse_markdown_spans("end *italic*", base());
        assert_eq!(texts(&spans), vec!["end ", "italic"]);
        assert_eq!(spans[1].style, italic(base()));
    }

    #[test]
    fn italic_whole_string() {
        let spans = parse_markdown_spans("*everything*", base());
        assert_eq!(texts(&spans), vec!["everything"]);
        assert_eq!(spans[0].style, italic(base()));
    }

    // ═══════════════════════════════════════════════════════════════════
    // parse_markdown_spans — Mixed styles
    // ═══════════════════════════════════════════════════════════════════

    #[test]
    fn bold_then_code() {
        let spans = parse_markdown_spans("**bold** and `code`", base());
        assert_eq!(texts(&spans), vec!["bold", " and ", "code"]);
        assert_eq!(spans[0].style, bold(base()));
        assert_eq!(spans[2].style, code_style());
    }

    #[test]
    fn code_then_bold() {
        let spans = parse_markdown_spans("`code` then **bold**", base());
        assert_eq!(texts(&spans), vec!["code", " then ", "bold"]);
        assert_eq!(spans[0].style, code_style());
        assert_eq!(spans[2].style, bold(base()));
    }

    #[test]
    fn italic_then_code() {
        let spans = parse_markdown_spans("*italic* and `code`", base());
        assert_eq!(texts(&spans), vec!["italic", " and ", "code"]);
        assert_eq!(spans[0].style, italic(base()));
        assert_eq!(spans[2].style, code_style());
    }

    #[test]
    fn bold_then_italic() {
        let spans = parse_markdown_spans("**bold** *italic*", base());
        assert_eq!(texts(&spans), vec!["bold", " ", "italic"]);
        assert_eq!(spans[0].style, bold(base()));
        assert_eq!(spans[2].style, italic(base()));
    }

    #[test]
    fn all_three_styles() {
        let spans = parse_markdown_spans("**bold** *italic* `code`", base());
        assert_eq!(texts(&spans), vec!["bold", " ", "italic", " ", "code"]);
        assert_eq!(spans[0].style, bold(base()));
        assert_eq!(spans[2].style, italic(base()));
        assert_eq!(spans[4].style, code_style());
    }

    // ═══════════════════════════════════════════════════════════════════
    // parse_markdown_spans — Edge cases
    // ═══════════════════════════════════════════════════════════════════

    #[test]
    fn unclosed_backtick() {
        // Unclosed backtick: everything after the ` is consumed as code
        let spans = parse_markdown_spans("open `code never closed", base());
        // The backtick opens code mode and consumes to end
        assert!(spans
            .iter()
            .any(|s| s.content.contains("code never closed")));
    }

    #[test]
    fn unclosed_bold() {
        // **bold never closed — consumed to end
        let spans = parse_markdown_spans("open **bold never closed", base());
        assert!(spans
            .iter()
            .any(|s| s.content.contains("bold never closed")));
    }

    #[test]
    fn just_two_asterisks() {
        // ** alone — opens bold but no content
        let spans = parse_markdown_spans("**", base());
        // Should produce at least one span (even if empty)
        assert!(!spans.is_empty());
    }

    #[test]
    fn just_one_asterisk_with_space() {
        // Single asterisk followed by space — not italic
        let spans = parse_markdown_spans("* item", base());
        // The `*` is followed by ` item`; there is no closing `*`, so
        // whether it's treated as italic or plain depends on the look-ahead.
        // The function checks if the rest (before next space) contains `*`.
        // "item" doesn't contain `*`, so `*` should be treated as plain text.
        assert!(spans.iter().any(|s| s.content.contains("*")));
    }

    #[test]
    fn empty_backtick_pair() {
        // `` — empty code, should still produce a span (empty code is skipped)
        let spans = parse_markdown_spans("``", base());
        assert!(!spans.is_empty());
    }

    #[test]
    fn empty_bold_pair() {
        // **** — empty bold, should still produce a span
        let spans = parse_markdown_spans("****", base());
        assert!(!spans.is_empty());
    }

    #[test]
    fn asterisk_in_math_context() {
        // 2*3 should not be treated as italic because there's no closing * before space
        let spans = parse_markdown_spans("2*3 equals 6", base());
        // After `*`, rest before next space is "3" which doesn't contain `*`
        // so `*` is treated as plain text
        let full: String = spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(full.contains("2*3"));
    }

    #[test]
    fn asterisk_multiplication_no_italic() {
        let spans = parse_markdown_spans("multiply 5*10 here", base());
        let full: String = spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(full.contains("5*10"));
    }

    #[test]
    fn consecutive_styled_regions() {
        let spans = parse_markdown_spans("**a****b**", base());
        // First **a** then **b**
        assert!(spans.iter().any(|s| s.content.as_ref() == "a"));
        assert!(spans.iter().any(|s| s.content.as_ref() == "b"));
    }

    #[test]
    fn consecutive_code_regions() {
        let spans = parse_markdown_spans("`a``b`", base());
        assert!(spans.iter().any(|s| s.content.as_ref() == "a"));
        assert!(spans.iter().any(|s| s.content.as_ref() == "b"));
    }

    // ═══════════════════════════════════════════════════════════════════
    // parse_markdown_spans — Unicode
    // ═══════════════════════════════════════════════════════════════════

    #[test]
    fn unicode_plain() {
        let spans = parse_markdown_spans("日本語テキスト", base());
        assert_eq!(texts(&spans), vec!["日本語テキスト"]);
    }

    #[test]
    fn unicode_with_bold() {
        let spans = parse_markdown_spans("**日本語**テキスト", base());
        assert_eq!(texts(&spans), vec!["日本語", "テキスト"]);
        assert_eq!(spans[0].style, bold(base()));
    }

    #[test]
    fn unicode_with_code() {
        let spans = parse_markdown_spans("use `λ` in code", base());
        assert_eq!(texts(&spans), vec!["use ", "λ", " in code"]);
        assert_eq!(spans[1].style, code_style());
    }

    #[test]
    fn emoji_with_markdown() {
        let spans = parse_markdown_spans("**🎉** party", base());
        assert!(spans.iter().any(|s| s.content.contains("🎉")));
    }

    // ═══════════════════════════════════════════════════════════════════
    // parse_markdown_spans — Long text
    // ═══════════════════════════════════════════════════════════════════

    #[test]
    fn very_long_plain_text() {
        let long = "a".repeat(10_000);
        let spans = parse_markdown_spans(&long, base());
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].content.len(), 10_000);
    }

    #[test]
    fn very_long_with_bold() {
        let long = format!("start **{}** end", "x".repeat(5_000));
        let spans = parse_markdown_spans(&long, base());
        let bold_span = spans.iter().find(|s| s.style == bold(base()));
        assert!(bold_span.is_some());
        assert_eq!(bold_span.unwrap().content.len(), 5_000);
    }

    // ═══════════════════════════════════════════════════════════════════
    // parse_markdown_spans — Multiple segments in one line
    // ═══════════════════════════════════════════════════════════════════

    #[test]
    fn multiple_bold_segments() {
        let spans = parse_markdown_spans("**a** b **c** d **e**", base());
        let bold_spans: Vec<_> = spans.iter().filter(|s| s.style == bold(base())).collect();
        assert_eq!(bold_spans.len(), 3);
    }

    #[test]
    fn multiple_code_segments() {
        let spans = parse_markdown_spans("`a` b `c` d `e`", base());
        let code_spans: Vec<_> = spans.iter().filter(|s| s.style == code_style()).collect();
        assert_eq!(code_spans.len(), 3);
    }

    #[test]
    fn alternating_styles() {
        let spans = parse_markdown_spans("**bold** `code` **bold** `code`", base());
        assert!(spans.len() >= 4);
    }

    // ═══════════════════════════════════════════════════════════════════
    // parse_markdown_spans — Whitespace handling
    // ═══════════════════════════════════════════════════════════════════

    #[test]
    fn leading_whitespace_preserved() {
        let spans = parse_markdown_spans("  hello", base());
        assert_eq!(spans[0].content.as_ref(), "  hello");
    }

    #[test]
    fn trailing_whitespace_preserved() {
        let spans = parse_markdown_spans("hello  ", base());
        let full: String = spans.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(full, "hello  ");
    }

    #[test]
    fn whitespace_around_bold() {
        let spans = parse_markdown_spans("  **bold**  ", base());
        let full: String = spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(full.starts_with("  "));
        assert!(full.ends_with("  "));
    }

    #[test]
    fn tabs_in_text() {
        let spans = parse_markdown_spans("a\tb\tc", base());
        let full: String = spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(full.contains('\t'));
    }

    // ═══════════════════════════════════════════════════════════════════
    // parse_markdown_spans — Base style propagation
    // ═══════════════════════════════════════════════════════════════════

    #[test]
    fn custom_base_style_on_plain() {
        let custom = Style::default().fg(Color::Red);
        let spans = parse_markdown_spans("hello", custom);
        assert_eq!(spans[0].style, custom);
    }

    #[test]
    fn custom_base_style_on_bold() {
        let custom = Style::default().fg(Color::Red);
        let spans = parse_markdown_spans("**bold**", custom);
        assert_eq!(spans[0].style, custom.add_modifier(Modifier::BOLD));
    }

    #[test]
    fn custom_base_style_on_italic() {
        let custom = Style::default().fg(Color::Green);
        let spans = parse_markdown_spans("*italic*", custom);
        assert_eq!(spans[0].style, custom.add_modifier(Modifier::ITALIC));
    }

    // ═══════════════════════════════════════════════════════════════════
    // is_table_separator — Valid separators
    // ═══════════════════════════════════════════════════════════════════

    #[test]
    fn table_sep_basic() {
        assert!(is_table_separator("|---|---|"));
    }

    #[test]
    fn table_sep_with_spaces() {
        assert!(is_table_separator("| --- | --- |"));
    }

    #[test]
    fn table_sep_aligned_left() {
        assert!(is_table_separator("|:---|:---|"));
    }

    #[test]
    fn table_sep_aligned_center() {
        assert!(is_table_separator("|:---:|:---:|"));
    }

    #[test]
    fn table_sep_aligned_right() {
        assert!(is_table_separator("|---:|---:|"));
    }

    #[test]
    fn table_sep_mixed_alignment() {
        assert!(is_table_separator("|:---|:---:|---:|"));
    }

    #[test]
    fn table_sep_leading_trailing_whitespace() {
        assert!(is_table_separator("  |---|---|  "));
    }

    #[test]
    fn table_sep_lots_of_dashes() {
        assert!(is_table_separator("|------------|------------|"));
    }

    // ═══════════════════════════════════════════════════════════════════
    // is_table_separator — Invalid inputs
    // ═══════════════════════════════════════════════════════════════════

    #[test]
    fn table_sep_regular_text() {
        assert!(!is_table_separator("hello world"));
    }

    #[test]
    fn table_sep_empty() {
        assert!(!is_table_separator(""));
    }

    #[test]
    fn table_sep_just_pipes() {
        // "|" contains '|' and all chars pass the predicate
        assert!(is_table_separator("|"));
    }

    #[test]
    fn table_sep_just_dashes() {
        assert!(!is_table_separator("---"));
    }

    #[test]
    fn table_sep_whitespace_only() {
        assert!(!is_table_separator("   "));
    }

    #[test]
    fn table_sep_mixed_content() {
        assert!(!is_table_separator("| hello | world |"));
    }

    #[test]
    fn table_sep_data_row() {
        assert!(!is_table_separator("| foo | bar |"));
    }

    #[test]
    fn table_sep_with_letters() {
        assert!(!is_table_separator("|---a---|"));
    }
}
