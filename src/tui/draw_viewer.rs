//! Viewer panel rendering
//!
//! Shows file content when a file is selected from FileTree,
//! or diff detail when a diff is selected from Output.

use ratatui::{
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Paragraph},
    Frame,
};
use textwrap::{wrap, Options};

use crate::app::{App, Focus, ViewerMode};

/// Draw the viewer panel showing file content or diff detail
pub fn draw_viewer(f: &mut Frame, app: &mut App, area: Rect) {
    let is_focused = app.focus == Focus::Viewer;
    let viewport_height = area.height.saturating_sub(2) as usize;
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

                        let wrapped = wrap_spans(spans, content_width);
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
                                let new_spans = apply_selection_to_line(
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
                    let edit_idx = app.selected_tool_diff.map(|i| i + 1).unwrap_or(1);
                    let edit_total = app.clickable_paths.len();
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
                            Style::default().fg(Color::Cyan)
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
                                let new_spans = apply_selection_to_line(
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
    };

    // Use green border/title when in Edit diff view, cyan when focused normally, white otherwise
    let in_edit_diff = app.viewer_edit_diff.is_some();
    let border_color = if in_edit_diff {
        Color::Green
    } else if is_focused {
        Color::Cyan
    } else {
        Color::White
    };

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

fn wrap_text(text: &str, max_width: usize) -> Vec<String> {
    if text.is_empty() { return vec![String::new()]; }
    let opts = Options::new(max_width).break_words(true);
    wrap(text, opts).into_iter().map(|cow| cow.into_owned()).collect()
}

fn wrap_spans(spans: Vec<Span<'static>>, max_width: usize) -> Vec<Vec<Span<'static>>> {
    if max_width == 0 { return vec![spans]; }

    let mut full_text = String::new();
    let mut style_ranges: Vec<(usize, usize, Style)> = Vec::new();

    for span in &spans {
        let start = full_text.len();
        full_text.push_str(&span.content);
        let end = full_text.len();
        style_ranges.push((start, end, span.style));
    }

    if full_text.is_empty() { return vec![vec![]]; }

    let opts = Options::new(max_width).break_words(true);
    let wrapped_lines: Vec<String> = wrap(&full_text, opts)
        .into_iter()
        .map(|cow| cow.into_owned())
        .collect();

    let mut result: Vec<Vec<Span<'static>>> = Vec::new();
    let mut char_offset = 0;

    for wrapped in wrapped_lines {
        let line_start = char_offset;
        let line_end = char_offset + wrapped.len();
        let mut line_spans: Vec<Span<'static>> = Vec::new();

        for &(range_start, range_end, style) in &style_ranges {
            if range_end <= line_start || range_start >= line_end { continue; }
            let overlap_start = range_start.max(line_start);
            let overlap_end = range_end.min(line_end);
            if overlap_start < overlap_end {
                let local_start = overlap_start - line_start;
                let local_end = overlap_end - line_start;
                let text: String = wrapped.chars().skip(local_start).take(local_end - local_start).collect();
                if !text.is_empty() {
                    line_spans.push(Span::styled(text, style));
                }
            }
        }

        result.push(line_spans);
        char_offset = line_end;
        if char_offset < full_text.len() { char_offset += 1; }
    }

    if result.is_empty() { result.push(vec![]); }
    result
}

/// Apply selection highlighting to a line based on visual line indices.
/// `gutter` skips that many leading chars (line number column) from highlighting.
pub(crate) fn apply_selection_to_line(
    spans: Vec<Span<'static>>,
    line_content: &str,
    visual_line_idx: usize,
    sel_start_line: usize,
    sel_start_col: usize,
    sel_end_line: usize,
    sel_end_col: usize,
    gutter: usize,
) -> Vec<Span<'static>> {
    let line_len = line_content.chars().count();
    let sel_start = if visual_line_idx == sel_start_line { sel_start_col.max(gutter) } else { gutter };
    let sel_end = if visual_line_idx == sel_end_line { sel_end_col.max(gutter) } else { line_len };

    if sel_start >= sel_end || sel_end == 0 { return spans; }

    let selection_style = Style::default().bg(Color::Rgb(60, 60, 100));
    let mut result: Vec<Span<'static>> = Vec::new();
    let mut char_pos = 0;

    for span in spans {
        let span_len = span.content.chars().count();
        let span_end = char_pos + span_len;

        if span_end <= sel_start || char_pos >= sel_end {
            result.push(span);
        } else {
            let chars: Vec<char> = span.content.chars().collect();
            if char_pos < sel_start {
                let before: String = chars[..(sel_start - char_pos)].iter().collect();
                result.push(Span::styled(before, span.style));
            }
            let sel_in_span_start = sel_start.saturating_sub(char_pos);
            let sel_in_span_end = (sel_end - char_pos).min(span_len);
            if sel_in_span_start < sel_in_span_end {
                let selected: String = chars[sel_in_span_start..sel_in_span_end].iter().collect();
                result.push(Span::styled(selected, span.style.patch(selection_style)));
            }
            if span_end > sel_end {
                let after: String = chars[(sel_end - char_pos)..].iter().collect();
                result.push(Span::styled(after, span.style));
            }
        }
        char_pos = span_end;
    }
    result
}

/// Apply selection highlighting to spans for a given line
fn apply_selection_to_spans(
    spans: Vec<Span<'static>>,
    line_content: &str,
    line_idx: usize,
    sel_start_line: usize,
    sel_start_col: usize,
    sel_end_line: usize,
    sel_end_col: usize,
) -> Vec<Span<'static>> {
    // Calculate selection range for this line
    let line_len = line_content.chars().count();
    let sel_start = if line_idx == sel_start_line { sel_start_col } else { 0 };
    let sel_end = if line_idx == sel_end_line { sel_end_col } else { line_len };

    if sel_start >= sel_end { return spans; }

    let selection_style = Style::default().bg(Color::Rgb(60, 60, 100));

    // Rebuild spans with selection applied
    let mut result: Vec<Span<'static>> = Vec::new();
    let mut char_pos = 0;

    for span in spans {
        let span_len = span.content.chars().count();
        let span_end = char_pos + span_len;

        if span_end <= sel_start || char_pos >= sel_end {
            // Span is entirely outside selection
            result.push(span);
        } else {
            // Span overlaps with selection - split it
            let chars: Vec<char> = span.content.chars().collect();

            // Part before selection
            if char_pos < sel_start {
                let before: String = chars[..(sel_start - char_pos)].iter().collect();
                result.push(Span::styled(before, span.style));
            }

            // Selected part
            let sel_in_span_start = sel_start.saturating_sub(char_pos);
            let sel_in_span_end = (sel_end - char_pos).min(span_len);
            if sel_in_span_start < sel_in_span_end {
                let selected: String = chars[sel_in_span_start..sel_in_span_end].iter().collect();
                result.push(Span::styled(selected, span.style.patch(selection_style)));
            }

            // Part after selection
            if span_end > sel_end {
                let after: String = chars[(sel_end - char_pos)..].iter().collect();
                result.push(Span::styled(after, span.style));
            }
        }

        char_pos = span_end;
    }

    result
}

/// Draw viewer in edit mode
fn draw_edit_mode(f: &mut Frame, app: &mut App, area: Rect, viewport_height: usize, viewport_width: usize) {
    let path_str = app.viewer_path.as_ref()
        .map(|p| p.file_name().unwrap_or_default().to_string_lossy().to_string())
        .unwrap_or_else(|| "File".to_string());

    // Title on left, modified indicator on right
    let title = format!(" EDIT: {} ", path_str);

    let total_lines = app.viewer_edit_content.len();
    let line_num_width = total_lines.to_string().len().max(3);
    let content_width = viewport_width.saturating_sub(line_num_width + 3);

    // Get syntax highlighting for all edit content
    let full_content = app.viewer_edit_content.join("\n");
    let highlighted = app.syntax_highlighter.highlight_file(&full_content, &path_str);

    let scroll = app.viewer_scroll;
    let (cursor_line, cursor_col) = app.viewer_edit_cursor;

    // Normalize selection so start <= end (user can select backwards)
    let selection = app.viewer_edit_selection.map(|(sl, sc, el, ec)| {
        if sl < el || (sl == el && sc <= ec) {
            (sl, sc, el, ec)
        } else {
            (el, ec, sl, sc)
        }
    });

    // Build all wrapped lines with source line tracking
    // Each entry: (source_line_idx, wrap_idx, Line)
    let mut all_lines: Vec<(usize, usize, Line)> = Vec::new();
    for (idx, line_content) in app.viewer_edit_content.iter().enumerate() {
        let line_num_style = if idx == cursor_line {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default().fg(Color::DarkGray)
        };

        // Get highlighted spans for this line and wrap them
        let spans = highlighted.get(idx).cloned().unwrap_or_default();

        // Apply selection highlighting if this line is in selection range
        let spans = if let Some((sel_start_line, sel_start_col, sel_end_line, sel_end_col)) = selection {
            if idx >= sel_start_line && idx <= sel_end_line {
                apply_selection_to_spans(spans, line_content, idx, sel_start_line, sel_start_col, sel_end_line, sel_end_col)
            } else {
                spans
            }
        } else {
            spans
        };

        let wrapped = wrap_spans(spans, content_width);

        for (wrap_idx, wrapped_spans) in wrapped.into_iter().enumerate() {
            let line_num = if wrap_idx == 0 {
                format!("{:>width$} │ ", idx + 1, width = line_num_width)
            } else {
                format!("{:>width$} │ ", "", width = line_num_width)
            };
            let mut line_spans = vec![Span::styled(line_num, line_num_style)];
            line_spans.extend(wrapped_spans);
            all_lines.push((idx, wrap_idx, Line::from(line_spans)));
        }
    }

    // Get display lines based on scroll
    let display_lines: Vec<Line> = all_lines.iter()
        .skip(scroll)
        .take(viewport_height)
        .map(|(_, _, line)| line.clone())
        .collect();

    // Pad with empty lines if needed
    let mut final_lines = display_lines;
    while final_lines.len() < viewport_height {
        let line_num = format!("{:>width$} │ ", "~", width = line_num_width);
        final_lines.push(Line::from(Span::styled(line_num, Style::default().fg(Color::DarkGray))));
    }

    // Build title line with indicator on right
    let title_line = if app.viewer_edit_dirty {
        let padding = area.width.saturating_sub(title.len() as u16 + 13) as usize;
        Line::from(vec![
            Span::styled(title, Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
            Span::raw(" ".repeat(padding)),
            Span::styled("[modified]", Style::default().fg(Color::Yellow)),
        ])
    } else {
        Line::from(Span::styled(title, Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)))
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Double)
        .title(title_line)
        .border_style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD));

    let widget = Paragraph::new(final_lines).block(block);
    f.render_widget(widget, area);

    // Calculate cursor position accounting for wrapping
    // Find which visual line the cursor is on
    let mut cursor_visual_line: Option<usize> = None;
    let mut cursor_visual_col = cursor_col;

    for (visual_idx, (src_line, wrap_idx, _)) in all_lines.iter().enumerate() {
        if *src_line == cursor_line {
            let wrap_start = wrap_idx * content_width;
            let wrap_end = wrap_start + content_width;
            if cursor_col >= wrap_start && cursor_col < wrap_end {
                cursor_visual_line = Some(visual_idx);
                cursor_visual_col = cursor_col - wrap_start;
                break;
            } else if cursor_col >= wrap_end && *wrap_idx == 0 {
                // Continue looking for the right wrap segment
            } else if *wrap_idx > 0 && cursor_col < wrap_start {
                // Cursor is before this wrap, use previous
                cursor_visual_line = Some(visual_idx.saturating_sub(1));
                cursor_visual_col = content_width; // End of previous line
                break;
            }
        }
        // If we passed the cursor line, cursor is at end of last wrap
        if *src_line > cursor_line && cursor_visual_line.is_none() {
            cursor_visual_line = Some(visual_idx.saturating_sub(1));
            break;
        }
    }

    // Handle cursor at end of line
    if cursor_visual_line.is_none() && cursor_line < app.viewer_edit_content.len() {
        // Find last visual line for cursor_line
        for (visual_idx, (src_line, _, _)) in all_lines.iter().enumerate().rev() {
            if *src_line == cursor_line {
                cursor_visual_line = Some(visual_idx);
                let line_len = app.viewer_edit_content[cursor_line].chars().count();
                cursor_visual_col = cursor_col.min(line_len) % content_width.max(1);
                break;
            }
        }
    }

    // Position cursor if visible in viewport
    if let Some(visual_line) = cursor_visual_line {
        if visual_line >= scroll && visual_line < scroll + viewport_height {
            let screen_line = visual_line - scroll;
            f.set_cursor_position((
                area.x + 1 + line_num_width as u16 + 3 + cursor_visual_col as u16,
                area.y + 1 + screen_line as u16,
            ));
        }
    }
}

