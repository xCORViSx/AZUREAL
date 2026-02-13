//! Convo pane rendering
//!
//! Expensive work (markdown parsing, syntax highlighting, text wrapping) runs
//! on a background render thread. The main event loop sends render requests
//! via `submit_render_request()` (non-blocking) and polls for completed results
//! via `poll_render_result()` (non-blocking). The draw function itself is cheap —
//! just clones a viewport slice and renders from the pre-built cache.

use ratatui::{
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Paragraph, Wrap},
    Frame,
};

use crate::app::{App, Focus, ViewMode};
use crate::models::RebaseState;
use super::render_thread::{PreScanState, RenderRequest};
use super::colorize::ORANGE;
use super::util::{colorize_output, detect_message_type, MessageType, AZURE};

/// On initial load of large conversations, only render this many events from the tail.
/// The user starts at the bottom so they see the most recent messages instantly.
/// Full render happens lazily when they scroll to the top.
const DEFERRED_RENDER_TAIL: usize = 200;

/// Scan already-rendered events to extract state flags needed by the render thread.
/// Runs on the main thread reading its own memory (zero allocation, no clone).
/// This lets us send only NEW events to the render thread instead of ALL events.
fn pre_scan_events(events: &[crate::events::DisplayEvent]) -> PreScanState {
    use crate::events::DisplayEvent;
    let mut s = PreScanState::default();
    for event in events {
        match event {
            DisplayEvent::Init { .. } => { s.saw_init = true; }
            DisplayEvent::Hook { name, output } => {
                s.last_hook = Some((name.clone(), output.clone()));
            }
            DisplayEvent::Plan { .. } | DisplayEvent::UserMessage { .. }
            | DisplayEvent::AssistantText { .. } | DisplayEvent::ToolCall { .. }
            | DisplayEvent::ToolResult { .. } => {
                s.saw_content = true;
                s.last_hook = None;
                if let DisplayEvent::ToolCall { tool_name, input, .. } = event {
                    if tool_name == "ExitPlanMode" {
                        s.saw_exit_plan_mode = true;
                        s.saw_user_after_exit_plan = false;
                    }
                    if tool_name == "AskUserQuestion" {
                        s.saw_ask_user_question = true;
                        s.saw_user_after_ask = false;
                        s.last_ask_input = Some(input.clone());
                    }
                }
                if let DisplayEvent::UserMessage { .. } = event {
                    if s.saw_exit_plan_mode { s.saw_user_after_exit_plan = true; }
                    if s.saw_ask_user_question { s.saw_user_after_ask = true; }
                }
            }
            _ => {}
        }
    }
    s
}

/// Submit a render request to the background thread (NON-BLOCKING).
/// The main event loop calls this when `rendered_lines_dirty` is true.
/// The actual rendering happens on the render thread — the main thread
/// keeps processing events immediately after this returns.
pub fn submit_render_request(app: &mut App, convo_width: u16) {
    if app.display_events.is_empty() || app.view_mode != ViewMode::Output { return; }

    let inner_width = convo_width.saturating_sub(2);

    // Deferred render: if user scrolled to top and there are unrendered early events,
    // expand to full render now (they want to see old messages)
    if app.rendered_events_start > 0 && app.output_scroll == 0 {
        app.rendered_lines_dirty = true;
        app.rendered_events_start = 0;
        app.rendered_events_count = 0;
        app.rendered_content_line_count = 0;
    }

    // Only submit if cache is dirty or width changed
    if !app.rendered_lines_dirty && app.rendered_lines_width == inner_width { return; }

    let event_count = app.display_events.len();
    let can_incremental = app.rendered_lines_width == inner_width
        && app.rendered_events_count > 0
        && event_count > app.rendered_events_count
        && app.rendered_events_start == 0;

    // Build the render request with cloned data (the thread works on its own copy).
    // IMPORTANT: we clone (not take) the existing cache so the main thread still has
    // content to display while the render thread works. Taking would empty the cache,
    // breaking scroll-to-bottom (clamp_output_scroll sees 0 lines → jumps to top).
    let req = if can_incremental {
        // Trim existing cache to content_line_count to strip the trailing
        // pending user message bubble (if any). The render thread re-appends
        // it at the correct position after the new events.
        let trim = app.rendered_content_line_count;
        let mut existing_lines = app.rendered_lines_cache.clone();
        let mut existing_anim = app.animation_line_indices.clone();
        let mut existing_bubbles = app.message_bubble_positions.clone();
        let existing_clickable = app.clickable_paths.clone();
        if trim < existing_lines.len() {
            existing_lines.truncate(trim);
            existing_anim.retain(|&(idx, _)| idx < trim);
            if let Some(&(line_idx, _)) = existing_bubbles.last() {
                if line_idx >= trim { existing_bubbles.pop(); }
            }
        }

        // Pre-compute state flags by scanning existing events (zero-cost: just
        // reads references in main thread's own memory). This eliminates the need
        // to clone ALL display_events — render thread only gets new events.
        let pre_scan = pre_scan_events(&app.display_events[..app.rendered_events_count]);

        // Only clone NEW events (from rendered_events_count onwards).
        // Previously cloned ALL events — the #1 cause of CPU spike during streaming.
        let new_events = app.display_events[app.rendered_events_count..].to_vec();

        RenderRequest {
            events: new_events,
            width: inner_width,
            pending_tools: app.pending_tool_calls.clone(),
            failed_tools: app.failed_tool_calls.clone(),
            pending_user_message: app.pending_user_message.clone(),
            existing_lines,
            existing_anim,
            existing_bubbles,
            existing_clickable,
            pre_scan,
            total_events: event_count,
            deferred_start: 0,
            seq: 0,
        }
    } else {
        let deferred_start = if app.rendered_events_start == 0
            && app.rendered_events_count == 0
            && event_count > DEFERRED_RENDER_TAIL
        {
            event_count.saturating_sub(DEFERRED_RENDER_TAIL)
        } else {
            0
        };

        // Clone only the events we'll actually render (from deferred_start onwards).
        // Previously cloned ALL events even though events before deferred_start are
        // never touched — wasted cloning of potentially huge serde_json::Value fields.
        RenderRequest {
            events: app.display_events[deferred_start..].to_vec(),
            width: inner_width,
            pending_tools: app.pending_tool_calls.clone(),
            failed_tools: app.failed_tool_calls.clone(),
            pending_user_message: app.pending_user_message.clone(),
            existing_lines: Vec::new(),
            existing_anim: Vec::new(),
            existing_bubbles: Vec::new(),
            existing_clickable: Vec::new(),
            pre_scan: PreScanState::default(),
            total_events: event_count,
            deferred_start,
            seq: 0,
        }
    };

    app.render_thread.send(req);
    app.render_in_flight = true;
    // Mark dirty as false so we don't re-submit every loop iteration.
    // If new events arrive before the render completes, invalidate_render_cache()
    // sets dirty=true again and we'll submit a new request.
    app.rendered_lines_dirty = false;
}

