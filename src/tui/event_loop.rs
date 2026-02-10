//! Core event loop and event handling

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEventKind, MouseButton, MouseEventKind};
use crossterm::{cursor, execute, style};
use std::io::{self, Write};
use std::time::{Duration, Instant};

use ratatui::{backend::CrosstermBackend, Terminal};

use crate::app::{App, Focus};
use crate::claude::{ClaudeEvent, ClaudeProcess};
use crate::config::Config;

use super::keybindings::{Action, KeyContext, lookup_action};
use super::input_dialogs::{handle_branch_dialog_input, handle_context_menu_input, handle_run_command_picker_input, handle_run_command_dialog_input};
use super::input_file_tree::handle_file_tree_input;
use super::input_output::handle_output_input;
use super::input_worktrees::handle_worktrees_input;
use super::input_terminal::{handle_input_mode, handle_worktree_creation_input};
use super::input_viewer::handle_viewer_input;
use super::input_projects::handle_projects_input;
use super::input_wizard::handle_wizard_input;
use super::draw_output::{submit_render_request, poll_render_result};
use super::draw_input::{word_wrap_break_points, display_width};
use super::draw_viewer::word_wrap_breaks;
use super::run::ui;

/// Main TUI event loop
pub async fn run_app(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
    config: Config,
) -> Result<()> {
    let claude_process = ClaudeProcess::new(config);
    let mut last_draw = Instant::now();
    let mut last_session_poll = Instant::now();
    let mut last_animation = Instant::now();
    // Every draw costs ~18ms (terminal I/O). To avoid blocking key events, we
    // throttle ALL draws — even key-triggered ones — to this interval. This
    // guarantees at least one event-only loop iteration between draws, giving
    // crossterm a window to buffer incoming keystrokes.
    let min_draw_interval = Duration::from_millis(33); // ~30fps max
    let min_poll_interval = Duration::from_millis(500); // Poll session file max 2x/sec
    let min_animation_interval = Duration::from_millis(250); // 4fps for pulsating indicators

    // Cache terminal size, update on resize events
    let (mut cached_width, mut cached_height) = crossterm::terminal::size().unwrap_or((80, 24));

    // Initial draw
    terminal.draw(|f| ui(f, app))?;

    loop {
        // Only poll terminal when in terminal mode (avoid unnecessary rx check)
        let terminal_changed = app.terminal_mode && app.poll_terminal();

        // Throttle animation updates (4fps) to avoid constant redraws
        let now_anim = Instant::now();
        let animation_due = now_anim.duration_since(last_animation) >= min_animation_interval;
        let has_pending_tools = !app.pending_tool_calls.is_empty();
        if animation_due && has_pending_tools {
            app.animation_tick = app.animation_tick.wrapping_add(1);
            last_animation = now_anim;
        }

        // Only redraw for animation if it actually updated
        let mut needs_redraw = terminal_changed || (animation_due && has_pending_tools);
        let mut scroll_delta: i32 = 0;
        let mut scroll_col: u16 = 0;
        let mut scroll_row: u16 = 0;
        let mut had_key_event = false;

        // Poll timeout: short when busy (render in-flight or Claude streaming)
        // so we pick up completed renders and key events quickly. Longer when
        // idle to avoid burning CPU spinning on an empty event queue.
        // Short poll when we have pending work: draw waiting, render in-flight,
        // or Claude streaming. Ensures fast pickup without burning CPU when idle.
        let poll_ms = if app.draw_pending || app.render_in_flight || !app.claude_receivers.is_empty() || app.stt_recording || app.stt_transcribing || app.session_file_dirty || app.file_tree_refresh_pending { 16 } else { 100 };
        if event::poll(Duration::from_millis(poll_ms))? {
            // Drain all available events without blocking
            loop {
                match event::read()? {
                    Event::Key(key) => {
                        // Accept Press AND Repeat — Repeat fires when a key
                        // is held down (Kitty REPORT_EVENT_TYPES). Without this,
                        // holding arrow keys only moves cursor once.
                        if matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) {
                            handle_key_event(key, app, &claude_process)?;
                            had_key_event = true;
                        }
                    }
                    Event::Mouse(mouse) => {
                        match mouse.kind {
                            MouseEventKind::ScrollDown => {
                                scroll_delta += 3;
                                scroll_col = mouse.column;
                                scroll_row = mouse.row;
                            }
                            MouseEventKind::ScrollUp => {
                                scroll_delta -= 3;
                                scroll_col = mouse.column;
                                scroll_row = mouse.row;
                            }
                            // Left click: convert screen→cache coords for drag anchor,
                            // clear selections, focus/select. Cache coords stored so
                            // auto-scroll during drag doesn't shift the anchor.
                            MouseEventKind::Down(MouseButton::Left) => {
                                app.viewer_selection = None;
                                app.output_selection = None;
                                let (mc, mr) = (mouse.column, mouse.row);
                                use ratatui::layout::Position;
                                let mpos = Position::new(mc, mr);
                                if app.pane_viewer.contains(mpos) {
                                    if app.viewer_edit_mode {
                                        // Edit mode: click sets edit cursor, drag anchor stores source coords
                                        if let Some((src_line, src_col)) = screen_to_edit_pos(app, mc, mr) {
                                            app.mouse_drag_start = Some((src_line, src_col, 3));
                                        }
                                    } else if let Some((cl, cc)) = screen_to_cache_pos(mc, mr, app.pane_viewer, app.viewer_scroll, app.viewer_lines_cache.len()) {
                                        app.mouse_drag_start = Some((cl, cc, 0));
                                    }
                                } else if app.pane_convo.contains(mpos) {
                                    app.clamp_output_scroll();
                                    if let Some((cl, cc)) = screen_to_cache_pos(mc, mr, app.pane_convo, app.output_scroll, app.rendered_lines_cache.len()) {
                                        app.mouse_drag_start = Some((cl, cc, 1));
                                    }
                                } else if app.input_area.contains(mpos) && app.prompt_mode && !app.terminal_mode {
                                    let ci = screen_to_input_char(app, mc, mr);
                                    app.mouse_drag_start = Some((ci, 0, 2));
                                } else {
                                    app.mouse_drag_start = None;
                                }
                                if handle_mouse_click(app, mc, mr) {
                                    needs_redraw = true;
                                }
                            }
                            // Drag: compute text selection from start to current
                            MouseEventKind::Drag(MouseButton::Left) => {
                                if handle_mouse_drag(app, mouse.column, mouse.row) {
                                    needs_redraw = true;
                                }
                            }
                            // Release: stop drag tracking, keep selection
                            MouseEventKind::Up(MouseButton::Left) => {
                                app.mouse_drag_start = None;
                            }
                            _ => {} // Discard motion, right-click
                        }
                    }
                    Event::Resize(w, h) => {
                        cached_width = w;
                        cached_height = h;
                        needs_redraw = true;
                    }
                    _ => {}
                }
                // Check if more events pending (non-blocking)
                if !event::poll(Duration::from_millis(0))? {
                    break;
                }
            }
        }

        // Process Claude events — drain all available from each receiver.
        // We must collect first (borrows claude_receivers) then process (borrows app mutably).
        // Avoid nested Vec + flat_map — single drain loop per receiver is simpler.
        if !app.claude_receivers.is_empty() {
            let mut claude_events: Vec<(String, ClaudeEvent)> = Vec::new();
            for (sid, rx) in &app.claude_receivers {
                while let Ok(event) = rx.try_recv() {
                    claude_events.push((sid.clone(), event));
                }
            }
            for (session_id, event) in claude_events {
                handle_claude_event(&session_id, event, app, &claude_process)?;
                needs_redraw = true;
            }
        }

        // Poll speech-to-text events (non-blocking, only if handle exists)
        if app.stt_handle.is_some() {
            if app.poll_stt() {
                needs_redraw = true;
            }
        }

        // --- File watcher: drain kernel-level notify events (non-blocking) ---
        // When notify is active, filesystem events set dirty flags directly.
        // Falls back to stat() polling if the watcher failed to initialize.
        if let Some(ref watcher) = app.file_watcher {
            while let Some(evt) = watcher.try_recv() {
                match evt {
                    crate::watcher::WatchEvent::SessionFileChanged => {
                        app.session_file_dirty = true;
                    }
                    crate::watcher::WatchEvent::WorktreeChanged => {
                        app.file_tree_refresh_pending = true;
                        app.worktree_last_notify = Instant::now();
                    }
                    crate::watcher::WatchEvent::WatcherFailed(_) => {
                        app.file_watcher = None;
                        break;
                    }
                }
            }
        }

        let now_poll = Instant::now();

        // Parse session file when dirty (set by watcher or fallback polling)
        if app.session_file_dirty {
            if app.poll_session_file() { needs_redraw = true; }
        }

        // Fallback: stat() polling when watcher is unavailable
        if app.file_watcher.is_none() && now_poll.duration_since(last_session_poll) >= min_poll_interval {
            app.check_session_file();
            if app.poll_session_file() { needs_redraw = true; }
        }

        // Debounced file tree refresh: wait 500ms after last worktree change
        // to coalesce rapid creates/deletes (e.g., Claude creating many files)
        if app.file_tree_refresh_pending
            && now_poll.duration_since(app.worktree_last_notify) >= Duration::from_millis(500)
        {
            app.load_file_tree();
            app.file_tree_refresh_pending = false;
            needs_redraw = true;
        }

        // Interactive sessions + timer-based housekeeping
        if now_poll.duration_since(last_session_poll) >= min_poll_interval {
            if app.poll_interactive_sessions() { needs_redraw = true; }
            last_session_poll = now_poll;
        }

        // Apply accumulated scroll using cached terminal size
        let mut scroll_changed = false;
        if scroll_delta != 0 {
            scroll_changed = apply_scroll_cached(app, scroll_delta, scroll_col, scroll_row, cached_width, cached_height);
        }

        // Submit render request to background thread if convo cache is dirty.
        // This is NON-BLOCKING — the render thread does the expensive work while
        // we keep processing events. No more frozen input during convo updates!
        // BACKPRESSURE: skip if a render is already in flight — avoids cloning
        // the entire event array every 16ms while Claude streams, which was the
        // root cause of 100%+ CPU on prompt submit.
        // THROTTLE: also skip if less than 50ms since last submit — batches rapid
        // streaming events into fewer render cycles (clones). During Claude streaming
        // events arrive at ~60Hz; without this, every poll_render_result completion
        // immediately triggers another clone+submit, keeping CPU high.
        if app.rendered_lines_dirty && !app.render_in_flight
            && app.last_render_submit.elapsed() >= Duration::from_millis(50)
        {
            // Convo pane is fixed at 80 columns (Constraint::Length(80) in run.rs).
            // We pass this directly — the old formula `(terminal - 80) / 2` was a
            // leftover from the 50/50 split layout and made bubbles way too narrow.
            submit_render_request(app, 80);
            app.last_render_submit = Instant::now();
        }

        // Poll for completed render results from the background thread (non-blocking).
        // If fresh content arrived, trigger a redraw to show it.
        if poll_render_result(app) {
            needs_redraw = true;
        }

        // Mark that we need a draw (will be fulfilled on a quiet iteration)
        if had_key_event || needs_redraw || scroll_changed {
            app.draw_pending = true;
        }

        // Fast-path input rendering: when the user is typing in prompt mode,
        // skip the expensive terminal.draw() (~18ms) and instead write the
        // input box content directly via crossterm (~0.1ms). This gives instant
        // keystroke feedback while the full UI catches up on the next quiet frame.
        // Skip fast-path for multi-line input — the input box must resize via
        // full draw when newlines are added/removed. Single-line typing (the
        // common case) still gets the fast path.
        // Skip fast-path when selection is active — fast_draw_input doesn't
        // render selection highlighting, so the full draw_input must handle it
        if had_key_event && app.prompt_mode && !app.terminal_mode && app.focus == Focus::Input && app.input_area.width > 2 && !app.input.contains('\n') && !app.has_input_selection() {
            fast_draw_input(app);
        }

        // Full draw: terminal.draw() costs ~18ms. Only run on quiet iterations
        // (no key events) to avoid blocking the event loop during typing.
        let now = Instant::now();
        let draw_ready = now.duration_since(last_draw) >= min_draw_interval;
        // Defer draw when typing single-line in Claude prompt (fast-path handles it).
        // Multi-line input needs immediate full draw to resize the input box.
        // Terminal mode needs immediate draws — PTY output has no fast-path.
        let has_fast_path = app.prompt_mode && !app.terminal_mode && !app.input.contains('\n') && !app.has_input_selection();
        let defer_for_typing = had_key_event && has_fast_path;
        let should_draw = app.draw_pending && draw_ready && !defer_for_typing;

        if should_draw {
            // Pre-draw drain: catch events that arrived between the top-of-loop
            // drain and now (~0-5ms gap). If a key arrives here, skip draw.
            let mut got_key = false;
            while event::poll(Duration::from_millis(0))? {
                match event::read()? {
                    Event::Key(key) if matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) && !matches!(key.code, KeyCode::Modifier(_)) => {
                        handle_key_event(key, app, &claude_process)?;
                        got_key = true;
                    }
                    Event::Resize(w, h) => { cached_width = w; cached_height = h; }
                    _ => {}
                }
            }
            if !got_key {
                terminal.draw(|f| ui(f, app))?;
                last_draw = Instant::now();
                app.draw_pending = false;
            }
        }

        if app.should_quit { break; }
    }

    Ok(())
}

