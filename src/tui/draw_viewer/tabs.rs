//! Tab bar rendering for the viewer panel
//!
//! Fixed-width tab bar (up to 12 tabs across 2 rows) and tab picker dialog.

use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Paragraph},
    Frame,
};

use crate::app::App;
use super::super::util::AZURE;

/// How many rows the tab bar occupies (0 if no tabs, 1 for ≤6, 2 for >6)
pub(super) fn tab_bar_rows(tab_count: usize) -> u16 {
    if tab_count == 0 { 0 }
    else if tab_count <= 6 { 1 }
    else { 2 }
}

/// Draw fixed-width tab bar: 6 tabs per row, up to 2 rows (12 max).
/// Each "slot" is inner_width/6. Tab content fills slot_w-1 chars + 1 char gap.
pub(super) fn draw_tab_bar(f: &mut Frame, app: &App, area: Rect) {
    let inner_w = area.width.saturating_sub(2) as usize;
    if inner_w < 12 { return; }
    // Each slot includes the tab + 1 trailing gap char. 6 slots fill the row.
    let slot_w = inner_w / 6;
    // Visible tab content = slot minus gap(1) minus leading pad(1)
    let name_max = slot_w.saturating_sub(2);
    let rows = tab_bar_rows(app.viewer_tabs.len());

    for row in 0..rows {
        let y = area.y + 1 + row;
        let bar_area = Rect::new(area.x + 1, y, inner_w as u16, 1);
        let start = row as usize * 6;
        let end = (start + 6).min(app.viewer_tabs.len());
        let mut spans: Vec<Span> = Vec::new();

        for idx in start..end {
            let name = app.viewer_tabs[idx].name();
            // Truncate to fit, ellipsis if too long
            let display = if name.chars().count() > name_max {
                let trunc: String = name.chars().take(name_max.saturating_sub(1)).collect();
                format!("{trunc}…")
            } else {
                name.to_string()
            };
            let is_active = idx == app.viewer_active_tab;
            let style = if is_active {
                Style::default().fg(Color::Black).bg(AZURE).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Gray).bg(Color::DarkGray)
            };
            // " name" padded to slot_w-1, then 1 gap char = total slot_w
            let padded = format!(" {:<width$}", display, width = slot_w - 2);
            let tab_str: String = padded.chars().take(slot_w - 1).collect();
            spans.push(Span::styled(tab_str, style));
            spans.push(Span::raw(" "));
        }
        f.render_widget(Paragraph::new(Line::from(spans)), bar_area);
    }
}

/// Draw tab dialog overlay for switching between tabs
pub(super) fn draw_tab_dialog(f: &mut Frame, app: &App, area: Rect) {
    let tab_count = app.viewer_tabs.len();
    if tab_count == 0 { return; }

    let dialog_width = 40u16.min(area.width.saturating_sub(4));
    let dialog_height = (tab_count as u16 + 4).min(area.height.saturating_sub(4));
    let x = area.x + (area.width.saturating_sub(dialog_width)) / 2;
    let y = area.y + (area.height.saturating_sub(dialog_height)) / 2;
    let dialog_area = Rect::new(x, y, dialog_width, dialog_height);

    f.render_widget(Clear, dialog_area);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Double)
        .title(Span::styled(" Tabs ", Style::default().fg(AZURE).add_modifier(Modifier::BOLD)))
        .border_style(Style::default().fg(AZURE));

    f.render_widget(block.clone(), dialog_area);

    // List tabs inside dialog
    let inner = block.inner(dialog_area);
    let mut lines: Vec<Line> = Vec::new();

    for (idx, tab) in app.viewer_tabs.iter().enumerate() {
        let name = tab.name();
        let is_active = idx == app.viewer_active_tab;

        let prefix = if is_active { "▸ " } else { "  " };
        let style = if is_active {
            Style::default().fg(AZURE).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::White)
        };

        let num_style = Style::default().fg(Color::DarkGray);
        lines.push(Line::from(vec![
            Span::styled(format!("{}", idx + 1), num_style),
            Span::raw(" "),
            Span::styled(prefix, style),
            Span::styled(name.to_string(), style),
        ]));
    }

    // Add hints at bottom
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "j/k:nav Enter:select x:close Esc:cancel",
        Style::default().fg(Color::DarkGray)
    )));

    let para = Paragraph::new(lines);
    f.render_widget(para, inner);
}