/// Check for completed render results from the background thread (NON-BLOCKING).
/// Returns true if new content was applied (caller should trigger a redraw).
pub fn poll_render_result(app: &mut App) -> bool {
    let Some(result) = app.render_thread.try_recv() else { return false; };

    // Discard stale results (a newer request was already applied)
    if result.seq <= app.render_seq_applied { return false; }

    // Compute content line count (lines before the trailing pending bubble).
    // If the last bubble is a user bubble AND there's a pending_user_message,
    // the pending bubble starts 2 lines before its recorded position.
    let total_lines = result.lines.len();
    let content_lines = if app.pending_user_message.is_some() {
        if let Some(&(line_idx, true)) = result.bubble_positions.last() {
            if line_idx >= 2 { line_idx - 2 } else { total_lines }
        } else { total_lines }
    } else { total_lines };

    // Apply the completed render to app state
    app.rendered_lines_cache = result.lines;
    app.animation_line_indices = result.anim_indices;
    app.message_bubble_positions = result.bubble_positions;
    app.clickable_paths = result.clickable_paths;
    app.rendered_lines_width = result.width;
    app.rendered_events_count = result.events_count;
    app.rendered_content_line_count = content_lines;
    app.rendered_events_start = result.events_start;
    app.render_seq_applied = result.seq;
    app.render_in_flight = false;
    // Content shifted — stale highlight would point at wrong position
    app.clicked_path_highlight = None;

    // Invalidate viewport cache since underlying content changed
    app.output_viewport_scroll = usize::MAX;
    true
}

