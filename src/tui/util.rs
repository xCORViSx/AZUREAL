//! Utility functions for TUI rendering

use ratatui::{
    layout::Alignment,
    style::{Color, Modifier, Style},
    text::{Line, Span},
};

use crate::events::DisplayEvent;

/// Orange color constant for Claude messages
const ORANGE: Color = Color::Rgb(255, 140, 0);

/// Parse inline markdown (bold, italic, inline code) into styled spans
/// Returns a Vec of Spans with appropriate styling applied
fn parse_markdown_spans(text: &str, base_style: Style) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    let mut chars = text.char_indices().peekable();
    let mut current_text = String::new();

    while let Some((i, c)) = chars.next() {
        match c {
            // Inline code: `code`
            '`' => {
                // Flush current text
                if !current_text.is_empty() {
                    spans.push(Span::styled(current_text.clone(), base_style));
                    current_text.clear();
                }
                // Collect until closing backtick
                let mut code = String::new();
                while let Some((_, ch)) = chars.next() {
                    if ch == '`' { break; }
                    code.push(ch);
                }
                if !code.is_empty() {
                    spans.push(Span::styled(
                        code,
                        Style::default().fg(Color::Yellow).bg(Color::Rgb(40, 40, 40))
                    ));
                }
            }
            // Bold or italic: ** or *
            '*' => {
                // Check for bold (**text**)
                if chars.peek().map(|(_, ch)| *ch == '*').unwrap_or(false) {
                    chars.next(); // consume second *
                    // Flush current text
                    if !current_text.is_empty() {
                        spans.push(Span::styled(current_text.clone(), base_style));
                        current_text.clear();
                    }
                    // Collect until closing **
                    let mut bold_text = String::new();
                    while let Some((_, ch)) = chars.next() {
                        if ch == '*' {
                            if chars.peek().map(|(_, c)| *c == '*').unwrap_or(false) {
                                chars.next(); // consume closing **
                                break;
                            }
                        }
                        bold_text.push(ch);
                    }
                    if !bold_text.is_empty() {
                        spans.push(Span::styled(
                            bold_text,
                            base_style.add_modifier(Modifier::BOLD)
                        ));
                    }
                } else {
                    // Single * - italic
                    // Check if there's a closing * (not at word boundary)
                    let rest: String = text[i + 1..].chars().take_while(|&ch| ch != ' ' && ch != '\n').collect();
                    if rest.contains('*') && !rest.starts_with(' ') {
                        // Flush current text
                        if !current_text.is_empty() {
                            spans.push(Span::styled(current_text.clone(), base_style));
                            current_text.clear();
                        }
                        // Collect until closing *
                        let mut italic_text = String::new();
                        while let Some((_, ch)) = chars.next() {
                            if ch == '*' { break; }
                            italic_text.push(ch);
                        }
                        if !italic_text.is_empty() {
                            spans.push(Span::styled(
                                italic_text,
                                base_style.add_modifier(Modifier::ITALIC)
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

    // Flush remaining text
    if !current_text.is_empty() {
        spans.push(Span::styled(current_text, base_style));
    }

    if spans.is_empty() {
        spans.push(Span::styled("", base_style));
    }

    spans
}

/// Parse a markdown table row into styled spans with box drawing
fn parse_table_row(line: &str, is_separator: bool) -> Vec<Span<'static>> {
    if is_separator {
        // Convert --- separators to box drawing
        let cells: Vec<&str> = line.split('|').filter(|s| !s.is_empty()).collect();
        let mut result = vec![Span::styled("├", Style::default().fg(Color::DarkGray))];
        for (i, cell) in cells.iter().enumerate() {
            let width = cell.len();
            result.push(Span::styled("─".repeat(width), Style::default().fg(Color::DarkGray)));
            if i < cells.len() - 1 {
                result.push(Span::styled("┼", Style::default().fg(Color::DarkGray)));
            }
        }
        result.push(Span::styled("┤", Style::default().fg(Color::DarkGray)));
        result
    } else {
        // Regular data row
        let cells: Vec<&str> = line.split('|').collect();
        let mut result = vec![Span::styled("│", Style::default().fg(Color::DarkGray))];
        for (i, cell) in cells.iter().enumerate() {
            if i == 0 && cell.is_empty() { continue; } // Skip leading empty
            if i == cells.len() - 1 && cell.is_empty() { continue; } // Skip trailing empty
            let trimmed = cell.trim();
            result.extend(parse_markdown_spans(trimmed, Style::default().fg(Color::White)));
            result.push(Span::styled("│", Style::default().fg(Color::DarkGray)));
        }
        result
    }
}

/// Check if a line is a markdown table separator (contains only |, -, :, and spaces)
fn is_table_separator(line: &str) -> bool {
    let trimmed = line.trim();
    trimmed.contains('|') && trimmed.chars().all(|c| c == '|' || c == '-' || c == ':' || c == ' ')
}

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
            if cmd.chars().count() > 50 {
                format!("{}...", cmd.chars().take(47).collect::<String>())
            } else {
                cmd.to_string()
            }
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
                .map(|s| if s.chars().count() > 60 { format!("{}...", s.chars().take(57).collect::<String>()) } else { s.to_string() })
                .unwrap_or_default()
        }
    }
}

/// Truncate a string to max length, adding ellipsis if needed
pub fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max { s.to_string() } else { format!("{}…", s.chars().take(max - 1).collect::<String>()) }
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
pub fn render_display_events(
    events: &[DisplayEvent],
    width: u16,
    pending_tools: &std::collections::HashSet<String>,
    failed_tools: &std::collections::HashSet<String>,
    animation_tick: u64,
) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    let w = width as usize;
    let bubble_width = (w * 2 / 3).max(40); // Message bubbles take 2/3 of width

    // Track what we've already rendered to filter duplicates
    let mut saw_init = false;
    let mut saw_content = false; // Track if we've seen user/assistant/tool content
    // Track last hook to only deduplicate consecutive identical hooks (not globally)
    let mut last_hook: Option<(String, String)> = None;

    for event in events {
        match event {
            DisplayEvent::Init { model, cwd, .. } => {
                // Only show Init at the start - skip if we've already seen content or another Init
                if saw_init || saw_content { continue; }
                saw_init = true;

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
                // Only deduplicate consecutive identical hooks (not globally)
                let key = (name.clone(), output.clone());
                if last_hook.as_ref() == Some(&key) { continue; }
                last_hook = Some(key);

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
            DisplayEvent::Command { name } => {
                // 3-line tall command display - stands out prominently
                let cmd_style = Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD);
                lines.push(Line::from(""));
                lines.push(Line::from(vec![
                    Span::styled("━".repeat(20), Style::default().fg(Color::Magenta)),
                ]).alignment(Alignment::Center));
                lines.push(Line::from(vec![
                    Span::styled(format!("  {}  ", name), cmd_style),
                ]).alignment(Alignment::Center));
                lines.push(Line::from(vec![
                    Span::styled("━".repeat(20), Style::default().fg(Color::Magenta)),
                ]).alignment(Alignment::Center));
                lines.push(Line::from(""));
            }
            DisplayEvent::Compacting => {
                // 3-line tall compacting indicator - very prominent
                let compact_style = Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD);
                lines.push(Line::from(""));
                lines.push(Line::from(vec![
                    Span::styled("═".repeat(30), Style::default().fg(Color::Yellow)),
                ]).alignment(Alignment::Center));
                lines.push(Line::from(vec![
                    Span::styled("  COMPACTING CONVERSATION  ", compact_style),
                ]).alignment(Alignment::Center));
                lines.push(Line::from(vec![
                    Span::styled("═".repeat(30), Style::default().fg(Color::Yellow)),
                ]).alignment(Alignment::Center));
                lines.push(Line::from(""));
            }
            DisplayEvent::Compacted => {
                // 3-line tall compacted indicator - green for success
                let compact_style = Style::default().fg(Color::Green).add_modifier(Modifier::BOLD);
                lines.push(Line::from(""));
                lines.push(Line::from(vec![
                    Span::styled("═".repeat(30), Style::default().fg(Color::Green)),
                ]).alignment(Alignment::Center));
                lines.push(Line::from(vec![
                    Span::styled("  CONVERSATION COMPACTED  ", compact_style),
                ]).alignment(Alignment::Center));
                lines.push(Line::from(vec![
                    Span::styled("═".repeat(30), Style::default().fg(Color::Green)),
                ]).alignment(Alignment::Center));
                lines.push(Line::from(""));
            }
            DisplayEvent::UserMessage { content, .. } => {
                saw_content = true;
                last_hook = None; // Reset so same hook can appear after content
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
                saw_content = true;
                last_hook = None; // Reset so same hook can appear after content
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

                // Left-aligned content with full markdown parsing
                let mut in_code_block = false;
                let mut in_table = false;
                let text_lines: Vec<&str> = text.lines().collect();

                for (i, line) in text_lines.iter().enumerate() {
                    let trimmed = line.trim();

                    // Code block fence
                    if trimmed.starts_with("```") {
                        in_code_block = !in_code_block;
                        let lang = trimmed.trim_start_matches('`').trim();
                        let mut spans = vec![Span::styled("│ ", Style::default().fg(ORANGE))];
                        if in_code_block && !lang.is_empty() {
                            // Opening fence with language
                            spans.push(Span::styled("┌─ ", Style::default().fg(Color::DarkGray)));
                            spans.push(Span::styled(lang.to_string(), Style::default().fg(Color::Cyan)));
                            spans.push(Span::styled(" ─", Style::default().fg(Color::DarkGray)));
                        } else if !in_code_block {
                            // Closing fence
                            spans.push(Span::styled("└──────", Style::default().fg(Color::DarkGray)));
                        } else {
                            spans.push(Span::styled("┌──────", Style::default().fg(Color::DarkGray)));
                        }
                        lines.push(Line::from(spans));
                        continue;
                    }

                    // Inside code block - preserve as-is with yellow styling
                    if in_code_block {
                        lines.push(Line::from(vec![
                            Span::styled("│ ", Style::default().fg(ORANGE)),
                            Span::styled("│ ", Style::default().fg(Color::DarkGray)),
                            Span::styled(line.to_string(), Style::default().fg(Color::Yellow)),
                        ]));
                        continue;
                    }

                    // Table detection and handling
                    let is_table_line = trimmed.contains('|') && !trimmed.starts_with('|') == false;
                    let is_sep = is_table_separator(trimmed);

                    if is_table_line || is_sep {
                        if !in_table && is_table_line && !is_sep {
                            in_table = true;
                            // Check if next line is separator for header styling
                            let next_is_sep = text_lines.get(i + 1).map(|l| is_table_separator(l)).unwrap_or(false);
                            let mut spans = vec![Span::styled("│ ", Style::default().fg(ORANGE))];
                            if next_is_sep {
                                // Header row - bold
                                let cells: Vec<&str> = trimmed.split('|').filter(|s| !s.is_empty()).collect();
                                spans.push(Span::styled("│", Style::default().fg(Color::DarkGray)));
                                for (j, cell) in cells.iter().enumerate() {
                                    spans.push(Span::styled(
                                        cell.trim().to_string(),
                                        Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
                                    ));
                                    if j < cells.len() - 1 {
                                        spans.push(Span::styled("│", Style::default().fg(Color::DarkGray)));
                                    }
                                }
                                spans.push(Span::styled("│", Style::default().fg(Color::DarkGray)));
                            } else {
                                spans.extend(parse_table_row(trimmed, false));
                            }
                            lines.push(Line::from(spans));
                        } else if is_sep {
                            let mut spans = vec![Span::styled("│ ", Style::default().fg(ORANGE))];
                            spans.extend(parse_table_row(trimmed, true));
                            lines.push(Line::from(spans));
                        } else {
                            let mut spans = vec![Span::styled("│ ", Style::default().fg(ORANGE))];
                            spans.extend(parse_table_row(trimmed, false));
                            lines.push(Line::from(spans));
                        }
                        continue;
                    } else {
                        in_table = false;
                    }

                    // Headers (# ## ### etc) - display styled without the # prefix
                    if trimmed.starts_with('#') {
                        let header_level = trimmed.chars().take_while(|&c| c == '#').count();
                        let header_text = trimmed.trim_start_matches('#').trim();
                        let (prefix, style) = match header_level {
                            1 => ("█ ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD | Modifier::UNDERLINED)),
                            2 => ("▓ ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                            3 => ("▒ ", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
                            _ => ("░ ", Style::default().fg(Color::Green)),
                        };
                        lines.push(Line::from(vec![
                            Span::styled("│ ", Style::default().fg(ORANGE)),
                            Span::styled(prefix, style),
                            Span::styled(header_text.to_string(), style),
                        ]));
                        continue;
                    }

                    // Bullet points - parse inline markdown in content
                    if trimmed.starts_with("- ") || trimmed.starts_with("* ") || trimmed.starts_with("• ") {
                        let bullet_content = trimmed.trim_start_matches("- ").trim_start_matches("* ").trim_start_matches("• ");
                        let mut spans = vec![
                            Span::styled("│ ", Style::default().fg(ORANGE)),
                            Span::styled("  • ", Style::default().fg(Color::Cyan)),
                        ];
                        spans.extend(parse_markdown_spans(bullet_content, Style::default().fg(Color::White)));
                        lines.push(Line::from(spans));
                        continue;
                    }

                    // Numbered lists
                    if trimmed.chars().next().map(|c| c.is_ascii_digit()).unwrap_or(false)
                        && trimmed.contains(". ") {
                        let num_end = trimmed.find(". ").unwrap_or(0);
                        let num = &trimmed[..num_end];
                        let content = &trimmed[num_end + 2..];
                        let mut spans = vec![
                            Span::styled("│ ", Style::default().fg(ORANGE)),
                            Span::styled(format!("  {}. ", num), Style::default().fg(Color::Cyan)),
                        ];
                        spans.extend(parse_markdown_spans(content, Style::default().fg(Color::White)));
                        lines.push(Line::from(spans));
                        continue;
                    }

                    // Blockquotes
                    if trimmed.starts_with("> ") {
                        let quote_content = trimmed.trim_start_matches("> ");
                        let mut spans = vec![
                            Span::styled("│ ", Style::default().fg(ORANGE)),
                            Span::styled("┃ ", Style::default().fg(Color::DarkGray)),
                        ];
                        spans.extend(parse_markdown_spans(quote_content, Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC)));
                        lines.push(Line::from(spans));
                        continue;
                    }

                    // Regular text with inline markdown parsing
                    let mut spans = vec![Span::styled("│ ", Style::default().fg(ORANGE))];
                    spans.extend(parse_markdown_spans(line, Style::default().fg(Color::White)));
                    lines.push(Line::from(spans));
                }

                // Left-aligned bottom border
                lines.push(Line::from(vec![
                    Span::styled(format!("└{}", "─".repeat(bubble_width - 1)), Style::default().fg(ORANGE)),
                ]));
            }
            DisplayEvent::ToolCall { tool_name, file_path, input, tool_use_id, .. } => {
                saw_content = true;
                last_hook = None; // Reset so same hook can appear after content
                // Timeline node style for tool calls
                let tool_color = Color::Cyan;
                let is_pending = pending_tools.contains(tool_use_id);

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

                // Pulsating indicator for pending, green for success, red for failed
                let is_failed = failed_tools.contains(tool_use_id);
                let (indicator, indicator_color) = if is_pending {
                    // Pulsate between bright white and dim gray based on tick
                    let pulse_colors = [Color::White, Color::Gray, Color::DarkGray, Color::Gray];
                    let pulse_idx = (animation_tick / 2) as usize % pulse_colors.len();
                    ("◐ ", pulse_colors[pulse_idx])
                } else if is_failed {
                    ("✗ ", Color::Red)
                } else {
                    ("● ", Color::Green)
                };

                lines.push(Line::from(vec![
                    Span::styled(" ┣━", Style::default().fg(tool_color)),
                    Span::styled(indicator, Style::default().fg(indicator_color)),
                    Span::styled(tool_name.clone(), Style::default().fg(tool_color).add_modifier(Modifier::BOLD)),
                    Span::styled("  ", Style::default()),
                    Span::styled(param_display, Style::default().fg(ORANGE)),
                ]));

                // Edit tool: show the actual diff inline with the tool call
                if tool_name == "Edit" {
                    let old_str = input.get("old_string").and_then(|v| v.as_str()).unwrap_or("");
                    let new_str = input.get("new_string").and_then(|v| v.as_str()).unwrap_or("");

                    if !old_str.is_empty() || !new_str.is_empty() {
                        let old_lines: Vec<&str> = old_str.lines().collect();
                        let new_lines: Vec<&str> = new_str.lines().collect();

                        // Find actual starting line number by reading the file
                        let start_line = file_path.as_ref().and_then(|path| {
                            std::fs::read_to_string(path).ok().and_then(|content| {
                                content.find(new_str).map(|pos| {
                                    content[..pos].chars().filter(|&c| c == '\n').count() + 1
                                })
                            })
                        }).unwrap_or(1);

                        let max_line = start_line + old_lines.len().max(new_lines.len());
                        let num_width = max_line.to_string().len().max(2);

                        // Find which lines actually changed by comparing old vs new
                        let max_len = old_lines.len().max(new_lines.len());
                        for i in 0..max_len {
                            let old_line = old_lines.get(i).copied();
                            let new_line = new_lines.get(i).copied();

                            match (old_line, new_line) {
                                (Some(old), Some(new)) if old == new => {
                                    // Unchanged line - show in gray (context)
                                    lines.push(Line::from(vec![
                                        Span::styled(" ┃  ", Style::default().fg(tool_color)),
                                        Span::styled(
                                            format!(" {:>width$}   {} ", start_line + i, old, width = num_width),
                                            Style::default().fg(Color::DarkGray),
                                        ),
                                    ]));
                                }
                                (Some(old), Some(new)) => {
                                    // Changed line - show old (red) then new (green)
                                    lines.push(Line::from(vec![
                                        Span::styled(" ┃  ", Style::default().fg(tool_color)),
                                        Span::styled(
                                            format!(" {:>width$} - {} ", start_line + i, old, width = num_width),
                                            Style::default().fg(Color::White).bg(Color::Red),
                                        ),
                                    ]));
                                    lines.push(Line::from(vec![
                                        Span::styled(" ┃  ", Style::default().fg(tool_color)),
                                        Span::styled(
                                            format!(" {:>width$} + {} ", start_line + i, new, width = num_width),
                                            Style::default().fg(Color::Black).bg(Color::Green),
                                        ),
                                    ]));
                                }
                                (Some(old), None) => {
                                    // Removed line (old has more lines than new)
                                    lines.push(Line::from(vec![
                                        Span::styled(" ┃  ", Style::default().fg(tool_color)),
                                        Span::styled(
                                            format!(" {:>width$} - {} ", start_line + i, old, width = num_width),
                                            Style::default().fg(Color::White).bg(Color::Red),
                                        ),
                                    ]));
                                }
                                (None, Some(new)) => {
                                    // Added line (new has more lines than old)
                                    lines.push(Line::from(vec![
                                        Span::styled(" ┃  ", Style::default().fg(tool_color)),
                                        Span::styled(
                                            format!(" {:>width$} + {} ", start_line + i, new, width = num_width),
                                            Style::default().fg(Color::Black).bg(Color::Green),
                                        ),
                                    ]));
                                }
                                (None, None) => {}
                            }
                        }
                    }
                }
                // Write tool: show line count + purpose line from input content
                if tool_name == "Write" {
                    if let Some(content) = input.get("content").and_then(|v| v.as_str()) {
                        let content_lines: Vec<&str> = content.lines().collect();
                        let line_count = content_lines.len();

                        // Find first meaningful line (comment or first code line)
                        let purpose_line = content_lines.iter()
                            .find(|l| {
                                let trimmed = l.trim();
                                trimmed.starts_with("//") || trimmed.starts_with("#") ||
                                trimmed.starts_with("/*") || trimmed.starts_with("\"\"\"") ||
                                trimmed.starts_with("///") || trimmed.starts_with("//!")
                            })
                            .or(content_lines.first())
                            .map(|s| *s)
                            .unwrap_or("");

                        lines.push(Line::from(vec![
                            Span::styled(" ┃  └─ ", Style::default().fg(tool_color)),
                            Span::styled("✓ ", Style::default().fg(Color::Green)),
                            Span::styled(format!("{} lines", line_count), Style::default().fg(Color::White)),
                            if !purpose_line.is_empty() {
                                Span::styled(format!("  {}", truncate_line(purpose_line, 70)), Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC))
                            } else {
                                Span::raw("")
                            },
                        ]));
                    }
                }
            }
            DisplayEvent::ToolResult { tool_name, file_path, content, .. } => {
                saw_content = true;
                last_hook = None; // Reset so same hook can appear after content
                let result_lines = render_tool_result(tool_name, file_path.as_deref(), content);
                lines.extend(result_lines);
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
            // Filtered events are skipped (rewound/superseded messages)
            DisplayEvent::Filtered => {}
        }
    }

    lines
}

/// Render tool result output based on tool type
/// Each tool has a specific display format optimized for readability
fn render_tool_result(tool_name: &str, _file_path: Option<&str>, content: &str) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    let tool_color = Color::Cyan;
    // Use Gray (lighter than DarkGray) for tool results - more visible than hooks
    let result_style = Style::default().fg(Color::Gray);

    // Filter out system-reminder blocks that Claude Code appends to tool results
    // Remove everything from <system-reminder> to </system-reminder> inclusive
    let content = if let Some(start) = content.find("<system-reminder>") {
        &content[..start]
    } else {
        content
    }.trim_end();

    match tool_name {
        // Read: First + Last line
        "Read" => {
            let content_lines: Vec<&str> = content.lines().collect();
            let line_count = content_lines.len();
            if line_count == 0 {
                lines.push(Line::from(vec![
                    Span::styled(" ┃  └─ ", result_style.fg(tool_color)),
                    Span::styled("(empty file)", result_style),
                ]));
            } else if line_count <= 2 {
                for l in content_lines {
                    lines.push(Line::from(vec![
                        Span::styled(" ┃  │ ", result_style.fg(tool_color)),
                        Span::styled(truncate_line(l, 100), result_style),
                    ]));
                }
            } else {
                lines.push(Line::from(vec![
                    Span::styled(" ┃  │ ", result_style.fg(tool_color)),
                    Span::styled(truncate_line(content_lines[0], 100), result_style),
                ]));
                lines.push(Line::from(vec![
                    Span::styled(" ┃  │ ", result_style.fg(tool_color)),
                    Span::styled(format!("... ({} lines)", line_count - 2), Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC)),
                ]));
                // Find last non-empty line (skip lines that are just "N→" with no content)
                let last_line = content_lines.iter().rev()
                    .find(|l| {
                        // Check if line has content after the "N→" prefix
                        l.find('→').map(|i| l[i+3..].trim().len() > 0).unwrap_or(l.trim().len() > 0)
                    })
                    .unwrap_or(&content_lines[line_count - 1]);
                lines.push(Line::from(vec![
                    Span::styled(" ┃  └─ ", result_style.fg(tool_color)),
                    Span::styled(truncate_line(last_line, 100), result_style),
                ]));
            }
        }

        // Bash: Exit code + last 2 lines
        "Bash" => {
            let content_lines: Vec<&str> = content.lines().collect();
            let line_count = content_lines.len();

            // Try to find exit code in content (often at end or in format "exit code: N")
            let exit_hint = if content.contains("exit code") || content.contains("Exit code") {
                ""
            } else {
                " → exit 0"
            };

            if line_count == 0 {
                lines.push(Line::from(vec![
                    Span::styled(" ┃  └─ ", result_style.fg(tool_color)),
                    Span::styled(format!("✓{}", exit_hint), Style::default().fg(Color::Green)),
                ]));
            } else if line_count <= 2 {
                for (i, l) in content_lines.iter().enumerate() {
                    let prefix = if i == line_count - 1 { " ┃  └─ " } else { " ┃  │ " };
                    lines.push(Line::from(vec![
                        Span::styled(prefix, result_style.fg(tool_color)),
                        Span::styled(truncate_line(l, 100), result_style),
                    ]));
                }
            } else {
                lines.push(Line::from(vec![
                    Span::styled(" ┃  │ ", result_style.fg(tool_color)),
                    Span::styled(format!("... ({} lines)", line_count - 2), Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC)),
                ]));
                for l in content_lines.iter().skip(line_count - 2) {
                    lines.push(Line::from(vec![
                        Span::styled(" ┃  │ ", result_style.fg(tool_color)),
                        Span::styled(truncate_line(l, 100), result_style),
                    ]));
                }
            }
        }

        // Edit: diff is shown in ToolCall, just show success here
        "Edit" => {
            lines.push(Line::from(vec![
                Span::styled(" ┃  └─ ", result_style.fg(tool_color)),
                Span::styled(truncate_line(content, 80), result_style),
            ]));
        }

        // Write: Just show truncated result (content display handled in ToolCall)
        "Write" => {
            // Result is just "File created successfully..." - show truncated
            lines.push(Line::from(vec![
                Span::styled(" ┃  └─ ", result_style.fg(tool_color)),
                Span::styled(truncate_line(content.lines().next().unwrap_or("written"), 80), result_style),
            ]));
        }

        // Grep: First 3 matches
        "Grep" => {
            let content_lines: Vec<&str> = content.lines().collect();
            let line_count = content_lines.len();
            let show_count = 3.min(line_count);

            for (i, l) in content_lines.iter().take(show_count).enumerate() {
                let prefix = if i == show_count - 1 && line_count <= 3 { " ┃  └─ " } else { " ┃  │ " };
                lines.push(Line::from(vec![
                    Span::styled(prefix, result_style.fg(tool_color)),
                    Span::styled(truncate_line(l, 100), result_style),
                ]));
            }
            if line_count > 3 {
                lines.push(Line::from(vec![
                    Span::styled(" ┃  └─ ", result_style.fg(tool_color)),
                    Span::styled(format!("... ({} more matches)", line_count - 3), Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC)),
                ]));
            }
        }

        // Glob: Directory summary
        "Glob" => {
            let content_lines: Vec<&str> = content.lines().collect();
            let line_count = content_lines.len();

            // Group by directory
            let mut dir_counts: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
            for l in &content_lines {
                let dir = l.rsplit('/').nth(1).unwrap_or(".");
                *dir_counts.entry(dir).or_insert(0) += 1;
            }

            lines.push(Line::from(vec![
                Span::styled(" ┃  │ ", result_style.fg(tool_color)),
                Span::styled(format!("→ {} files", line_count), Style::default().fg(Color::White)),
            ]));

            // Show top directories
            let mut dirs: Vec<_> = dir_counts.into_iter().collect();
            dirs.sort_by(|a, b| b.1.cmp(&a.1));
            let dir_summary: String = dirs.iter()
                .take(5)
                .map(|(d, c)| format!("{}/ ({})", d, c))
                .collect::<Vec<_>>()
                .join("  ");

            lines.push(Line::from(vec![
                Span::styled(" ┃  └─ ", result_style.fg(tool_color)),
                Span::styled(truncate_line(&dir_summary, 100), result_style),
            ]));
        }

        // Task: Summary line
        "Task" => {
            let first_line = content.lines().next().unwrap_or("completed");
            lines.push(Line::from(vec![
                Span::styled(" ┃  └─ ", result_style.fg(tool_color)),
                Span::styled("→ ", Style::default().fg(Color::Yellow)),
                Span::styled(truncate_line(first_line, 90), result_style),
            ]));
        }

        // WebFetch: Title + preview
        "WebFetch" => {
            let content_lines: Vec<&str> = content.lines().collect();
            // First line is often the title
            if let Some(title) = content_lines.first() {
                lines.push(Line::from(vec![
                    Span::styled(" ┃  │ ", result_style.fg(tool_color)),
                    Span::styled(format!("\"{}\"", truncate_line(title, 60)), Style::default().fg(Color::Yellow)),
                ]));
            }
            // Second line as preview
            if let Some(preview) = content_lines.get(1) {
                lines.push(Line::from(vec![
                    Span::styled(" ┃  └─ ", result_style.fg(tool_color)),
                    Span::styled(truncate_line(preview, 80), result_style),
                ]));
            } else {
                lines.push(Line::from(vec![
                    Span::styled(" ┃  └─ ", result_style.fg(tool_color)),
                    Span::styled("✓ fetched", Style::default().fg(Color::Green)),
                ]));
            }
        }

        // WebSearch: First 3 results
        "WebSearch" => {
            let content_lines: Vec<&str> = content.lines().collect();
            let line_count = content_lines.len();
            let show_count = 3.min(line_count);

            for (i, l) in content_lines.iter().take(show_count).enumerate() {
                let prefix = if i == show_count - 1 && line_count <= 3 { " ┃  └─ " } else { " ┃  │ " };
                lines.push(Line::from(vec![
                    Span::styled(prefix, result_style.fg(tool_color)),
                    Span::styled(format!("{}. ", i + 1), Style::default().fg(Color::Yellow)),
                    Span::styled(truncate_line(l, 90), result_style),
                ]));
            }
            if line_count > 3 {
                lines.push(Line::from(vec![
                    Span::styled(" ┃  └─ ", result_style.fg(tool_color)),
                    Span::styled(format!("... ({} more results)", line_count - 3), Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC)),
                ]));
            }
        }

        // LSP: Result + context
        "LSP" => {
            let content_lines: Vec<&str> = content.lines().collect();
            for (i, l) in content_lines.iter().take(3).enumerate() {
                let prefix = if i == content_lines.len().min(3) - 1 { " ┃  └─ " } else { " ┃  │ " };
                lines.push(Line::from(vec![
                    Span::styled(prefix, result_style.fg(tool_color)),
                    Span::styled(truncate_line(l, 100), result_style),
                ]));
            }
        }

        // Default: First line + count
        _ => {
            let content_lines: Vec<&str> = content.lines().collect();
            let line_count = content_lines.len();
            let first_line = content_lines.first().map(|s| *s).unwrap_or("✓");

            if line_count <= 1 {
                lines.push(Line::from(vec![
                    Span::styled(" ┃  └─ ", result_style.fg(tool_color)),
                    Span::styled(truncate_line(first_line, 100), result_style),
                ]));
            } else {
                lines.push(Line::from(vec![
                    Span::styled(" ┃  │ ", result_style.fg(tool_color)),
                    Span::styled(truncate_line(first_line, 100), result_style),
                ]));
                lines.push(Line::from(vec![
                    Span::styled(" ┃  └─ ", result_style.fg(tool_color)),
                    Span::styled(format!("... ({} lines)", line_count - 1), Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC)),
                ]));
            }
        }
    }

    lines
}

/// Truncate a line to max length, adding ellipsis if needed
fn truncate_line(s: &str, max_len: usize) -> String {
    let trimmed = s.trim();
    if trimmed.chars().count() <= max_len {
        trimmed.to_string()
    } else {
        format!("{}...", trimmed.chars().take(max_len.saturating_sub(3)).collect::<String>())
    }
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
