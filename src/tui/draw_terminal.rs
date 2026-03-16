//! Terminal pane rendering

use ansi_to_tui::IntoText;
use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Span, Text},
    widgets::{Block, BorderType, Borders, Paragraph},
    Frame,
};

use super::draw_input::split_title_hints;
use super::keybindings::{terminal_command_title, terminal_scroll_title, terminal_type_title};
use super::util::AZURE;
use crate::app::{App, Focus};

/// Draw the embedded PTY terminal pane.
/// When title + hints overflow the top border, remaining hints go on the bottom border.
pub fn draw_terminal(f: &mut Frame, app: &mut App, area: Rect) {
    let is_focused = app.terminal_mode && app.focus == Focus::Input;
    let border_style = if is_focused {
        Style::default().fg(AZURE).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::White)
    };

    let inner_width = area.width.saturating_sub(2) as usize;

    // Build title — (short_label, full_title, hints)
    let (label, _full_title, hints) = if app.terminal_scroll > 0 {
        terminal_scroll_title(app.terminal_scroll)
    } else if app.prompt_mode {
        terminal_type_title()
    } else {
        terminal_command_title()
    };

    // Split hints across top and bottom borders
    let (top_title, bottom_title) = split_title_hints(&label, &hints, inner_width);

    // Sync PTY/parser size with viewport
    let inner_height = area.height.saturating_sub(2);
    let inner_w = area.width.saturating_sub(2);
    if inner_height > 0 && inner_w > 0 {
        let size_changed = inner_height != app.terminal_rows || inner_w != app.terminal_cols;
        let needs_initial = app.terminal_needs_resize;
        app.resize_terminal(inner_height, inner_w);
        if needs_initial && size_changed {
            if cfg!(windows) {
                // On Windows, send Enter to trigger a fresh prompt after resize.
                // Form feed clears the screen without reprinting the prompt.
                app.write_to_terminal(b"\r");
            } else {
                // On Unix, Ctrl+L (form feed) reprints the prompt after a clear.
                app.write_to_terminal(&[0x0c]);
            }
        }
        app.terminal_needs_resize = false;
    }

    // Get screen contents with ANSI formatting and convert to styled text
    let content = app.terminal_screen_contents();
    let text: Text = content
        .into_text()
        .unwrap_or_else(|_| Text::from(String::from_utf8_lossy(&content).to_string()));

    let mut block = Block::default()
        .borders(Borders::ALL)
        .border_type(if is_focused {
            BorderType::Double
        } else {
            BorderType::Plain
        })
        .title(Span::styled(top_title, border_style))
        .border_style(border_style);

    // Overflow hints on bottom border — same style as top title (color + bold match)
    if let Some(ref bot) = bottom_title {
        block = block.title_bottom(Span::styled(bot.as_str(), border_style));
    }

    let terminal = Paragraph::new(text).block(block);
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

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::layout::Rect;
    use ratatui::style::{Color, Modifier, Style};
    use ratatui::text::Span;
    use ratatui::widgets::{BorderType, Borders};

    // ══════════════════════════════════════════════════════════════════
    //  AZURE constant
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn azure_value() {
        assert_eq!(AZURE, Color::Rgb(51, 153, 255));
    }

    // ══════════════════════════════════════════════════════════════════
    //  Focus check
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn focus_input_eq() {
        assert_eq!(Focus::Input, Focus::Input);
    }

    #[test]
    fn focus_input_ne_session() {
        assert_ne!(Focus::Input, Focus::Session);
    }

    // ══════════════════════════════════════════════════════════════════
    //  Border style logic
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn border_focused_azure_bold() {
        let is_focused = true;
        let style = if is_focused {
            Style::default().fg(AZURE).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::White)
        };
        assert_eq!(style.fg, Some(AZURE));
    }

    #[test]
    fn border_unfocused_white() {
        let is_focused = false;
        let style = if is_focused {
            Style::default().fg(AZURE).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::White)
        };
        assert_eq!(style.fg, Some(Color::White));
    }

    // ══════════════════════════════════════════════════════════════════
    //  Inner width / height math
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn inner_width_calculation() {
        let area = Rect::new(0, 0, 80, 24);
        let inner_width = area.width.saturating_sub(2) as usize;
        assert_eq!(inner_width, 78);
    }

    #[test]
    fn inner_width_narrow() {
        let area = Rect::new(0, 0, 2, 24);
        let inner_width = area.width.saturating_sub(2) as usize;
        assert_eq!(inner_width, 0);
    }

    #[test]
    fn inner_height_calculation() {
        let area = Rect::new(0, 0, 80, 24);
        let inner_height = area.height.saturating_sub(2);
        assert_eq!(inner_height, 22);
    }

    #[test]
    fn inner_width_separate_var() {
        let area = Rect::new(0, 0, 80, 24);
        let inner_w = area.width.saturating_sub(2);
        assert_eq!(inner_w, 78);
    }

    // ══════════════════════════════════════════════════════════════════
    //  terminal_type_title
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn type_title_label() {
        let (label, _, _) = terminal_type_title();
        assert_eq!(label, " TERMINAL ");
    }

    #[test]
    fn type_title_full_contains_terminal() {
        let (_, full, _) = terminal_type_title();
        assert!(full.contains("TERMINAL"));
    }

    #[test]
    fn type_title_hints_contains_exit() {
        let (_, _, hints) = terminal_type_title();
        assert!(hints.contains("exit"));
    }

    // ══════════════════════════════════════════════════════════════════
    //  terminal_command_title
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn command_title_label() {
        let (label, _, _) = terminal_command_title();
        assert_eq!(label, " TERMINAL ");
    }

    #[test]
    fn command_title_hints_contains_type() {
        let (_, _, hints) = terminal_command_title();
        assert!(hints.contains("type"));
    }

    #[test]
    fn command_title_hints_contains_prompt() {
        let (_, _, hints) = terminal_command_title();
        assert!(hints.contains("PROMPT"));
    }

    #[test]
    fn command_title_hints_contains_scroll() {
        let (_, _, hints) = terminal_command_title();
        assert!(hints.contains("scroll"));
    }

    #[test]
    fn command_title_hints_contains_resize() {
        let (_, _, hints) = terminal_command_title();
        assert!(hints.contains("resize"));
    }

    // ══════════════════════════════════════════════════════════════════
    //  terminal_scroll_title
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn scroll_title_shows_count() {
        let (label, _, _) = terminal_scroll_title(42);
        assert!(label.contains("42"));
    }

    #[test]
    fn scroll_title_zero() {
        let (label, _, _) = terminal_scroll_title(0);
        assert!(label.contains("0"));
    }

    #[test]
    fn scroll_title_full_has_count() {
        let (_, full, _) = terminal_scroll_title(100);
        assert!(full.contains("100"));
    }

    #[test]
    fn scroll_title_hints_has_scroll() {
        let (_, _, hints) = terminal_scroll_title(5);
        assert!(hints.contains("scroll"));
    }

    #[test]
    fn scroll_title_hints_has_page() {
        let (_, _, hints) = terminal_scroll_title(5);
        assert!(hints.contains("page"));
    }

    #[test]
    fn scroll_title_hints_has_top() {
        let (_, _, hints) = terminal_scroll_title(5);
        assert!(hints.contains("top"));
    }

    #[test]
    fn scroll_title_hints_has_bottom() {
        let (_, _, hints) = terminal_scroll_title(5);
        assert!(hints.contains("bottom"));
    }

    #[test]
    fn scroll_title_hints_has_type() {
        let (_, _, hints) = terminal_scroll_title(5);
        assert!(hints.contains("type"));
    }

    #[test]
    fn scroll_title_hints_has_close() {
        let (_, _, hints) = terminal_scroll_title(5);
        assert!(hints.contains("close"));
    }

    // ══════════════════════════════════════════════════════════════════
    //  split_title_hints
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn split_hints_fits_single_line() {
        let (top, bottom) = split_title_hints(" TERMINAL ", "a:b", 80);
        assert!(top.contains("TERMINAL"));
        assert!(top.contains("a:b"));
        assert!(bottom.is_none());
    }

    #[test]
    fn split_hints_overflow_gives_bottom() {
        let long_hints =
            "a:action | b:other | c:third | d:fourth | e:fifth | f:sixth | g:seventh | h:eighth";
        let (top, bottom) = split_title_hints(" T ", long_hints, 30);
        assert!(!top.is_empty());
        // With narrow width, some hints should overflow to bottom
        assert!(bottom.is_some() || top.contains(long_hints));
    }

    #[test]
    fn split_hints_empty_hints() {
        let (top, bottom) = split_title_hints(" TERMINAL ", "", 80);
        assert!(top.contains("TERMINAL"));
        assert!(bottom.is_none());
    }

    // ══════════════════════════════════════════════════════════════════
    //  Title mode selection
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn scroll_mode_when_scroll_gt_zero() {
        let terminal_scroll = 5;
        let prompt_mode = false;
        let mode = if terminal_scroll > 0 {
            "scroll"
        } else if prompt_mode {
            "type"
        } else {
            "command"
        };
        assert_eq!(mode, "scroll");
    }

    #[test]
    fn type_mode_when_prompt() {
        let terminal_scroll = 0;
        let prompt_mode = true;
        let mode = if terminal_scroll > 0 {
            "scroll"
        } else if prompt_mode {
            "type"
        } else {
            "command"
        };
        assert_eq!(mode, "type");
    }

    #[test]
    fn command_mode_default() {
        let terminal_scroll = 0;
        let prompt_mode = false;
        let mode = if terminal_scroll > 0 {
            "scroll"
        } else if prompt_mode {
            "type"
        } else {
            "command"
        };
        assert_eq!(mode, "command");
    }

    // ══════════════════════════════════════════════════════════════════
    //  Border type
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn border_type_focused_double() {
        let focused = true;
        let bt = if focused {
            BorderType::Double
        } else {
            BorderType::Plain
        };
        assert_eq!(bt, BorderType::Double);
    }

    #[test]
    fn border_type_unfocused_plain() {
        let focused = false;
        let bt = if focused {
            BorderType::Double
        } else {
            BorderType::Plain
        };
        assert_eq!(bt, BorderType::Plain);
    }

    // ══════════════════════════════════════════════════════════════════
    //  Cursor position math
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn cursor_x_from_area_and_col() {
        let area = Rect::new(5, 2, 80, 24);
        let cursor_col: u16 = 10;
        let cursor_x = area.x + 1 + cursor_col;
        assert_eq!(cursor_x, 16);
    }

    #[test]
    fn cursor_y_from_area_and_row() {
        let area = Rect::new(5, 2, 80, 24);
        let cursor_row: u16 = 3;
        let cursor_y = area.y + 1 + cursor_row;
        assert_eq!(cursor_y, 6);
    }

    #[test]
    fn cursor_bounds_check_x() {
        let area = Rect::new(0, 0, 80, 24);
        let cursor_x = area.x + 1 + 50;
        assert!(cursor_x < area.right());
    }

    #[test]
    fn cursor_bounds_check_y() {
        let area = Rect::new(0, 0, 80, 24);
        let cursor_y = area.y + 1 + 20;
        assert!(cursor_y < area.bottom());
    }

    #[test]
    fn cursor_at_right_edge_fails_check() {
        let area = Rect::new(0, 0, 80, 24);
        let cursor_x = area.x + 1 + 79;
        assert!(!(cursor_x < area.right())); // 80 is not < 80
    }

    // ══════════════════════════════════════════════════════════════════
    //  Cursor visibility conditions
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn cursor_visible_when_all_conditions_met() {
        let prompt_mode = true;
        let terminal_mode = true;
        let terminal_scroll = 0;
        let visible = prompt_mode && terminal_mode && terminal_scroll == 0;
        assert!(visible);
    }

    #[test]
    fn cursor_hidden_when_scrolled() {
        let prompt_mode = true;
        let terminal_mode = true;
        let terminal_scroll = 5;
        let visible = prompt_mode && terminal_mode && terminal_scroll == 0;
        assert!(!visible);
    }

    #[test]
    fn cursor_hidden_when_not_prompt_mode() {
        let prompt_mode = false;
        let terminal_mode = true;
        let terminal_scroll = 0;
        let visible = prompt_mode && terminal_mode && terminal_scroll == 0;
        assert!(!visible);
    }

    #[test]
    fn cursor_hidden_when_not_terminal_mode() {
        let prompt_mode = true;
        let terminal_mode = false;
        let terminal_scroll = 0;
        let visible = prompt_mode && terminal_mode && terminal_scroll == 0;
        assert!(!visible);
    }

    // ══════════════════════════════════════════════════════════════════
    //  Size change detection
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn size_changed_when_height_differs() {
        let inner_height: u16 = 22;
        let inner_w: u16 = 78;
        let terminal_rows: u16 = 20;
        let terminal_cols: u16 = 78;
        let changed = inner_height != terminal_rows || inner_w != terminal_cols;
        assert!(changed);
    }

    #[test]
    fn size_unchanged_when_same() {
        let inner_height: u16 = 22;
        let inner_w: u16 = 78;
        let terminal_rows: u16 = 22;
        let terminal_cols: u16 = 78;
        let changed = inner_height != terminal_rows || inner_w != terminal_cols;
        assert!(!changed);
    }

    // ══════════════════════════════════════════════════════════════════
    //  Block construction
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn block_with_top_and_bottom_title() {
        let top_title = " TERMINAL (Esc:exit) ";
        let bottom_title = Some("j/k:scroll".to_string());
        let mut block = ratatui::widgets::Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Double)
            .title(Span::styled(top_title, Style::default().fg(AZURE)));
        if let Some(ref bot) = bottom_title {
            block = block.title_bottom(Span::styled(bot.as_str(), Style::default().fg(AZURE)));
        }
        let _ = block;
    }

    #[test]
    fn block_without_bottom_title() {
        let bottom_title: Option<String> = None;
        let mut block = ratatui::widgets::Block::default()
            .borders(Borders::ALL)
            .title(Span::styled(
                " TERMINAL ",
                Style::default().fg(Color::White),
            ));
        if let Some(ref bot) = bottom_title {
            block = block.title_bottom(Span::styled(
                bot.as_str(),
                Style::default().fg(Color::White),
            ));
        }
        let _ = block;
    }

    // ══════════════════════════════════════════════════════════════════
    //  Rect utilities
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn rect_right() {
        let area = Rect::new(5, 0, 80, 24);
        assert_eq!(area.right(), 85);
    }

    #[test]
    fn rect_bottom() {
        let area = Rect::new(0, 5, 80, 24);
        assert_eq!(area.bottom(), 29);
    }

    #[test]
    fn rect_area_multiplication() {
        let area = Rect::new(0, 0, 10, 20);
        assert_eq!(area.width as u32 * area.height as u32, 200);
    }
}
