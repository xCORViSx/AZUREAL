//! Viewer panel rendering
//!
//! Shows file content when a file is selected from FileTree,
//! or diff detail when a diff is selected from Output.
//!
//! Submodules:
//! - `wrapping`: Text wrapping utilities (word-boundary breaks for plain text and styled spans)
//! - `selection`: Selection highlighting (mouse drag visual selection)
//! - `edit_mode`: Full edit mode rendering with cursor and syntax highlighting
//! - `dialogs`: Save/discard confirmation dialogs
//! - `tabs`: Tab bar rendering and tab picker dialog
//! - `git_viewer`: Git panel diff viewer

mod dialogs;
mod edit_mode;
mod git_viewer;
mod selection;
mod tabs;
mod wrapping;

/// Re-export public API so existing imports work unchanged
pub(crate) use selection::apply_selection_to_line;
pub(crate) use wrapping::word_wrap_breaks;

use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Paragraph, StatefulWidget},
    Frame,
};
use ratatui_image::StatefulImage;

use crate::app::{App, Focus, ViewerMode};
use super::util::AZURE;

use dialogs::{draw_discard_dialog, draw_save_dialog};
use edit_mode::draw_edit_mode;
use git_viewer::draw_git_viewer_selectable;
use selection::apply_selection_to_line as sel_line;
use tabs::{draw_tab_bar, draw_tab_dialog, tab_bar_rows};
use wrapping::{wrap_spans_word, wrap_text};

