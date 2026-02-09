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
#[derive(Clone, Copy, PartialEq)]
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
