//! Session pane rendering
//!
//! Expensive work (markdown parsing, syntax highlighting, text wrapping) runs
//! on a background render thread. The main event loop sends render requests
//! via `submit_render_request()` (non-blocking) and polls for completed results
//! via `poll_render_result()` (non-blocking). The draw function itself is cheap —
//! just clones a viewport slice and renders from the pre-built cache.
//!
//! Submodules:
//! - `render_submit`: Background render thread submit/poll coordination
//! - `session_list`: Session browser overlay with filter and content search
//! - `todo_widget`: Sticky task progress tracker at bottom of session pane
mod render_submit;
mod session_list;
mod todo_widget;

/// Re-export public API so existing `use super::draw_output::{...}` imports work unchanged
pub use render_submit::{submit_render_request, poll_render_result};

use ratatui::{
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Paragraph},
    Frame,
};

use crate::app::{App, Focus, ViewMode};
use super::colorize::ORANGE;
use super::util::{colorize_output, detect_message_type, MessageType, GIT_BROWN, GIT_ORANGE, AZURE};

/// Draw the main output/diff panel — cheap, just reads from pre-rendered caches
pub fn draw_output(f: &mut Frame, app: &mut App, area: Rect) {
    // Git panel mode — show commit log instead of conversation
    if let Some(ref panel) = app.git_actions_panel {
        draw_git_commits(f, panel, area);
        return;
    }

    // Session list overlay takes over the entire session pane when active
    if app.show_session_list {
        session_list::draw_session_list(f, app, area);
        return;
    }

    // Split area for sticky todo widget at bottom (visible whenever todos exist —
    // stays visible even when all completed, cleared on next user prompt or session switch)
    let has_todos = !app.current_todos.is_empty() || !app.subagent_todos.is_empty();
    let todo_height = if has_todos {
        // Account for text wrapping: each todo may span multiple visual lines.
        // Inner width = area width minus 2 for borders (minus 1 more if scrollbar needed).
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
        let total_content_lines = main_lines + sub_lines;
        app.todo_total_lines = total_content_lines;
        // Cap at 20 content lines + 2 border = 22, also ensure session pane has >= 10 rows
        let max_h = 22u16.min(area.height.saturating_sub(10));
        (total_content_lines + 2).min(max_h)
    } else { app.todo_total_lines = 0; 0 };
    // Search bar at bottom of session pane: visible when search is active or has residual matches
    let has_search = app.session_find_active || !app.session_find_matches.is_empty();
    let search_height: u16 = if has_search { 3 } else { 0 };
    let [session_area, search_area, todo_area] = Layout::vertical([
        Constraint::Min(1),
        Constraint::Length(search_height),
        Constraint::Length(todo_height),
    ]).areas(area);
    let area = session_area;
    let viewport_height = area.height.saturating_sub(2) as usize;

    // Cache viewport height for scroll operations (input handling uses this)
    app.session_viewport_height = viewport_height;

    let (title, content) = match app.view_mode {
        ViewMode::Session => {
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
                // leave session_scroll as usize::MAX so it keeps following on next frame.
                let scroll = if app.session_scroll == usize::MAX {
                    app.session_natural_bottom()
                } else {
                    app.session_scroll.min(app.session_max_scroll())
                };

                // Check if viewport cache is still valid — skip the clone if so.
                // Selection changes also invalidate (must re-apply highlight)
                let cache_valid = scroll == app.session_viewport_scroll
                    && app.animation_tick == app.session_viewport_anim_tick
                    && app.session_selection == app.session_selection_cached
                    && app.session_viewport_cache.len() == viewport_height.min(app.rendered_lines_cache.len().saturating_sub(scroll));

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
                    if let Some((sl, sc, el, ec)) = app.session_selection {
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
                    app.session_selection_cached = app.session_selection;

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

                    // Apply session find match highlighting (yellow bg for matches,
                    // bright yellow for current match — same span-splitting technique)
                    if !app.session_find_matches.is_empty() {
                        let match_style = Style::default().bg(Color::DarkGray).fg(Color::Yellow);
                        let current_style = Style::default().bg(Color::Yellow).fg(Color::Black);
                        for (mi, &(line_idx, sc, ec)) in app.session_find_matches.iter().enumerate() {
                            if line_idx < scroll || line_idx >= scroll + viewport_height { continue; }
                            let vi = line_idx - scroll;
                            let Some(line) = lines.get_mut(vi) else { continue };
                            let style = if mi == app.session_find_current { current_style } else { match_style };
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
                        format!(" Session [{}/{}] ", current_msg, total_msgs)
                    } else {
                        " Session ".to_string()
                    };

                    app.session_viewport_cache = lines;
                    app.session_viewport_scroll = scroll;
                    app.session_viewport_anim_tick = app.animation_tick;
                    app.session_viewport_title = title;
                }

                (app.session_viewport_title.clone(), app.session_viewport_cache.clone())
            } else if !app.session_lines.is_empty() || !app.session_buffer.is_empty() {
                // Fallback: using session_lines with colorize_output
                let mut all_lines: Vec<Line> = Vec::new();
                let mut last_msg_type = MessageType::Other;

                for line in app.session_lines.iter() {
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

                if !app.session_buffer.is_empty() {
                    let msg_type = detect_message_type(&app.session_buffer);
                    if (last_msg_type == MessageType::User && msg_type == MessageType::Assistant)
                        || (last_msg_type == MessageType::Assistant && msg_type == MessageType::User)
                    {
                        all_lines.push(Line::from(""));
                        all_lines.push(Line::from(""));
                    }
                    all_lines.push(colorize_output(&app.session_buffer));
                }

                let total = all_lines.len();
                let max_scroll = total.saturating_sub(viewport_height);
                // Resolve sentinel to concrete position for THIS frame only —
                // don't write it back so usize::MAX survives and keeps
                // following bottom as new content arrives.
                let scroll = if app.session_scroll == usize::MAX { max_scroll }
                    else { app.session_scroll.min(max_scroll) };
                let lines: Vec<Line> = all_lines.into_iter().skip(scroll).take(viewport_height).collect();
                let title = if total > viewport_height {
                    format!(" Session [{}/{}] ", scroll + viewport_height.min(total - scroll), total)
                } else {
                    " Session ".to_string()
                };
                (title, lines)
            } else {
                (" Session ".to_string(), vec![])
            }
        }
    };

    let is_focused = app.focus == Focus::Session;
    let rcr_active = app.rcr_session.is_some();
    let border_style = if rcr_active {
        // RCR mode: green borders to visually indicate active conflict resolution
        Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)
    } else if is_focused {
        Style::default().fg(AZURE).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::White)
    };

    // Build right-aligned title: token percentage + PID/exit code
    let branch = app.current_worktree().map(|s| s.branch_name.clone());
    let right_title: Option<Line<'static>> = {
        let mut spans: Vec<Span<'static>> = Vec::new();

        // Token usage percentage badge — pre-computed in update_token_badge(), just read the cache
        if let Some((ref text, color)) = app.token_badge_cache {
            spans.push(Span::styled(
                text.clone(),
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            ));
        }

        // PID while running (active slot's PID = its key), exit code after.
        // Suppress when viewing a historic (non-active) session file to prevent
        // showing another session's PID or exit code in the border.
        if let Some(b) = branch.as_deref() {
            if !app.viewing_historic_session {
                // The active slot's key IS the PID string
                let active_pid = app.active_slot.get(b)
                    .filter(|slot| app.running_sessions.contains(*slot))
                    .and_then(|slot| slot.parse::<u32>().ok());
                if let Some(pid) = active_pid {
                    spans.push(Span::styled(
                        format!(" PID:{} ", pid),
                        Style::default().fg(Color::Green).add_modifier(Modifier::BOLD),
                    ));
                } else if let Some(&code) = app.active_slot.get(b)
                    .and_then(|slot| app.claude_exit_codes.get(slot))
                    .or_else(|| app.claude_exit_codes.get(b)) {
                    let (text, color) = if code == 0 {
                        (" exit:0 ".to_string(), Color::Green)
                    } else {
                        (format!(" exit:{} ", code), Color::Red)
                    };
                    spans.push(Span::styled(text, Style::default().fg(color)));
                }
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
            format!("[{}\u{2026}]", trunc)
        } else {
            String::new()
        };
        if !bracketed.is_empty() {
            let title_color = if rcr_active { Color::Green } else { Color::White };
            block = block.title(
                Line::from(Span::styled(bracketed, Style::default().fg(title_color)))
                    .alignment(Alignment::Center)
            );
        }
    }

    // Add right-aligned PID/exit title — ratatui fills gap with border chars
    if let Some(rt) = right_title {
        block = block.title(rt);
    }

    // RCR review mode: show ⌃a hint on bottom border when dialog is dismissed
    if let Some(ref rcr) = app.rcr_session {
        if !rcr.approval_pending {
            block = block.title_bottom(
                Line::from(vec![
                    Span::styled(" ⌃a ", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
                    Span::styled("Accept/Abort ", Style::default().fg(Color::DarkGray)),
                ]).alignment(Alignment::Center)
            );
        }
    }

    // ⌃m model indicator on bottom border (right-aligned)
    {
        let model_name = app.display_model_name();
        let model_color = match model_name {
            "opus" => Color::Magenta,
            "sonnet" => Color::Cyan,
            "haiku" => Color::Yellow,
            _ => Color::DarkGray,
        };
        block = block.title_bottom(
            Line::from(vec![
                Span::styled(" ⌃m", Style::default().fg(Color::DarkGray)),
                Span::styled(":", Style::default().fg(Color::DarkGray)),
                Span::styled(format!("{} ", model_name), Style::default().fg(model_color).add_modifier(Modifier::BOLD)),
            ]).alignment(Alignment::Right)
        );
    }

    let output = Paragraph::new(content).block(block);
    f.render_widget(output, area);

    // Render session find bar at bottom of session content area
    if has_search {
        let match_info = if app.session_find_matches.is_empty() {
            if app.session_find.is_empty() { String::new() } else { " 0/0 ".to_string() }
        } else {
            format!(" {}/{} ", app.session_find_current + 1, app.session_find_matches.len())
        };
        let border_color = if app.session_find_active { Color::Yellow } else { Color::DarkGray };
        let search_widget = Paragraph::new(app.session_find.clone())
            .block(Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(border_color))
                .title(Span::styled("/", Style::default().fg(Color::Yellow)))
                .title(Line::from(Span::styled(match_info, Style::default().fg(Color::DarkGray))).alignment(Alignment::Right)),
            );
        f.render_widget(search_widget, search_area);
        // Show cursor in search bar when actively typing
        if app.session_find_active {
            let cursor_x = search_area.x + 1 + app.session_find.len() as u16;
            let cursor_y = search_area.y + 1;
            if cursor_x < search_area.right() {
                f.set_cursor_position((cursor_x, cursor_y));
            }
        }
    }

    // Render sticky todo widget at bottom of session pane (main + subagent todos).
    // Cache the rect for mouse scroll hit-testing, and clamp scroll to valid range.
    if todo_height > 0 {
        app.pane_todo = todo_area;
        let content_h = todo_area.height.saturating_sub(2);
        let max_scroll = app.todo_total_lines.saturating_sub(content_h);
        if app.todo_scroll > max_scroll { app.todo_scroll = max_scroll; }
        todo_widget::draw_todo_widget(f, &app.current_todos, &app.subagent_todos, app.subagent_parent_idx, todo_area, app.animation_tick, app.todo_scroll, app.todo_total_lines);
    } else {
        // No todos visible — clear cached rect so mouse scroll won't hit-test stale area
        app.pane_todo = Rect::default();
    }
}

