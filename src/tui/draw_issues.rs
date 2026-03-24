//! GitHub Issues panel rendering — centered modal overlay showing issues from
//! xCORViSx/AZUREAL repo with search/filter and issue creation capability.

use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Paragraph},
    Frame,
};

use super::keybindings;
use super::util::AZURE;
use crate::app::App;

/// Draw the Issues panel as a centered modal overlay.
pub fn draw_issues_panel(f: &mut Frame, app: &App) {
    let Some(ref panel) = app.issues_panel else {
        return;
    };
    let area = f.area();

    // Center modal — same size as Health panel (55% width, 70% height, min 50x16)
    let modal_w = (area.width * 55 / 100).max(50).min(area.width);
    let modal_h = (area.height * 70 / 100).max(16).min(area.height);
    let modal = Rect::new(
        area.x + (area.width.saturating_sub(modal_w)) / 2,
        area.y + (area.height.saturating_sub(modal_h)) / 2,
        modal_w,
        modal_h,
    );
    f.render_widget(Clear, modal);

    let inner_w = modal.width.saturating_sub(4) as usize;
    // Content height: total modal minus top/bottom border (2) minus title/footer rows
    let content_h = modal.height.saturating_sub(4) as usize;
    let mut lines: Vec<Line> = Vec::new();

    // ── Filter bar (when active or has text) ──
    if panel.filter_active || !panel.filter.is_empty() {
        let filter_style = if panel.filter_active {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default().fg(Color::DarkGray)
        };
        lines.push(Line::from(vec![
            Span::styled("  / ", filter_style),
            Span::styled(&panel.filter, Style::default().fg(Color::White)),
            if panel.filter_active {
                Span::styled("│", Style::default().fg(Color::Yellow))
            } else {
                Span::raw("")
            },
        ]));
        lines.push(Line::from(""));
    }

    if panel.loading {
        // Centered loading message
        let msg = "Loading issues...";
        let pad = inner_w.saturating_sub(msg.len()) / 2;
        lines.push(Line::from(""));
        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::raw(" ".repeat(pad)),
            Span::styled(msg, Style::default().fg(AZURE)),
        ]));
    } else if let Some(ref error) = panel.error {
        // Error display
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            format!("  Error: {}", error),
            Style::default().fg(Color::Red),
        )));
    } else if panel.filtered_indices.is_empty() {
        // Empty state
        let msg = if panel.filter.is_empty() {
            "No issues found"
        } else {
            "No matching issues"
        };
        let pad = inner_w.saturating_sub(msg.len()) / 2;
        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::raw(" ".repeat(pad)),
            Span::styled(msg, Style::default().fg(Color::DarkGray)),
        ]));
    } else {
        // Issue list
        let filter_lines = if panel.filter_active || !panel.filter.is_empty() {
            2
        } else {
            0
        };
        let visible_h = content_h.saturating_sub(filter_lines);

        // Compute scroll
        let scroll = if panel.selected >= panel.scroll + visible_h {
            panel.selected.saturating_sub(visible_h - 1)
        } else if panel.selected < panel.scroll {
            panel.selected
        } else {
            panel.scroll
        };

        for (vi, &idx) in panel
            .filtered_indices
            .iter()
            .enumerate()
            .skip(scroll)
            .take(visible_h)
        {
            let issue = &panel.issues[idx];
            let is_selected = vi == panel.selected;

            let num_style = if is_selected {
                Style::default().fg(AZURE).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::DarkGray)
            };
            let title_style = if is_selected {
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Gray)
            };

            let mut spans = vec![
                Span::raw("  "),
                Span::styled(format!("#{:<5}", issue.number), num_style),
                Span::raw(" "),
            ];

            // Truncate title to fit
            let label_len: usize = issue.labels.iter().map(|l| l.len() + 3).sum();
            let avail = inner_w.saturating_sub(10 + label_len);
            let title_display = if issue.title.len() > avail && avail > 3 {
                format!("{}...", &issue.title[..avail - 3])
            } else {
                issue.title.clone()
            };
            spans.push(Span::styled(title_display, title_style));

            // Labels
            for label in &issue.labels {
                let label_color = label_color(label);
                spans.push(Span::raw(" "));
                spans.push(Span::styled(
                    format!("[{}]", label),
                    Style::default().fg(label_color),
                ));
            }

            lines.push(Line::from(spans));
        }
    }

    // Build footer hints
    let footer = keybindings::issues_browse_hints();

    let title = Line::from(vec![Span::styled(
        format!(
            " Issues ({}) ",
            if panel.loading {
                "...".to_string()
            } else {
                panel.issues.len().to_string()
            }
        ),
        Style::default().fg(AZURE).bold(),
    )])
    .alignment(ratatui::layout::Alignment::Center);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Double)
        .border_style(Style::default().fg(AZURE))
        .title(title)
        .title_bottom(Line::from(Span::styled(
            format!(" {} ", footer),
            Style::default().fg(AZURE),
        )));

    let para = Paragraph::new(lines).block(block);
    f.render_widget(para, modal);
}

/// Draw the issue approval dialog over the session pane.
/// Shown when `issue_session.approval_pending` is true.
pub fn draw_issue_approval(f: &mut Frame, app: &App) {
    let Some(ref _issue) = app.issue_session else {
        return;
    };

    let area = f.area();
    let dialog_w = 50u16.min(area.width);
    let dialog_h = 7u16.min(area.height);
    let dialog = Rect::new(
        area.x + (area.width.saturating_sub(dialog_w)) / 2,
        area.y + (area.height.saturating_sub(dialog_h)) / 2,
        dialog_w,
        dialog_h,
    );
    f.render_widget(Clear, dialog);

    let lines = vec![
        Line::from(""),
        Line::from(Span::styled(
            "  Issue draft ready. Submit to GitHub?",
            Style::default().fg(Color::White),
        )),
        Line::from(""),
        Line::from(vec![
            Span::styled("  y", Style::default().fg(Color::Green).bold()),
            Span::styled(" Accept & submit", Style::default().fg(Color::Gray)),
            Span::raw("    "),
            Span::styled("n", Style::default().fg(Color::Red).bold()),
            Span::styled(" Discard", Style::default().fg(Color::Gray)),
        ]),
    ];

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Double)
        .border_style(Style::default().fg(Color::Green))
        .title(Line::from(Span::styled(
            " Issue Approval ",
            Style::default().fg(Color::Green).bold(),
        )));

    let para = Paragraph::new(lines).block(block);
    f.render_widget(para, dialog);
}

/// Map label names to colors for visual distinction.
fn label_color(label: &str) -> Color {
    match label.to_lowercase().as_str() {
        "bug" => Color::Red,
        "enhancement" => Color::Green,
        "documentation" => Color::Blue,
        "question" => Color::Magenta,
        "good first issue" => Color::Cyan,
        "help wanted" => Color::Yellow,
        _ => Color::DarkGray,
    }
}