/// Draw discard confirmation dialog
fn draw_discard_dialog(f: &mut Frame, area: Rect, from_edit_diff: bool) {
    let dialog_width = if from_edit_diff { 50u16 } else { 40u16 };
    let dialog_height = if from_edit_diff { 9u16 } else { 7u16 };
    let x = area.x + (area.width.saturating_sub(dialog_width)) / 2;
    let y = area.y + (area.height.saturating_sub(dialog_height)) / 2;
    let dialog_area = Rect::new(x, y, dialog_width, dialog_height);

    f.render_widget(Clear, dialog_area);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Double)
        .title(Span::styled(" Unsaved Changes ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)))
        .border_style(Style::default().fg(Color::Yellow));

    f.render_widget(block, dialog_area);

    if from_edit_diff {
        let chunks = Layout::default()
            .constraints([
                Constraint::Length(2),
                Constraint::Length(2),
                Constraint::Length(2),
            ])
            .margin(1)
            .split(dialog_area);

        let msg = Paragraph::new("Discard changes?")
            .alignment(Alignment::Center)
            .style(Style::default().fg(Color::White));
        f.render_widget(msg, chunks[0]);

        let options1 = Paragraph::new("(y)es discard  (n)o cancel")
            .alignment(Alignment::Center)
            .style(Style::default().fg(Color::Gray));
        f.render_widget(options1, chunks[1]);

        let options2 = Paragraph::new("(s)ave → diff  (f)save → file")
            .alignment(Alignment::Center)
            .style(Style::default().fg(Color::Gray));
        f.render_widget(options2, chunks[2]);
    } else {
        let chunks = Layout::default()
            .constraints([
                Constraint::Length(2),
                Constraint::Length(2),
                Constraint::Length(2),
            ])
            .margin(1)
            .split(dialog_area);

        let msg = Paragraph::new("Discard changes?")
            .alignment(Alignment::Center)
            .style(Style::default().fg(Color::White));
        f.render_widget(msg, chunks[0]);

        let options = Paragraph::new("(y)es  (n)o  (s)ave and exit")
            .alignment(Alignment::Center)
            .style(Style::default().fg(Color::Gray));
        f.render_widget(options, chunks[1]);
    }
}

/// Draw tab bar at top of viewer showing open tabs
fn draw_tab_bar(f: &mut Frame, app: &App, area: Rect) {
    // Tab bar goes inside the border, at the top
    let bar_area = Rect::new(area.x + 1, area.y + 1, area.width.saturating_sub(2), 1);
    if bar_area.width < 10 || bar_area.height == 0 { return; }

    let mut spans: Vec<Span> = Vec::new();
    let max_tab_width = 15usize;

    for (idx, tab) in app.viewer_tabs.iter().enumerate() {
        let name = tab.name();
        let display_name = if name.len() > max_tab_width - 4 {
            format!("{}…", &name[..max_tab_width - 5])
        } else {
            name.to_string()
        };

        let is_active = idx == app.viewer_active_tab;
        let style = if is_active {
            Style::default().fg(Color::Black).bg(Color::Cyan).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Gray).bg(Color::DarkGray)
        };

        // Add separator before non-first tabs
        if idx > 0 {
            spans.push(Span::raw(" "));
        }

        spans.push(Span::styled(format!(" {} ", display_name), style));
    }

    // Add hint for more tabs
    if app.viewer_tabs.len() > 1 {
        spans.push(Span::styled("  [/] ", Style::default().fg(Color::DarkGray)));
    }

    let line = Line::from(spans);
    let para = Paragraph::new(line);
    f.render_widget(para, bar_area);
}