/// Draw the Claude session list overlay — full-pane list of all Claude session files across worktrees.
/// Each row shows: status symbol, worktree name, Claude session name/UUID, mtime, [N msgs].
fn draw_session_list(f: &mut Frame, app: &mut App, area: Rect) {
    // Show a small centered "Loading..." dialog while message counts are computing.
    // This renders on the first frame after 's' is pressed, before the I/O starts.
    if app.session_list_loading {
        let msg = " Loading sessions… ";
        let w = (msg.len() as u16 + 4).min(area.width);
        let h = 3u16;
        let x = area.x + (area.width.saturating_sub(w)) / 2;
        let y = area.y + (area.height.saturating_sub(h)) / 2;
        let dialog = Paragraph::new(Span::styled(msg, Style::default().fg(Color::White)))
            .alignment(Alignment::Center)
            .block(Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(AZURE))
                .title(Span::styled(" Sessions ", Style::default().fg(AZURE).add_modifier(Modifier::BOLD))));
        f.render_widget(dialog, Rect::new(x, y, w, h));
        return;
    }

    let is_focused = app.focus == Focus::Output;

    // Split area: filter bar at top when filter is active or has text
    let has_filter = app.session_filter_active || !app.session_filter.is_empty();
    let (filter_area, list_area) = if has_filter {
        let chunks = Layout::vertical([
            Constraint::Length(3),
            Constraint::Min(1),
        ]).split(area);
        (Some(chunks[0]), chunks[1])
    } else {
        (None, area)
    };

    // Draw filter input bar when active
    if let Some(fa) = filter_area {
        let mode_prefix = if app.session_content_search { "//" } else { "/" };
        let border_color = if app.session_filter_active { Color::Yellow } else { Color::DarkGray };
        let right_info = if app.session_content_search {
            format!(" {} results ", app.session_search_results.len())
        } else {
            String::new()
        };
        let filter_widget = Paragraph::new(app.session_filter.clone())
            .block(Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(border_color))
                .title(Span::styled(mode_prefix, Style::default().fg(Color::Yellow)))
                .title(Line::from(Span::styled(right_info, Style::default().fg(Color::DarkGray))).alignment(Alignment::Right)),
            );
        f.render_widget(filter_widget, fa);
        if app.session_filter_active {
            let cursor_x = fa.x + 1 + app.session_filter.len() as u16;
            let cursor_y = fa.y + 1;
            if cursor_x < fa.right() {
                f.set_cursor_position((cursor_x, cursor_y));
            }
        }
    }

    let viewport_height = list_area.height.saturating_sub(2) as usize;
    let inner_width = list_area.width.saturating_sub(2) as usize;

    // Content search mode: show search results instead of normal session list
    if app.session_content_search {
        let session_names = app.load_all_session_names();
        let mut rows: Vec<Line<'static>> = Vec::new();
        for (idx, (_row, session_id, preview)) in app.session_search_results.iter().enumerate() {
            let is_selected = idx == app.session_list_selected;
            let name_display = session_names.get(session_id.as_str())
                .cloned()
                .unwrap_or_else(|| session_id.chars().take(12).collect::<String>());
            let bg = if is_selected { Style::default().bg(AZURE).fg(Color::Black) } else { Style::default() };
            let name_style = if is_selected {
                Style::default().bg(AZURE).fg(Color::Black).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            // Truncate preview to fit
            let prefix_len = name_display.chars().count() + 4; // " name │ "
            let preview_space = inner_width.saturating_sub(prefix_len);
            let trunc_preview: String = preview.chars().take(preview_space).collect();

            rows.push(Line::from(vec![
                Span::styled(format!(" {} ", name_display), name_style),
                Span::styled("│ ", if is_selected { bg } else { Style::default().fg(Color::DarkGray) }),
                Span::styled(trunc_preview, if is_selected { bg } else { Style::default().fg(Color::DarkGray) }),
            ]));
        }
        let total = rows.len();
        if app.session_list_selected >= total && total > 0 {
            app.session_list_selected = total - 1;
        }
        let max_scroll = total.saturating_sub(viewport_height);
        if app.session_list_selected < app.session_list_scroll {
            app.session_list_scroll = app.session_list_selected;
        } else if app.session_list_selected >= app.session_list_scroll + viewport_height {
            app.session_list_scroll = app.session_list_selected.saturating_sub(viewport_height - 1);
        }
        app.session_list_scroll = app.session_list_scroll.min(max_scroll);
        let display: Vec<Line> = rows.into_iter().skip(app.session_list_scroll).take(viewport_height).collect();
        let title = format!(" Search [{}/{}] ", app.session_list_selected.saturating_add(1).min(total.max(1)), total.max(1));
        let border_style = if is_focused {
            Style::default().fg(AZURE).add_modifier(Modifier::BOLD)
        } else { Style::default().fg(Color::White) };
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(if is_focused { BorderType::Double } else { BorderType::Plain })
            .title(Span::styled(title, border_style))
            .border_style(border_style);
        f.render_widget(Paragraph::new(display).block(block), list_area);
        return;
    }

    // Session list scoped to current worktree only — no wt_name column needed
    let session_names = app.load_all_session_names();
    let filter_lower = app.session_filter.to_lowercase();
    let filtering = !filter_lower.is_empty();
    let mut rows: Vec<Line<'static>> = Vec::new();
    let mut total_unfiltered = 0usize;

    let branch = app.current_session().map(|s| s.branch_name.clone());
    let files = branch.as_deref().and_then(|b| app.session_files.get(b));

    if let Some(files) = files {
        for (session_id, _path, time_str) in files.iter() {
            total_unfiltered += 1;
            let name_display = session_names.get(session_id.as_str())
                .cloned()
                .unwrap_or_else(|| session_id.clone());

            // Name filter: skip rows that don't match session name or session id
            if filtering {
                let matches = name_display.to_lowercase().contains(&filter_lower)
                    || session_id.to_lowercase().contains(&filter_lower);
                if !matches { continue; }
            }

            let msg_count = app.session_msg_counts.get(session_id).map(|&(c, _)| c).unwrap_or(0);
            let msg_badge = format!("[{} msgs]", msg_count);
            let suffix = format!(" {} {} ", time_str, msg_badge);
            // Row: " session_name    mtime [N msgs]"
            let name_space = inner_width.saturating_sub(1 + suffix.chars().count());
            let truncated_name = if name_display.chars().count() > name_space {
                let trunc: String = name_display.chars().take(name_space.saturating_sub(1)).collect();
                format!("{}…", trunc)
            } else {
                name_display
            };
            let pad = name_space.saturating_sub(truncated_name.chars().count());

            let is_selected = rows.len() == app.session_list_selected;
            let name_style = if is_selected {
                Style::default().bg(AZURE).fg(Color::Black).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            let bg_style = if is_selected {
                Style::default().bg(AZURE).fg(Color::Black)
            } else {
                Style::default()
            };

            rows.push(Line::from(vec![
                Span::styled(" ", bg_style),
                Span::styled(truncated_name, name_style),
                Span::styled(" ".repeat(pad), bg_style),
                Span::styled(format!(" {} ", time_str), if is_selected { bg_style } else { Style::default().fg(Color::DarkGray) }),
                Span::styled(msg_badge, if is_selected { bg_style } else { Style::default().fg(AZURE) }),
            ]));
        }
    }

    // Clamp selection
    let total = rows.len();
    if app.session_list_selected >= total && total > 0 {
        app.session_list_selected = total - 1;
    }

    // Auto-scroll to keep selection visible
    let max_scroll = total.saturating_sub(viewport_height);
    if app.session_list_selected < app.session_list_scroll {
        app.session_list_scroll = app.session_list_selected;
    } else if app.session_list_selected >= app.session_list_scroll + viewport_height {
        app.session_list_scroll = app.session_list_selected.saturating_sub(viewport_height - 1);
    }
    app.session_list_scroll = app.session_list_scroll.min(max_scroll);

    let display: Vec<Line> = rows.into_iter()
        .skip(app.session_list_scroll)
        .take(viewport_height)
        .collect();

    // Title includes worktree name since list is scoped to current worktree
    let wt_label = app.current_session().map(|s| s.name().to_string()).unwrap_or_default();
    let title = if filtering {
        format!(" {} [{}/{} of {}] ", wt_label, app.session_list_selected.saturating_add(1).min(total.max(1)), total, total_unfiltered)
    } else {
        format!(" {} [{}/{}] ", wt_label, app.session_list_selected + 1, total.max(1))
    };
    let border_style = if is_focused {
        Style::default().fg(AZURE).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::White)
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(if is_focused { BorderType::Double } else { BorderType::Plain })
        .title(Span::styled(title, border_style))
        .border_style(border_style);

    let widget = Paragraph::new(display).block(block);
    f.render_widget(widget, list_area);
}

/// Draw the main output/diff panel — cheap, just reads from pre-rendered caches
pub fn draw_output(f: &mut Frame, app: &mut App, area: Rect) {
    // Session list overlay takes over the entire convo pane when active
    if app.show_session_list {
        draw_session_list(f, app, area);
        return;
    }

    // Split area for sticky todo widget at bottom (visible whenever todos exist —
    // stays visible even when all completed, cleared on next user prompt or session switch)
    let has_todos = !app.current_todos.is_empty() || !app.subagent_todos.is_empty();
    let todo_height = if has_todos {
        // Account for text wrapping: each todo may span multiple visual lines.
        // Inner width = area width minus 2 for borders.
        let inner_w = area.width.saturating_sub(2) as usize;
        // Helper closure: count wrapped visual lines for a todo list.
        // prefix_extra = extra chars before text (e.g. 2 for "↳ " indent on subtasks)
        let count_lines = |todos: &[crate::app::TodoItem], prefix_extra: usize| -> u16 {
            if inner_w == 0 { return todos.len() as u16; }
            todos.iter().map(|t| {
                let text = if t.status == crate::app::TodoStatus::InProgress && !t.active_form.is_empty() {
                    &t.active_form
                } else { &t.content };
                // 2 chars for icon ("✓ ") + prefix_extra for indent
                let text_w: usize = text.chars().map(|c| unicode_width::UnicodeWidthChar::width(c).unwrap_or(1)).sum::<usize>() + 2 + prefix_extra;
                ((text_w + inner_w - 1) / inner_w).max(1) as u16
            }).sum()
        };
        let main_lines = count_lines(&app.current_todos, 0);
        // Subagent todos get "↳ " prefix (2 display-width chars)
        let sub_lines = count_lines(&app.subagent_todos, 2);
        // +2 for border top/bottom, cap so convo still has at least 10 rows
        (main_lines + sub_lines + 2).min(area.height.saturating_sub(10))
    } else { 0 };
    // Search bar at bottom of convo: visible when search is active or has residual matches
    let has_search = app.convo_search_active || !app.convo_search_matches.is_empty();
    let search_height: u16 = if has_search { 3 } else { 0 };
    let [convo_area, search_area, todo_area] = Layout::vertical([
        Constraint::Min(1),
        Constraint::Length(search_height),
        Constraint::Length(todo_height),
    ]).areas(area);
    let area = convo_area;
    let viewport_height = area.height.saturating_sub(2) as usize;

    // Cache viewport height for scroll operations (input handling uses this)
    app.output_viewport_height = viewport_height;

    let (title, content) = match app.view_mode {
        ViewMode::Output => {
            // If the cache width doesn't match the actual draw area (e.g. resize),
            // mark dirty so the next loop iteration submits a new render request.
            // We NEVER render synchronously here — draw uses whatever cache exists.
            let inner_width = area.width.saturating_sub(2);
            if !app.display_events.is_empty()
                && app.rendered_lines_width != inner_width
                && !app.rendered_lines_dirty
            {
                app.rendered_lines_dirty = true;
            }

            if !app.rendered_lines_cache.is_empty() {
                // Resolve scroll for this frame WITHOUT destroying the usize::MAX sentinel.
                // If user is following bottom (sentinel), compute concrete position but
                // leave output_scroll as usize::MAX so it keeps following on next frame.
                let scroll = if app.output_scroll == usize::MAX {
                    app.output_natural_bottom()
                } else {
                    app.output_scroll.min(app.output_max_scroll())
                };

                // Check if viewport cache is still valid — skip the clone if so.
                // Selection changes also invalidate (must re-apply highlight)
                let cache_valid = scroll == app.output_viewport_scroll
                    && app.animation_tick == app.output_viewport_anim_tick
                    && app.output_selection == app.output_selection_cached
                    && app.output_viewport_cache.len() == viewport_height.min(app.rendered_lines_cache.len().saturating_sub(scroll));

                if !cache_valid {
                    // Clone viewport slice from the pre-rendered line cache
                    let mut lines: Vec<Line> = app.rendered_lines_cache.iter()
                        .skip(scroll)
                        .take(viewport_height)
                        .cloned()
                        .collect();

                    // Patch animation colors only when there are pending tool indicators
                    if !app.animation_line_indices.is_empty() {
                        let pulse_colors = [Color::White, Color::Gray, Color::DarkGray, Color::Gray];
                        let pulse_color = pulse_colors[(app.animation_tick / 2) as usize % pulse_colors.len()];
                        for &(line_idx, span_idx) in &app.animation_line_indices {
                            if line_idx >= scroll && line_idx < scroll + viewport_height {
                                let viewport_idx = line_idx - scroll;
                                if let Some(line) = lines.get_mut(viewport_idx) {
                                    if let Some(span) = line.spans.get_mut(span_idx) {
                                        span.style = span.style.fg(pulse_color);
                                    }
                                }
                            }
                        }
                    }

                    // Apply text selection highlighting if active
                    if let Some((sl, sc, el, ec)) = app.output_selection {
                        for (vi, line) in lines.iter_mut().enumerate() {
                            let ci = scroll + vi;
                            if ci >= sl && ci <= el {
                                let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
                                let new_spans = super::draw_viewer::apply_selection_to_line(
                                    line.spans.clone(), &text, ci, sl, sc, el, ec, 0,
                                );
                                *line = Line::from(new_spans);
                            }
                        }
                    }
                    app.output_selection_cached = app.output_selection;

                    // Apply inverted highlight on clicked file path (orange bg, black fg)
                    // Covers all wrapped lines of the path (first line uses column range,
                    // continuation lines highlight all content)
                    if let Some((hl, hsc, hec, wlc)) = app.clicked_path_highlight {
                        let hl_style = Style::default().bg(ORANGE).fg(Color::Black);
                        for row in 0..wlc {
                            let cache_line = hl + row;
                            if cache_line < scroll || cache_line >= scroll + viewport_height { continue; }
                            let vi = cache_line - scroll;
                            let Some(line) = lines.get_mut(vi) else { continue };
                            // First line: highlight only the path portion [hsc..hec)
                            // Continuation lines: highlight from same start col to end of text
                            // (skip the indent spaces, stop at end of path text — don't highlight padding)
                            let (start, end) = if row == 0 {
                                (hsc, hec)
                            } else {
                                (hsc, line.spans.iter().map(|s| s.content.chars().count()).sum())
                            };
                            let mut new_spans: Vec<Span<'static>> = Vec::new();
                            let mut col = 0usize;
                            for span in line.spans.iter() {
                                let span_len = span.content.chars().count();
                                let span_end = col + span_len;
                                if span_end <= start || col >= end {
                                    new_spans.push(span.clone());
                                } else {
                                    let chars: Vec<char> = span.content.chars().collect();
                                    let hs = start.saturating_sub(col);
                                    let he = (end - col).min(span_len);
                                    if hs > 0 {
                                        let before: String = chars[..hs].iter().collect();
                                        new_spans.push(Span::styled(before, span.style));
                                    }
                                    let mid: String = chars[hs..he].iter().collect();
                                    new_spans.push(Span::styled(mid, hl_style));
                                    if he < span_len {
                                        let after: String = chars[he..].iter().collect();
                                        new_spans.push(Span::styled(after, span.style));
                                    }
                                }
                                col = span_end;
                            }
                            *line = Line::from(new_spans);
                        }
                    }

                    // Apply convo search match highlighting (yellow bg for matches,
                    // bright yellow for current match — same span-splitting technique)
                    if !app.convo_search_matches.is_empty() {
                        let match_style = Style::default().bg(Color::DarkGray).fg(Color::Yellow);
                        let current_style = Style::default().bg(Color::Yellow).fg(Color::Black);
                        for (mi, &(line_idx, sc, ec)) in app.convo_search_matches.iter().enumerate() {
                            if line_idx < scroll || line_idx >= scroll + viewport_height { continue; }
                            let vi = line_idx - scroll;
                            let Some(line) = lines.get_mut(vi) else { continue };
                            let style = if mi == app.convo_search_current { current_style } else { match_style };
                            let mut new_spans: Vec<Span<'static>> = Vec::new();
                            let mut col = 0usize;
                            for span in line.spans.iter() {
                                let span_len = span.content.chars().count();
                                let span_end = col + span_len;
                                if span_end <= sc || col >= ec {
                                    new_spans.push(span.clone());
                                } else {
                                    let chars: Vec<char> = span.content.chars().collect();
                                    let hs = sc.saturating_sub(col);
                                    let he = (ec - col).min(span_len);
                                    if hs > 0 {
                                        new_spans.push(Span::styled(chars[..hs].iter().collect::<String>(), span.style));
                                    }
                                    new_spans.push(Span::styled(chars[hs..he].iter().collect::<String>(), style));
                                    if he < span_len {
                                        new_spans.push(Span::styled(chars[he..].iter().collect::<String>(), span.style));
                                    }
                                }
                                col = span_end;
                            }
                            *line = Line::from(new_spans);
                        }
                    }

                    // Build title with message count
                    // Total counts ALL display events (not just rendered tail from deferred render)
                    // so the denominator is accurate even before the user scrolls to top
                    let total_msgs = app.display_events.iter().filter(|e| matches!(e,
                        crate::events::DisplayEvent::UserMessage { .. } |
                        crate::events::DisplayEvent::AssistantText { .. }
                    )).count();
                    let title = if total_msgs > 0 {
                        let current_line = scroll.saturating_add(3);
                        // Current position from rendered bubble positions (only covers rendered tail)
                        // Add the unrendered bubble count as offset so numbering is correct
                        let rendered_bubbles = app.message_bubble_positions.len();
                        let unrendered_offset = total_msgs.saturating_sub(rendered_bubbles);
                        let current_msg = app.message_bubble_positions.iter()
                            .enumerate()
                            .rev()
                            .find(|(_, (line_idx, _))| *line_idx <= current_line)
                            .map(|(idx, _)| idx + 1 + unrendered_offset)
                            .unwrap_or(1);
                        format!(" Convo [{}/{}] ", current_msg, total_msgs)
                    } else {
                        " Convo ".to_string()
                    };

                    app.output_viewport_cache = lines;
                    app.output_viewport_scroll = scroll;
                    app.output_viewport_anim_tick = app.animation_tick;
                    app.output_viewport_title = title;
                }

                (app.output_viewport_title.clone(), app.output_viewport_cache.clone())
            } else if !app.output_lines.is_empty() || !app.output_buffer.is_empty() {
                // Fallback: using output_lines with colorize_output
                let mut all_lines: Vec<Line> = Vec::new();
                let mut last_msg_type = MessageType::Other;

                for line in app.output_lines.iter() {
                    let msg_type = detect_message_type(line);
                    if (last_msg_type == MessageType::User && msg_type == MessageType::Assistant)
                        || (last_msg_type == MessageType::Assistant && msg_type == MessageType::User)
                    {
                        all_lines.push(Line::from(""));
                        all_lines.push(Line::from(""));
                    }
                    all_lines.push(colorize_output(line));
                    if msg_type != MessageType::Other { last_msg_type = msg_type; }
                }

                if !app.output_buffer.is_empty() {
                    let msg_type = detect_message_type(&app.output_buffer);
                    if (last_msg_type == MessageType::User && msg_type == MessageType::Assistant)
                        || (last_msg_type == MessageType::Assistant && msg_type == MessageType::User)
                    {
                        all_lines.push(Line::from(""));
                        all_lines.push(Line::from(""));
                    }
                    all_lines.push(colorize_output(&app.output_buffer));
                }

                let total = all_lines.len();
                let max_scroll = total.saturating_sub(viewport_height);
                // Resolve sentinel to concrete position for THIS frame only —
                // don't write it back so usize::MAX survives and keeps
                // following bottom as new content arrives.
                let scroll = if app.output_scroll == usize::MAX { max_scroll }
                    else { app.output_scroll.min(max_scroll) };
                let lines: Vec<Line> = all_lines.into_iter().skip(scroll).take(viewport_height).collect();
                let title = if total > viewport_height {
                    format!(" Convo [{}/{}] ", scroll + viewport_height.min(total - scroll), total)
                } else {
                    " Convo ".to_string()
                };
                (title, lines)
            } else {
                (" Convo ".to_string(), vec![])
            }
        }
        ViewMode::Diff => {
            if let Some(ref diff) = app.diff_text {
                if app.diff_lines_dirty {
                    app.diff_lines_cache = app.diff_highlighter.colorize_diff(diff);
                    app.diff_lines_dirty = false;
                }
                let total = app.diff_lines_cache.len();
                let scroll = app.diff_scroll.min(total.saturating_sub(viewport_height));
                app.diff_scroll = scroll;
                let lines: Vec<Line> = app.diff_lines_cache.iter()
                    .skip(scroll).take(viewport_height)
                    .map(|spans| Line::from(spans.clone())).collect();
                let title = if total > viewport_height {
                    format!(" Diff (Syntax Highlighted) [{}/{}] ", scroll + viewport_height.min(total - scroll), total)
                } else {
                    " Diff (Syntax Highlighted) ".to_string()
                };
                (title, lines)
            } else {
                (" Diff ".to_string(), vec![Line::from("No diff available")])
            }
        }
        ViewMode::Messages => {
            (" Messages ".to_string(), vec![Line::from("Messages view not implemented")])
        }
        ViewMode::Rebase => {
            (" Rebase ".to_string(), draw_rebase_content(app))
        }
    };

    let is_focused = app.focus == Focus::Output;
    let border_style = if is_focused {
        Style::default().fg(AZURE).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::White)
    };

    // Build right-aligned title: token percentage + PID/exit code
    let branch = app.current_session().map(|s| s.branch_name.clone());
    let right_title: Option<Line<'static>> = {
        let mut spans: Vec<Span<'static>> = Vec::new();

        // Token usage percentage badge — pre-computed in update_token_badge(), just read the cache
        if let Some((ref text, color)) = app.token_badge_cache {
            spans.push(Span::styled(
                text.clone(),
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            ));
        }

        // PID while running, exit code after
        if let Some(b) = branch.as_deref() {
            if let Some(&pid) = app.claude_pids.get(b) {
                spans.push(Span::styled(
                    format!(" PID:{} ", pid),
                    Style::default().fg(Color::Green).add_modifier(Modifier::BOLD),
                ));
            } else if let Some(&code) = app.claude_exit_codes.get(b) {
                let (text, color) = if code == 0 {
                    (" exit:0 ".to_string(), Color::Green)
                } else {
                    (format!(" exit:{} ", code), Color::Red)
                };
                spans.push(Span::styled(text, Style::default().fg(color)));
            }
        }

        if spans.is_empty() { None } else { Some(Line::from(spans).alignment(Alignment::Right)) }
    };

    let mut block = Block::default()
        .borders(Borders::ALL)
        .border_type(if is_focused { BorderType::Double } else { BorderType::Plain })
        .title(Span::styled(title.clone(), border_style))
        .border_style(border_style);

    // Centered session name in [brackets] on top border
    if !app.title_session_name.is_empty() {
        // Available space: total border width minus left title, right title, and some padding
        let right_len = right_title.as_ref().map(|rt| rt.spans.iter().map(|s| s.content.len()).sum::<usize>()).unwrap_or(0);
        let avail = (area.width as usize).saturating_sub(title.len() + right_len + 4);
        let name = &app.title_session_name;
        let bracketed = if name.chars().count() + 2 <= avail {
            format!("[{}]", name)
        } else if avail > 5 {
            let trunc: String = name.chars().take(avail - 3).collect();
            format!("[{}…]", trunc)
        } else {
            String::new()
        };
        if !bracketed.is_empty() {
            block = block.title(
                Line::from(Span::styled(bracketed, Style::default().fg(Color::White)))
                    .alignment(Alignment::Center)
            );
        }
    }

    // Add right-aligned PID/exit title — ratatui fills gap with border chars
    if let Some(rt) = right_title {
        block = block.title(rt);
    }

    let output = Paragraph::new(content).block(block);
    f.render_widget(output, area);

    // Render convo search bar at bottom of convo content area
    if has_search {
        let match_info = if app.convo_search_matches.is_empty() {
            if app.convo_search.is_empty() { String::new() } else { " 0/0 ".to_string() }
        } else {
            format!(" {}/{} ", app.convo_search_current + 1, app.convo_search_matches.len())
        };
        let border_color = if app.convo_search_active { Color::Yellow } else { Color::DarkGray };
        let search_widget = Paragraph::new(app.convo_search.clone())
            .block(Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(border_color))
                .title(Span::styled("/", Style::default().fg(Color::Yellow)))
                .title(Line::from(Span::styled(match_info, Style::default().fg(Color::DarkGray))).alignment(Alignment::Right)),
            );
        f.render_widget(search_widget, search_area);
        // Show cursor in search bar when actively typing
        if app.convo_search_active {
            let cursor_x = search_area.x + 1 + app.convo_search.len() as u16;
            let cursor_y = search_area.y + 1;
            if cursor_x < search_area.right() {
                f.set_cursor_position((cursor_x, cursor_y));
            }
        }
    }

    // Render sticky todo widget at bottom of convo pane (main + subagent todos)
    if todo_height > 0 {
        draw_todo_widget(f, &app.current_todos, &app.subagent_todos, app.subagent_parent_idx, todo_area, app.animation_tick);
    }
}

