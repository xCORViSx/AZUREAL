//! Output colorization for Claude messages
//!
//! Provides legacy colorization fallback when display_events is empty.

use ratatui::{
    layout::Alignment,
    style::{Color, Modifier, Style},
    text::{Line, Span},
};

use super::util::AZURE;

/// Orange color constant for Claude messages
pub const ORANGE: Color = Color::Rgb(255, 140, 0);

/// Strip ANSI escape codes from a string for pattern matching
pub fn strip_ansi(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            while let Some(&next) = chars.peek() {
                chars.next();
                if next.is_ascii_alphabetic() { break; }
            }
        } else {
            result.push(c);
        }
    }
    result
}

/// Message type for tracking transitions
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MessageType {
    User,
    Assistant,
    Other,
}

/// Detect message type from line content
pub fn detect_message_type(line: &str) -> MessageType {
    let stripped = strip_ansi(line);
    let trimmed = stripped.trim();
    if trimmed.starts_with("You:") || trimmed.starts_with("> ") || trimmed.starts_with("❯")
        || trimmed.starts_with("Human:") || trimmed.starts_with("[H]") {
        MessageType::User
    } else if trimmed.starts_with("Claude:") || trimmed.starts_with("Assistant:") || trimmed.starts_with("[A]") {
        MessageType::Assistant
    } else {
        MessageType::Other
    }
}