/// Fast-path: render ONLY the input box content via direct crossterm writes.
/// Costs ~0.1ms vs ~18ms for terminal.draw(). Used during rapid typing so
/// keystrokes get instant visual feedback while the full UI catches up later.
/// Writes the input text into the cached input_area rect, positions the cursor,
/// and flushes. Ratatui's internal buffer becomes stale but the next full draw
/// will reconcile everything.
fn fast_draw_input(app: &App) {
    let area = app.input_area;
    let inner_width = area.width.saturating_sub(2) as usize;
    let visible_rows = area.height.saturating_sub(2) as usize;
    if inner_width == 0 || visible_rows == 0 { return; }

    // Compute word-wrap break points (identical logic as draw_input.rs)
    let chars: Vec<char> = app.input.chars().collect();
    let breaks = word_wrap_break_points(&chars, inner_width);
    let target = app.input_cursor.min(chars.len());

    // Walk rows from break points to find cursor row + col + build visual lines
    let mut visual_lines: Vec<String> = Vec::new();
    let mut cursor_row = 0usize;
    let mut cursor_col = 0usize;
    let mut prev = 0usize;
    for &bp in &breaks {
        if target >= prev && target < bp {
            cursor_row = visual_lines.len();
            cursor_col = display_width(&chars[prev..target]);
        }
        // Collect row text (exclude trailing newline if any)
        let end = if bp > 0 && chars.get(bp - 1) == Some(&'\n') { bp - 1 } else { bp };
        visual_lines.push(chars[prev..end].iter().collect());
        prev = bp;
    }
    // Final row
    if target >= prev {
        cursor_row = visual_lines.len();
        cursor_col = display_width(&chars[prev..target.min(chars.len())]);
    }
    visual_lines.push(chars[prev..].iter().collect());

    // Scroll offset: keep cursor visible
    let scroll_offset = if cursor_row >= visible_rows {
        cursor_row - visible_rows + 1
    } else { 0 };

    let mut stdout = io::stdout();

    // Write each visible row inside the border (x+1, y+1 = inside border)
    for row_idx in 0..visible_rows {
        let line_idx = scroll_offset + row_idx;
        let text = visual_lines.get(line_idx).map(|s| s.as_str()).unwrap_or("");
        // Pad to inner_width (display columns) to overwrite stale content
        let text_width: usize = text.chars()
            .map(|c| unicode_width::UnicodeWidthChar::width(c).unwrap_or(1))
            .sum();
        let pad = inner_width.saturating_sub(text_width);
        let padded = format!("{}{}", text, " ".repeat(pad));
        let _ = execute!(
            stdout,
            cursor::MoveTo(area.x + 1, area.y + 1 + row_idx as u16),
            style::Print(&padded)
        );
    }

    // Position cursor
    let adjusted_row = cursor_row.saturating_sub(scroll_offset);
    let _ = execute!(
        stdout,
        cursor::MoveTo(
            area.x + 1 + cursor_col as u16,
            area.y + 1 + adjusted_row as u16,
        ),
        cursor::Show
    );
    let _ = stdout.flush();
}

