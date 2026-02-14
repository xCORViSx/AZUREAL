//! Git panel rendering — centered modal overlay showing git operations
//! (rebase, merge, fetch, pull, push) and changed files list.
//! Uses Git brand orange (#F05032) for border styling.

use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Paragraph},
    Frame,
};

use crate::app::App;
use super::util::{GIT_BROWN, GIT_ORANGE};

/// Action labels displayed in the panel — index must match ACTION_COUNT in input handler
const ACTIONS: &[(&str, &str)] = &[
    ("r", "Rebase from main"),
    ("m", "Merge from main"),
    ("f", "Fetch"),
    ("l", "Pull"),
    ("P", "Push"),
];

/// Render the Git Actions panel as a centered modal overlay.
/// Called from ui() in run.rs when app.git_actions_panel.is_some().
pub fn draw_git_actions_panel(f: &mut Frame, app: &App) {
    let panel = match app.git_actions_panel {
        Some(ref p) => p,
        None => return,
    };
    let area = f.area();

    // Size the modal: 55% width (min 50), 70% height (min 16)
    let modal_w = (area.width * 55 / 100).max(50).min(area.width);
    let modal_h = (area.height * 70 / 100).max(16).min(area.height);
    let modal = Rect::new(
        area.x + (area.width.saturating_sub(modal_w)) / 2,
        area.y + (area.height.saturating_sub(modal_h)) / 2,
        modal_w, modal_h,
    );

    // Clear the area behind the modal so underlying panes don't bleed through
    f.render_widget(Clear, modal);

    let inner_w = modal.width.saturating_sub(4) as usize;
    let mut lines: Vec<Line> = Vec::new();

    // ── Actions section header ──
    lines.push(Line::from(Span::styled(
        "  ACTIONS",
        Style::default().fg(GIT_BROWN).add_modifier(Modifier::BOLD),
    )));

    // Each action row: "  > [r] Rebase from main" or "    [r] Rebase from main"
    for (i, (key, label)) in ACTIONS.iter().enumerate() {
        let selected = panel.actions_focused && i == panel.selected_action;
        let prefix = if selected { "  \u{25b8} " } else { "    " };
        let style = if selected {
            Style::default().fg(GIT_ORANGE).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::White)
        };
        let key_style = if selected {
            Style::default().fg(GIT_ORANGE).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(GIT_BROWN)
        };
        lines.push(Line::from(vec![
            Span::styled(prefix, style),
            Span::styled(format!("[{}]", key), key_style),
            Span::styled(format!(" {}", label), style),
        ]));
    }

    lines.push(Line::from(""));

    // ── Changed files section header ──
    let total_add: usize = panel.changed_files.iter().map(|f| f.additions).sum();
    let total_del: usize = panel.changed_files.iter().map(|f| f.deletions).sum();
    if panel.changed_files.is_empty() {
        lines.push(Line::from(Span::styled(
            "  CHANGED FILES (none)",
            Style::default().fg(GIT_BROWN).add_modifier(Modifier::BOLD),
        )));
    } else {
        let hdr_style = Style::default().fg(GIT_BROWN).add_modifier(Modifier::BOLD);
        lines.push(Line::from(vec![
            Span::styled(format!("  CHANGED FILES ({} files, ", panel.changed_files.len()), hdr_style),
            Span::styled(format!("+{}", total_add), Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
            Span::styled("/", hdr_style),
            Span::styled(format!("-{}", total_del), Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
            Span::styled(")", hdr_style),
        ]));
    }

    // Thin separator under the header
    let sep_len = inner_w.min(40);
    lines.push(Line::from(Span::styled(
        format!("  {}", "\u{2500}".repeat(sep_len)),
        Style::default().fg(GIT_BROWN),
    )));

    // How many file rows can we fit? Reserve lines for: actions header(1) + actions(7) +
    // blank(1) + files header(1) + separator(1) + result(2) + borders(2) = 15 fixed
    let visible_files = (modal_h as usize).saturating_sub(15);

    // Adjust scroll so selected file is visible
    let scroll = if panel.selected_file < panel.file_scroll {
        panel.selected_file
    } else if panel.selected_file >= panel.file_scroll + visible_files {
        panel.selected_file.saturating_sub(visible_files.saturating_sub(1))
    } else {
        panel.file_scroll
    };

    // Render each visible file row
    for (i, file) in panel.changed_files.iter().enumerate().skip(scroll).take(visible_files) {
        let selected = !panel.actions_focused && i == panel.selected_file;
        let prefix = if selected { "  \u{25b8} " } else { "    " };

        // Status character color: M=yellow, A=green, D=red, R=cyan, ?=magenta (untracked)
        let status_color = match file.status {
            'A' => Color::Green,
            'D' => Color::Red,
            'M' => Color::Yellow,
            'R' => Color::Cyan,
            '?' => Color::Magenta,
            _ => Color::White,
        };

        // Right-aligned +N/-N stats — green for additions, red for deletions
        let add_str = format!("+{}", file.additions);
        let del_str = format!("-{}", file.deletions);
        let stat_len = add_str.len() + 1 + del_str.len(); // "+N/-N" total width
        // How much space for the file path? prefix(4) + status(2) + padding(1+) + stats
        let path_budget = inner_w.saturating_sub(prefix.len() + 2 + stat_len + 1);
        let path_display = if file.path.len() > path_budget {
            format!("\u{2026}{}", &file.path[file.path.len().saturating_sub(path_budget.saturating_sub(1))..])
        } else {
            file.path.clone()
        };
        // Padding between path and stats to right-align
        let padding = inner_w.saturating_sub(prefix.len() + 2 + path_display.len() + stat_len);

        // Path style: underlined always, orange+bold when selected
        let path_style = if selected {
            Style::default().fg(GIT_ORANGE).add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
        } else {
            Style::default().fg(Color::White).add_modifier(Modifier::UNDERLINED)
        };
        // Selected rows use orange override; unselected uses semantic green/red
        let add_style = if selected { Style::default().fg(GIT_ORANGE) } else { Style::default().fg(Color::Green) };
        let del_style = if selected { Style::default().fg(GIT_ORANGE) } else { Style::default().fg(Color::Red) };
        let slash_style = if selected { Style::default().fg(GIT_ORANGE) } else { Style::default().fg(GIT_BROWN) };

        lines.push(Line::from(vec![
            Span::styled(prefix, Style::default()),
            Span::styled(format!("{} ", file.status), Style::default().fg(status_color)),
            Span::styled(path_display, path_style),
            Span::raw(" ".repeat(padding)),
            Span::styled(add_str, add_style),
            Span::styled("/", slash_style),
            Span::styled(del_str, del_style),
        ]));
    }

    // Scroll indicator when file list overflows
    if panel.changed_files.len() > visible_files && visible_files > 0 {
        let end = (scroll + visible_files).min(panel.changed_files.len());
        lines.push(Line::from(Span::styled(
            format!("    {}\u{2013}{} of {}", scroll + 1, end, panel.changed_files.len()),
            Style::default().fg(GIT_BROWN),
        )));
    }

    // ── Result message (green=success, red=error) ──
    if let Some((ref msg, is_error)) = panel.result_message {
        lines.push(Line::from(""));
        let color = if is_error { Color::Red } else { Color::Green };
        let truncated = if msg.len() > inner_w { &msg[..inner_w] } else { msg.as_str() };
        lines.push(Line::from(Span::styled(
            format!("  {}", truncated),
            Style::default().fg(color),
        )));
    }

    // ── Modal chrome: orange border with centered title ──
    let title = Line::from(vec![
        Span::styled(format!(" Git: {} ", panel.worktree_name), Style::default().fg(GIT_ORANGE).bold()),
    ]);
    let block = Block::default()
        .title(title)
        .title_alignment(ratatui::layout::Alignment::Center)
        .borders(Borders::ALL)
        .border_type(BorderType::QuadrantOutside)
        .border_style(Style::default().fg(GIT_ORANGE));

    let paragraph = Paragraph::new(lines).block(block);
    f.render_widget(paragraph, modal);

    // ── Footer hints rendered on top of the bottom border ──
    let footer = " Tab:switch  Enter:exec/view  R:refresh  Esc ";
    let footer_y = modal.y + modal.height.saturating_sub(1);
    let footer_x = modal.x + (modal.width.saturating_sub(footer.len() as u16)) / 2;
    if footer_y < area.height && footer_x + (footer.len() as u16) <= area.x + area.width {
        let footer_rect = Rect::new(footer_x, footer_y, footer.len() as u16, 1);
        f.render_widget(Paragraph::new(Line::from(Span::styled(
            footer,
            Style::default().fg(GIT_BROWN),
        ))), footer_rect);
    }
}