/// Colorization for Claude output lines with rich styling
/// This is the fallback when display_events is empty
pub fn colorize_output(line: &str) -> Line<'static> {
    let stripped = strip_ansi(line);
    let trimmed = stripped.trim();
    let line_owned = line.to_string();

    // User prompts - cyan background header
    if trimmed.starts_with("You:") || trimmed.starts_with("> ") || trimmed.starts_with("❯") {
        return Line::from(vec![
            Span::styled(" You ▶ ", Style::default().fg(Color::Black).bg(AZURE).add_modifier(Modifier::BOLD)),
            Span::styled(" ", Style::default()),
            Span::styled(trimmed.trim_start_matches("You:").trim_start_matches("> ").trim_start_matches("❯").trim().to_string(), Style::default().fg(Color::White)),
        ]).alignment(Alignment::Right);
    }

    // Human/user markers
    if trimmed.starts_with("Human:") || trimmed.starts_with("[H]") {
        return Line::from(vec![
            Span::styled(" You ▶ ", Style::default().fg(Color::Black).bg(AZURE).add_modifier(Modifier::BOLD)),
            Span::styled(" ", Style::default()),
            Span::styled(trimmed.trim_start_matches("Human:").trim_start_matches("[H]").trim().to_string(), Style::default().fg(Color::White)),
        ]).alignment(Alignment::Right);
    }

    // Claude/Assistant responses - check for tool use first
    if trimmed.starts_with("Claude:") || trimmed.starts_with("Assistant:") || trimmed.starts_with("[A]") {
        let content = trimmed
            .trim_start_matches("Claude:")
            .trim_start_matches("Assistant:")
            .trim_start_matches("[A]")
            .trim();

        if content.starts_with("[Using ") {
            let inner = content
                .trim_start_matches("[Using ")
                .trim_end_matches("...]")
                .trim_end_matches(']');
            let (tool_name, param) = if let Some(pipe_pos) = inner.find(" | ") {
                (&inner[..pipe_pos], Some(&inner[pipe_pos + 3..]))
            } else {
                (inner, None)
            };
            return Line::from(vec![
                Span::styled(" ┣━", Style::default().fg(AZURE)),
                Span::styled("● ", Style::default().fg(Color::Yellow)),
                Span::styled(tool_name.to_string(), Style::default().fg(AZURE).add_modifier(Modifier::BOLD)),
                Span::styled(param.map(|p| format!("  {}", p)).unwrap_or_default(), Style::default().fg(Color::DarkGray)),
            ]);
        }

        return Line::from(vec![
            Span::styled(" ◀ Claude ", Style::default().fg(Color::Black).bg(ORANGE).add_modifier(Modifier::BOLD)),
            Span::styled(" ", Style::default()),
            Span::styled(content.to_string(), Style::default().fg(Color::White)),
        ]);
    }

    // Standalone tool use line
    if trimmed.starts_with("[Using ") {
        let inner = trimmed
            .trim_start_matches("[Using ")
            .trim_end_matches("...]")
            .trim_end_matches(']');
        let (tool_name, param) = if let Some(pipe_pos) = inner.find(" | ") {
            (&inner[..pipe_pos], Some(&inner[pipe_pos + 3..]))
        } else {
            (inner, None)
        };
        return Line::from(vec![
            Span::styled(" ┣━", Style::default().fg(AZURE)),
            Span::styled("● ", Style::default().fg(Color::Yellow)),
            Span::styled(tool_name.to_string(), Style::default().fg(AZURE).add_modifier(Modifier::BOLD)),
            Span::styled(param.map(|p| format!("  {}", p)).unwrap_or_default(), Style::default().fg(Color::DarkGray)),
        ]);
    }

    // Done/completion markers
    if trimmed.starts_with("[Done:") || trimmed.starts_with("✓") || trimmed.starts_with("✔") {
        let result_text = trimmed
            .trim_start_matches("[Done:")
            .trim_start_matches("✓ ")
            .trim_start_matches("✔ ")
            .trim_end_matches(']');
        return Line::from(vec![
            Span::styled(" ┃  └─ ", Style::default().fg(AZURE)),
            Span::styled("✓ ", Style::default().fg(Color::Green)),
            Span::styled(result_text.to_string(), Style::default().fg(Color::DarkGray)),
        ]);
    }

    // Errors
    if trimmed.starts_with("Error") || trimmed.starts_with("error") || trimmed.starts_with("✗") || trimmed.starts_with("✘") {
        return Line::from(Span::styled(line_owned, Style::default().fg(Color::Red)));
    }

    // Code blocks (indented lines)
    if line.starts_with("    ") || line.starts_with("\t") || line.starts_with("│") {
        return Line::from(Span::styled(line_owned, Style::default().fg(Color::Yellow)));
    }

    // JSON/structured data
    if trimmed.starts_with('{') || trimmed.starts_with('[') {
        return Line::from(Span::styled(line_owned, Style::default().fg(AZURE)));
    }

    // Bullet points and lists
    if trimmed.starts_with("- ") || trimmed.starts_with("* ") || trimmed.starts_with("• ") {
        return Line::from(vec![
            Span::styled("  ", Style::default()),
            Span::styled(trimmed.to_string(), Style::default().fg(Color::White)),
        ]);
    }

    // Headers (markdown style)
    if trimmed.starts_with("# ") {
        return Line::from(Span::styled(trimmed.to_string(), Style::default().fg(AZURE).add_modifier(Modifier::BOLD)));
    }
    if trimmed.starts_with("## ") || trimmed.starts_with("### ") {
        return Line::from(Span::styled(trimmed.to_string(), Style::default().fg(AZURE)));
    }

    // File paths
    if trimmed.contains('/') && (trimmed.ends_with(".rs") || trimmed.ends_with(".ts") ||
        trimmed.ends_with(".py") || trimmed.ends_with(".js") || trimmed.ends_with(".md")) {
        return Line::from(Span::styled(line_owned, Style::default().fg(Color::Green).add_modifier(Modifier::UNDERLINED)));
    }

    // Default: white text
    Line::from(Span::styled(line_owned, Style::default().fg(Color::White)))
}

#[cfg(test)]
mod tests {
    use super::*;

    // ─── Helpers ──────────────────────────────────────────────────────
    fn spans_text(line: &Line) -> String {
        line.spans.iter().map(|s| s.content.as_ref()).collect()
    }

    fn has_color(line: &Line, color: Color) -> bool {
        line.spans.iter().any(|s| s.style.fg == Some(color))
    }

    fn has_bg(line: &Line, color: Color) -> bool {
        line.spans.iter().any(|s| s.style.bg == Some(color))
    }

    // ═══════════════════════════════════════════════════════════════════
    // strip_ansi
    // ═══════════════════════════════════════════════════════════════════

    #[test]
    fn strip_ansi_plain_text() {
        assert_eq!(strip_ansi("hello world"), "hello world");
    }

    #[test]
    fn strip_ansi_empty() {
        assert_eq!(strip_ansi(""), "");
    }

    #[test]
    fn strip_ansi_removes_color() {
        assert_eq!(strip_ansi("\x1b[31mred\x1b[0m"), "red");
    }

    #[test]
    fn strip_ansi_removes_bold() {
        assert_eq!(strip_ansi("\x1b[1mbold\x1b[0m"), "bold");
    }