/// Compute visual row for cursor using word-wrap break points (matches draw_input.rs)
fn compute_cursor_row_fast(input: &str, cursor_idx: usize, inner_width: usize) -> usize {
    let chars: Vec<char> = input.chars().collect();
    let target = cursor_idx.min(chars.len());
    let breaks = word_wrap_break_points(&chars, inner_width);
    // Each break point starts a new row; cursor is on row N if target falls in
    // the range [breaks[N-1]..breaks[N]) (with breaks[-1] = 0)
    let mut row = 0usize;
    let mut prev = 0usize;
    for &bp in &breaks {
        if target >= prev && target < bp { return row; }
        row += 1;
        prev = bp;
    }
    row // cursor in final row
}

/// Map a visual (row, col) coordinate back to a char index in the input text.
/// Uses word-wrap break points so clicking and cursor math agree with rendering.
fn row_col_to_char_index(input: &str, target_row: usize, target_col: usize, inner_width: usize) -> usize {
    let chars: Vec<char> = input.chars().collect();
    if chars.is_empty() { return 0; }
    let breaks = word_wrap_break_points(&chars, inner_width);

    // Find the start and end char indices for the target row
    let mut row = 0usize;
    let mut prev = 0usize;
    let mut row_start = 0usize;
    let mut row_end = chars.len();
    let mut found = false;
    for &bp in &breaks {
        if row == target_row { row_start = prev; row_end = bp; found = true; break; }
        row += 1;
        prev = bp;
    }
    // If target_row is the last (or only) row
    if !found {
        if row == target_row { row_start = prev; row_end = chars.len(); }
        else { return chars.len(); } // clicked below content
    }

    // Skip trailing newline from row content (it's not a visible character)
    let content_end = if row_end > row_start && chars.get(row_end - 1) == Some(&'\n') {
        row_end - 1
    } else { row_end };

    // Walk chars in this row until display width reaches or passes target_col
    let mut col_accum = 0usize;
    for i in row_start..content_end {
        if col_accum >= target_col { return i; }
        col_accum += unicode_width::UnicodeWidthChar::width(chars[i]).unwrap_or(1);
    }
    content_end // click past row content → place at row end
}

/// Apply accumulated scroll to the appropriate panel using cached pane rects
fn apply_scroll_cached(app: &mut App, delta: i32, col: u16, row: u16, _term_width: u16, _term_height: u16) -> bool {
    use ratatui::layout::Position;
    let pos = Position::new(col, row);

    if app.pane_worktrees.contains(pos) {
        if app.show_file_tree {
            // File tree overlay is showing — scroll file tree items
            let old = app.file_tree_selected;
            if delta > 0 { for _ in 0..delta.abs() { app.file_tree_next(); } }
            else { for _ in 0..delta.abs() { app.file_tree_prev(); } }
            app.file_tree_selected != old
        } else {
            let old = app.selected_worktree;
            if delta > 0 { for _ in 0..delta.abs() { app.select_next_session(); } }
            else { for _ in 0..delta.abs() { app.select_prev_session(); } }
            app.selected_worktree != old
        }
    } else if app.pane_viewer.contains(pos) {
        app.viewer_selection = None;
        if delta > 0 { app.scroll_viewer_down(delta as usize) }
        else { app.scroll_viewer_up((-delta) as usize) }
    } else if app.terminal_mode && app.input_area.contains(pos) {
        if delta > 0 { app.scroll_terminal_down(delta as usize); }
        else { app.scroll_terminal_up((-delta) as usize); }
        true
    } else if app.pane_convo.contains(pos) {
        // Session list overlay: scroll selected item
        if app.show_session_list {
            let total: usize = app.sessions.iter().map(|s| {
                app.session_files.get(&s.branch_name).map(|f| f.len().max(1)).unwrap_or(1)
            }).sum();
            if delta > 0 {
                app.session_list_selected = (app.session_list_selected + delta as usize).min(total.saturating_sub(1));
            } else {
                app.session_list_selected = app.session_list_selected.saturating_sub((-delta) as usize);
            }
            return true;
        }
        app.output_selection = None;
        if delta > 0 {
            match app.view_mode {
                crate::app::ViewMode::Output => app.scroll_output_down(delta as usize),
                crate::app::ViewMode::Diff => app.scroll_diff_down(delta as usize),
                _ => false
            }
        } else {
            match app.view_mode {
                crate::app::ViewMode::Output => app.scroll_output_up((-delta) as usize),
                crate::app::ViewMode::Diff => app.scroll_diff_up((-delta) as usize),
                _ => false
            }
        }
    } else {
        false
    }
}

/// Handle left-click: focus pane, select items, position input cursor.
/// Returns true if a redraw is needed.
fn handle_mouse_click(app: &mut App, col: u16, row: u16) -> bool {
    use ratatui::layout::Position;
    use crate::app::SidebarRowAction;
    let pos = Position::new(col, row);

    // Overlays first — clicking anywhere dismisses them
    if app.show_help { app.show_help = false; return true; }
    if app.context_menu.is_some() { app.context_menu = None; return true; }
    if app.run_command_picker.is_some() { app.run_command_picker = None; return true; }
    if app.run_command_dialog.is_some() { app.run_command_dialog = None; return true; }
    if app.branch_dialog.is_some() { app.branch_dialog = None; return true; }
    if app.creation_wizard.is_some() { app.creation_wizard = None; app.focus = Focus::Worktrees; return true; }

    // Worktrees/FileTree pane — when file tree overlay is active, click selects entries;
    // otherwise click selects worktrees or their Claude session files
    if app.pane_worktrees.contains(pos) {
        if app.show_file_tree {
            // File tree overlay: click to select, double-click to open/expand
            app.focus = Focus::FileTree;
            let visual_row = (row.saturating_sub(app.pane_worktrees.y + 1)) as usize;
            let entry_idx = visual_row + app.file_tree_scroll;
            if entry_idx < app.file_tree_entries.len() {
                app.file_tree_selected = Some(entry_idx);
                app.invalidate_file_tree();
                let now = std::time::Instant::now();
                let is_double = app.last_click.map_or(false, |(t, c, r)| {
                    c == col && r == row && now.duration_since(t).as_millis() < 500
                });
                if is_double {
                    let entry = &app.file_tree_entries[entry_idx];
                    if entry.is_dir {
                        app.toggle_file_tree_dir();
                    } else {
                        app.load_file_into_viewer();
                        app.focus = Focus::Viewer;
                    }
                }
            }
        } else {
            // Worktree list: click to select worktree or Claude session file
            app.focus = Focus::Worktrees;
            let visual_row = (row.saturating_sub(app.pane_worktrees.y + 1)) as usize;
            if let Some(action) = app.sidebar_row_map.get(visual_row).cloned() {
                match action {
                    SidebarRowAction::Worktree(idx) => {
                        if app.selected_worktree != Some(idx) {
                            app.save_current_terminal();
                            app.selected_worktree = Some(idx);
                            app.load_session_output();
                            app.invalidate_sidebar();
                        }
                    }
                    SidebarRowAction::WorktreeFile(sess_idx, file_idx) => {
                        if app.selected_worktree != Some(sess_idx) {
                            app.save_current_terminal();
                            app.selected_worktree = Some(sess_idx);
                        }
                        if let Some(session) = app.sessions.get(sess_idx) {
                            let branch = session.branch_name.clone();
                            app.select_session_file(&branch, file_idx);
                        }
                    }
                }
            }
        }
        app.last_click = Some((std::time::Instant::now(), col, row));
        return true;
    }

    // Viewer pane — focus, and position edit cursor if in edit mode
    if app.pane_viewer.contains(pos) {
        app.focus = Focus::Viewer;
        if app.viewer_edit_mode {
            if let Some((src_line, src_col)) = screen_to_edit_pos(app, col, row) {
                app.viewer_edit_cursor = (src_line, src_col);
                app.viewer_edit_selection = None;
            }
        }
        app.last_click = Some((std::time::Instant::now(), col, row));
        return true;
    }

    // Convo pane — focus + clickable file path detection
    if app.pane_convo.contains(pos) {
        app.focus = Focus::Output;
        // Check if the click landed on an underlined file path link
        app.clamp_output_scroll();
        if let Some((cache_line, cache_col)) = screen_to_cache_pos(col, row, app.pane_convo, app.output_scroll, app.rendered_lines_cache.len()) {
            // Search clickable_paths for a hit: first line checks column range,
            // continuation lines match anywhere within the wrapped path region
            let hit = app.clickable_paths.iter().find(|(li, sc, ec, _, _, _, wlc)| {
                if cache_line == *li { cache_col >= *sc && cache_col < *ec }
                else { *wlc > 1 && cache_line > *li && cache_line < *li + *wlc }
            }).cloned();
            if let Some((li, sc, ec, file_path, old_s, new_s, wlc)) = hit {
                // Set inverted-color highlight on the clicked path (including wrap count)
                app.clicked_path_highlight = Some((li, sc, ec, wlc));
                // Invalidate viewport cache so highlight is rendered on next draw
                app.output_viewport_scroll = usize::MAX;
                // Edit tool: open file with diff overlay in Viewer
                // Read/Write tool: open file plain in Viewer
                if !old_s.is_empty() || !new_s.is_empty() {
                    // Set selected_tool_diff so ⌥←/⌥→ cycling knows where we are
                    let click_idx = app.clickable_paths.iter().position(|(l, s, e, _, _, _, _)| *l == li && *s == sc && *e == ec);
                    app.selected_tool_diff = click_idx;
                    app.load_file_with_edit_diff(&file_path, &old_s, &new_s);
                } else {
                    app.load_file_at_path(&file_path);
                }
            } else {
                // Clicked somewhere else in convo — clear any previous highlight
                if app.clicked_path_highlight.is_some() {
                    app.clicked_path_highlight = None;
                    app.output_viewport_scroll = usize::MAX;
                }
            }
        }
        app.last_click = Some((std::time::Instant::now(), col, row));
        return true;
    }

    // Input/Terminal pane — enter prompt mode or position cursor
    if app.input_area.contains(pos) {
        if app.terminal_mode {
            // Clicking terminal area — no cursor positioning needed
            return true;
        }
        // Enter prompt mode and position cursor at click point
        if !app.prompt_mode {
            app.prompt_mode = true;
        }
        app.focus = Focus::Input;
        app.input_selection = None;
        click_to_input_cursor(app, col, row);
        app.last_click = Some((std::time::Instant::now(), col, row));
        return true;
    }

    false
}

