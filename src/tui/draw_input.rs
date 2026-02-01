//! Input field rendering

use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::Span,
    widgets::{Block, BorderType, Borders, Paragraph, Wrap},
    Frame,
};

use crate::app::{App, Focus};

/// Draw the Claude prompt input field with text wrapping
pub fn draw_input(f: &mut Frame, app: &App, area: Rect) {
    let (border_color, title) = if app.insert_mode {
        (Color::Yellow, " INPROMPT (Esc:command | Enter:submit | Ctrl+X:cancel response) ")
    } else {
        (Color::Red, " COMMAND (i:inprompt | t:terminal | Ctrl+X:cancel response) ")
    };

    let is_focused = app.focus == Focus::Input;
    let input = Paragraph::new(app.input.as_str())
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

    // Show cursor only in insert mode when focused
    // Calculate wrapped cursor position
    if app.insert_mode && is_focused {
        let inner_width = area.width.saturating_sub(2) as usize;
        if inner_width > 0 {
            let cursor_row = app.input_cursor / inner_width;
            let cursor_col = app.input_cursor % inner_width;
            f.set_cursor_position((
                area.x + 1 + cursor_col as u16,
                area.y + 1 + cursor_row as u16,
            ));
        }
    }
}
