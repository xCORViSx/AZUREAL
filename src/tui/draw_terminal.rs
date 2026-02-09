//! Terminal pane rendering

use ansi_to_tui::IntoText;
use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::Text,
    widgets::{Block, BorderType, Borders, Paragraph},
    Frame,
};

use crate::app::{App, Focus};
use super::keybindings::{terminal_type_title, terminal_command_title, terminal_scroll_title};
use super::util::AZURE;

/// Draw the embedded PTY terminal pane
pub fn draw_terminal(f: &mut Frame, app: &mut App, area: Rect) {
    let is_focused = app.terminal_mode && app.focus == Focus::Input;
    let border_style = if is_focused {
        Style::default().fg(AZURE).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::White)
    };

    // Sync PTY/parser size with viewport
    let inner_height = area.height.saturating_sub(2);
    let inner_width = area.width.saturating_sub(2);
    if inner_height > 0 && inner_width > 0 {
        let size_changed = inner_height != app.terminal_rows || inner_width != app.terminal_cols;
        let needs_initial = app.terminal_needs_resize;
        app.resize_terminal(inner_height, inner_width);
        // On first resize with size change, send Ctrl+L to force shell redraw
        if needs_initial && size_changed {
            app.write_to_terminal(&[0x0c]);
        }
        app.terminal_needs_resize = false;
    }

    // Get screen contents with ANSI formatting and convert to styled text
    let content = app.terminal_screen_contents();
    let text: Text = content.into_text().unwrap_or_else(|_| {
        Text::from(String::from_utf8_lossy(&content).to_string())
    });

    // Build title with scroll indicator (sourced from keybindings.rs)
    let title = if app.terminal_scroll > 0 {
        terminal_scroll_title(app.terminal_scroll)
    } else if app.prompt_mode {
        terminal_type_title()
    } else {
        terminal_command_title()
    };

    let terminal = Paragraph::new(text)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(if is_focused { BorderType::Double } else { BorderType::Plain })
                .title(title)
                .title_style(border_style)
                .border_style(border_style),
        );

    f.render_widget(terminal, area);

    // Show cursor only in type mode at live view (scroll == 0)
    if app.prompt_mode && app.terminal_mode && app.terminal_scroll == 0 {
        let (cursor_row, cursor_col) = app.terminal_cursor_position();
        let cursor_x = area.x + 1 + cursor_col;
        let cursor_y = area.y + 1 + cursor_row;
        if cursor_x < area.right() && cursor_y < area.bottom() {
            f.set_cursor_position((cursor_x, cursor_y));
        }
    }
}