/// Position the input cursor at the clicked screen coordinates.
/// Uses word-wrap break points (identical to draw_input.rs) to map
/// the clicked (col, row) → char index in the input buffer.
fn click_to_input_cursor(app: &mut App, click_col: u16, click_row: u16) {
    let inner_x = app.input_area.x + 1;
    let inner_y = app.input_area.y + 1;
    let inner_width = (app.input_area.width.saturating_sub(2)) as usize;
    if inner_width == 0 { return; }
    let target_col = (click_col.saturating_sub(inner_x)) as usize;
    let target_row = (click_row.saturating_sub(inner_y)) as usize;

    // Scroll offset so we map screen row → absolute visual row
    let visible_rows = app.input_area.height.saturating_sub(2) as usize;
    let cursor_row_current = compute_cursor_row_fast(&app.input, app.input_cursor, inner_width);
    let scroll_offset = if visible_rows > 0 && cursor_row_current >= visible_rows {
        cursor_row_current - visible_rows + 1
    } else { 0 };
    let actual_row = target_row + scroll_offset;

    app.input_cursor = row_col_to_char_index(&app.input, actual_row, target_col, inner_width);
}

/// Map screen coordinates to a char index in the input text.
/// Uses word-wrap break points to find the exact char at (row, col).
fn screen_to_input_char(app: &App, click_col: u16, click_row: u16) -> usize {
    let inner_x = app.input_area.x + 1;
    let inner_y = app.input_area.y + 1;
    let inner_width = (app.input_area.width.saturating_sub(2)) as usize;
    if inner_width == 0 { return 0; }
    let target_row = (click_row.saturating_sub(inner_y)) as usize;
    let target_col = (click_col.saturating_sub(inner_x)) as usize;
    let visible_rows = app.input_area.height.saturating_sub(2) as usize;
    let cursor_row_current = compute_cursor_row_fast(&app.input, app.input_cursor, inner_width);
    let scroll_offset = if visible_rows > 0 && cursor_row_current >= visible_rows {
        cursor_row_current - visible_rows + 1
    } else { 0 };
    let actual_row = target_row + scroll_offset;
    row_col_to_char_index(&app.input, actual_row, target_col, inner_width)
}

/// Map screen coordinates to (cache_line, cache_col) within a bordered pane.
/// Returns None if outside the content area (inside borders).
fn screen_to_cache_pos(
    screen_col: u16, screen_row: u16,
    pane: ratatui::layout::Rect, scroll: usize, cache_len: usize,
) -> Option<(usize, usize)> {
    // Content sits inside the 1px border on all sides
    let cx = pane.x + 1;
    let cy = pane.y + 1;
    let ch = pane.height.saturating_sub(2) as usize;
    if screen_col < cx || screen_row < cy { return None; }
    let vrow = (screen_row - cy) as usize;
    let col = (screen_col - cx) as usize;
    if vrow >= ch { return None; }
    let line = scroll + vrow;
    if line >= cache_len { return None; }
    Some((line, col))
}

/// Map screen coordinates to (source_line, source_col) in the edit buffer.
/// Walks source lines summing their visual wrap counts to find which source
/// line the clicked visual row falls on, then computes the source column
/// from the wrap segment offset + click column within content area.
fn screen_to_edit_pos(app: &App, screen_col: u16, screen_row: u16) -> Option<(usize, usize)> {
    let pane = app.pane_viewer;
    let cx = pane.x + 1; // inside left border
    let cy = pane.y + 1; // inside top border
    if screen_row < cy || screen_col < cx { return None; }

    let total_lines = app.viewer_edit_content.len();
    let line_num_width = total_lines.to_string().len().max(3);
    let gutter = line_num_width + 3; // "NNN │ " = line_num_width + " │ "
    let cw = app.viewer_edit_content_width.max(1);

    // Click column relative to content area (after gutter)
    let click_x = if (screen_col as usize) >= (cx as usize + gutter) {
        (screen_col as usize) - (cx as usize) - gutter
    } else {
        0
    };
    // Click visual row (absolute, accounting for scroll)
    let visual_row = app.viewer_scroll + (screen_row - cy) as usize;

    // Walk source lines, summing visual line counts, to find which source
    // line the clicked visual row falls on
    let mut running = 0usize;
    for (i, line_str) in app.viewer_edit_content.iter().enumerate() {
        let len = line_str.chars().count();
        let breaks = word_wrap_breaks(line_str, cw);
        let wraps = breaks.len();
        if visual_row < running + wraps {
            // Found it — wrap_seg tells us which visual row within this source line
            let wrap_seg = visual_row - running;
            // Convert click_x to a char offset within the source line using break positions
            let row_start = breaks[wrap_seg];
            let row_end = if wrap_seg + 1 < breaks.len() { breaks[wrap_seg + 1] } else { len };
            let src_col = (row_start + click_x).min(row_end);
            return Some((i, src_col));
        }
        running += wraps;
    }
    // Click is past last line — place at end of last line
    if !app.viewer_edit_content.is_empty() {
        let last = total_lines - 1;
        let last_len = app.viewer_edit_content[last].chars().count();
        return Some((last, last_len));
    }
    None
}