/// Draw the viewer panel showing file content or diff detail
pub fn draw_viewer(f: &mut Frame, app: &mut App, area: Rect) {
    let is_focused = app.focus == Focus::Viewer;
    // Tab bar eats 1-2 rows from the top of the content area
    let tb_rows = tab_bar_rows(app.viewer_tabs.len()) as usize;
    let viewport_height = area.height.saturating_sub(2).saturating_sub(tb_rows as u16) as usize;
    let viewport_width = area.width.saturating_sub(2) as usize;

    // Cache viewport height for scroll operations (input handling uses this)
    app.viewer_viewport_height = viewport_height;

    // Edit mode takes over rendering
    if app.viewer_edit_mode {
        draw_edit_mode(f, app, area, viewport_height, viewport_width);
        // Draw dialogs on top if active
        if app.viewer_edit_save_dialog {
            draw_save_dialog(f, area);
        } else if app.viewer_edit_discard_dialog {
            draw_discard_dialog(f, area, app.viewer_edit_diff.is_some());
        }
        return;
    }

    // Git panel mode — show diff content instead of file viewer
    if app.git_actions_panel.is_some() {
        draw_git_viewer_selectable(f, app, area, is_focused, viewport_height);
        return;
    }

    // Image mode — render via terminal graphics protocol (Kitty/Sixel/halfblock)
    if app.viewer_mode == ViewerMode::Image {
        if let Some(ref mut proto) = app.viewer_image_state {
            let path_str = app.viewer_path.as_ref()
                .map(|p| p.file_name().unwrap_or_default().to_string_lossy().to_string())
                .unwrap_or_else(|| "Image".to_string());
            let border_color = if is_focused { AZURE } else { Color::White };
            let border_mod = if is_focused { Modifier::BOLD } else { Modifier::empty() };
            let block = Block::default()
                .borders(Borders::ALL)
                .border_type(if is_focused { BorderType::Double } else { BorderType::Plain })
                .title(Span::styled(format!(" {} ", path_str), Style::default().fg(border_color).add_modifier(border_mod)))
                .border_style(Style::default().fg(border_color).add_modifier(border_mod));
            // Compute inner area for the image (inside border, below tab bar)
            let inner = block.inner(area);
            f.render_widget(block, area);
            let img_area = Rect {
                y: inner.y + tb_rows as u16,
                height: inner.height.saturating_sub(tb_rows as u16),
                ..inner
            };
            if img_area.width > 0 && img_area.height > 0 {
                StatefulImage::new().render(img_area, f.buffer_mut(), proto);
            }
            // Tab bar still draws on top
            if !app.viewer_tabs.is_empty() { draw_tab_bar(f, app, area); }
            if app.viewer_tab_dialog { draw_tab_dialog(f, app, area); }
            return;
        }
    }

    let (title, lines) = match app.viewer_mode {
        ViewerMode::Empty => {
            let placeholder = vec![
                Line::from(""),
                Line::from(Span::styled("Select a file from the tree", Style::default().fg(Color::DarkGray))),
                Line::from(Span::styled("or a diff from output", Style::default().fg(Color::DarkGray))),
            ];
            (" Viewer ".to_string(), placeholder)
        }
        ViewerMode::File => {
            let path_str = app.viewer_path.as_ref()
                .map(|p| p.file_name().unwrap_or_default().to_string_lossy().to_string())
                .unwrap_or_else(|| "File".to_string());

            if app.viewer_content.is_some() {
                // Only re-render if cache is dirty or width changed
                if app.viewer_lines_dirty || app.viewer_lines_width != viewport_width {
                    let content = app.viewer_content.as_ref().unwrap();
                    let content_lines: Vec<&str> = content.lines().collect();
                    let highlighted = app.syntax_highlighter.highlight_file(content, &path_str);

                    // Check for edit diff overlay
                    let (diff_start_line, diff_line_count, old_lines) = if let Some((ref old_str, ref new_str)) = app.viewer_edit_diff {
                        // Find where new_str starts in the file
                        let new_lines: Vec<&str> = new_str.lines().collect();
                        let old_lines_vec: Vec<&str> = old_str.lines().collect();
                        let mut found_line = None;

                        // Search for the first line of new_str
                        if !new_lines.is_empty() {
                            for (idx, line) in content_lines.iter().enumerate() {
                                if *line == new_lines[0] {
                                    // Check if all subsequent lines match
                                    let mut matches = true;
                                    for (offset, new_line) in new_lines.iter().enumerate() {
                                        if content_lines.get(idx + offset) != Some(new_line) {
                                            matches = false;
                                            break;
                                        }
                                    }
                                    if matches {
                                        found_line = Some(idx);
                                        break;
                                    }
                                }
                            }
                        }

                        if let Some(start) = found_line {
                            (Some(start), new_lines.len(), old_lines_vec)
                        } else {
                            (None, 0, vec![])
                        }
                    } else {
                        (None, 0, vec![])
                    };

                    // Calculate line number width including old lines that will be inserted
                    let total_visual_lines = highlighted.len() + old_lines.len();
                    let line_num_width = total_visual_lines.to_string().len().max(3);
                    let content_width = viewport_width.saturating_sub(line_num_width + 3);

                    let mut all_lines: Vec<Line> = Vec::new();
                    let mut line_numbers: Vec<usize> = Vec::new();

                    for (line_idx, spans) in highlighted.into_iter().enumerate() {
                        // Insert old (deleted) lines before the new content at diff_start_line
                        if diff_start_line == Some(line_idx) && !old_lines.is_empty() {
                            for old_line in &old_lines {
                                let line_num = format!("{:>width$} │ ", "-", width = line_num_width);
                                let mut all_spans = vec![
                                    Span::styled(line_num, Style::default().fg(Color::Red)),
                                    Span::styled((*old_line).to_string(), Style::default().fg(Color::Red).bg(Color::Rgb(60, 20, 20))),
                                ];
                                // Pad to fill width for background
                                let old_len = old_line.chars().count();
                                if old_len < content_width {
                                    all_spans.push(Span::styled(" ".repeat(content_width - old_len), Style::default().bg(Color::Rgb(60, 20, 20))));
                                }
                                all_lines.push(Line::from(all_spans));
                                line_numbers.push(0); // 0 = deleted line (not in file)
                            }
                        }

                        // Check if this line is part of the added (new) content
                        let is_added_line = if let Some(start) = diff_start_line {
                            line_idx >= start && line_idx < start + diff_line_count
                        } else {
                            false
                        };

                        let wrapped = wrap_spans_word(spans, content_width);
                        for (wrap_idx, mut wrapped_spans) in wrapped.into_iter().enumerate() {
                            let line_num = if wrap_idx == 0 {
                                format!("{:>width$} │ ", line_idx + 1, width = line_num_width)
                            } else {
                                format!("{:>width$} │ ", "", width = line_num_width)
                            };

                            // Apply green background for added lines
                            if is_added_line {
                                let line_num_style = Style::default().fg(Color::Green);
                                let green_bg = Color::Rgb(20, 60, 20);
                                wrapped_spans = wrapped_spans.into_iter()
                                    .map(|s| Span::styled(s.content.to_string(), s.style.bg(green_bg)))
                                    .collect();
                                let content_len: usize = wrapped_spans.iter().map(|s| s.content.chars().count()).sum();
                                let mut all_spans = vec![Span::styled(line_num, line_num_style)];
                                all_spans.extend(wrapped_spans);
                                // Pad to fill width for background
                                if content_len < content_width {
                                    all_spans.push(Span::styled(" ".repeat(content_width - content_len), Style::default().bg(green_bg)));
                                }
                                all_lines.push(Line::from(all_spans));
                            } else {
                                let mut all_spans = vec![Span::styled(line_num, Style::default().fg(Color::DarkGray))];
                                all_spans.extend(wrapped_spans);
                                all_lines.push(Line::from(all_spans));
                            }
                            line_numbers.push(line_idx + 1); // 1-indexed original line
                        }
                    }

                    app.viewer_lines_cache = all_lines;
                    app.viewer_line_numbers = line_numbers;
                    app.viewer_original_line_count = content_lines.len();
                    app.viewer_lines_width = viewport_width;
                    app.viewer_lines_dirty = false;
                }

                let total = app.viewer_original_line_count;

                // Clamp scroll to valid range (resolves usize::MAX sentinel)
                app.clamp_viewer_scroll();
                let scroll = app.viewer_scroll;

                // Gutter = char width of line number column (first span, e.g. "  1 │ ")
                let gutter = app.viewer_lines_cache.first()
                    .and_then(|l| l.spans.first())
                    .map(|s| s.content.chars().count())
                    .unwrap_or(0);

                // Build viewport slice with selection highlighting if active
                let display_lines: Vec<Line> = app.viewer_lines_cache.iter()
                    .enumerate()
                    .skip(scroll)
                    .take(viewport_height)
                    .map(|(visual_idx, line)| {
                        if let Some((sel_start_line, sel_start_col, sel_end_line, sel_end_col)) = app.viewer_selection {
                            if visual_idx >= sel_start_line && visual_idx <= sel_end_line {
                                let line_content: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
                                let new_spans = sel_line(
                                    line.spans.clone(),
                                    &line_content,
                                    visual_idx,
                                    sel_start_line, sel_start_col,
                                    sel_end_line, sel_end_col,
                                    gutter,
                                );
                                Line::from(new_spans)
                            } else {
                                line.clone()
                            }
                        } else {
                            line.clone()
                        }
                    })
                    .collect();

                // Find the last visible original line number for the title
                let last_visible_idx = (scroll + display_lines.len()).saturating_sub(1);
                let last_visible_line = app.viewer_line_numbers.get(last_visible_idx).copied().unwrap_or(total);

                // Show "Edit" indicator if viewing an edit diff
                let title = if app.viewer_edit_diff.is_some() {
                    // Count only Edit tool entries (non-empty old/new strings)
                    let edits: Vec<usize> = app.clickable_paths.iter().enumerate()
                        .filter(|(_, (_, _, _, _, o, n, _))| !o.is_empty() || !n.is_empty())
                        .map(|(i, _)| i).collect();
                    let edit_total = edits.len();
                    // Find which edit-only position we're at (1-indexed)
                    let edit_idx = app.selected_tool_diff
                        .and_then(|s| edits.iter().position(|&e| e == s))
                        .map(|p| p + 1)
                        .unwrap_or(1);
                    format!(" {} [Edit {}/{}] ", path_str, edit_idx, edit_total)
                } else if app.viewer_lines_cache.len() > viewport_height {
                    format!(" {} [{}/{}] ", path_str, last_visible_line, total)
                } else {
                    format!(" {} ({} lines) ", path_str, total)
                };

                (title, display_lines)
            } else {
                (format!(" {} ", path_str), vec![Line::from("No content")])
            }
        }
        ViewerMode::Diff => {
            if app.viewer_content.is_some() {
                // Cache diff lines too (wrapping is expensive)
                if app.viewer_lines_dirty || app.viewer_lines_width != viewport_width {
                    let content = app.viewer_content.as_ref().unwrap();
                    let mut all_lines: Vec<Line> = Vec::new();
                    for line in content.lines() {
                        let style = if line.starts_with('+') && !line.starts_with("+++") {
                            Style::default().fg(Color::Green)
                        } else if line.starts_with('-') && !line.starts_with("---") {
                            Style::default().fg(Color::Rgb(100, 100, 100))
                        } else if line.starts_with("@@") {
                            Style::default().fg(AZURE)
                        } else if line.starts_with("diff ") || line.starts_with("index ") {
                            Style::default().fg(Color::Yellow)
                        } else {
                            Style::default()
                        };

                        for wrapped in wrap_text(line, viewport_width) {
                            all_lines.push(Line::from(Span::styled(wrapped, style)));
                        }
                    }

                    app.viewer_lines_cache = all_lines;
                    app.viewer_lines_width = viewport_width;
                    app.viewer_lines_dirty = false;
                }

                let total = app.viewer_lines_cache.len();

                // Clamp scroll to valid range (resolves usize::MAX sentinel)
                app.clamp_viewer_scroll();
                let scroll = app.viewer_scroll;

                // Build viewport slice with selection highlighting if active (no gutter in Diff mode)
                let display_lines: Vec<Line> = app.viewer_lines_cache.iter()
                    .enumerate()
                    .skip(scroll)
                    .take(viewport_height)
                    .map(|(visual_idx, line)| {
                        if let Some((sel_start_line, sel_start_col, sel_end_line, sel_end_col)) = app.viewer_selection {
                            if visual_idx >= sel_start_line && visual_idx <= sel_end_line {
                                let line_content: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
                                let new_spans = sel_line(
                                    line.spans.clone(),
                                    &line_content,
                                    visual_idx,
                                    sel_start_line, sel_start_col,
                                    sel_end_line, sel_end_col,
                                    0,
                                );
                                Line::from(new_spans)
                            } else {
                                line.clone()
                            }
                        } else {
                            line.clone()
                        }
                    })
                    .collect();

                // Show worktree name in title if available
                let name = app.viewer_path.as_ref()
                    .and_then(|p| p.file_name())
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| "Diff".to_string());
                let title = if total > viewport_height {
                    format!(" {} [{}/{}] ", name, scroll + 1, total)
                } else {
                    format!(" {} ({} lines) ", name, total)
                };

                (title, display_lines)
            } else {
                (" Diff ".to_string(), vec![Line::from("Press 'd' on a worktree to view diff")])
            }
        }
        // Image mode is handled by the early return above — this arm is unreachable
        ViewerMode::Image => (" Image ".to_string(), vec![]),
    };

    // Use green border/title when in Edit diff view, cyan when focused normally, white otherwise
    let in_edit_diff = app.viewer_edit_diff.is_some();
    let border_color = if in_edit_diff {
        Color::Green
    } else if is_focused {
        AZURE
    } else {
        Color::White
    };

    // Prepend empty lines so the tab bar overlays blank space, not real content.
    // The Paragraph renders from inner row 0, and the tab bar draws on top of rows 0-1.
    let lines = if tb_rows > 0 {
        let mut padded = vec![Line::from(""); tb_rows];
        padded.extend(lines);
        padded
    } else { lines };

    let widget = Paragraph::new(lines).block(
        Block::default()
            .borders(Borders::ALL)
            .border_type(if is_focused || in_edit_diff { BorderType::Double } else { BorderType::Plain })
            .title(Span::styled(title, Style::default().fg(border_color).add_modifier(if is_focused || in_edit_diff { Modifier::BOLD } else { Modifier::empty() })))
            .border_style(Style::default().fg(border_color).add_modifier(if is_focused || in_edit_diff { Modifier::BOLD } else { Modifier::empty() })),
    );

    f.render_widget(widget, area);

    // Draw tab bar if there are tabs
    if !app.viewer_tabs.is_empty() {
        draw_tab_bar(f, app, area);
    }

    // Draw tab dialog if active
    if app.viewer_tab_dialog {
        draw_tab_dialog(f, app, area);
    }
}