    #[test]
    fn strip_ansi_multiple_codes() {
        assert_eq!(strip_ansi("\x1b[31m\x1b[1mred bold\x1b[0m"), "red bold");
    }

    #[test]
    fn strip_ansi_mixed_text() {
        assert_eq!(strip_ansi("before \x1b[32mgreen\x1b[0m after"), "before green after");
    }

    #[test]
    fn strip_ansi_no_escape_chars() {
        let text = "just plain text with special chars: @#$%^&*()";
        assert_eq!(strip_ansi(text), text);
    }

    #[test]
    fn strip_ansi_256_color() {
        assert_eq!(strip_ansi("\x1b[38;5;196mtext\x1b[0m"), "text");
    }

    #[test]
    fn strip_ansi_rgb_color() {
        assert_eq!(strip_ansi("\x1b[38;2;255;0;0mtext\x1b[0m"), "text");
    }

    #[test]
    fn strip_ansi_unicode_content() {
        assert_eq!(strip_ansi("\x1b[33m日本語\x1b[0m"), "日本語");
    }

    #[test]
    fn strip_ansi_consecutive_escapes() {
        assert_eq!(strip_ansi("\x1b[1m\x1b[31m\x1b[42mhello\x1b[0m"), "hello");
    }

    // ═══════════════════════════════════════════════════════════════════
    // detect_message_type — User markers
    // ═══════════════════════════════════════════════════════════════════

    #[test]
    fn detect_you_colon() {
        assert_eq!(detect_message_type("You: hello"), MessageType::User);
    }

    #[test]
    fn detect_angle_bracket() {
        assert_eq!(detect_message_type("> some input"), MessageType::User);
    }

    #[test]
    fn detect_chevron() {
        assert_eq!(detect_message_type("❯ command"), MessageType::User);
    }

    #[test]
    fn detect_human_colon() {
        assert_eq!(detect_message_type("Human: what is this"), MessageType::User);
    }

    #[test]
    fn detect_h_bracket() {
        assert_eq!(detect_message_type("[H] message"), MessageType::User);
    }

    #[test]
    fn detect_user_with_leading_space() {
        assert_eq!(detect_message_type("  You: hello"), MessageType::User);
    }

    #[test]
    fn detect_user_with_ansi() {
        assert_eq!(detect_message_type("\x1b[1mYou: hello\x1b[0m"), MessageType::User);
    }

    // ═══════════════════════════════════════════════════════════════════
    // detect_message_type — Assistant markers
    // ═══════════════════════════════════════════════════════════════════

    #[test]
    fn detect_claude_colon() {
        assert_eq!(detect_message_type("Claude: hi"), MessageType::Assistant);
    }

    #[test]
    fn detect_assistant_colon() {
        assert_eq!(detect_message_type("Assistant: response"), MessageType::Assistant);
    }

    #[test]
    fn detect_a_bracket() {
        assert_eq!(detect_message_type("[A] thinking"), MessageType::Assistant);
    }

    #[test]
    fn detect_assistant_with_leading_space() {
        assert_eq!(detect_message_type("  Claude: hi"), MessageType::Assistant);
    }

    #[test]
    fn detect_assistant_with_ansi() {
        assert_eq!(detect_message_type("\x1b[33mAssistant: hi\x1b[0m"), MessageType::Assistant);
    }

    // ═══════════════════════════════════════════════════════════════════
    // detect_message_type — Other
    // ═══════════════════════════════════════════════════════════════════

    #[test]
    fn detect_random_text() {
        assert_eq!(detect_message_type("just some text"), MessageType::Other);
    }

    #[test]
    fn detect_empty() {
        assert_eq!(detect_message_type(""), MessageType::Other);
    }

    #[test]
    fn detect_numbers() {
        assert_eq!(detect_message_type("12345"), MessageType::Other);
    }

    #[test]
    fn detect_partial_match_you() {
        // "YouTube:" starts with "You" but not "You:"
        assert_eq!(detect_message_type("YouTube: video"), MessageType::Other);
    }

    #[test]
    fn detect_partial_match_claude() {
        // "Claudette:" starts with "Claude" but not "Claude:"
        assert_eq!(detect_message_type("Claudette: hi"), MessageType::Other);
    }

    // ═══════════════════════════════════════════════════════════════════
    // colorize_output — User messages
    // ═══════════════════════════════════════════════════════════════════