/// Handle mouse drag: compute text selection from drag anchor to current position.
/// Drag anchor is stored in cache coordinates (computed on MouseDown) so
/// auto-scroll during drag doesn't shift the start point.
fn handle_mouse_drag(app: &mut App, col: u16, row: u16) -> bool {
    let Some((anchor_line, anchor_col, pane_id)) = app.mouse_drag_start else { return false };

    match pane_id {
        // --- Input pane: anchor_line = char index, anchor_col unused ---
        2 => {
            let end_idx = screen_to_input_char(app, col, row);
            if anchor_line != end_idx {
                let new_sel = Some((anchor_line, end_idx));
                if app.input_selection != new_sel {
                    app.input_selection = new_sel;
                    app.input_cursor = end_idx;
                    return true;
                }
            }
            false
        }
        // --- Viewer pane: anchor = (cache_line, cache_col) ---
        0 => {
            // Auto-scroll when dragging above/below pane
            if row < app.pane_viewer.y + 1 { app.scroll_viewer_up(1); }
            else if row >= app.pane_viewer.y + app.pane_viewer.height.saturating_sub(1) { app.scroll_viewer_down(1); }
            // Clamp end to pane content bounds
            let ec = col.max(app.pane_viewer.x + 1).min(app.pane_viewer.x + app.pane_viewer.width.saturating_sub(1));
            let er = row.max(app.pane_viewer.y + 1).min(app.pane_viewer.y + app.pane_viewer.height.saturating_sub(1));
            let Some((el, ecc)) = screen_to_cache_pos(ec, er, app.pane_viewer, app.viewer_scroll, app.viewer_lines_cache.len()) else { return false };
            // Normalize so start <= end (anchor is always the fixed point)
            let sel = if anchor_line < el || (anchor_line == el && anchor_col <= ecc) {
                (anchor_line, anchor_col, el, ecc)
            } else {
                (el, ecc, anchor_line, anchor_col)
            };
            let new = Some(sel);
            if app.viewer_selection != new { app.viewer_selection = new; return true; }
            false
        }
        // --- Convo pane: anchor = (cache_line, cache_col) ---
        1 => {
            app.clamp_output_scroll();
            // Auto-scroll when dragging above/below pane
            if row < app.pane_convo.y + 1 { app.scroll_output_up(1); }
            else if row >= app.pane_convo.y + app.pane_convo.height.saturating_sub(1) { app.scroll_output_down(1); }
            let ec = col.max(app.pane_convo.x + 1).min(app.pane_convo.x + app.pane_convo.width.saturating_sub(1));
            let er = row.max(app.pane_convo.y + 1).min(app.pane_convo.y + app.pane_convo.height.saturating_sub(1));
            let Some((el, ecc)) = screen_to_cache_pos(ec, er, app.pane_convo, app.output_scroll, app.rendered_lines_cache.len()) else { return false };
            let sel = if anchor_line < el || (anchor_line == el && anchor_col <= ecc) {
                (anchor_line, anchor_col, el, ecc)
            } else {
                (el, ecc, anchor_line, anchor_col)
            };
            let new = Some(sel);
            if app.output_selection != new {
                app.output_selection = new;
                app.output_viewport_scroll = usize::MAX;
                return true;
            }
            false
        }
        // --- Edit mode viewer: anchor = (source_line, source_col) ---
        3 => {
            // Auto-scroll when dragging above/below pane
            if row < app.pane_viewer.y + 1 { app.scroll_viewer_up(1); }
            else if row >= app.pane_viewer.y + app.pane_viewer.height.saturating_sub(1) { app.scroll_viewer_down(1); }
            let Some((el, ec)) = screen_to_edit_pos(app, col, row) else { return false };
            // Update edit selection from anchor to current drag position
            let new = Some((anchor_line, anchor_col, el, ec));
            if app.viewer_edit_selection != new {
                app.viewer_edit_selection = new;
                app.viewer_edit_cursor = (el, ec);
                return true;
            }
            false
        }
        _ => false,
    }
}

/// Extract plain text from a slice of ratatui Lines within a selection range.
/// Lines are joined with newlines. First/last line use column offsets.
/// `gutter` chars are stripped from the start of every line (for viewer line numbers).
fn extract_text_from_cache(
    cache: &[ratatui::text::Line],
    sl: usize, sc: usize, el: usize, ec: usize,
    gutter: usize,
) -> String {
    let mut out = String::new();
    for idx in sl..=el {
        let Some(line) = cache.get(idx) else { continue };
        let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        let chars: Vec<char> = text.chars().collect();
        // Shift columns past the gutter so line numbers are never copied
        let start = if idx == sl { sc.max(gutter) } else { gutter };
        let end = if idx == el { ec.max(gutter) } else { chars.len() };
        if start < end && start < chars.len() {
            out.extend(&chars[start..end.min(chars.len())]);
        }
        if idx < el { out.push('\n'); }
    }
    out
}

/// Copy text selected in the viewer pane to clipboard.
/// Strips line number gutter (first span per line) when viewer is in File mode,
/// so copied text contains only file content — no "  1 │ " prefixes.
fn copy_viewer_selection(app: &mut App) {
    let Some((sl, sc, el, ec)) = app.viewer_selection else { return };
    // In File mode, first span of each cache line is the line number gutter
    let gutter = if app.viewer_mode == crate::app::ViewerMode::File {
        app.viewer_lines_cache.first()
            .and_then(|l| l.spans.first())
            .map(|s| s.content.chars().count())
            .unwrap_or(0)
    } else { 0 };
    let text = extract_text_from_cache(&app.viewer_lines_cache, sl, sc, el, ec, gutter);
    if text.is_empty() { return; }
    if let Ok(mut cb) = arboard::Clipboard::new() { let _ = cb.set_text(&text); }
    app.clipboard = text;
    app.set_status("Copied to clipboard");
}

/// Copy text selected in the convo pane to clipboard
fn copy_output_selection(app: &mut App) {
    let Some((sl, sc, el, ec)) = app.output_selection else { return };
    let text = extract_text_from_cache(&app.rendered_lines_cache, sl, sc, el, ec, 0);
    if text.is_empty() { return; }
    if let Ok(mut cb) = arboard::Clipboard::new() { let _ = cb.set_text(&text); }
    app.clipboard = text;
    app.set_status("Copied to clipboard");
}

/// Handle Claude process events for a specific session.
/// After an exit event, auto-sends any staged prompt (user hit Enter mid-convo
/// which cancelled the old run and staged the new prompt in one keystroke).
fn handle_claude_event(session_id: &str, event: ClaudeEvent, app: &mut App, claude_process: &ClaudeProcess) -> Result<()> {
    let is_exit = matches!(event, ClaudeEvent::Exited { .. });
    match event {
        ClaudeEvent::Output(output) => app.handle_claude_output(session_id, output.output_type, output.data),
        ClaudeEvent::Started { pid } => app.handle_claude_started(session_id, pid),
        ClaudeEvent::SessionId(claude_session_id) => app.set_claude_session_id(session_id, claude_session_id),
        ClaudeEvent::Exited { code } => app.handle_claude_exited(session_id, code),
        ClaudeEvent::Error(e) => app.handle_claude_error(session_id, e),
    }

    // Auto-send staged prompt after Claude exits — no second Enter needed.
    // CRITICAL: force a session file re-parse BEFORE spawning the new process.
    // handle_claude_exited() sets parse_offset=0 + dirty=true, but once the new
    // process starts, is_current_session_running() returns true and poll_session_file()
    // skips the parse. Without this, user messages and responses from the previous
    // turn never get loaded from the JSONL (they only existed as live-stream events
    // which were cleared), causing messages to vanish.
    if is_exit {
        if app.staged_prompt.is_some() {
            // Session is NOT running right now (just exited) — parse will succeed
            app.check_session_file();
            app.poll_session_file();
        }
        if let Some(prompt) = app.staged_prompt.take() {
            if let Some(wt_path) = app.current_session().and_then(|s| s.worktree_path.clone()) {
                let branch = app.current_session().map(|s| s.branch_name.clone()).unwrap_or_default();
                app.add_user_message(prompt.clone());
                app.process_output_chunk(&format!("You: {}\n", prompt));
                app.current_todos.clear();
                let resume_id = app.get_claude_session_id(&branch).cloned();
                match claude_process.spawn(&wt_path, &prompt, resume_id.as_deref()) {
                    Ok(rx) => { app.register_claude(branch, rx); app.set_status("Running..."); }
                    Err(e) => app.set_status(format!("Failed to start: {}", e)),
                }
            }
        }
    }
    Ok(())
}