/// Draw tab dialog overlay for switching between tabs
fn draw_tab_dialog(f: &mut Frame, app: &App, area: Rect) {
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
        .title(Span::styled(" Tabs ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)))
        .border_style(Style::default().fg(Color::Cyan));

    f.render_widget(block.clone(), dialog_area);

    // List tabs inside dialog
    let inner = block.inner(dialog_area);
    let mut lines: Vec<Line> = Vec::new();

    for (idx, tab) in app.viewer_tabs.iter().enumerate() {
        let name = tab.name();
        let is_active = idx == app.viewer_active_tab;

        let prefix = if is_active { "▸ " } else { "  " };
        let style = if is_active {
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
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

/// Draw post-save dialog (after saving from Edit diff view)
fn draw_save_dialog(f: &mut Frame, area: Rect) {
    let dialog_width = 45u16;
    let dialog_height = 8u16;
    let x = area.x + (area.width.saturating_sub(dialog_width)) / 2;
    let y = area.y + (area.height.saturating_sub(dialog_height)) / 2;
    let dialog_area = Rect::new(x, y, dialog_width, dialog_height);

    f.render_widget(Clear, dialog_area);

    let chunks = Layout::default()
        .constraints([
            Constraint::Length(2),
            Constraint::Length(2),
            Constraint::Length(2),
        ])
        .margin(1)
        .split(dialog_area);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Double)
        .title(Span::styled(" File Saved ", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)))
        .border_style(Style::default().fg(Color::Green));

    f.render_widget(block, dialog_area);

    let msg = Paragraph::new("Where would you like to go?")
        .alignment(Alignment::Center)
        .style(Style::default().fg(Color::White));
    f.render_widget(msg, chunks[0]);

    let options = Paragraph::new("(d)iff view  (f)ile view  (Esc)continue")
        .alignment(Alignment::Center)
        .style(Style::default().fg(Color::Gray));
    f.render_widget(options, chunks[1]);
}  