/// Draw rebase status content
fn draw_rebase_content(app: &App) -> Vec<Line<'static>> {
    let mut lines = Vec::new();

    if let Some(ref status) = app.rebase_status {
        let state_color = status.state.color();
        lines.push(Line::from(vec![
            Span::styled("State: ", Style::default().add_modifier(Modifier::BOLD)),
            Span::styled(
                format!("{} {}", status.state.symbol(), status.state.as_str()),
                Style::default().fg(state_color),
            ),
        ]));

        if let (Some(current), Some(total)) = (status.current_step, status.total_steps) {
            lines.push(Line::from(vec![
                Span::styled("Progress: ", Style::default().add_modifier(Modifier::BOLD)),
                Span::raw(format!("{}/{}", current, total)),
            ]));
        }

        if let Some(ref head) = status.head_name {
            lines.push(Line::from(vec![
                Span::styled("Rebasing: ", Style::default().add_modifier(Modifier::BOLD)),
                Span::styled(head.clone(), Style::default().fg(Color::Green)),
            ]));
        }

        if let Some(ref onto) = status.onto_branch {
            lines.push(Line::from(vec![
                Span::styled("Onto: ", Style::default().add_modifier(Modifier::BOLD)),
                Span::styled(onto.clone(), Style::default().fg(AZURE)),
            ]));
        }

        if let Some(ref commit) = status.current_commit {
            lines.push(Line::from(""));
            lines.push(Line::from(vec![
                Span::styled("Current commit: ", Style::default().add_modifier(Modifier::BOLD)),
                Span::styled(commit.clone(), Style::default().fg(Color::Yellow)),
            ]));
            if let Some(ref msg) = status.current_commit_message {
                lines.push(Line::from(vec![
                    Span::raw("  "),
                    Span::raw(msg.clone()),
                ]));
            }
        }

        if !status.conflicted_files.is_empty() {
            lines.push(Line::from(""));
            lines.push(Line::from(vec![
                Span::styled(
                    format!("Conflicts ({}):", status.conflicted_files.len()),
                    Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                ),
            ]));
            for (idx, file) in status.conflicted_files.iter().enumerate() {
                let is_selected = app.selected_conflict == Some(idx);
                let style = if is_selected {
                    Style::default().bg(Color::DarkGray).fg(Color::White)
                } else {
                    Style::default().fg(Color::Red)
                };
                let prefix = if is_selected { "▸ " } else { "  " };
                lines.push(Line::from(vec![
                    Span::raw(prefix),
                    Span::styled(file.clone(), style),
                ]));
            }
            lines.push(Line::from(""));
            lines.push(Line::from(vec![
                Span::styled("Commands: ", Style::default().add_modifier(Modifier::BOLD)),
            ]));
            lines.push(Line::from("  o: use ours (keep our changes)"));
            lines.push(Line::from("  t: use theirs (accept incoming)"));
            lines.push(Line::from("  Enter: view conflict diff"));
            lines.push(Line::from("  c: continue rebase"));
            lines.push(Line::from("  s: skip this commit"));
            lines.push(Line::from("  A: abort rebase"));
        } else if status.state == RebaseState::InProgress {
            lines.push(Line::from(""));
            lines.push(Line::from(vec![
                Span::styled("No conflicts. ", Style::default().fg(Color::Green)),
                Span::raw("Press 'c' to continue."),
            ]));
        }
    } else {
        lines.push(Line::from("No rebase in progress"));
    }

    lines
}