/// Handle keyboard input events.
/// All key → action resolution goes through lookup_action() in keybindings.rs.
/// Modal overlays (help, context menu, wizard, dialogs) bypass this and consume
/// all input directly. Focus-specific handlers only see keys that lookup_action()
/// didn't resolve (text input, dialog nav, etc.).
fn handle_key_event(key: event::KeyEvent, app: &mut App, claude_process: &ClaudeProcess) -> Result<()> {
    // Bare modifier presses (Shift, Ctrl, Alt) arrive via Kitty protocol — ignore globally
    if matches!(key.code, KeyCode::Modifier(_)) { return Ok(()); }

    // --- Modal overlays consume ALL input (bypass keybinding system) ---

    // Help overlay: only ? and Esc close it, everything else ignored
    if app.show_help {
        match key.code {
            KeyCode::Char('?') | KeyCode::Esc => app.toggle_help(),
            _ => {}
        }
        return Ok(());
    }

    // Full-screen modals that intercept everything
    if app.context_menu.is_some() { return handle_context_menu_input(key, app, claude_process); }
    if app.is_projects_panel_active() { return handle_projects_input(key, app); }
    if app.is_wizard_active() { handle_wizard_input(app, key, claude_process); return Ok(()); }
    if app.run_command_picker.is_some() { return handle_run_command_picker_input(key, app); }
    if app.run_command_dialog.is_some() { return handle_run_command_dialog_input(key, app, &claude_process); }

    // --- Centralized keybinding resolution ---
    // Build context from app state, resolve key once, dispatch action.
    // Input/terminal handlers and dialog handlers own their own key execution —
    // lookup_action() resolves their bindings for help/title display, but the
    // actual execution stays in the handlers (Submit needs claude_process, text
    // editing is tightly coupled, etc.). Only global + navigation + focus-specific
    // COMMAND bindings go through execute_action().
    let ctx = KeyContext::from_app(app);
    if let Some(action) = lookup_action(&ctx, key.modifiers, key.code) {
        // Input-specific actions: let the input handler execute them (it has
        // the full context: claude_process, plan approval state, etc.)
        let is_input_action = matches!(action,
            Action::Submit | Action::InsertNewline | Action::ExitPromptMode
            | Action::WordLeft | Action::WordRight | Action::DeleteWord
            | Action::ClearInput | Action::HistoryPrev | Action::HistoryNext
            | Action::ToggleStt | Action::EnterTerminalType
        ) && matches!(app.focus, Focus::Input);
        if !is_input_action {
            return execute_action(action, app, claude_process);
        }
    }

    // --- Fallthrough: focus-specific handlers for text input / unresolved keys ---
    // Input handlers also process their own resolved bindings (Submit, word nav, etc.)
    match app.focus {
        Focus::Worktrees => handle_worktrees_input(key, app)?,
        Focus::FileTree => handle_file_tree_input(key, app)?,
        Focus::Viewer => handle_viewer_input(key, app)?,
        Focus::Output => handle_output_input(key, app)?,
        Focus::Input => handle_input_mode(key, app, claude_process)?,
        Focus::WorktreeCreation => handle_worktree_creation_input(key, app, claude_process)?,
        Focus::BranchDialog => handle_branch_dialog_input(key, app)?,
    }

    Ok(())
}

/// Execute a resolved keybinding action. Called by handle_key_event() after
/// lookup_action() identifies WHAT to do. This function handles the HOW.
fn execute_action(action: Action, app: &mut App, _claude_process: &ClaudeProcess) -> Result<()> {
    match action {
        // --- Global actions ---
        Action::Quit => { app.should_quit = true; }
        Action::Restart => { app.should_restart = true; app.should_quit = true; }
        Action::DumpDebug => { app.dump_debug_output(); }
        Action::CancelClaude => { app.cancel_current_claude(); }
        Action::CopySelection => {
            // Copy from whichever pane has an active selection
            if app.prompt_mode && app.has_input_selection() {
                app.input_copy();
            } else if app.viewer_selection.is_some() {
                copy_viewer_selection(app);
            } else if app.output_selection.is_some() {
                copy_output_selection(app);
            }
        }
        Action::ToggleHelp => { app.toggle_help(); }
        Action::EnterPromptMode => {
            app.show_help = false;
            if app.terminal_mode { app.close_terminal(); }
            app.focus = Focus::Input;
            app.prompt_mode = true;
        }
        Action::ToggleTerminal => {
            app.show_help = false;
            app.toggle_terminal();
            app.focus = Focus::Input;
        }
        Action::CycleFocusForward => {
            // Clear sidebar filter on focus change
            if app.sidebar_filter_active || !app.sidebar_filter.is_empty() {
                app.sidebar_filter.clear();
                app.sidebar_filter_active = false;
                app.invalidate_sidebar();
            }
            app.prompt_mode = false;
            app.viewer_selection = None;
            app.output_selection = None;
            app.focus_next();
        }
        Action::CycleFocusBackward => {
            if app.sidebar_filter_active || !app.sidebar_filter.is_empty() {
                app.sidebar_filter.clear();
                app.sidebar_filter_active = false;
                app.invalidate_sidebar();
            }
            app.prompt_mode = false;
            app.viewer_selection = None;
            app.output_selection = None;
            app.focus_prev();
        }

        // --- Wizard actions ---
        Action::WizardNextTab => {
            if let Some(wizard) = app.creation_wizard.as_mut() { wizard.next_tab(); }
        }
        Action::WizardPrevTab => {
            if let Some(wizard) = app.creation_wizard.as_mut() { wizard.prev_tab(); }
        }
        // WizardNextField: wizard intercepts all input before lookup_action() runs,
        // so this arm never fires. Exists only for help text generation.
        Action::WizardNextField => {}

        // --- Terminal resize (global when terminal is open) ---
        Action::ResizeUp => { app.adjust_terminal_height(2); }
        Action::ResizeDown => { app.adjust_terminal_height(-2); }

        // --- All other actions are focus-specific; dispatch inline ---
        // Worktrees
        Action::ToggleFileTree => {
            if app.current_session().and_then(|s| s.worktree_path.as_ref()).is_some() {
                app.show_file_tree = true;
                app.focus = Focus::FileTree;
                app.load_file_tree();
                app.invalidate_file_tree();
            } else {
                app.set_status("No worktree path available");
            }
        }
        Action::EnterInputMode => {
            if app.is_current_session_running() {
                app.focus = Focus::Input;
                app.set_status("Enter input to send to Claude:");
            } else {
                app.set_status("No Claude running in this session");
            }
        }
        Action::ReturnToWorktrees => {
            app.show_file_tree = false;
            app.focus = Focus::Worktrees;
            app.invalidate_sidebar();
        }
        Action::ToggleSessionList => {
            open_session_list(app);
        }

        // --- Viewer tab management ---
        Action::ViewerTabCurrent => { app.viewer_tab_current(); }
        Action::ViewerOpenTabDialog => {
            if !app.viewer_tabs.is_empty() { app.toggle_viewer_tab_dialog(); }
        }
        Action::ViewerNextTab => { app.viewer_next_tab(); }
        Action::ViewerPrevTab => { app.viewer_prev_tab(); }
        Action::ViewerCloseTab => { app.viewer_close_current_tab(); }
        Action::SelectAll => {
            // Read-only viewer: select entire cache. Edit mode: select all edit content.
            if app.viewer_edit_mode {
                app.viewer_edit_select_all();
            } else {
                let last = app.viewer_lines_cache.len().saturating_sub(1);
                let last_col = app.viewer_lines_cache.last()
                    .map(|l| l.spans.iter().map(|s| s.content.chars().count()).sum::<usize>())
                    .unwrap_or(0);
                app.viewer_selection = Some((0, 0, last, last_col));
            }
        }

        // --- Viewer navigation ---
        Action::EnterEditMode => {
            if app.viewer_path.is_some() { app.enter_viewer_edit_mode(); }
        }
        Action::JumpNextEdit => { jump_edit(app, true); }
        Action::JumpPrevEdit => { jump_edit(app, false); }

        // --- Viewer edit mode ---
        Action::Save => {
            match app.save_viewer_edits() {
                Ok(()) => {
                    app.set_status("File saved");
                    if app.viewer_edit_diff.is_some() {
                        app.viewer_edit_save_dialog = true;
                    }
                }
                Err(e) => app.set_status(format!("Save failed: {}", e)),
            }
        }
        Action::Undo => { app.viewer_edit_undo(); }
        Action::Redo => { app.viewer_edit_redo(); }

        // --- Shared navigation (used by viewer, output, worktrees, file tree, terminal) ---
        Action::NavDown => { dispatch_nav_down(app); }
        Action::NavUp => { dispatch_nav_up(app); }
        Action::NavLeft => { dispatch_nav_left(app); }
        Action::NavRight => { dispatch_nav_right(app); }
        Action::PageDown => { dispatch_page_down(app); }
        Action::PageUp => { dispatch_page_up(app); }
        Action::GoToTop => { dispatch_go_to_top(app); }
        Action::GoToBottom => { dispatch_go_to_bottom(app); }

        // --- Worktree-specific ---
        Action::SearchFilter => {
            app.sidebar_filter_active = true;
            app.sidebar_filter.clear();
            app.invalidate_sidebar();
        }
        // Project jumping — currently single-project mode, no-op until multi-project navigation exists
        Action::SelectNextProject | Action::SelectPrevProject => {}
        Action::OpenContextMenu => {
            app.open_context_menu();
        }
        Action::NewWorktree => {
            app.start_wizard();
        }
        Action::BrowseBranches => {
            if let Some(project) = app.current_project() {
                match crate::git::Git::list_available_branches(&project.path) {
                    Ok(branches) => app.open_branch_dialog(branches),
                    Err(e) => app.set_status(format!("Failed to list branches: {}", e)),
                }
            }
        }
        Action::ViewDiff => {
            if let Err(e) = app.load_diff() {
                app.set_status(format!("Failed to get diff: {}", e));
            } else if app.focus == Focus::Output {
                app.diff_scroll = 0;
            }
        }
        Action::RunCommand => { app.open_run_command_picker(); }
        Action::AddRunCommand => { app.open_run_command_dialog(); }
        Action::RebaseOntoMain => {
            rebase_current(app);
        }
        Action::ArchiveWorktree => {
            if let Err(e) = app.archive_current_session() {
                app.set_status(format!("Failed to archive: {}", e));
            }
        }
        Action::StartResume => {
            start_or_resume(app);
        }
        Action::OpenProjects => {
            app.open_projects_panel();
        }

        // --- FileTree ---
        Action::ToggleDir => {
            if let Some(idx) = app.file_tree_selected {
                if let Some(entry) = app.file_tree_entries.get(idx) {
                    if entry.is_dir { app.toggle_file_tree_dir(); }
                }
            }
        }
        Action::OpenFile => {
            if let Some(idx) = app.file_tree_selected {
                if let Some(entry) = app.file_tree_entries.get(idx) {
                    if entry.is_dir {
                        app.toggle_file_tree_dir();
                    } else {
                        app.load_file_into_viewer();
                        app.focus = Focus::Viewer;
                    }
                }
            }
        }
        Action::AddFile => {
            app.file_tree_action = Some(crate::app::types::FileTreeAction::Add(String::new()));
        }
        Action::DeleteFile => {
            if app.file_tree_selected.is_some() {
                app.file_tree_action = Some(crate::app::types::FileTreeAction::Delete);
            }
        }
        Action::RenameFile => {
            if let Some(idx) = app.file_tree_selected {
                if let Some(entry) = app.file_tree_entries.get(idx) {
                    app.file_tree_action = Some(crate::app::types::FileTreeAction::Rename(entry.name.clone()));
                }
            }
        }
        Action::CopyFile => {
            if let Some(idx) = app.file_tree_selected {
                if let Some(entry) = app.file_tree_entries.get(idx) {
                    app.file_tree_action = Some(crate::app::types::FileTreeAction::Copy(entry.path.clone()));
                    app.set_status("Copy: select target dir, Enter to paste");
                    app.invalidate_file_tree();
                }
            }
        }
        Action::MoveFile => {
            if let Some(idx) = app.file_tree_selected {
                if let Some(entry) = app.file_tree_entries.get(idx) {
                    app.file_tree_action = Some(crate::app::types::FileTreeAction::Move(entry.path.clone()));
                    app.set_status("Move: select target dir, Enter to paste");
                    app.invalidate_file_tree();
                }
            }
        }

        // --- Output/Convo ---
        Action::JumpNextBubble => {
            if app.view_mode == crate::app::ViewMode::Output { app.jump_to_next_bubble(false); }
        }
        Action::JumpPrevBubble => {
            if app.view_mode == crate::app::ViewMode::Output { app.jump_to_prev_bubble(false); }
        }
        Action::JumpNextMessage => {
            if app.view_mode == crate::app::ViewMode::Output { app.jump_to_next_bubble(true); }
        }
        Action::JumpPrevMessage => {
            if app.view_mode == crate::app::ViewMode::Output { app.jump_to_prev_bubble(true); }
        }
        Action::SwitchToOutput => {
            app.view_mode = crate::app::ViewMode::Output;
            app.output_scroll = usize::MAX;
        }
        Action::RebaseStatus => {
            if let Some(session) = app.current_session() {
                if let Some(ref wt_path) = session.worktree_path {
                    if crate::git::Git::is_rebase_in_progress(wt_path) {
                        if let Ok(status) = crate::git::Git::get_rebase_status(wt_path) {
                            app.set_rebase_status(status);
                        }
                    }
                }
            }
        }

        // --- Input/Terminal actions: handled by their own handlers (skip here) ---
        // These are filtered out in handle_key_event() and fall through to
        // handle_input_mode(). Listed here for exhaustive match.
        Action::Submit | Action::InsertNewline | Action::ExitPromptMode
        | Action::WordLeft | Action::WordRight | Action::DeleteWord
        | Action::ClearInput | Action::HistoryPrev | Action::HistoryNext
        | Action::ToggleStt | Action::EnterTerminalType => {}

        // --- Generic escape: context-dependent close/back ---
        Action::Escape => {
            dispatch_escape(app);
        }

        // --- Dialog actions (not reached here — modals intercept above) ---
        Action::Confirm | Action::Cancel | Action::DeleteSelected | Action::EditSelected => {}
    }

    Ok(())
}

