//! God File panel rendering — centered modal showing source files >1k LOC
//! with checkboxes for batch modularization. Same full-screen modal pattern
//! as the Projects panel.

use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Paragraph},
    Frame,
};

use crate::app::App;
use super::util::AZURE;

/// Draw the God File panel as a centered modal overlay.
/// Caller should return early after this — it takes over the whole screen.
pub fn draw_god_files_panel(f: &mut Frame, app: &App) {
    let Some(ref panel) = app.god_file_panel else { return };
    let area = f.area();

    // Center a modal box (65% width, 75% height, min 50x12)
    let modal_w = (area.width * 65 / 100).max(50).min(area.width);
    let modal_h = (area.height * 75 / 100).max(12).min(area.height);
    let modal = Rect::new(
        area.x + (area.width.saturating_sub(modal_w)) / 2,
        area.y + (area.height.saturating_sub(modal_h)) / 2,
        modal_w,
        modal_h,
    );

    // Clear background behind the modal
    f.render_widget(Clear, modal);

    // Usable width inside borders + padding
    let inner_w = modal.width.saturating_sub(4) as usize;

    // Build content lines
    let mut lines: Vec<Line> = Vec::new();

    if panel.entries.is_empty() {
        // No god files found — congratulations!
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "  No source files exceed 1000 lines. Your codebase is well-modularized!",
            Style::default().fg(Color::Green),
        )));
    } else {
        // Column header
        let checked_count = panel.entries.iter().filter(|e| e.checked).count();
        let header_text = format!(
            "  {} files over 1000 LOC ({} checked)",
            panel.entries.len(), checked_count,
        );
        lines.push(Line::from(Span::styled(header_text, Style::default().fg(Color::DarkGray))));
        lines.push(Line::from(""));

        // Calculate scroll window — how many items visible in the modal
        // (subtract 5 for: title border, header, blank, footer hint, bottom border)
        let visible_items = (modal_h as usize).saturating_sub(6);

        // Adjust scroll to keep selected item visible
        let scroll = if panel.selected < panel.scroll {
            panel.selected
        } else if panel.selected >= panel.scroll + visible_items {
            panel.selected.saturating_sub(visible_items.saturating_sub(1))
        } else {
            panel.scroll
        };

        // Render visible entries
        for (i, entry) in panel.entries.iter().enumerate().skip(scroll).take(visible_items) {
            let is_selected = i == panel.selected;

            // Checkbox: [x] or [ ]
            let checkbox = if entry.checked { "[x] " } else { "[ ] " };
            let checkbox_color = if entry.checked { Color::Green } else { Color::DarkGray };

            // File path — truncate if needed, leave room for line count
            let line_count_str = format!(" {} lines", entry.line_count);
            let path_max = inner_w.saturating_sub(checkbox.len() + line_count_str.len() + 1);
            let path_display = if entry.rel_path.len() > path_max {
                format!("…{}", &entry.rel_path[entry.rel_path.len().saturating_sub(path_max - 1)..])
            } else {
                entry.rel_path.clone()
            };

            // Pad path to right-align line count
            let padding = inner_w.saturating_sub(checkbox.len() + path_display.len() + line_count_str.len());
            let pad_str = " ".repeat(padding);

            // Style: selected row gets azure highlight, others white
            let (path_style, count_style) = if is_selected {
                (
                    Style::default().fg(AZURE).add_modifier(Modifier::BOLD),
                    Style::default().fg(AZURE),
                )
            } else {
                (
                    Style::default().fg(Color::White),
                    Style::default().fg(Color::DarkGray),
                )
            };

            lines.push(Line::from(vec![
                Span::styled(checkbox, Style::default().fg(checkbox_color)),
                Span::styled(path_display, path_style),
                Span::raw(pad_str),
                Span::styled(line_count_str, count_style),
            ]));
        }

        // Scroll indicator if list is longer than viewport
        if panel.entries.len() > visible_items {
            let pos = scroll + 1;
            let total = panel.entries.len();
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                format!("  {}-{} of {}", pos, (pos + visible_items - 1).min(total), total),
                Style::default().fg(Color::DarkGray),
            )));
        }
    }

    // Footer key hints
    let footer = " Space:check  a:all  Enter/m:modularize  Esc:close ";

    // Render the modal block with border and title
    let block = Block::default()
        .title(" God Files (>1000 LOC) ")
        .title_alignment(ratatui::layout::Alignment::Center)
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(AZURE));

    let paragraph = Paragraph::new(lines)
        .block(block);

    f.render_widget(paragraph, modal);

    // Render footer hints at the bottom of the modal
    let footer_y = modal.y + modal.height.saturating_sub(1);
    let footer_x = modal.x + (modal.width.saturating_sub(footer.len() as u16)) / 2;
    if footer_y < area.height && footer_x < area.width {
        let footer_rect = Rect::new(footer_x, footer_y, footer.len() as u16, 1);
        let footer_widget = Paragraph::new(Line::from(Span::styled(
            footer,
            Style::default().fg(Color::DarkGray),
        )));
        f.render_widget(footer_widget, footer_rect);
    }
}
