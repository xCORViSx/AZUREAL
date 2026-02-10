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

    // Determine clipboard source path for highlighting (Copy/Move mode)
    let clipboard_src: Option<&std::path::PathBuf> = match &app.file_tree_action {
        Some(FileTreeAction::Copy(p)) | Some(FileTreeAction::Move(p)) => Some(p),
        _ => None,
    };
    let clipboard_is_move = matches!(&app.file_tree_action, Some(FileTreeAction::Move(_)));

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

            // Check if this entry is the clipboard source (Copy/Move target)
            let is_clipboard_src = clipboard_src.map_or(false, |p| *p == entry.path);

            let max_name_len = 38usize.saturating_sub(entry.depth * 2 + 2);
            if is_clipboard_src {
                // Wrap name in box chars: ┃name┃ for copy, ╎name╎ for move (dashed)
                let (l, r) = if clipboard_is_move { ("╎", "╎") } else { ("┃", "┃") };
                let border_style = Style::default().fg(Color::Magenta);
                spans.push(Span::styled(l, border_style));
                spans.push(Span::styled(truncate(&entry.name, max_name_len.saturating_sub(2)), name_style));
                spans.push(Span::styled(r, border_style));
            } else {
                spans.push(Span::styled(truncate(&entry.name, max_name_len), name_style));
            }
            lines.push(Line::from(spans));
        }
    }

    lines
}

/// Build action bar text for the current file tree action.
/// Returns a flat string and its styled spans for wrapping.
fn build_action_bar_content(action: &FileTreeAction) -> (String, Vec<(String, Style)>) {
    match action {
        FileTreeAction::Copy(src) => {
            let name = src.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            let parts = vec![
                ("Copy ".to_string(), Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD)),
                (name, Style::default().fg(Color::White)),
                (" → Enter:paste Esc:cancel".to_string(), Style::default().fg(Color::DarkGray)),
            ];
            let plain: String = parts.iter().map(|(t, _)| t.as_str()).collect();
            (plain, parts)
        }
        FileTreeAction::Move(src) => {
            let name = src.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            let parts = vec![
                ("Move ".to_string(), Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD)),
                (name, Style::default().fg(Color::White)),
                (" → Enter:paste Esc:cancel".to_string(), Style::default().fg(Color::DarkGray)),
            ];
            let plain: String = parts.iter().map(|(t, _)| t.as_str()).collect();
            (plain, parts)
        }
        FileTreeAction::Add(buf) => {
            let label = "Add (/ = dir): ";
            let parts = vec![
                (label.to_string(), Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                (buf.clone(), Style::default().fg(Color::White)),
                ("█".to_string(), Style::default().fg(Color::White)),
            ];
            let plain: String = parts.iter().map(|(t, _)| t.as_str()).collect();
            (plain, parts)
        }
        FileTreeAction::Rename(buf) => {
            let label = "Rename: ";
            let parts = vec![
                (label.to_string(), Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                (buf.clone(), Style::default().fg(Color::White)),
                ("█".to_string(), Style::default().fg(Color::White)),
            ];
            let plain: String = parts.iter().map(|(t, _)| t.as_str()).collect();
            (plain, parts)
        }
        FileTreeAction::Delete => {
            let parts = vec![
                ("Delete? (y/N) ".to_string(), Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
            ];
            let plain: String = parts.iter().map(|(t, _)| t.as_str()).collect();
            (plain, parts)
        }
    }
}

/// Wrap styled parts into multiple Lines at `max_width` character boundary.
/// Walks each part's chars and starts a new Line when the running column hits max_width.
fn wrap_action_bar(parts: &[(String, Style)], max_width: usize) -> Vec<Line<'static>> {
    let mut lines: Vec<Line<'static>> = Vec::new();
    let mut current_spans: Vec<Span<'static>> = Vec::new();
    let mut col = 0usize;

    for (text, style) in parts {
        let mut chunk = String::new();
        for ch in text.chars() {
            if col >= max_width {
                // Flush current chunk as a span and start a new line
                if !chunk.is_empty() {
                    current_spans.push(Span::styled(chunk.clone(), *style));
                    chunk.clear();
                }
                lines.push(Line::from(std::mem::take(&mut current_spans)));
                col = 0;
            }
            chunk.push(ch);
            col += 1;
        }
        if !chunk.is_empty() {
            current_spans.push(Span::styled(chunk, *style));
        }
    }
    // Flush remaining spans
    if !current_spans.is_empty() {
        lines.push(Line::from(current_spans));
    }
    lines
}

/// Draw the file tree panel showing the session's worktree files
pub fn draw_file_tree(f: &mut Frame, app: &mut App, area: Rect) {
    let is_focused = app.focus == Focus::FileTree;
    // Inner width = area minus 2 border chars
    let inner_width = area.width.saturating_sub(2) as usize;

    // Compute how many lines the action bar needs (0 if no action)
    let action_lines: Vec<Line<'static>> = if let Some(ref action) = app.file_tree_action {
        let (_plain, parts) = build_action_bar_content(action);
        wrap_action_bar(&parts, inner_width.max(1))
    } else {
        Vec::new()
    };
    let action_line_count = action_lines.len();

    // Reserve action_line_count lines at bottom (+ 2 for top/bottom borders)
    let viewport_height = area.height.saturating_sub(2 + action_line_count as u16) as usize;

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

    // Append wrapped action bar lines at the bottom
    for line in action_lines {
        display_lines.push(line);
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