/// Navigation dispatch — routes NavDown to the correct pane handler
fn dispatch_nav_down(app: &mut App) {
    match app.focus {
        Focus::Viewer => { app.scroll_viewer_down(1); }
        Focus::Output => {
            match app.view_mode {
                crate::app::ViewMode::Output => { app.scroll_output_down(1); }
                crate::app::ViewMode::Diff => { app.scroll_diff_down(1); }
                _ => {}
            }
        }
        Focus::Worktrees => {
            if app.is_current_worktree_expanded() { app.session_file_next(); }
            else { app.select_next_session(); }
        }
        Focus::FileTree => app.file_tree_next(),
        Focus::Input if app.terminal_mode && !app.prompt_mode => {
            app.scroll_terminal_down(1);
        }
        _ => {}
    }
}

fn dispatch_nav_up(app: &mut App) {
    match app.focus {
        Focus::Viewer => { app.scroll_viewer_up(1); }
        Focus::Output => {
            match app.view_mode {
                crate::app::ViewMode::Output => { app.scroll_output_up(1); }
                crate::app::ViewMode::Diff => { app.scroll_diff_up(1); }
                _ => {}
            }
        }
        Focus::Worktrees => {
            if app.is_current_worktree_expanded() { app.session_file_prev(); }
            else { app.select_prev_session(); }
        }
        Focus::FileTree => app.file_tree_prev(),
        Focus::Input if app.terminal_mode && !app.prompt_mode => {
            app.scroll_terminal_up(1);
        }
        _ => {}
    }
}

fn dispatch_nav_left(app: &mut App) {
    match app.focus {
        Focus::Worktrees => {
            if app.is_current_worktree_expanded() {
                if let Some(session) = app.current_session() {
                    let branch = session.branch_name.clone();
                    app.collapse_worktree(&branch);
                }
            }
        }
        Focus::FileTree => {
            if let Some(idx) = app.file_tree_selected {
                if let Some(entry) = app.file_tree_entries.get(idx).cloned() {
                    if entry.is_dir && app.file_tree_expanded.contains(&entry.path) {
                        app.toggle_file_tree_dir();
                    } else if let Some(parent) = entry.path.parent() {
                        let parent_path = parent.to_path_buf();
                        if let Some(pi) = app.file_tree_entries.iter().position(|e| e.path == parent_path && e.is_dir) {
                            if app.file_tree_expanded.contains(&parent_path) {
                                app.file_tree_selected = Some(pi);
                                app.toggle_file_tree_dir();
                            }
                        }
                    }
                }
            }
        }
        _ => {}
    }
}

fn dispatch_nav_right(app: &mut App) {
    match app.focus {
        Focus::Worktrees => {
            if !app.is_current_worktree_expanded() {
                if let Some(session) = app.current_session() {
                    let branch = session.branch_name.clone();
                    app.expand_worktree(&branch);
                }
            }
        }
        Focus::FileTree => {
            if let Some(idx) = app.file_tree_selected {
                if let Some(entry) = app.file_tree_entries.get(idx).cloned() {
                    if entry.is_dir && !app.file_tree_expanded.contains(&entry.path) {
                        app.toggle_file_tree_dir();
                    }
                }
            }
        }
        _ => {}
    }
}

fn dispatch_page_down(app: &mut App) {
    match app.focus {
        Focus::Viewer => { app.scroll_viewer_down(app.viewer_viewport_height.saturating_sub(2)); }
        Focus::Output => {
            let page = app.output_viewport_height.saturating_sub(2);
            match app.view_mode {
                crate::app::ViewMode::Output => { app.scroll_output_down(page); }
                crate::app::ViewMode::Diff => { app.scroll_diff_down(page); }
                _ => {}
            }
        }
        Focus::Input if app.terminal_mode && !app.prompt_mode => {
            app.scroll_terminal_down((app.terminal_height as usize).saturating_sub(2));
        }
        _ => {}
    }
}

