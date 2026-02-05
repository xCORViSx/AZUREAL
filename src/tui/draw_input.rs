//! Input field rendering

use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Paragraph, Wrap},
    Frame,
};

use crate::app::{App, Focus};
use super::keybindings::{prompt_type_title, prompt_command_title};

/// Draw the Claude prompt input field with text wrapping and optional selection highlighting
pub fn draw_input(f: &mut Frame, app: &App, area: Rect) {
    let (border_color, title) = if app.prompt_mode {
        (Color::Yellow, prompt_type_title())
    } else {
        (Color::Red, prompt_command_title())
    };

    let is_focused = app.focus == Focus::Input;
    let inner_width = area.width.saturating_sub(2) as usize;

    // Build styled content with selection highlighting
    let content = build_input_content(app, inner_width);

    let input = Paragraph::new(content)
        .wrap(Wrap { trim: false })
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(if is_focused { BorderType::Double } else { BorderType::Plain })
                .title(if is_focused {
                    Span::styled(title, Style::default().fg(border_color).add_modifier(Modifier::BOLD))
                } else {
                    Span::styled(title, Style::default().fg(Color::White))
                })
                .border_style(if is_focused {
                    Style::default().fg(border_color).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::White)
                }),
        );

    f.render_widget(input, area);

    // Show cursor only in prompt mode when focused
    // Calculate wrapped cursor position correctly using character widths
    if app.prompt_mode && is_focused && inner_width > 0 {
        let chars: Vec<char> = app.input.chars().collect();
        let cursor_char_idx = app.input_cursor.min(chars.len());

        // Walk through characters to find visual position (accounts for word wrapping)
        let mut visual_col = 0usize;
        let mut visual_row = 0usize;
        for i in 0..cursor_char_idx {
            let c = chars[i];
            let char_width = unicode_width::UnicodeWidthChar::width(c).unwrap_or(1);
            // Check if adding this char would overflow the line
            if visual_col + char_width > inner_width {
                visual_row += 1;
                visual_col = char_width;
            } else {
                visual_col += char_width;
            }
        }

        f.set_cursor_position((
            area.x + 1 + visual_col as u16,
            area.y + 1 + visual_row as u16,
        ));
    }
}

/// Build styled input content with selection highlighting
fn build_input_content<'a>(app: &App, _inner_width: usize) -> Vec<Line<'a>> {
    let chars: Vec<char> = app.input.chars().collect();
    if chars.is_empty() {
        return vec![Line::from("")];
    }

    // Get normalized selection range (if any)
    let selection = app.input_selection.and_then(|(s, e)| {
        if s == e { None } else if s < e { Some((s, e)) } else { Some((e, s)) }
    });

    let mut spans = Vec::new();
    let normal_style = Style::default();
    let selection_style = Style::default().bg(Color::Blue).fg(Color::White);

    match selection {
        Some((sel_start, sel_end)) => {
            // Text before selection
            if sel_start > 0 {
                let before: String = chars[..sel_start].iter().collect();
                spans.push(Span::styled(before, normal_style));
            }
            // Selected text
            let selected: String = chars[sel_start..sel_end.min(chars.len())].iter().collect();
            spans.push(Span::styled(selected, selection_style));
            // Text after selection
            if sel_end < chars.len() {
                let after: String = chars[sel_end..].iter().collect();
                spans.push(Span::styled(after, normal_style));
            }
        }
        None => {
            spans.push(Span::styled(app.input.clone(), normal_style));
        }
    }

    vec![Line::from(spans)]
}
