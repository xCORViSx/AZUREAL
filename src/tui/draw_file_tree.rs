//! FileTree panel rendering
//!
//! Shows the directory structure for the selected session's worktree.
//! Supports expand/collapse of directories and file selection.

use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Paragraph},
    Frame,
};

use crate::app::{App, Focus};
use crate::app::types::FileTreeAction;
use super::util::{truncate, AZURE};

/// Build file tree lines (extracted for caching)
fn build_file_tree_lines(app: &App) -> Vec<Line<'static>> {
    let mut lines: Vec<Line> = Vec::new();

    if app.file_tree_entries.is_empty() {
        if app.current_session().and_then(|s| s.worktree_path.as_ref()).is_none() {
            lines.push(Line::from(Span::styled(
                "No worktree",
                Style::default().fg(Color::DarkGray),
            )));
        } else {
            lines.push(Line::from(Span::styled(
                "Empty directory",
                Style::default().fg(Color::DarkGray),
            )));
        }
    } else {
        for (idx, entry) in app.file_tree_entries.iter().enumerate() {
            let is_selected = app.file_tree_selected == Some(idx);
            let indent = "  ".repeat(entry.depth);

            let (icon, icon_color) = if entry.is_dir {
                let expanded = app.file_tree_expanded.contains(&entry.path);
                // Hidden dirs get dimmed yellow icon
                let color = if entry.is_hidden { Color::Rgb(120, 100, 60) } else { Color::Yellow };
                if expanded { ("▼ ", color) } else { ("▶ ", color) }
            } else {
                let icon = match entry.path.extension().and_then(|e| e.to_str()) {
                    Some("rs") => "🦀",
                    Some("toml") => "⚙ ",
                    Some("md") => "📝",
                    Some("json") => "{}",
                    Some("yaml") | Some("yml") => "📋",
                    Some("lock") => "🔒",
                    _ => "  ",
                };
                // Hidden files get dimmed icon color
                let color = if entry.is_hidden { Color::Rgb(100, 100, 100) } else { Color::White };
                (icon, color)
            };

            let mut spans = vec![
                Span::raw(indent),
                Span::styled(icon, Style::default().fg(icon_color)),
            ];

            let name_style = if is_selected {
                Style::default().bg(Color::Blue).fg(Color::White).add_modifier(Modifier::BOLD)
            } else if entry.is_dir {
                // Hidden dirs get dimmed cyan, normal dirs get bright cyan
                let color = if entry.is_hidden { Color::Rgb(80, 120, 130) } else { AZURE };
                Style::default().fg(color).add_modifier(Modifier::BOLD)
            } else {
                // Hidden files get dimmed gray, normal files get white
                let color = if entry.is_hidden { Color::Rgb(100, 100, 100) } else { Color::White };
                Style::default().fg(color)
            };

            let max_name_len = 38usize.saturating_sub(entry.depth * 2 + 2);
            spans.push(Span::styled(truncate(&entry.name, max_name_len), name_style));
            lines.push(Line::from(spans));
        }
    }

    lines
}

/// Draw the file tree panel showing the session's worktree files
pub fn draw_file_tree(f: &mut Frame, app: &mut App, area: Rect) {
    let is_focused = app.focus == Focus::FileTree;
    // Reserve 1 line at bottom for action bar when an action is active
    let has_action = app.file_tree_action.is_some();
    let viewport_height = area.height.saturating_sub(if has_action { 3 } else { 2 }) as usize;

    // Rebuild cache if dirty or selection changed (selection affects highlight)
    if app.file_tree_dirty {
        app.file_tree_lines_cache = build_file_tree_lines(app);
        app.file_tree_dirty = false;
        app.file_tree_scroll_cached = usize::MAX; // Force viewport rebuild
    }

    let total = app.file_tree_lines_cache.len();
    let max_scroll = total.saturating_sub(viewport_height);

    // Auto-scroll to keep selection visible
    let scroll = if let Some(selected) = app.file_tree_selected {
        if selected < app.file_tree_scroll {
            selected
        } else if selected >= app.file_tree_scroll + viewport_height {
            selected.saturating_sub(viewport_height - 1)
        } else {
            app.file_tree_scroll
        }
    } else {
        app.file_tree_scroll
    }.min(max_scroll);
    app.file_tree_scroll = scroll;

    // Build viewport slice directly (single clone operation)
    let mut display_lines: Vec<Line> = app.file_tree_lines_cache.iter()
        .skip(scroll)
        .take(viewport_height)
        .cloned()
        .collect();

    let title = if total > viewport_height {
        format!(" Filetree [{}/{}] ", scroll + display_lines.len().min(total - scroll), total)
    } else {
        " Filetree ".to_string()
    };

    // Build action bar line if an action is active
    let action_line: Option<Line> = app.file_tree_action.as_ref().map(|a| {
        let (label, buf) = match a {
            FileTreeAction::Add(b) => ("Add (/ = dir): ", b.as_str()),
            FileTreeAction::Rename(b) => ("Rename: ", b.as_str()),
            FileTreeAction::Copy(b) => ("Copy to: ", b.as_str()),
            FileTreeAction::Move(b) => ("Move to: ", b.as_str()),
            FileTreeAction::Delete => ("Delete? (y/N) ", ""),
        };
        let max_input = area.width.saturating_sub(2 + label.len() as u16) as usize;
        // Show tail of input if it's longer than available space
        let visible = if buf.len() > max_input { &buf[buf.len() - max_input..] } else { buf };
        Line::from(vec![
            Span::styled(label, Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
            Span::styled(visible.to_string(), Style::default().fg(Color::White)),
            Span::styled("█", Style::default().fg(Color::White)),
        ])
    });

    // Append action bar as the last line if active
    if let Some(action) = action_line {
        display_lines.push(action);
    }

    let widget = Paragraph::new(display_lines).block(
        Block::default()
            .borders(Borders::ALL)
            .border_type(if is_focused { BorderType::Double } else { BorderType::Plain })
            .title(if is_focused {
                Span::styled(title, Style::default().fg(AZURE).add_modifier(Modifier::BOLD))
            } else {
                Span::styled(title, Style::default().fg(Color::White))
            })
            .border_style(if is_focused {
                Style::default().fg(AZURE).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            }),
    );

    f.render_widget(widget, area);
}
