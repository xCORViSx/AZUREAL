//! Full-width table popup overlay (click a table in session pane to open)

use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Paragraph},
    Frame,
};

use crate::tui::util::AZURE;

/// Draw full-width table popup overlay (click a table in session pane to open)
pub fn draw_table_popup(f: &mut Frame, popup: &crate::app::types::TablePopup, area: Rect) {
    if popup.lines.is_empty() {
        return;
    }

    // Size: use most of the terminal, capped to content
    let max_w = area.width.saturating_sub(4);
    // Measure widest rendered line (span content widths)
    let content_w: u16 = popup
        .lines
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|s| s.content.chars().count() as u16)
                .sum::<u16>()
        })
        .max()
        .unwrap_or(40);
    let w = (content_w + 4).min(max_w).max(30); // +4 for border + padding
    let max_h = area.height.saturating_sub(4);
    let content_h = popup.total_lines as u16;
    let h = (content_h + 2).min(max_h).max(5); // +2 for border
    let x = area.x + (area.width.saturating_sub(w)) / 2;
    let y = area.y + (area.height.saturating_sub(h)) / 2;
    let rect = Rect::new(x, y, w, h);

    f.render_widget(Clear, rect);

    let inner_h = h.saturating_sub(2) as usize;
    let can_scroll = popup.total_lines > inner_h;
    let scroll_info = if can_scroll {
        format!(
            " {}/{} ",
            popup.scroll + 1,
            popup.total_lines.saturating_sub(inner_h) + 1
        )
    } else {
        String::new()
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Double)
        .border_style(Style::default().fg(AZURE))
        .title(Span::styled(
            " Table ",
            Style::default().fg(AZURE).add_modifier(Modifier::BOLD),
        ))
        .title_bottom(Line::from(vec![
            Span::styled(" Esc", Style::default().fg(Color::DarkGray)),
            Span::styled(" close ", Style::default().fg(Color::DarkGray)),
            if can_scroll {
                Span::styled(scroll_info, Style::default().fg(Color::DarkGray))
            } else {
                Span::raw("")
            },
        ]));

    let visible: Vec<Line> = popup
        .lines
        .iter()
        .skip(popup.scroll)
        .take(inner_h)
        .cloned()
        .collect();

    let para = Paragraph::new(visible).block(block);
    f.render_widget(para, rect);
}
