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
use super::file_icons::file_icon;
use super::util::{truncate, AZURE};

/// Check whether a file tree entry is "inside" one of the god file filter directories.
/// A directory itself counts as inside if it's in the set. Files and subdirectories
/// are inside if any ancestor path is in the set.
fn is_in_god_file_scope(app: &App, path: &std::path::Path, is_dir: bool) -> bool {
    if !app.god_file_filter_mode { return false; }
    // Direct membership — the dir itself is in the filter set
    if is_dir && app.god_file_filter_dirs.contains(path) { return true; }
    // Walk ancestors to see if any parent is in the filter set
    let mut p = path.parent();
    while let Some(ancestor) = p {
        if app.god_file_filter_dirs.contains(ancestor) { return true; }
        p = ancestor.parent();
    }
    false
}

/// Check if a directory is in the god file filter set OR is a subdirectory of one.
/// Subdirs of accepted dirs automatically inherit accepted status (bright green).
fn is_god_file_filter_dir(app: &App, path: &std::path::Path) -> bool {
    if !app.god_file_filter_mode { return false; }
    if app.god_file_filter_dirs.contains(path) { return true; }
    // Walk ancestors — if any parent is accepted, this subdir is too
    let mut p = path.parent();
    while let Some(ancestor) = p {
        if app.god_file_filter_dirs.contains(ancestor) { return true; }
        p = ancestor.parent();
    }
    false
}

