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
            '*' => {
                if chars.peek().map(|(_, ch)| *ch == '*').unwrap_or(false) {
                    chars.next();
                    if !current_text.is_empty() {
                        spans.push(Span::styled(current_text.clone(), base_style));
                        current_text.clear();
                    }
                    let mut bold_text = String::new();
                    while let Some((_, ch)) = chars.next() {
                        if ch == '*'
                            && chars.peek().map(|(_, c)| *c == '*').unwrap_or(false) {
                                chars.next();
                                break;
                            }
                        bold_text.push(ch);
                    }
                    if !bold_text.is_empty() {
                        spans.push(Span::styled(bold_text, base_style.add_modifier(Modifier::BOLD)));
                    }
                } else {
                    let rest: String = text[i + 1..].chars().take_while(|&ch| ch != ' ' && ch != '\n').collect();
                    if rest.contains('*') && !rest.starts_with(' ') {
                        if !current_text.is_empty() {
                            spans.push(Span::styled(current_text.clone(), base_style));
                            current_text.clear();
                        }
                        let mut italic_text = String::new();
                        for (_, ch) in chars.by_ref() {
                            if ch == '*' { break; }
                            italic_text.push(ch);
                        }
                        if !italic_text.is_empty() {
                            spans.push(Span::styled(italic_text, base_style.add_modifier(Modifier::ITALIC)));
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
    trimmed.contains('|') && trimmed.chars().all(|c| c == '|' || c == '-' || c == ':' || c == ' ')
}
