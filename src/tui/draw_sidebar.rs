//! Sidebar rendering — Git Actions panel and FileTree overlay

use ratatui::{
    layout::{Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Paragraph},
    Frame,
};

use crate::app::App;
use super::keybindings;
use super::util::{GIT_BROWN, GIT_ORANGE};

/// Draw the sidebar — in Git mode shows Actions + Changed Files,
/// otherwise delegates to the file tree pane.
pub fn draw_sidebar(f: &mut Frame, app: &mut App, area: Rect) {
    if let Some(ref panel) = app.git_actions_panel {
        draw_git_sidebar(f, app, panel, area);
    }
}

/// Draw the file tree in the left pane (always visible in normal mode)
pub fn draw_file_tree_overlay(f: &mut Frame, app: &mut App, area: Rect) {
    super::draw_file_tree::draw_file_tree(f, app, area);
}

/// Git panel sidebar — Actions list (top) + Changed Files (bottom)
fn draw_git_sidebar(f: &mut Frame, app: &App, panel: &crate::app::types::GitActionsPanel, area: Rect) {
    // Split vertically: actions (auto-height) | files (fill)
    let action_rows = if panel.is_on_main { 8 } else { 10 };
    let splits = Layout::vertical([
        ratatui::layout::Constraint::Length(action_rows),
        ratatui::layout::Constraint::Min(4),
    ]).split(area);
    let actions_area = splits[0];
    let files_area = splits[1];

    // ─── Actions pane (top) ──────────────────────────────────────────────────
    let actions_focused = panel.focused_pane == 0;
    let mut action_lines: Vec<Line> = Vec::new();

    let action_labels = keybindings::git_actions_labels(panel.is_on_main);
    for (i, (key, label)) in action_labels.iter().enumerate() {
        let selected = actions_focused && i == panel.selected_action;
        let prefix = if selected { " \u{25b8} " } else { "   " };
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
        action_lines.push(Line::from(vec![
            Span::styled(prefix, style),
            Span::styled(format!("[{}]", key), key_style),
            Span::styled(format!(" {}", label), style),
        ]));
    }

    // Divider + toggles (feature branches get a visual separator)
    if !panel.is_on_main {
        let inner_w = actions_area.width.saturating_sub(2) as usize;
        action_lines.push(Line::from(Span::styled(
            "\u{2500}".repeat(inner_w),
            Style::default().fg(GIT_BROWN),
        )));

        let enabled = app.auto_rebase_enabled.contains(&panel.worktree_name);
        let (indicator, ind_color) = if enabled {
            ("\u{25cf} ON", Color::Green)
        } else {
            ("\u{25cb} OFF", Color::DarkGray)
        };
        action_lines.push(Line::from(vec![
            Span::styled("   ", Style::default()),
            Span::styled("[a]", Style::default().fg(GIT_BROWN)),
            Span::styled(" Auto-rebase ", Style::default().fg(Color::White)),
            Span::styled(indicator, Style::default().fg(ind_color).add_modifier(Modifier::BOLD)),
        ]));
    }

    // Main branch also gets a divider before auto-resolve
    if panel.is_on_main {
        let inner_w = actions_area.width.saturating_sub(2) as usize;
        action_lines.push(Line::from(Span::styled(
            "\u{2500}".repeat(inner_w),
            Style::default().fg(GIT_BROWN),
        )));
    }

    // Auto-resolve files count
    let ar_count = panel.auto_resolve_files.len();
    action_lines.push(Line::from(vec![
        Span::styled("   ", Style::default()),
        Span::styled("[s]", Style::default().fg(GIT_BROWN)),
        Span::styled(format!(" Auto-resolve ({})", ar_count), Style::default().fg(Color::White)),
    ]));

    let actions_block = Block::default()
        .title(Span::styled(" Actions ", Style::default()
            .fg(if actions_focused { GIT_ORANGE } else { GIT_BROWN })
            .add_modifier(if actions_focused { Modifier::BOLD } else { Modifier::empty() })))
        .borders(Borders::ALL)
        .border_type(if actions_focused { BorderType::Double } else { BorderType::Plain })
        .border_style(if actions_focused {
            Style::default().fg(GIT_ORANGE).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(GIT_BROWN)
        });
    f.render_widget(Paragraph::new(action_lines).block(actions_block), actions_area);

    // ─── Changed Files pane (bottom) ─────────────────────────────────────────
    let files_focused = panel.focused_pane == 1;
    let inner_w = files_area.width.saturating_sub(2) as usize;
    let inner_h = files_area.height.saturating_sub(2) as usize;
    let mut file_lines: Vec<Line> = Vec::new();

    // Scroll so selected file is visible
    let visible_files = inner_h;
    let scroll = if panel.selected_file < panel.file_scroll {
        panel.selected_file
    } else if panel.selected_file >= panel.file_scroll + visible_files {
        panel.selected_file.saturating_sub(visible_files.saturating_sub(1))
    } else {
        panel.file_scroll
    };

    for (i, file) in panel.changed_files.iter().enumerate().skip(scroll).take(visible_files) {
        let selected = files_focused && i == panel.selected_file;
        let prefix = if selected { " \u{25b8} " } else { "   " };

        let status_color = match file.status {
            'A' => Color::Green,
            'D' => Color::Red,
            'M' => Color::Yellow,
            'R' => Color::Cyan,
            '?' => Color::Magenta,
            _ => Color::White,
        };

        let add_str = format!("+{}", file.additions);
        let del_str = format!("-{}", file.deletions);
        let stat_len = add_str.len() + 1 + del_str.len();
        let path_budget = inner_w.saturating_sub(prefix.len() + 2 + stat_len + 1);
        let path_display = if file.path.len() > path_budget {
            format!("\u{2026}{}", &file.path[file.path.len().saturating_sub(path_budget.saturating_sub(1))..])
        } else {
            file.path.clone()
        };
        let padding = inner_w.saturating_sub(prefix.len() + 2 + path_display.len() + stat_len);

        let path_style = if selected {
            Style::default().fg(GIT_ORANGE).add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
        } else {
            Style::default().fg(Color::White).add_modifier(Modifier::UNDERLINED)
        };
        let add_style = if selected { Style::default().fg(GIT_ORANGE) } else { Style::default().fg(Color::Green) };
        let del_style = if selected { Style::default().fg(GIT_ORANGE) } else { Style::default().fg(Color::Red) };
        let slash_style = if selected { Style::default().fg(GIT_ORANGE) } else { Style::default().fg(GIT_BROWN) };

        file_lines.push(Line::from(vec![
            Span::styled(prefix, Style::default()),
            Span::styled(format!("{} ", file.status), Style::default().fg(status_color)),
            Span::styled(path_display, path_style),
            Span::raw(" ".repeat(padding)),
            Span::styled(add_str, add_style),
            Span::styled("/", slash_style),
            Span::styled(del_str, del_style),
        ]));
    }

    // Title with file count and +/- stats
    let files_title = if panel.changed_files.is_empty() {
        " Changed Files (none) ".to_string()
    } else {
        let total_add: usize = panel.changed_files.iter().map(|f| f.additions).sum();
        let total_del: usize = panel.changed_files.iter().map(|f| f.deletions).sum();
        format!(" Changed Files ({}, +{}/-{}) ", panel.changed_files.len(), total_add, total_del)
    };

    let files_block = Block::default()
        .title(Span::styled(files_title, Style::default()
            .fg(if files_focused { GIT_ORANGE } else { GIT_BROWN })
            .add_modifier(if files_focused { Modifier::BOLD } else { Modifier::empty() })))
        .borders(Borders::ALL)
        .border_type(if files_focused { BorderType::Double } else { BorderType::Plain })
        .border_style(if files_focused {
            Style::default().fg(GIT_ORANGE).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(GIT_BROWN)
        });
    f.render_widget(Paragraph::new(file_lines).block(files_block), files_area);
}
