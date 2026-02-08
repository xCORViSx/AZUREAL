//! Convo pane rendering
//!
//! Expensive work (markdown parsing, syntax highlighting, text wrapping) runs
//! on a background render thread. The main event loop sends render requests
//! via `submit_render_request()` (non-blocking) and polls for completed results
//! via `poll_render_result()` (non-blocking). The draw function itself is cheap —
//! just clones a viewport slice and renders from the pre-built cache.

use ratatui::{
    layout::{Alignment, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Paragraph},
    Frame,
};

use crate::app::{App, Focus, ViewMode};
use crate::models::RebaseState;
use super::render_thread::RenderRequest;
use super::util::{colorize_output, detect_message_type, MessageType};

/// On initial load of large conversations, only render this many events from the tail.
/// The user starts at the bottom so they see the most recent messages instantly.
/// Full render happens lazily when they scroll to the top.
const DEFERRED_RENDER_TAIL: usize = 200;

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
        if trim < existing_lines.len() {
            existing_lines.truncate(trim);
            existing_anim.retain(|&(idx, _)| idx < trim);
            // Remove the trailing pending bubble position
            if let Some(&(line_idx, _)) = existing_bubbles.last() {
                if line_idx >= trim { existing_bubbles.pop(); }
            }
        }
        RenderRequest {
            events: app.display_events.clone(),
            start_idx: app.rendered_events_count,
            width: inner_width,
            pending_tools: app.pending_tool_calls.clone(),
            failed_tools: app.failed_tool_calls.clone(),
            pending_user_message: app.pending_user_message.clone(),
            existing_lines,
            existing_anim,
            existing_bubbles,
            deferred_start: 0,
            seq: 0, // filled by send()
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

        RenderRequest {
            events: app.display_events.clone(),
            start_idx: 0,
            width: inner_width,
            pending_tools: app.pending_tool_calls.clone(),
            failed_tools: app.failed_tool_calls.clone(),
            pending_user_message: app.pending_user_message.clone(),
            existing_lines: Vec::new(),
            existing_anim: Vec::new(),
            existing_bubbles: Vec::new(),
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

    // If the user was at/near the bottom of the OLD cache (or the sentinel was
    // already resolved to a concrete position), re-set the follow-bottom sentinel
    // so the next draw scrolls to the NEW bottom. Without this, the scroll position
    // stays at the old bottom and the newly appended pending bubble is off-screen.
    let old_len = app.rendered_lines_cache.len();
    let was_at_bottom = app.output_scroll == usize::MAX
        || app.output_scroll >= old_len.saturating_sub(app.output_viewport_height);

    // Apply the completed render to app state
    app.rendered_lines_cache = result.lines;
    app.animation_line_indices = result.anim_indices;
    app.message_bubble_positions = result.bubble_positions;
    app.rendered_lines_width = result.width;
    app.rendered_events_count = result.events_count;
    app.rendered_content_line_count = content_lines;
    app.rendered_events_start = result.events_start;
    app.render_seq_applied = result.seq;
    app.render_in_flight = false;

    // Re-set follow-bottom sentinel so the next draw shows the new content
    if was_at_bottom { app.output_scroll = usize::MAX; }

    // Invalidate viewport cache since underlying content changed
    app.output_viewport_scroll = usize::MAX;
    true
}

/// Draw the main output/diff panel — cheap, just reads from pre-rendered caches
pub fn draw_output(f: &mut Frame, app: &mut App, area: Rect) {
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
                // Clamp scroll to valid range (resolves usize::MAX sentinel)
                app.clamp_output_scroll();
                let scroll = app.output_scroll;

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

                    // Build title with message count
                    let title = if !app.message_bubble_positions.is_empty() {
                        let total_msgs = app.message_bubble_positions.len();
                        let current_line = scroll.saturating_add(3);
                        let current_msg = app.message_bubble_positions.iter()
                            .enumerate()
                            .rev()
                            .find(|(_, (line_idx, _))| *line_idx <= current_line)
                            .map(|(idx, _)| idx + 1)
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
                let scroll = if app.output_scroll == usize::MAX { max_scroll }
                    else { app.output_scroll.min(max_scroll) };
                app.output_scroll = scroll;
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
        Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::White)
    };

    // Build right-aligned title: PID while running, exit code after
    let branch = app.current_session().map(|s| s.branch_name.clone());
    let right_title: Option<Line<'static>> = branch.as_deref().and_then(|b| {
        if let Some(&pid) = app.claude_pids.get(b) {
            // Running — show PID in green
            Some(Line::from(Span::styled(
                format!(" PID:{} ", pid),
                Style::default().fg(Color::Green).add_modifier(Modifier::BOLD),
            )).alignment(Alignment::Right))
        } else if let Some(&code) = app.claude_exit_codes.get(b) {
            // Exited — show exit code (green for 0, red for non-zero)
            let (text, color) = if code == 0 {
                (" exit:0 ".to_string(), Color::Green)
            } else {
                (format!(" exit:{} ", code), Color::Red)
            };
            Some(Line::from(Span::styled(text, Style::default().fg(color)))
                .alignment(Alignment::Right))
        } else {
            None
        }
    });

    let mut block = Block::default()
        .borders(Borders::ALL)
        .border_type(if is_focused { BorderType::Double } else { BorderType::Plain })
        .title(Span::styled(title, border_style))
        .border_style(border_style);

    // Add right-aligned PID/exit title — ratatui fills gap with border chars
    if let Some(rt) = right_title {
        block = block.title(rt);
    }

    let output = Paragraph::new(content).block(block);
    f.render_widget(output, area);
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
                Span::styled(onto.clone(), Style::default().fg(Color::Cyan)),
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
