//! Viewer panel rendering
//!
//! Shows file content when a file is selected from FileTree,
//! or diff detail when a diff is selected from Output.

use ratatui::{
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Paragraph, StatefulWidget},
    Frame,
};
use ratatui_image::StatefulImage;
use textwrap::{wrap, Options};

use crate::app::{App, Focus, ViewerMode};
use super::util::{GIT_BROWN, AZURE};

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
    if let Some(ref panel) = app.git_actions_panel {
        draw_git_viewer(f, panel, area, is_focused);
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

fn wrap_text(text: &str, max_width: usize) -> Vec<String> {
    if text.is_empty() { return vec![String::new()]; }
    let opts = Options::new(max_width).break_words(true);
    wrap(text, opts).into_iter().map(|cow| cow.into_owned()).collect()
}

/// Compute word-boundary wrap break positions for a single line. Returns a
/// Vec of char offsets where each visual row starts (first is always 0).
/// Uses textwrap for word boundaries, falls back to hard breaks for long words.
/// Used by both display wrapping and cursor/scroll math.
pub(crate) fn word_wrap_breaks(text: &str, max_width: usize) -> Vec<usize> {
    if max_width == 0 || text.is_empty() { return vec![0]; }
    let char_count = text.chars().count();
    if char_count <= max_width { return vec![0]; }
    let opts = Options::new(max_width).break_words(true);
    let wrapped = wrap(text, opts);
    let mut breaks = Vec::with_capacity(wrapped.len());
    let mut offset = 0usize;
    for segment in &wrapped {
        breaks.push(offset);
        offset += segment.chars().count();
        // textwrap eats the space at the break point — account for it
        // by checking if the next char in the original text is a space
        let next_char = text.chars().nth(offset);
        if next_char == Some(' ') { offset += 1; }
    }
    breaks
}

/// Word-boundary wrapping for styled spans. Uses textwrap to find break
/// positions, then slices the styled spans at those positions. Preserves
/// syntax highlighting across wrap boundaries.
fn wrap_spans_word(spans: Vec<Span<'static>>, max_width: usize) -> Vec<Vec<Span<'static>>> {
    if max_width == 0 { return vec![spans]; }
    // Flatten to (char, style) pairs and plain text for textwrap
    let mut chars_styled: Vec<(char, Style)> = Vec::new();
    let mut plain = String::new();
    for span in &spans {
        for c in span.content.chars() {
            chars_styled.push((c, span.style));
            plain.push(c);
        }
    }
    if chars_styled.is_empty() { return vec![vec![]]; }
    // Get break positions via textwrap
    let breaks = word_wrap_breaks(&plain, max_width);
    let total = chars_styled.len();
    let mut result: Vec<Vec<Span<'static>>> = Vec::with_capacity(breaks.len());
    for (i, &start) in breaks.iter().enumerate() {
        let end = if i + 1 < breaks.len() {
            // End at next break, but trim trailing space at the break boundary
            let next = breaks[i + 1];
            if next > 0 && start < next && chars_styled.get(next - 1).map(|c| c.0) == Some(' ') {
                next - 1
            } else {
                next
            }
        } else {
            total
        };
        // Merge consecutive chars with same style into spans
        let mut line_spans: Vec<Span<'static>> = Vec::new();
        if start < end {
            let mut buf = String::new();
            let mut cur_style = chars_styled[start].1;
            for &(c, style) in &chars_styled[start..end] {
                if style == cur_style {
                    buf.push(c);
                } else {
                    if !buf.is_empty() { line_spans.push(Span::styled(std::mem::take(&mut buf), cur_style)); }
                    buf.push(c);
                    cur_style = style;
                }
            }
            if !buf.is_empty() { line_spans.push(Span::styled(buf, cur_style)); }
        }
        result.push(line_spans);
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

    // Title on left — show REC/... prefix when STT is active, modified indicator on right
    let title = if app.stt_recording {
        format!(" REC EDIT: {} ", path_str)
    } else if app.stt_transcribing {
        format!(" ... EDIT: {} ", path_str)
    } else {
        format!(" EDIT: {} ", path_str)
    };
    // Border color: magenta during voice input, yellow normally
    let border_color = if app.stt_recording || app.stt_transcribing { Color::Magenta } else { Color::Yellow };

    let total_lines = app.viewer_edit_content.len();
    let line_num_width = total_lines.to_string().len().max(3);
    let content_width = viewport_width.saturating_sub(line_num_width + 3);
    // Cache so cursor movement logic can navigate wrapped visual lines
    app.viewer_edit_content_width = content_width;

    // Cache syntax highlighting — only re-run when content actually changes.
    // Track via monotonic edit counter (not undo stack len, which caps at 100).
    // Also invalidate when cache is empty (first render) or entering edit mode (ver = MAX).
    let edit_ver = app.viewer_edit_version;
    if app.viewer_edit_highlight_ver != edit_ver || app.viewer_edit_highlight_cache.is_empty() {
        let full_content = app.viewer_edit_content.join("\n");
        app.viewer_edit_highlight_cache = app.syntax_highlighter.highlight_file(&full_content, &path_str);
        app.viewer_edit_highlight_ver = edit_ver;
    }

    let scroll = app.viewer_scroll;
    let (cursor_line, cursor_col) = app.viewer_edit_cursor;
    let cw = content_width.max(1);

    // Normalize selection so start <= end (user can select backwards)
    let selection = app.viewer_edit_selection.map(|(sl, sc, el, ec)| {
        if sl < el || (sl == el && sc <= ec) {
            (sl, sc, el, ec)
        } else {
            (el, ec, sl, sc)
        }
    });

    // Find which source lines are visible. Walk source lines summing wrap
    // counts until we've covered scroll + viewport_height visual lines.
    // Only process those source lines (avoids O(file) per frame).
    let mut visual_row = 0usize;
    let mut first_src = 0usize; // first visible source line
    let mut first_wrap_skip = 0usize; // wrap rows to skip on first source line
    let mut found_first = false;
    let mut last_src = app.viewer_edit_content.len(); // exclusive

    for (i, line_str) in app.viewer_edit_content.iter().enumerate() {
        let wraps = word_wrap_breaks(line_str, cw).len();

        if !found_first {
            if visual_row + wraps > scroll {
                first_src = i;
                first_wrap_skip = scroll - visual_row;
                found_first = true;
            }
        }
        visual_row += wraps;
        if found_first && visual_row >= scroll + viewport_height {
            last_src = i + 1;
            break;
        }
    }
    if !found_first { first_src = app.viewer_edit_content.len(); }

    // Build only the visible display lines
    let mut final_lines: Vec<Line> = Vec::with_capacity(viewport_height);
    // Track which source lines are in the viewport (for cursor positioning)
    let viewport_visual_base = scroll; // visual index of first display line
    let _ = viewport_visual_base; // used implicitly via scroll

    for idx in first_src..last_src.min(app.viewer_edit_content.len()) {
        let line_content = &app.viewer_edit_content[idx];
        let line_num_style = if idx == cursor_line {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default().fg(Color::DarkGray)
        };

        let spans = app.viewer_edit_highlight_cache.get(idx).cloned().unwrap_or_default();

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

        let wrapped = wrap_spans_word(spans, content_width);

        for (wrap_idx, wrapped_spans) in wrapped.into_iter().enumerate() {
            // Skip wrap rows before scroll start (only applies to first_src)
            if idx == first_src && wrap_idx < first_wrap_skip { continue; }
            if final_lines.len() >= viewport_height { break; }

            let line_num = if wrap_idx == 0 {
                format!("{:>width$} │ ", idx + 1, width = line_num_width)
            } else {
                format!("{:>width$} │ ", "", width = line_num_width)
            };
            let mut line_spans = vec![Span::styled(line_num, line_num_style)];
            line_spans.extend(wrapped_spans);
            final_lines.push(Line::from(line_spans));
        }
        if final_lines.len() >= viewport_height { break; }
    }

    // Pad with empty lines if needed
    while final_lines.len() < viewport_height {
        let line_num = format!("{:>width$} │ ", "~", width = line_num_width);
        final_lines.push(Line::from(Span::styled(line_num, Style::default().fg(Color::DarkGray))));
    }

    let border_style = Style::default().fg(border_color).add_modifier(Modifier::BOLD);
    let title_line = Line::from(Span::styled(&title, border_style));

    let mut block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Double)
        .title(title_line)
        .border_style(border_style);
    // Right-aligned [modified] indicator — ratatui fills the gap with border chars
    if app.viewer_edit_dirty {
        block = block.title(Line::from(
            Span::styled("[modified] ", Style::default().fg(border_color))
        ).alignment(Alignment::Right));
    }

    let widget = Paragraph::new(final_lines).block(block);
    f.render_widget(widget, area);

    // Dashed double border when file is modified — punch gaps into every other
    // border cell to create a "═ ═ ═" / "║ ║" pattern as an unsaved-changes cue.
    // Only blanks cells that are actually border characters (═ or ║), so title
    // text on the top edge is preserved automatically.
    if app.viewer_edit_dirty {
        let buf = f.buffer_mut();
        let x0 = area.x;
        let x1 = area.x + area.width.saturating_sub(1);
        let y0 = area.y;
        let y1 = area.y + area.height.saturating_sub(1);
        // Top and bottom edges: blank every other ═ cell (skip corners)
        for x in (x0 + 1)..x1 {
            if (x - x0) % 2 == 0 {
                if buf[(x, y0)].symbol() == "═" { buf[(x, y0)].set_symbol(" "); }
                if buf[(x, y1)].symbol() == "═" { buf[(x, y1)].set_symbol(" "); }
            }
        }
        // Left and right edges: blank every other ║ cell (skip corners)
        for y in (y0 + 1)..y1 {
            if (y - y0) % 2 == 0 {
                if buf[(x0, y)].symbol() == "║" { buf[(x0, y)].set_symbol(" "); }
                if buf[(x1, y)].symbol() == "║" { buf[(x1, y)].set_symbol(" "); }
            }
        }
    }

    // Compute cursor visual position using word-wrap break positions.
    // Sum wrap row counts for source lines 0..cursor_line, then find which
    // wrap segment the cursor column falls in for the cursor's own line.
    let mut cursor_visual_line = 0usize;
    for i in 0..cursor_line.min(app.viewer_edit_content.len()) {
        cursor_visual_line += word_wrap_breaks(&app.viewer_edit_content[i], cw).len();
    }
    let cursor_line_str = app.viewer_edit_content.get(cursor_line).map(|s| s.as_str()).unwrap_or("");
    let cursor_breaks = word_wrap_breaks(cursor_line_str, cw);
    // Find which wrap row the cursor falls on
    let mut cursor_wrap_row = 0;
    for (j, &brk) in cursor_breaks.iter().enumerate() {
        if cursor_col >= brk { cursor_wrap_row = j; }
    }
    cursor_visual_line += cursor_wrap_row;
    let cursor_visual_col = cursor_col - cursor_breaks[cursor_wrap_row];

    // Position cursor if visible in viewport
    if cursor_visual_line >= scroll && cursor_visual_line < scroll + viewport_height {
        let screen_line = cursor_visual_line - scroll;
        f.set_cursor_position((
            area.x + 1 + line_num_width as u16 + 3 + cursor_visual_col as u16,
            area.y + 1 + screen_line as u16,
        ));
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

/// How many rows the tab bar occupies (0 if no tabs, 1 for ≤6, 2 for >6)
fn tab_bar_rows(tab_count: usize) -> u16 {
    if tab_count == 0 { 0 }
    else if tab_count <= 6 { 1 }
    else { 2 }
}

/// Draw fixed-width tab bar: 6 tabs per row, up to 2 rows (12 max).
/// Each "slot" is inner_width/6. Tab content fills slot_w-1 chars + 1 char gap.
fn draw_tab_bar(f: &mut Frame, app: &App, area: Rect) {
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

/// Git panel viewer — renders diff content from the git panel state
fn draw_git_viewer(f: &mut Frame, panel: &crate::app::types::GitActionsPanel, area: Rect, is_focused: bool) {
    let title = match panel.viewer_diff_title {
        Some(ref t) => format!(" {} ", t),
        None => " Viewer ".to_string(),
    };

    let border_style = if is_focused {
        Style::default().fg(AZURE).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::White)
    };

    let block = Block::default()
        .title(Span::styled(&title, border_style))
        .borders(Borders::ALL)
        .border_type(if is_focused { BorderType::Double } else { BorderType::Plain })
        .border_style(border_style);

    match panel.viewer_diff {
        Some(ref diff) => {
            let lines: Vec<Line> = diff.lines().map(|l| {
                let style = if l.starts_with('+') && !l.starts_with("+++") {
                    Style::default().fg(Color::Green)
                } else if l.starts_with('-') && !l.starts_with("---") {
                    Style::default().fg(Color::Red)
                } else if l.starts_with("@@") {
                    Style::default().fg(Color::Cyan)
                } else if l.starts_with("diff ") || l.starts_with("index ") {
                    Style::default().fg(GIT_BROWN)
                } else {
                    Style::default().fg(Color::White)
                };
                Line::from(Span::styled(format!(" {}", l), style))
            }).collect();
            f.render_widget(
                Paragraph::new(lines)
                    .block(block)
                    .wrap(ratatui::widgets::Wrap { trim: false }),
                area,
            );
        }
        None => {
            let hint = vec![
                Line::from(""),
                Line::from(Span::styled(
                    " Select a file or commit to view its diff",
                    Style::default().fg(Color::DarkGray),
                )),
            ];
            f.render_widget(Paragraph::new(hint).block(block), area);
        }
    }
}