/// Render the sticky todo widget — shows at the bottom of the convo pane.
/// Main agent todos show as ✓/●/○. Subagent todos show indented with "↳" prefix
/// directly after the parent todo item (the in_progress item when the Task spawned).
fn draw_todo_widget(
    f: &mut Frame,
    todos: &[crate::app::TodoItem],
    subagent_todos: &[crate::app::TodoItem],
    parent_idx: Option<usize>,
    area: Rect,
    animation_tick: u64,
) {
    use crate::app::TodoStatus;

    let pulse_colors = [Color::Yellow, Color::LightYellow, Color::Yellow, Color::DarkGray];
    let pulse = pulse_colors[(animation_tick / 3) as usize % pulse_colors.len()];

    // Convert a single TodoItem into a Line with optional "↳ " indent prefix
    let make_line = |t: &crate::app::TodoItem, is_subtask: bool| -> Line {
        let (icon, color) = match t.status {
            TodoStatus::Completed => ("✓ ", Color::Green),
            TodoStatus::InProgress => ("● ", pulse),
            TodoStatus::Pending => ("○ ", Color::DarkGray),
        };
        let text = if t.status == TodoStatus::InProgress && !t.active_form.is_empty() {
            &t.active_form
        } else { &t.content };
        let text_color = if t.status == TodoStatus::Completed { Color::DarkGray } else { Color::White };
        let mut spans = Vec::new();
        if is_subtask {
            spans.push(Span::styled("↳ ", Style::default().fg(Color::DarkGray)));
        }
        spans.push(Span::styled(icon, Style::default().fg(color)));
        spans.push(Span::styled(text.clone(), Style::default().fg(text_color)));
        Line::from(spans)
    };

    // Insert position: right after the parent todo item.
    // If no parent tracked, fall back to end of list (append).
    let insert_after = parent_idx.unwrap_or(todos.len().saturating_sub(1));

    let mut todo_lines: Vec<Line> = Vec::with_capacity(todos.len() + subagent_todos.len());
    for (i, t) in todos.iter().enumerate() {
        todo_lines.push(make_line(t, false));
        // Inject subagent subtasks right after the parent item
        if i == insert_after && !subagent_todos.is_empty() {
            for sub in subagent_todos {
                todo_lines.push(make_line(sub, true));
            }
        }
    }
    // Edge case: no main todos but subagent todos exist (shouldn't happen, but safe)
    if todos.is_empty() {
        for sub in subagent_todos {
            todo_lines.push(make_line(sub, true));
        }
    }

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .title(Span::styled(" Tasks ", Style::default().fg(AZURE).add_modifier(Modifier::BOLD)))
        .border_style(Style::default().fg(Color::DarkGray));

    let widget = Paragraph::new(todo_lines).block(block).wrap(Wrap { trim: false });
    f.render_widget(widget, area);
}