    #[test]
    fn colorize_you_colon() {
        let line = colorize_output("You: hello");
        let text = spans_text(&line);
        assert!(text.contains("hello"));
        assert!(has_bg(&line, AZURE));
    }

    #[test]
    fn colorize_angle_bracket() {
        let line = colorize_output("> what is rust");
        let text = spans_text(&line);
        assert!(text.contains("what is rust"));
        assert!(has_bg(&line, AZURE));
    }

    #[test]
    fn colorize_chevron() {
        let line = colorize_output("❯ test command");
        let text = spans_text(&line);
        assert!(text.contains("test command"));
    }

    #[test]
    fn colorize_human() {
        let line = colorize_output("Human: how are you");
        let text = spans_text(&line);
        assert!(text.contains("how are you"));
        assert!(has_bg(&line, AZURE));
    }

    #[test]
    fn colorize_h_bracket() {
        let line = colorize_output("[H] a question");
        let text = spans_text(&line);
        assert!(text.contains("a question"));
        assert!(has_bg(&line, AZURE));
    }

    #[test]
    fn colorize_user_right_aligned() {
        let line = colorize_output("You: hi");
        assert_eq!(line.alignment, Some(Alignment::Right));
    }

    // ═══════════════════════════════════════════════════════════════════
    // colorize_output — Assistant messages
    // ═══════════════════════════════════════════════════════════════════

    #[test]
    fn colorize_claude_colon() {
        let line = colorize_output("Claude: hello");
        let text = spans_text(&line);
        assert!(text.contains("hello"));
        assert!(has_bg(&line, ORANGE));
    }

    #[test]
    fn colorize_assistant_colon() {
        let line = colorize_output("Assistant: response text");
        let text = spans_text(&line);
        assert!(text.contains("response text"));
        assert!(has_bg(&line, ORANGE));
    }

    #[test]
    fn colorize_a_bracket() {
        let line = colorize_output("[A] thinking about it");
        let text = spans_text(&line);
        assert!(text.contains("thinking about it"));
        assert!(has_bg(&line, ORANGE));
    }

    // ═══════════════════════════════════════════════════════════════════
    // colorize_output — Tool use
    // ═══════════════════════════════════════════════════════════════════

    #[test]
    fn colorize_tool_use_standalone() {
        let line = colorize_output("[Using Read | /path/to/file]");
        let text = spans_text(&line);
        assert!(text.contains("Read"));
    }

    #[test]
    fn colorize_tool_use_no_param() {
        let line = colorize_output("[Using Bash]");
        let text = spans_text(&line);
        assert!(text.contains("Bash"));
    }

    #[test]
    fn colorize_tool_use_with_pipe() {
        let line = colorize_output("[Using Read | /some/file.rs]");
        let text = spans_text(&line);
        assert!(text.contains("Read"));
        assert!(text.contains("/some/file.rs"));
    }

    #[test]
    fn colorize_tool_use_from_assistant() {
        let line = colorize_output("Claude: [Using Read | /path]");
        let text = spans_text(&line);
        assert!(text.contains("Read"));
    }

    // ═══════════════════════════════════════════════════════════════════
    // colorize_output — Done markers
    // ═══════════════════════════════════════════════════════════════════

    #[test]
    fn colorize_done_marker() {
        let line = colorize_output("[Done: 5.0s, $0.01]");
        let text = spans_text(&line);
        assert!(text.contains("5.0s"));
        assert!(has_color(&line, Color::Green));
    }

    #[test]
    fn colorize_checkmark() {
        let line = colorize_output("✓ completed");
        assert!(has_color(&line, Color::Green));
    }

    #[test]
    fn colorize_heavy_checkmark() {
        let line = colorize_output("✔ done");
        assert!(has_color(&line, Color::Green));
    }

    // ═══════════════════════════════════════════════════════════════════
    // colorize_output — Errors
    // ═══════════════════════════════════════════════════════════════════

    #[test]
    fn colorize_error_uppercase() {
        let line = colorize_output("Error: something went wrong");
        assert!(has_color(&line, Color::Red));
    }

    #[test]
    fn colorize_error_lowercase() {
        let line = colorize_output("error[E0308]: mismatched types");
        assert!(has_color(&line, Color::Red));
    }

    #[test]
    fn colorize_x_mark() {
        let line = colorize_output("✗ failed");
        assert!(has_color(&line, Color::Red));
    }

