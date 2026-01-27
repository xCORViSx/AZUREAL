//! Utility functions for TUI rendering

use ratatui::{
    layout::Alignment,
    style::{Color, Modifier, Style},
    text::{Line, Span},
};

use crate::events::DisplayEvent;

/// Orange color constant for Claude messages
const ORANGE: Color = Color::Rgb(255, 140, 0);

/// Strip ANSI escape codes from a string for pattern matching
fn strip_ansi(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            // Skip until we hit a letter (end of escape sequence)
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

/// Extract the most relevant parameter from a tool's input for display
fn extract_tool_param(tool_name: &str, input: &serde_json::Value) -> String {
    match tool_name {
        "Read" | "read" => {
            input.get("file_path")
                .or_else(|| input.get("path"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string()
        }
        "Write" | "write" => {
            input.get("file_path")
                .or_else(|| input.get("path"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string()
        }
        "Edit" | "edit" => {
            input.get("file_path")
                .or_else(|| input.get("path"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string()
        }
        "Bash" | "bash" => {
            let cmd = input.get("command")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if cmd.len() > 50 { format!("{}...", &cmd[..47]) } else { cmd.to_string() }
        }
        "Glob" | "glob" => {
            input.get("pattern")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string()
        }
        "Grep" | "grep" => {
            input.get("pattern")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string()
        }
        "WebFetch" | "webfetch" => {
            input.get("url")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string()
        }
        "WebSearch" | "websearch" => {
            input.get("query")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string()
        }
        "Task" | "task" => {
            input.get("description")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string()
        }
        "LSP" | "lsp" => {
            let op = input.get("operation").and_then(|v| v.as_str()).unwrap_or("");
            let file = input.get("filePath").and_then(|v| v.as_str()).unwrap_or("");
            format!("{} {}", op, file)
        }
        _ => {
            // Generic: try common field names
            input.get("file_path")
                .or_else(|| input.get("path"))
                .or_else(|| input.get("command"))
                .or_else(|| input.get("query"))
                .or_else(|| input.get("pattern"))
                .and_then(|v| v.as_str())
                .map(|s| if s.len() > 60 { format!("{}...", &s[..57]) } else { s.to_string() })
                .unwrap_or_default()
        }
    }
}

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
    // Strip ANSI codes for pattern matching, but keep original for display
    let stripped = strip_ansi(line);
    let trimmed = stripped.trim();
    let line_owned = line.to_string();
    let lower = trimmed.to_lowercase();

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

    // Claude/Assistant responses - check for tool use first
    if trimmed.starts_with("Claude:") || trimmed.starts_with("Assistant:") || trimmed.starts_with("[A]") {
        let content = trimmed
            .trim_start_matches("Claude:")
            .trim_start_matches("Assistant:")
            .trim_start_matches("[A]")
            .trim();

        // Check if this is a tool use line: "Claude: [Using Read | path]" or "Claude: [Using Read...]"
        if content.starts_with("[Using ") {
            let inner = content
                .trim_start_matches("[Using ")
                .trim_end_matches("...]")
                .trim_end_matches(']');

            // Parse "Tool | param" or just "Tool"
            let (tool_name, param) = if let Some(pipe_pos) = inner.find(" | ") {
                (&inner[..pipe_pos], Some(&inner[pipe_pos + 3..]))
            } else {
                (inner, None)
            };

            return Line::from(vec![
                Span::styled(" ┣━", Style::default().fg(Color::Cyan)),
                Span::styled("● ", Style::default().fg(Color::Yellow)),
                Span::styled(tool_name.to_string(), Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                Span::styled(
                    param.map(|p| format!("  {}", p)).unwrap_or_default(),
                    Style::default().fg(Color::DarkGray)
                ),
            ]);
        }

        // Regular Claude response
        return Line::from(vec![
            Span::styled(" ◀ Claude ", Style::default().fg(Color::Black).bg(ORANGE).add_modifier(Modifier::BOLD)),
            Span::styled(" ", Style::default()),
            Span::styled(content.to_string(), Style::default().fg(Color::White)),
        ]);
    }

    // Standalone tool use line (when following text in same assistant message)
    if trimmed.starts_with("[Using ") {
        let inner = trimmed
            .trim_start_matches("[Using ")
            .trim_end_matches("...]")
            .trim_end_matches(']');

        // Parse "Tool | param" or just "Tool"
        let (tool_name, param) = if let Some(pipe_pos) = inner.find(" | ") {
            (&inner[..pipe_pos], Some(&inner[pipe_pos + 3..]))
        } else {
            (inner, None)
        };

        return Line::from(vec![
            Span::styled(" ┣━", Style::default().fg(Color::Cyan)),
            Span::styled("● ", Style::default().fg(Color::Yellow)),
            Span::styled(tool_name.to_string(), Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            Span::styled(
                param.map(|p| format!("  {}", p)).unwrap_or_default(),
                Style::default().fg(Color::DarkGray)
            ),
        ]);
    }

    // Done/completion markers - timeline result style
    if trimmed.starts_with("[Done:") || trimmed.starts_with("✓") || trimmed.starts_with("✔") {
        let result_text = trimmed
            .trim_start_matches("[Done:")
            .trim_start_matches("✓ ")
            .trim_start_matches("✔ ")
            .trim_end_matches(']');
        return Line::from(vec![
            Span::styled(" ┃  └─ ", Style::default().fg(Color::Cyan)),
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
                // Compact, dim hook display - no spacing, minimal attention
                if !output.trim().is_empty() {
                    lines.push(Line::from(vec![
                        Span::styled("› ", Style::default().fg(Color::DarkGray)),
                        Span::styled(name.clone(), Style::default().fg(Color::DarkGray)),
                        Span::styled(": ", Style::default().fg(Color::DarkGray)),
                        Span::styled(
                            output.lines().next().unwrap_or("").to_string(),
                            Style::default().fg(Color::DarkGray)
                        ),
                    ]));
                } else {
                    // Even empty hooks get a single dim line
                    lines.push(Line::from(vec![
                        Span::styled("› ", Style::default().fg(Color::DarkGray)),
                        Span::styled(name.clone(), Style::default().fg(Color::DarkGray)),
                    ]));
                }
            }
            DisplayEvent::UserMessage { content, .. } => {
                // Skip empty user messages
                if content.trim().is_empty() {
                    continue;
                }
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
                // Timeline node style for tool calls
                let tool_color = Color::Cyan;

                // Timeline connector
                lines.push(Line::from(vec![
                    Span::styled(" ┃", Style::default().fg(tool_color)),
                ]));

                // Tool node with name and primary parameter
                let param_display = if let Some(path) = file_path {
                    path.clone()
                } else {
                    // Extract meaningful parameter from input
                    extract_tool_param(tool_name, input)
                };

                lines.push(Line::from(vec![
                    Span::styled(" ┣━", Style::default().fg(tool_color)),
                    Span::styled("● ", Style::default().fg(Color::Yellow)),
                    Span::styled(tool_name.clone(), Style::default().fg(tool_color).add_modifier(Modifier::BOLD)),
                    Span::styled("  ", Style::default()),
                    Span::styled(param_display, Style::default().fg(Color::White)),
                ]));
            }
            DisplayEvent::ToolResult { success, output, .. } => {
                // Timeline result node
                let tool_color = Color::Cyan;
                let (icon, result_color) = if *success { ("✓", Color::Green) } else { ("✗", Color::Red) };

                let result_text = if !output.is_empty() {
                    let preview = if output.len() > 60 { format!("{}...", &output[..57]) } else { output.clone() };
                    preview.lines().next().unwrap_or("").to_string()
                } else {
                    "Done".to_string()
                };

                lines.push(Line::from(vec![
                    Span::styled(" ┃  └─ ", Style::default().fg(tool_color)),
                    Span::styled(format!("{} ", icon), Style::default().fg(result_color)),
                    Span::styled(result_text, Style::default().fg(Color::DarkGray)),
                ]));
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