fn dispatch_page_up(app: &mut App) {
    match app.focus {
        Focus::Viewer => { app.scroll_viewer_up(app.viewer_viewport_height.saturating_sub(2)); }
        Focus::Output => {
            let page = app.output_viewport_height.saturating_sub(2);
            match app.view_mode {
                crate::app::ViewMode::Output => { app.scroll_output_up(page); }
                crate::app::ViewMode::Diff => { app.scroll_diff_up(page); }
                _ => {}
            }
        }
        Focus::Input if app.terminal_mode && !app.prompt_mode => {
            app.scroll_terminal_up((app.terminal_height as usize).saturating_sub(2));
        }
        _ => {}
    }
}

fn dispatch_go_to_top(app: &mut App) {
    match app.focus {
        Focus::Viewer => app.viewer_scroll = 0,
        Focus::Output => {
            match app.view_mode {
                crate::app::ViewMode::Output => app.output_scroll = 0,
                crate::app::ViewMode::Diff => app.diff_scroll = 0,
                _ => {}
            }
        }
        Focus::Worktrees => {
            if app.is_current_worktree_expanded() { app.session_file_first(); }
            else { app.select_first_session(); }
        }
        Focus::FileTree => app.file_tree_first_sibling(),
        Focus::Input if app.terminal_mode && !app.prompt_mode => {
            app.terminal_scroll = 0;
        }
        _ => {}
    }
}

fn dispatch_go_to_bottom(app: &mut App) {
    match app.focus {
        Focus::Viewer => app.scroll_viewer_to_bottom(),
        Focus::Output => {
            match app.view_mode {
                crate::app::ViewMode::Output => app.scroll_output_to_bottom(),
                crate::app::ViewMode::Diff => app.scroll_diff_to_bottom(),
                _ => {}
            }
        }
        Focus::Worktrees => {
            if app.is_current_worktree_expanded() { app.session_file_last(); }
            else { app.select_last_session(); }
        }
        Focus::FileTree => app.file_tree_last_sibling(),
        Focus::Input if app.terminal_mode && !app.prompt_mode => {
            app.scroll_terminal_to_bottom();
        }
        _ => {}
    }
}

/// Escape dispatch — context-dependent close/back
fn dispatch_escape(app: &mut App) {
    match app.focus {
        Focus::Viewer if app.viewer_edit_mode => {
            if app.viewer_edit_dirty {
                app.viewer_edit_discard_dialog = true;
            } else {
                app.exit_viewer_edit_mode();
            }
        }
        Focus::Viewer => {
            // Close viewer / close diff overlay
            if app.viewer_edit_diff.is_some() {
                if let Some((prev_content, prev_path, prev_scroll)) = app.viewer_prev_state.take() {
                    app.viewer_content = prev_content;
                    app.viewer_path = prev_path;
                    app.viewer_scroll = prev_scroll;
                    app.viewer_mode = if app.viewer_content.is_some() {
                        crate::app::ViewerMode::File
                    } else {
                        crate::app::ViewerMode::Empty
                    };
                } else {
                    app.clear_viewer();
                }
                app.viewer_edit_diff = None;
                app.viewer_edit_diff_line = None;
                app.selected_tool_diff = None;
                app.viewer_lines_dirty = true;
                app.focus = Focus::FileTree;
            } else {
                app.clear_viewer();
                app.focus = Focus::FileTree;
            }
        }
        Focus::FileTree => {
            app.show_file_tree = false;
            app.focus = Focus::Worktrees;
            app.invalidate_sidebar();
        }
        Focus::Output => app.focus = Focus::Worktrees,
        Focus::Input if app.terminal_mode && !app.prompt_mode => {
            app.close_terminal();
        }
        Focus::Input if app.prompt_mode => {
            app.prompt_mode = false;
        }
        _ => {}
    }
}

/// Jump to next/prev Edit tool entry in the clickable paths list
fn jump_edit(app: &mut App, forward: bool) {
    let edits: Vec<usize> = app.clickable_paths.iter().enumerate()
        .filter(|(_, (_, _, _, _, o, n, _))| !o.is_empty() || !n.is_empty())
        .map(|(i, _)| i).collect();
    if edits.is_empty() { return; }
    let cur = app.selected_tool_diff.and_then(|s| edits.iter().position(|&e| e >= s));
    let target = if forward {
        match cur { Some(pos) => (pos + 1) % edits.len(), None => 0 }
    } else {
        match cur { Some(0) | None => edits.len() - 1, Some(pos) => pos - 1 }
    };
    let idx = edits[target];
    app.selected_tool_diff = Some(idx);
    if let Some((line_idx, sc, ec, file_path, old_str, new_str, wlc)) = app.clickable_paths.get(idx).cloned() {
        app.clicked_path_highlight = Some((line_idx, sc, ec, wlc));
        app.output_viewport_scroll = usize::MAX;
        app.load_file_with_edit_diff(&file_path, &old_str, &new_str);
        app.output_scroll = line_idx.saturating_sub(3);
    }
}

/// Open session list overlay (moved from input_output.rs for central dispatch)
fn open_session_list(app: &mut App) {
    app.show_session_list = true;
    app.session_list_selected = 0;
    app.session_list_scroll = 0;
    for session in &app.sessions {
        if !app.session_files.contains_key(&session.branch_name) {
            if let Some(ref wt_path) = session.worktree_path {
                let files = crate::config::list_claude_sessions(wt_path);
                app.session_files.insert(session.branch_name.clone(), files);
            }
        }
    }
    for files in app.session_files.values() {
        for (session_id, path, _) in files.iter() {
            if !app.session_msg_counts.contains_key(session_id) {
                let count = count_messages_in_jsonl(path);
                app.session_msg_counts.insert(session_id.clone(), count);
            }
        }
    }
}

/// Count human+assistant messages in a JSONL session file
fn count_messages_in_jsonl(path: &std::path::Path) -> usize {
    let Ok(content) = std::fs::read_to_string(path) else { return 0; };
    content.lines().filter(|line| {
        line.contains("\"type\":\"human\"") || line.contains("\"type\":\"assistant\"")
            || line.contains("\"type\": \"human\"") || line.contains("\"type\": \"assistant\"")
    }).count()
}

/// Rebase current worktree onto main
fn rebase_current(app: &mut App) {
    use crate::models::RebaseResult;
    if let Some(session) = app.current_session() {
        if let (Some(ref wt_path), Some(project)) = (&session.worktree_path, app.current_project()) {
            let wt = wt_path.clone();
            let main_branch = project.main_branch.clone();
            match crate::git::Git::rebase_onto_main(&wt, &main_branch) {
                Ok(RebaseResult::Success) => {
                    app.set_status("Rebase completed successfully");
                    app.clear_rebase_status();
                }
                Ok(RebaseResult::UpToDate) => app.set_status("Already up to date"),
                Ok(RebaseResult::Conflicts(status)) => {
                    let n = status.conflicted_files.len();
                    app.set_rebase_status(status);
                    app.set_status(format!("Rebase conflicts: {} file(s)", n));
                }
                Ok(RebaseResult::Aborted) => {
                    app.set_status("Rebase was aborted");
                    app.clear_rebase_status();
                }
                Ok(RebaseResult::Failed(e)) => app.set_status(format!("Rebase failed: {}", e)),
                Err(e) => app.set_status(format!("Rebase error: {}", e)),
            }
        } else {
            app.set_status("No worktree path available");
        }
    }
}

/// Start or resume a Claude session from worktrees Enter key
fn start_or_resume(app: &mut App) {
    use crate::models::SessionStatus;
    let is_expanded = app.is_current_worktree_expanded();
    if is_expanded {
        if let Some(session) = app.current_session() {
            let branch = session.branch_name.clone();
            let idx = *app.session_selected_file_idx.get(&branch).unwrap_or(&0);
            app.select_session_file(&branch, idx);
            app.collapse_worktree(&branch);
            app.set_status("Loaded selected session file");
        }
    } else if let Some(session) = app.current_session() {
        let status = session.status(&app.running_sessions);
        if matches!(status, SessionStatus::Pending | SessionStatus::Stopped
            | SessionStatus::Completed | SessionStatus::Failed | SessionStatus::Waiting)
        {
            app.focus = Focus::Input;
            app.prompt_mode = true;
            app.set_status("Type your prompt and press Enter to send");
        }
    }
}