/// Build file tree lines (extracted for caching)
fn build_file_tree_lines(app: &App) -> Vec<Line<'static>> {
    let mut lines: Vec<Line> = Vec::new();

    // Determine clipboard source path for highlighting (Copy/Move mode)
    let clipboard_src: Option<&std::path::PathBuf> = match &app.file_tree_action {
        Some(FileTreeAction::Copy(p)) | Some(FileTreeAction::Move(p)) => Some(p),
        _ => None,
    };
    let clipboard_is_move = matches!(&app.file_tree_action, Some(FileTreeAction::Move(_)));

    /// Green color for directories/files that are in the health scan scope
    const GF_GREEN: Color = Color::Rgb(80, 200, 80);
    /// Dim green for files inside a scanned directory (less prominent than dir itself)
    const GF_GREEN_DIM: Color = Color::Rgb(60, 140, 60);

    if app.file_tree_entries.is_empty() {
        if app.current_worktree().and_then(|s| s.worktree_path.as_ref()).is_none() {
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

            // Whether this entry is part of the god file scan scope
            let in_gf_scope = is_in_god_file_scope(app, &entry.path, entry.is_dir);
            let is_gf_dir = entry.is_dir && is_god_file_filter_dir(app, &entry.path);

            // Get icon glyph + color from file_icons module (Nerd Font or emoji fallback)
            let expanded = entry.is_dir && app.file_tree_expanded.contains(&entry.path);
            let (icon, mut icon_color) = file_icon(&entry.path, entry.is_dir, expanded, app.nerd_fonts);
            // In filter mode, scoped entries get green icons; hidden entries get dimmed
            if app.god_file_filter_mode && is_gf_dir {
                icon_color = GF_GREEN;
            } else if app.god_file_filter_mode && in_gf_scope {
                icon_color = GF_GREEN_DIM;
            } else if entry.is_hidden {
                icon_color = if entry.is_dir { Color::Rgb(120, 100, 60) } else { Color::Rgb(100, 100, 100) };
            }

            let mut spans = vec![
                Span::raw(indent),
                Span::styled(icon, Style::default().fg(icon_color)),
            ];

            let name_style = if is_selected {
                // In filter mode, selected row background changes from blue to green for scoped dirs
                let bg = if app.god_file_filter_mode && is_gf_dir {
                    Color::Rgb(30, 100, 30)
                } else {
                    Color::Blue
                };
                Style::default().bg(bg).fg(Color::White).add_modifier(Modifier::BOLD)
            } else if entry.is_dir {
                // Filter mode: scoped dirs are green, others dim; normal mode: cyan/dimmed
                let color = if app.god_file_filter_mode {
                    if is_gf_dir { GF_GREEN } else if entry.is_hidden { Color::Rgb(80, 120, 130) } else { Color::DarkGray }
                } else if entry.is_hidden {
                    Color::Rgb(80, 120, 130)
                } else {
                    AZURE
                };
                Style::default().fg(color).add_modifier(Modifier::BOLD)
            } else {
                // Filter mode: files in scope get dim green, others dim gray
                let color = if app.god_file_filter_mode {
                    if in_gf_scope { GF_GREEN_DIM } else if entry.is_hidden { Color::Rgb(70, 70, 70) } else { Color::Rgb(100, 100, 100) }
                } else if entry.is_hidden {
                    Color::Rgb(100, 100, 100)
                } else {
                    Color::White
                };
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

/// Wrap styled parts into multiple Lines, breaking at space boundaries so
/// tokens like "Enter:paste" or "Esc:cancel" stay together on one line.
/// Falls back to hard char-break only when a single token exceeds max_width.
fn wrap_action_bar(parts: &[(String, Style)], max_width: usize) -> Vec<Line<'static>> {
    // Flatten all parts into (word, style) tokens split on spaces.
    // Leading/trailing spaces become empty "" tokens to preserve spacing.
    let mut tokens: Vec<(&str, Style)> = Vec::new();
    for (text, style) in parts {
        let mut first = true;
        for word in text.split(' ') {
            if !first { tokens.push((" ", *style)); }
            if !word.is_empty() { tokens.push((word, *style)); }
            first = false;
        }
    }

    let mut lines: Vec<Line<'static>> = Vec::new();
    let mut current_spans: Vec<Span<'static>> = Vec::new();
    let mut col = 0usize;

    for (token, style) in tokens {
        let len = token.chars().count();
        // Space token — only emit if it fits, otherwise skip (line break absorbs it)
        if token == " " {
            if col + 1 <= max_width { current_spans.push(Span::styled(" ", style)); col += 1; }
            continue;
        }
        // Word fits on current line
        if col + len <= max_width {
            current_spans.push(Span::styled(token.to_string(), style));
            col += len;
            continue;
        }
        // Word doesn't fit — wrap to next line (flush current)
        if !current_spans.is_empty() {
            lines.push(Line::from(std::mem::take(&mut current_spans)));
            col = 0;
        }
        // If the word itself fits on a fresh line, emit it whole
        if len <= max_width {
            current_spans.push(Span::styled(token.to_string(), style));
            col = len;
        } else {
            // Single word wider than max_width — hard-break it char by char
            let mut chunk = String::new();
            for ch in token.chars() {
                if col >= max_width {
                    if !chunk.is_empty() { current_spans.push(Span::styled(chunk.clone(), style)); chunk.clear(); }
                    lines.push(Line::from(std::mem::take(&mut current_spans)));
                    col = 0;
                }
                chunk.push(ch);
                col += 1;
            }
            if !chunk.is_empty() { current_spans.push(Span::styled(chunk, style)); }
        }
    }
    if !current_spans.is_empty() { lines.push(Line::from(current_spans)); }
    lines
}

/// Names that can be toggled visible/hidden in the options overlay.
/// Order here = display order. All hidden by default on first launch.
const FT_OPTIONS: &[&str] = &["worktrees", ".git", ".claude", ".azureal", ".DS_Store"];

/// Draw the file tree options overlay — replaces normal tree content when
/// app.file_tree_options_mode is true. Shows toggleable checkboxes for each
/// hidden directory with QuadrantOutside border and "Filetree Options" title.
fn draw_file_tree_options(f: &mut Frame, app: &App, area: Rect) {
    let mut lines: Vec<Line> = Vec::new();
    lines.push(Line::from(""));

    for (i, &name) in FT_OPTIONS.iter().enumerate() {
        let checked = app.file_tree_hidden_dirs.contains(name);
        let selected = i == app.file_tree_options_selected;
        let checkbox = if checked { "[x]" } else { "[ ]" };
        let label = format!("  {} Hide {}", checkbox, name);
        let style = if selected {
            Style::default().fg(AZURE).add_modifier(Modifier::BOLD)
        } else if checked {
            Style::default().fg(Color::Green)
        } else {
            Style::default().fg(Color::DarkGray)
        };
        lines.push(Line::from(Span::styled(label, style)));
    }

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::QuadrantOutside)
        .border_style(Style::default().fg(AZURE))
        .title(Span::styled(" Filetree Options ", Style::default().fg(AZURE).add_modifier(Modifier::BOLD)))
        .title_bottom(Line::from(Span::styled(
            " Space:toggle  Esc:close ",
            Style::default().fg(AZURE),
        )));

    f.render_widget(Paragraph::new(lines).block(block), area);
}

/// Draw the file tree panel showing the session's worktree files
pub fn draw_file_tree(f: &mut Frame, app: &mut App, area: Rect) {
    // Options overlay replaces normal file tree content
    if app.file_tree_options_mode {
        draw_file_tree_options(f, app, area);
        return;
    }

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

    let in_filter_mode = app.god_file_filter_mode;
    /// Green color matching the scoped directory highlights (health scope mode)
    const GF_BORDER_GREEN: Color = Color::Rgb(80, 200, 80);

    let ro_suffix = if app.browsing_main { " (read-only)" } else { "" };
    let title = if in_filter_mode {
        // Filter mode title shows scope count and Enter/Esc hints
        let scope_count = app.god_file_filter_dirs.len();
        format!(" Health Scope ({} dir{}) ", scope_count, if scope_count == 1 { "" } else { "s" })
    } else if total > viewport_height {
        format!(" Filetree{} [{}/{}] ", ro_suffix, scroll + display_lines.len().min(total - scroll), total)
    } else {
        format!(" Filetree{} ", ro_suffix)
    };

    // Append wrapped action bar lines at the bottom
    for line in action_lines {
        display_lines.push(line);
    }

    // In filter mode: green border; main browse: yellow; normal: azure/white
    let (border_color, title_style) = if in_filter_mode {
        (GF_BORDER_GREEN, Style::default().fg(GF_BORDER_GREEN).add_modifier(Modifier::BOLD))
    } else if app.browsing_main {
        (Color::Yellow, Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))
    } else if is_focused {
        (AZURE, Style::default().fg(AZURE).add_modifier(Modifier::BOLD))
    } else {
        (Color::White, Style::default().fg(Color::White))
    };

    let mut block = Block::default()
        .borders(Borders::ALL)
        .border_type(if is_focused || in_filter_mode { BorderType::Double } else { BorderType::Plain })
        .title(Span::styled(title, title_style))
        .border_style(Style::default().fg(border_color));

    // Bottom border hint in scope mode: "Enter:toggle  Esc:save & rescan"
    if in_filter_mode {
        block = block.title_bottom(Line::from(Span::styled(
            " Enter:toggle  Esc:save & rescan ",
            Style::default().fg(GF_BORDER_GREEN),
        )));
    }

    let widget = Paragraph::new(display_lines).block(block);

    f.render_widget(widget, area);
}
