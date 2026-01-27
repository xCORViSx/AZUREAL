//! Utility functions for TUI rendering

use ratatui::{
    layout::Alignment,
    style::{Color, Modifier, Style},
    text::{Line, Span},
};

use crate::events::DisplayEvent;

/// Orange color constant for Claude messages
const ORANGE: Color = Color::Rgb(255, 140, 0);

/// Truncate a string to max length, adding ellipsis if needed
pub fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max { s.to_string() } else { format!("{}…", &s[..max - 1]) }
}

/// Check if output is scrolled to bottom
pub fn is_scrolled_to_bottom(output_scroll: usize, output_lines_len: usize) -> bool {
    if output_scroll == usize::MAX { return true; }
    if output_lines_len == 0 { return true; }
    output_scroll + 5 >= output_lines_len.saturating_sub(20)
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
    let trimmed = line.trim();
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
    let trimmed = line.trim();
    let line_owned = line.to_string();

    // User prompts - cyan background header
    if trimmed.starts_with("You:") || trimmed.starts_with("> ") || trimmed.starts_with("❯") {
        return Line::from(vec![
            Span::styled(" You ▶ ", Style::default().fg(Color::Black).bg(Color::Cyan).add_modifier(Modifier::BOLD)),
            Span::styled(" ", Style::default()),
            Span::styled(trimmed.trim_start_matches("You:").trim_start_matches("> ").trim_start_matches("❯").trim().to_string(), Style::default().fg(Color::White)),
        ]).alignment(Alignment::Right);
    }

    // Human/user markers
    if trimmed.starts_with("Human:") || trimmed.starts_with("[H]") {
        return Line::from(vec![
            Span::styled(" You ▶ ", Style::default().fg(Color::Black).bg(Color::Cyan).add_modifier(Modifier::BOLD)),
            Span::styled(" ", Style::default()),
            Span::styled(trimmed.trim_start_matches("Human:").trim_start_matches("[H]").trim().to_string(), Style::default().fg(Color::White)),
        ]).alignment(Alignment::Right);
    }

    // Claude/Assistant responses - orange background header
    if trimmed.starts_with("Claude:") || trimmed.starts_with("Assistant:") || trimmed.starts_with("[A]") {
        return Line::from(vec![
            Span::styled(" ◀ Claude ", Style::default().fg(Color::Black).bg(ORANGE).add_modifier(Modifier::BOLD)),
            Span::styled(" ", Style::default()),
            Span::styled(trimmed.trim_start_matches("Claude:").trim_start_matches("Assistant:").trim_start_matches("[A]").trim().to_string(), Style::default().fg(Color::White)),
        ]);
    }

    // Tool usage indicators
    if trimmed.starts_with("[Using ") || trimmed.contains("Tool:") || trimmed.starts_with("⏺") {
        return Line::from(vec![
            Span::styled("  🔧 ", Style::default().fg(Color::Cyan)),
            Span::styled(trimmed.trim_start_matches("[Using ").trim_end_matches("...]").to_string(),
                Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        ]);
    }

    // Done/completion markers
    if trimmed.starts_with("[Done:") || trimmed.starts_with("✓") || trimmed.starts_with("✔") {
        return Line::from(vec![
            Span::styled("  ✓ ", Style::default().fg(Color::Green)),
            Span::styled(trimmed.to_string(), Style::default().fg(Color::Green)),
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
        return Line::from(Span::styled(line_owned, Style::default().fg(Color::Cyan)));
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
        return Line::from(Span::styled(trimmed.to_string(), Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)));
    }
    if trimmed.starts_with("## ") || trimmed.starts_with("### ") {
        return Line::from(Span::styled(trimmed.to_string(), Style::default().fg(Color::Cyan)));
    }

    // File paths
    if trimmed.contains('/') && (trimmed.ends_with(".rs") || trimmed.ends_with(".ts") ||
        trimmed.ends_with(".py") || trimmed.ends_with(".js") || trimmed.ends_with(".md")) {
        return Line::from(Span::styled(line_owned, Style::default().fg(Color::Green).add_modifier(Modifier::UNDERLINED)));
    }

    // Default: white text
    Line::from(Span::styled(line_owned, Style::default().fg(Color::White)))
}

/// Render DisplayEvents into Lines for the output panel with iMessage-style layout
/// User messages are right-aligned (cyan), Claude messages are left-aligned (orange)
pub fn render_display_events(events: &[DisplayEvent], width: u16) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    let w = width as usize;
    let bubble_width = (w * 2 / 3).max(40); // Message bubbles take 2/3 of width

    for event in events {
        match event {
            DisplayEvent::Init { model, cwd, .. } => {
                lines.push(Line::from(""));
                lines.push(Line::from(vec![
                    Span::styled(" Session Started ", Style::default().fg(Color::Black).bg(Color::Green).add_modifier(Modifier::BOLD)),
                ]).alignment(Alignment::Center));
                lines.push(Line::from(vec![
                    Span::styled("Model: ", Style::default().fg(Color::DarkGray)),
                    Span::styled(model.clone(), Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                ]).alignment(Alignment::Center));
                lines.push(Line::from(vec![
                    Span::styled("Path: ", Style::default().fg(Color::DarkGray)),
                    Span::styled(cwd.clone(), Style::default().fg(Color::White)),
                ]).alignment(Alignment::Center));
                lines.push(Line::from(""));
            }
            DisplayEvent::Hook { name, output } => {
                if !output.trim().is_empty() {
                    lines.push(Line::from(vec![
                        Span::styled("⚡ ", Style::default().fg(Color::Yellow)),
                        Span::styled(name.clone(), Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                    ]));
                    for line in output.lines() {
                        lines.push(Line::from(Span::styled(line.to_string(), Style::default().fg(Color::DarkGray))));
                    }
                    lines.push(Line::from(""));
                }
            }
            DisplayEvent::UserMessage { content, .. } => {
                // Two blank lines before user message
                lines.push(Line::from(""));
                lines.push(Line::from(""));

                // Right-aligned header with cyan background
                let header = format!(" You ▶ ");
                let header_pad = " ".repeat(bubble_width.saturating_sub(header.len()));
                lines.push(Line::from(vec![
                    Span::styled(header_pad, Style::default().bg(Color::Cyan)),
                    Span::styled(header, Style::default().fg(Color::Black).bg(Color::Cyan).add_modifier(Modifier::BOLD)),
                ]).alignment(Alignment::Right));

                // Right-aligned content
                for line in content.lines() {
                    let text = line.to_string();
                    let padded = if text.len() < bubble_width - 4 {
                        format!("{:>width$} │", text, width = bubble_width - 3)
                    } else {
                        format!("{} │", text)
                    };
                    lines.push(Line::from(vec![
                        Span::styled(padded, Style::default().fg(Color::White)),
                    ]).alignment(Alignment::Right));
                }

                // Right-aligned bottom border
                lines.push(Line::from(vec![
                    Span::styled(format!("{}┘", "─".repeat(bubble_width - 1)), Style::default().fg(Color::Cyan)),
                ]).alignment(Alignment::Right));
            }
            DisplayEvent::AssistantText { text, .. } => {
                // Two blank lines before assistant message
                lines.push(Line::from(""));
                lines.push(Line::from(""));

                // Left-aligned header with orange background
                let header = format!(" ◀ Claude ");
                let header_pad = " ".repeat(bubble_width.saturating_sub(header.len()));
                lines.push(Line::from(vec![
                    Span::styled(header, Style::default().fg(Color::Black).bg(ORANGE).add_modifier(Modifier::BOLD)),
                    Span::styled(header_pad, Style::default().bg(ORANGE)),
                ]));

                // Left-aligned content
                let mut in_code_block = false;
                for line in text.lines() {
                    if line.trim().starts_with("```") {
                        in_code_block = !in_code_block;
                        lines.push(Line::from(vec![
                            Span::styled("│ ", Style::default().fg(ORANGE)),
                            Span::styled(line.to_string(), Style::default().fg(Color::Yellow)),
                        ]));
                        continue;
                    }

                    let content_style = if in_code_block {
                        Style::default().fg(Color::Yellow)
                    } else if line.trim().starts_with("- ") || line.trim().starts_with("* ") {
                        Style::default().fg(Color::Cyan)
                    } else if line.trim().starts_with('#') {
                        Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(Color::White)
                    };

                    lines.push(Line::from(vec![
                        Span::styled("│ ", Style::default().fg(ORANGE)),
                        Span::styled(line.to_string(), content_style),
                    ]));
                }

                // Left-aligned bottom border
                lines.push(Line::from(vec![
                    Span::styled(format!("└{}", "─".repeat(bubble_width - 1)), Style::default().fg(ORANGE)),
                ]));
            }
            DisplayEvent::ToolCall { tool_name, file_path, input, .. } => {
                lines.push(Line::from(""));
                lines.push(Line::from(vec![
                    Span::styled("  🔧 ", Style::default().fg(Color::Cyan)),
                    Span::styled(tool_name.clone(), Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                ]));
                if let Some(path) = file_path {
                    lines.push(Line::from(vec![
                        Span::styled("     → ", Style::default().fg(Color::DarkGray)),
                        Span::styled(path.clone(), Style::default().fg(Color::Green).add_modifier(Modifier::UNDERLINED)),
                    ]));
                } else {
                    let input_str = serde_json::to_string(input).unwrap_or_default();
                    let truncated = if input_str.len() > 80 { format!("{}...", &input_str[..77]) } else { input_str };
                    lines.push(Line::from(vec![
                        Span::styled("     ", Style::default()),
                        Span::styled(truncated, Style::default().fg(Color::DarkGray)),
                    ]));
                }
            }
            DisplayEvent::ToolResult { success, output, .. } => {
                let (icon, color) = if *success { ("✓", Color::Green) } else { ("✗", Color::Red) };
                if !output.is_empty() {
                    let preview = if output.len() > 100 { format!("{}...", &output[..97]) } else { output.clone() };
                    lines.push(Line::from(vec![
                        Span::styled(format!("     {} ", icon), Style::default().fg(color)),
                        Span::styled(preview, Style::default().fg(Color::DarkGray)),
                    ]));
                } else {
                    lines.push(Line::from(vec![
                        Span::styled(format!("     {} ", icon), Style::default().fg(color)),
                        Span::styled("Done", Style::default().fg(color)),
                    ]));
                }
            }
            DisplayEvent::Complete { duration_ms, cost_usd, success, .. } => {
                lines.push(Line::from(""));
                let (status, color) = if *success { ("Completed", Color::Green) } else { ("Failed", Color::Red) };
                lines.push(Line::from(vec![
                    Span::styled(format!(" ● {} ", status), Style::default().fg(Color::Black).bg(color).add_modifier(Modifier::BOLD)),
                    Span::styled(format!(" {:.1}s ", *duration_ms as f64 / 1000.0), Style::default().fg(Color::White)),
                    Span::styled(format!("${:.4}", cost_usd), Style::default().fg(Color::Yellow)),
                ]).alignment(Alignment::Center));
                lines.push(Line::from(""));
            }
            DisplayEvent::Error { message } => {
                lines.push(Line::from(""));
                lines.push(Line::from(vec![
                    Span::styled(" ✗ Error ", Style::default().fg(Color::White).bg(Color::Red).add_modifier(Modifier::BOLD)),
                ]).alignment(Alignment::Center));
                for line in message.lines() {
                    lines.push(Line::from(Span::styled(line.to_string(), Style::default().fg(Color::Red))).alignment(Alignment::Center));
                }
                lines.push(Line::from(""));
            }
        }
    }

    lines
}

/// Calculate the visual cursor position in a multi-line text area
pub fn calculate_cursor_position(text: &str, cursor: usize, width: usize) -> Option<(usize, usize)> {
    let mut x = 0;
    let mut y = 0;
    let mut pos = 0;

    for ch in text.chars() {
        if pos >= cursor { break; }
        if ch == '\n' {
            y += 1;
            x = 0;
        } else {
            x += 1;
            if x >= width {
                y += 1;
                x = 0;
            }
        }
        pos += ch.len_utf8();
    }

    Some((x, y))
}