    #[test]
    fn colorize_heavy_x_mark() {
        let line = colorize_output("✘ error occurred");
        assert!(has_color(&line, Color::Red));
    }

    // ═══════════════════════════════════════════════════════════════════
    // colorize_output — Code blocks (indented)
    // ═══════════════════════════════════════════════════════════════════

    #[test]
    fn colorize_four_space_indent() {
        let line = colorize_output("    fn main() {}");
        assert!(has_color(&line, Color::Yellow));
    }

    #[test]
    fn colorize_tab_indent() {
        let line = colorize_output("\tlet x = 1;");
        assert!(has_color(&line, Color::Yellow));
    }

    #[test]
    fn colorize_pipe_char() {
        let line = colorize_output("│ code");
        assert!(has_color(&line, Color::Yellow));
    }

    // ═══════════════════════════════════════════════════════════════════
    // colorize_output — JSON / structured data
    // ═══════════════════════════════════════════════════════════════════

    #[test]
    fn colorize_json_object() {
        let line = colorize_output("{\"key\": \"value\"}");
        assert!(has_color(&line, AZURE));
    }

    #[test]
    fn colorize_json_array() {
        let line = colorize_output("[1, 2, 3]");
        assert!(has_color(&line, AZURE));
    }

    // ═══════════════════════════════════════════════════════════════════
    // colorize_output — Bullet points
    // ═══════════════════════════════════════════════════════════════════

    #[test]
    fn colorize_dash_bullet() {
        let line = colorize_output("- item one");
        let text = spans_text(&line);
        assert!(text.contains("- item one"));
        assert!(has_color(&line, Color::White));
    }

    #[test]
    fn colorize_asterisk_bullet() {
        // Note: `* item` begins with `* ` which triggers bullet point, not italic
        let line = colorize_output("* item two");
        let text = spans_text(&line);
        assert!(text.contains("* item two"));
    }

    #[test]
    fn colorize_bullet_point_char() {
        let line = colorize_output("• item three");
        let text = spans_text(&line);
        assert!(text.contains("• item three"));
    }

    // ═══════════════════════════════════════════════════════════════════
    // colorize_output — Headers
    // ═══════════════════════════════════════════════════════════════════

    #[test]
    fn colorize_h1() {
        let line = colorize_output("# Title");
        assert!(has_color(&line, AZURE));
        // h1 gets bold modifier
        assert!(line.spans.iter().any(|s|
            s.style.add_modifier.contains(Modifier::BOLD)
        ));
    }

    #[test]
    fn colorize_h2() {
        let line = colorize_output("## Subtitle");
        assert!(has_color(&line, AZURE));
    }

    #[test]
    fn colorize_h3() {
        let line = colorize_output("### Section");
        assert!(has_color(&line, AZURE));
    }

    // ═══════════════════════════════════════════════════════════════════
    // colorize_output — File paths
    // ═══════════════════════════════════════════════════════════════════

    #[test]
    fn colorize_rust_path() {
        let line = colorize_output("src/main.rs");
        assert!(has_color(&line, Color::Green));
    }

    #[test]
    fn colorize_ts_path() {
        let line = colorize_output("src/index.ts");
        assert!(has_color(&line, Color::Green));
    }

    #[test]
    fn colorize_py_path() {
        let line = colorize_output("app/main.py");
        assert!(has_color(&line, Color::Green));
    }

    #[test]
    fn colorize_js_path() {
        let line = colorize_output("lib/util.js");
        assert!(has_color(&line, Color::Green));
    }

    #[test]
    fn colorize_md_path() {
        let line = colorize_output("docs/README.md");
        assert!(has_color(&line, Color::Green));
    }

    #[test]
    fn colorize_path_underlined() {
        let line = colorize_output("src/main.rs");
        assert!(line.spans.iter().any(|s|
            s.style.add_modifier.contains(Modifier::UNDERLINED)
        ));
    }

    // ═══════════════════════════════════════════════════════════════════
    // colorize_output — Default text
    // ═══════════════════════════════════════════════════════════════════

    #[test]
    fn colorize_default_text() {
        let line = colorize_output("just regular text");
        assert!(has_color(&line, Color::White));
    }

    #[test]
    fn colorize_default_empty() {
        let line = colorize_output("");
        assert!(has_color(&line, Color::White));
    }

    #[test]
    fn colorize_default_numbers_only() {
        let line = colorize_output("42");
        assert!(has_color(&line, Color::White));
    }
}