/// Draw the RCR approval dialog — a small centered green-bordered box asking
/// whether the user wants to accept the conflict resolution. Rendered over the
/// session pane after Claude exits during RCR mode.
pub fn draw_rcr_approval(f: &mut Frame, area: Rect) {
    // Size: 46 wide × 5 tall, centered within the session pane
    let w = 46u16.min(area.width.saturating_sub(2));
    let h = 5u16.min(area.height.saturating_sub(2));
    let x = area.x + (area.width.saturating_sub(w)) / 2;
    let y = area.y + (area.height.saturating_sub(h)) / 2;
    let dialog = Rect::new(x, y, w, h);

    f.render_widget(Clear, dialog);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Double)
        .border_style(Style::default().fg(Color::Green).add_modifier(Modifier::BOLD))
        .title(Span::styled(" RCR ", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)));

    let text = vec![
        Line::from(Span::styled("Accept conflict resolution?", Style::default().fg(Color::White).add_modifier(Modifier::BOLD))),
        Line::from(vec![
            Span::styled("[y]", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
            Span::raw(" Accept  "),
            Span::styled("[n]", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
            Span::raw(" Abort  "),
            Span::styled("[Esc]", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
            Span::raw(" Review"),
        ]),
    ];
    let para = Paragraph::new(text).block(block).alignment(Alignment::Center);
    f.render_widget(para, dialog);
}

/// Draw the post-merge dialog — asks whether to keep, archive, or delete the
/// worktree after a successful squash merge. Centered over the session pane.
pub fn draw_post_merge_dialog(f: &mut Frame, area: Rect, dialog_state: &crate::app::types::PostMergeDialog) {
    let w = 50u16.min(area.width.saturating_sub(2));
    let h = 9u16.min(area.height.saturating_sub(2));
    let x = area.x + (area.width.saturating_sub(w)) / 2;
    let y = area.y + (area.height.saturating_sub(h)) / 2;
    let rect = Rect::new(x, y, w, h);

    f.render_widget(Clear, rect);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Double)
        .border_style(Style::default().fg(AZURE).add_modifier(Modifier::BOLD))
        .title(Span::styled(
            format!(" {} merged ", dialog_state.display_name),
            Style::default().fg(AZURE).add_modifier(Modifier::BOLD),
        ));

    let arrow = |i: usize| if dialog_state.selected == i { "▸ " } else { "  " };
    let style = |i: usize, color: Color| {
        if dialog_state.selected == i {
            Style::default().fg(color).add_modifier(Modifier::BOLD)
        } else { Style::default().fg(Color::White) }
    };

    let text = vec![
        Line::from(Span::styled("What should happen to this worktree?", Style::default().fg(Color::White).add_modifier(Modifier::BOLD))),
        Line::from(""),
        Line::from(Span::styled(format!("{}Keep — continue working on this branch", arrow(0)), style(0, Color::Green))),
        Line::from(Span::styled(format!("{}Archive — remove worktree, keep branch", arrow(1)), style(1, Color::Yellow))),
        Line::from(Span::styled(format!("{}Delete — remove worktree and branch", arrow(2)), style(2, Color::Red))),
    ];
    let para = Paragraph::new(text).block(block).alignment(Alignment::Left);
    f.render_widget(para, rect);
}

/// Git panel commit log — scrollable list of recent commits
fn draw_git_commits(f: &mut Frame, panel: &crate::app::types::GitActionsPanel, area: Rect) {
    let focused = panel.focused_pane == 2;
    let inner_h = area.height.saturating_sub(2) as usize;
    let inner_w = area.width.saturating_sub(2) as usize;
    let mut lines: Vec<Line> = Vec::new();

    if panel.commits.is_empty() {
        lines.push(Line::from(Span::styled(
            " No commits",
            Style::default().fg(Color::DarkGray),
        )));
    } else {
        // Adjust scroll so selected commit is visible
        let scroll = if panel.selected_commit < panel.commit_scroll {
            panel.selected_commit
        } else if panel.selected_commit >= panel.commit_scroll + inner_h {
            panel.selected_commit.saturating_sub(inner_h.saturating_sub(1))
        } else {
            panel.commit_scroll
        };

        for (i, commit) in panel.commits.iter().enumerate().skip(scroll).take(inner_h) {
            let selected = focused && i == panel.selected_commit;
            let prefix = if selected { " \u{25b8} " } else { "   " };

            // Green for unpushed, dim for pushed
            let hash_color = if !commit.is_pushed { Color::Green } else { Color::DarkGray };
            let subject_color = if selected {
                GIT_ORANGE
            } else if !commit.is_pushed {
                Color::Green
            } else {
                Color::White
            };
            let subject_mod = if selected { Modifier::BOLD } else { Modifier::empty() };

            // Truncate subject to fit: prefix(3) + hash(7) + space(1) + subject
            let subject_budget = inner_w.saturating_sub(prefix.len() + 8);
            let subject_display = if commit.subject.len() > subject_budget {
                format!("{}\u{2026}", &commit.subject[..subject_budget.saturating_sub(1)])
            } else {
                commit.subject.clone()
            };

            lines.push(Line::from(vec![
                Span::styled(prefix, Style::default()),
                Span::styled(&commit.hash, Style::default().fg(hash_color)),
                Span::raw(" "),
                Span::styled(subject_display, Style::default().fg(subject_color).add_modifier(subject_mod)),
            ]));
        }
    }

    let title = format!(" Commits ({}) ", panel.commits.len());
    let border_style = if focused {
        Style::default().fg(GIT_ORANGE).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(GIT_BROWN)
    };
    let mut block = Block::default()
        .title(Span::styled(title, Style::default()
            .fg(if focused { GIT_ORANGE } else { GIT_BROWN })
            .add_modifier(if focused { Modifier::BOLD } else { Modifier::empty() })))
        .borders(Borders::ALL)
        .border_type(if focused { BorderType::Double } else { BorderType::Plain })
        .border_style(border_style);

    if !panel.is_on_main && panel.commits_behind_main > 0 {
        block = block.title(
            Line::from(Span::styled(
                format!(" {} behind ", panel.commits_behind_main),
                Style::default().fg(GIT_BROWN).add_modifier(Modifier::DIM),
            )).alignment(Alignment::Right)
        );
    }

    f.render_widget(Paragraph::new(lines).block(block), area);
}
