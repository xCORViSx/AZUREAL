//! Core event loop and event handling

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers, MouseButton, MouseEventKind};
use crossterm::{cursor, execute, style};
use std::io::{self, Write};
use std::time::{Duration, Instant};

use ratatui::{backend::CrosstermBackend, Terminal};

use crate::app::{App, Focus};
use crate::claude::{ClaudeEvent, ClaudeProcess};
use crate::config::Config;

use super::input_dialogs::{handle_branch_dialog_input, handle_context_menu_input, handle_run_command_picker_input, handle_run_command_dialog_input};
use super::input_file_tree::handle_file_tree_input;
use super::input_output::handle_output_input;
use super::input_worktrees::handle_worktrees_input;
use super::input_terminal::{handle_input_mode, handle_worktree_creation_input};
use super::input_viewer::handle_viewer_input;
use super::input_wizard::handle_wizard_input;
use super::draw_output::{submit_render_request, poll_render_result};
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
        let poll_ms = if app.draw_pending || app.render_in_flight || !app.claude_receivers.is_empty() { 16 } else { 100 };
        if event::poll(Duration::from_millis(poll_ms))? {
            // Drain all available events without blocking
            loop {
                match event::read()? {
                    Event::Key(key) => {
                        if key.kind == KeyEventKind::Press {
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
                            // Left click: focus pane, select item, position cursor
                            MouseEventKind::Down(MouseButton::Left) => {
                                if handle_mouse_click(app, mouse.column, mouse.row) {
                                    needs_redraw = true;
                                }
                            }
                            _ => {} // Discard motion, right-click, drag
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

        // Process Claude events only if we have receivers (skip allocation when empty)
        if !app.claude_receivers.is_empty() {
            let claude_events: Vec<_> = app.claude_receivers.iter()
                .flat_map(|(sid, rx)| {
                    std::iter::from_fn(|| rx.try_recv().ok())
                        .map(|e| (sid.clone(), e))
                        .collect::<Vec<_>>()
                })
                .collect();

            for (session_id, claude_event) in claude_events {
                handle_claude_event(&session_id, claude_event, app)?;
                needs_redraw = true;
            }
        }

        // Poll session file for changes (two-phase: lightweight size check, then parse if needed)
        // check_session_file() is cheap (just stat()), poll_session_file() does the expensive parse
        let now_poll = Instant::now();
        if now_poll.duration_since(last_session_poll) >= min_poll_interval {
            app.check_session_file();
            if app.poll_session_file() {
                needs_redraw = true;
            }
            if app.poll_interactive_sessions() {
                needs_redraw = true;
            }
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
        if app.rendered_lines_dirty {
            let convo_width = cached_width.saturating_sub(80) / 2;
            submit_render_request(app, convo_width);
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
        if had_key_event && app.prompt_mode && !app.terminal_mode && app.focus == Focus::Input && app.input_area.width > 2 && !app.input.contains('\n') {
            fast_draw_input(app);
        }

        // Full draw: terminal.draw() costs ~18ms. Only run on quiet iterations
        // (no key events) to avoid blocking the event loop during typing.
        let now = Instant::now();
        let draw_ready = now.duration_since(last_draw) >= min_draw_interval;
        // Defer draw when typing single-line in Claude prompt (fast-path handles it).
        // Multi-line input needs immediate full draw to resize the input box.
        // Terminal mode needs immediate draws — PTY output has no fast-path.
        let has_fast_path = app.prompt_mode && !app.terminal_mode && !app.input.contains('\n');
        let defer_for_typing = had_key_event && has_fast_path;
        let should_draw = app.draw_pending && draw_ready && !defer_for_typing;

        if should_draw {
            // Pre-draw drain: catch events that arrived between the top-of-loop
            // drain and now (~0-5ms gap). If a key arrives here, skip draw.
            let mut got_key = false;
            while event::poll(Duration::from_millis(0))? {
                match event::read()? {
                    Event::Key(key) if key.kind == KeyEventKind::Press && !matches!(key.code, KeyCode::Modifier(_)) => {
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

    // Figure out cursor row for scroll offset (same logic as draw_input.rs)
    let cursor_row = compute_cursor_row_fast(&app.input, app.input_cursor, inner_width);
    let scroll_offset = if visible_rows > 0 && cursor_row >= visible_rows {
        cursor_row - visible_rows + 1
    } else { 0 };

    // Build visible lines from input text with word-wrapping
    let chars: Vec<char> = app.input.chars().collect();
    let mut visual_lines: Vec<String> = Vec::new();
    let mut current_line = String::new();
    let mut col = 0usize;
    for &c in &chars {
        if c == '\n' {
            visual_lines.push(current_line);
            current_line = String::new();
            col = 0;
        } else {
            let w = unicode_width::UnicodeWidthChar::width(c).unwrap_or(1);
            if col + w > inner_width {
                visual_lines.push(current_line);
                current_line = String::new();
                col = 0;
            }
            current_line.push(c);
            col += w;
        }
    }
    visual_lines.push(current_line);

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

    // Position cursor at the right spot
    let cursor_col = compute_cursor_col_fast(&app.input, app.input_cursor, inner_width);
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

/// Compute visual row for cursor (word-wrap aware) — standalone version for
/// fast_draw_input to avoid depending on draw_input module.
fn compute_cursor_row_fast(input: &str, cursor_idx: usize, inner_width: usize) -> usize {
    let chars: Vec<char> = input.chars().collect();
    let target = cursor_idx.min(chars.len());
    let mut row = 0usize;
    let mut col = 0usize;
    for i in 0..target {
        if chars[i] == '\n' { row += 1; col = 0; }
        else {
            let w = unicode_width::UnicodeWidthChar::width(chars[i]).unwrap_or(1);
            if col + w > inner_width { row += 1; col = w; } else { col += w; }
        }
    }
    row
}

/// Compute visual column for cursor — standalone version for fast_draw_input.
fn compute_cursor_col_fast(input: &str, cursor_idx: usize, inner_width: usize) -> usize {
    let chars: Vec<char> = input.chars().collect();
    let target = cursor_idx.min(chars.len());
    let mut col = 0usize;
    for i in 0..target {
        if chars[i] == '\n' { col = 0; }
        else {
            let w = unicode_width::UnicodeWidthChar::width(chars[i]).unwrap_or(1);
            if col + w > inner_width { col = w; } else { col += w; }
        }
    }
    col
}

/// Apply accumulated scroll to the appropriate panel using cached pane rects
fn apply_scroll_cached(app: &mut App, delta: i32, col: u16, row: u16, _term_width: u16, _term_height: u16) -> bool {
    use ratatui::layout::Position;
    let pos = Position::new(col, row);

    if app.pane_sessions.contains(pos) {
        let old = app.selected_session;
        if delta > 0 { for _ in 0..delta.abs() { app.select_next_session(); } }
        else { for _ in 0..delta.abs() { app.select_prev_session(); } }
        app.selected_session != old
    } else if app.pane_file_tree.contains(pos) {
        let old = app.file_tree_selected;
        if delta > 0 { for _ in 0..delta.abs() { app.file_tree_next(); } }
        else { for _ in 0..delta.abs() { app.file_tree_prev(); } }
        app.file_tree_selected != old
    } else if app.pane_viewer.contains(pos) {
        if delta > 0 { app.scroll_viewer_down(delta as usize) }
        else { app.scroll_viewer_up((-delta) as usize) }
    } else if app.terminal_mode && app.input_area.contains(pos) {
        if delta > 0 { app.scroll_terminal_down(delta as usize); }
        else { app.scroll_terminal_up((-delta) as usize); }
        true
    } else if app.pane_convo.contains(pos) {
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

    // Sessions pane — click to select session or session file
    if app.pane_sessions.contains(pos) {
        app.focus = Focus::Worktrees;
        // Map screen row to sidebar item (1 for top border)
        let visual_row = (row.saturating_sub(app.pane_sessions.y + 1)) as usize;
        if let Some(action) = app.sidebar_row_map.get(visual_row).cloned() {
            match action {
                SidebarRowAction::Session(idx) => {
                    if app.selected_session != Some(idx) {
                        app.save_current_terminal();
                        app.selected_session = Some(idx);
                        app.load_session_output();
                        app.invalidate_sidebar();
                    }
                }
                SidebarRowAction::SessionFile(sess_idx, file_idx) => {
                    // First select the session if different
                    if app.selected_session != Some(sess_idx) {
                        app.save_current_terminal();
                        app.selected_session = Some(sess_idx);
                    }
                    // Then select the session file
                    if let Some(session) = app.sessions.get(sess_idx) {
                        let branch = session.branch_name.clone();
                        app.select_session_file(&branch, file_idx);
                    }
                }
                SidebarRowAction::ProjectHeader => {} // Just focus
            }
        }
        return true;
    }

    // FileTree pane — click to select entry, double-click to open/expand
    if app.pane_file_tree.contains(pos) {
        app.focus = Focus::FileTree;
        let visual_row = (row.saturating_sub(app.pane_file_tree.y + 1)) as usize;
        let entry_idx = visual_row + app.file_tree_scroll;
        if entry_idx < app.file_tree_entries.len() {
            app.file_tree_selected = Some(entry_idx);
            // Double-click detection: same row within 500ms → open/toggle
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
        app.last_click = Some((std::time::Instant::now(), col, row));
        return true;
    }

    // Viewer pane — just focus
    if app.pane_viewer.contains(pos) {
        app.focus = Focus::Viewer;
        app.last_click = Some((std::time::Instant::now(), col, row));
        return true;
    }

    // Convo pane — focus
    if app.pane_convo.contains(pos) {
        app.focus = Focus::Output;
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
/// Walks the input text with the same char-level wrapping logic as
/// fast_draw_input() and draw_input() to find which char index
/// corresponds to the clicked (col, row) within the input box.
fn click_to_input_cursor(app: &mut App, click_col: u16, click_row: u16) {
    let inner_x = app.input_area.x + 1;
    let inner_y = app.input_area.y + 1;
    let inner_width = (app.input_area.width.saturating_sub(2)) as usize;
    if inner_width == 0 { return; }

    let target_row = (click_row.saturating_sub(inner_y)) as usize;
    let target_col = (click_col.saturating_sub(inner_x)) as usize;

    // Account for scroll offset (input can scroll when multi-line overflows)
    let visible_rows = app.input_area.height.saturating_sub(2) as usize;
    let cursor_row_current = compute_cursor_row_fast(&app.input, app.input_cursor, inner_width);
    let scroll_offset = if visible_rows > 0 && cursor_row_current >= visible_rows {
        cursor_row_current - visible_rows + 1
    } else { 0 };
    let actual_row = target_row + scroll_offset;

    // Walk chars counting visual rows and columns (same wrapping as compute_cursor_row_fast)
    let chars: Vec<char> = app.input.chars().collect();
    let mut row = 0usize;
    let mut col_pos = 0usize;
    let mut best_idx = chars.len(); // default: end of input

    for (i, &c) in chars.iter().enumerate() {
        // Check if we've reached or passed the target row
        if row == actual_row && col_pos >= target_col {
            best_idx = i;
            break;
        }
        if row > actual_row {
            best_idx = i;
            break;
        }
        if c == '\n' {
            if row == actual_row {
                // Click is on this row past the newline — place at newline position
                best_idx = i;
                break;
            }
            row += 1;
            col_pos = 0;
        } else {
            let w = unicode_width::UnicodeWidthChar::width(c).unwrap_or(1);
            if col_pos + w > inner_width {
                // Wrap to next row
                if row == actual_row {
                    best_idx = i;
                    break;
                }
                row += 1;
                col_pos = w;
            } else {
                col_pos += w;
            }
        }
    }
    // If we walked through all chars and target row matches last row,
    // place cursor at end
    if row == actual_row && best_idx == chars.len() {
        // Already at end — correct
    } else if row < actual_row {
        best_idx = chars.len(); // Target row beyond content → end
    }

    app.input_cursor = best_idx;
}

/// Handle Claude process events for a specific session
fn handle_claude_event(session_id: &str, event: ClaudeEvent, app: &mut App) -> Result<()> {
    match event {
        ClaudeEvent::Output(output) => app.handle_claude_output(session_id, output.output_type, output.data),
        ClaudeEvent::Started { pid } => app.handle_claude_started(session_id, pid),
        ClaudeEvent::SessionId(claude_session_id) => app.set_claude_session_id(session_id, claude_session_id),
        ClaudeEvent::Exited { code } => app.handle_claude_exited(session_id, code),
        ClaudeEvent::Error(e) => app.handle_claude_error(session_id, e),
    }
    Ok(())
}

/// Handle keyboard input events
fn handle_key_event(key: event::KeyEvent, app: &mut App, claude_process: &ClaudeProcess) -> Result<()> {
    // With Kitty protocol REPORT_ALL_KEYS, bare modifier presses (Shift, Ctrl, Alt)
    // arrive as key events. Ignore them globally — no handler cares about these.
    if matches!(key.code, KeyCode::Modifier(_)) { return Ok(()); }

    // D key (uppercase, i.e. Shift+D) when not in prompt mode - Debug dump
    if !app.prompt_mode && key.modifiers == KeyModifiers::SHIFT && key.code == KeyCode::Char('D') {
        app.dump_debug_output();
        return Ok(());
    }

    // Global keybindings
    match (key.modifiers, key.code) {
        (KeyModifiers::CONTROL, KeyCode::Char('q')) => {
            app.should_quit = true;
            return Ok(());
        }
        (KeyModifiers::CONTROL, KeyCode::Char('r')) => {
            app.should_restart = true;
            app.should_quit = true;
            return Ok(());
        }
        (KeyModifiers::CONTROL, KeyCode::Char('c')) => {
            // Cancel running Claude response
            app.cancel_current_claude();
            return Ok(());
        }
        // Global 'p' - enter Claude prompt mode from anywhere (except viewer edit mode)
        (KeyModifiers::NONE, KeyCode::Char('p')) if !app.prompt_mode && !app.viewer_edit_mode && app.context_menu.is_none() && !app.is_wizard_active() => {
            app.show_help = false;
            if app.terminal_mode {
                app.close_terminal();
            }
            app.focus = Focus::Input;
            app.prompt_mode = true;
            return Ok(());
        }
        // Global 't' - toggle terminal (only when not in terminal, otherwise handled by terminal input)
        (KeyModifiers::NONE, KeyCode::Char('t')) if !app.prompt_mode && !app.terminal_mode && app.context_menu.is_none() && !app.is_wizard_active() => {
            app.show_help = false;
            app.toggle_terminal();
            app.focus = Focus::Input;
            return Ok(());
        }
        (KeyModifiers::NONE, KeyCode::Char('+')) | (KeyModifiers::SHIFT, KeyCode::Char('+')) if !app.prompt_mode && app.terminal_mode => {
            app.adjust_terminal_height(2);
            return Ok(());
        }
        (KeyModifiers::NONE, KeyCode::Char('-')) if !app.prompt_mode && app.terminal_mode => {
            app.adjust_terminal_height(-2);
            return Ok(());
        }
        // '?' - toggle help (SHIFT modifier allowed for US keyboards)
        (KeyModifiers::NONE, KeyCode::Char('?')) | (KeyModifiers::SHIFT, KeyCode::Char('?')) if !app.prompt_mode && !app.viewer_edit_mode => {
            app.toggle_help();
            return Ok(());
        }
        // Wizard tab cycling (must be before regular Tab handler)
        // Alt+Tab or ] to go forward, Shift+Tab or [ to go backward
        // Note: On macOS, Option+Tab might be intercepted by the system
        (KeyModifiers::NONE, KeyCode::Char(']')) | (KeyModifiers::ALT, KeyCode::Tab) if app.is_wizard_active() => {
            if let Some(wizard) = app.creation_wizard.as_mut() {
                wizard.next_tab();
            }
            return Ok(());
        }
        (KeyModifiers::NONE, KeyCode::Char('[')) | (KeyModifiers::SHIFT, KeyCode::BackTab) if app.is_wizard_active() => {
            if let Some(wizard) = app.creation_wizard.as_mut() {
                wizard.prev_tab();
            }
            return Ok(());
        }
        (KeyModifiers::NONE, KeyCode::Tab) => {
            // Cycle focus (works in both prompt and command mode)
            // Skip when wizard is active (wizard uses Tab for field cycling)
            if !app.show_help && !app.is_wizard_active() {
                app.prompt_mode = false; // Exit prompt mode when tabbing away
                app.focus_next();
                return Ok(());
            }
        }
        (KeyModifiers::SHIFT, KeyCode::BackTab) => {
            // Cycle focus backwards (works in both prompt and command mode)
            // Skip when wizard is active
            if !app.show_help && !app.is_wizard_active() {
                app.prompt_mode = false;
                app.focus_prev();
                return Ok(());
            }
        }
        _ => {}
    }

    // Help overlay is open - allow p and t to work (they close help first via global handlers above)
    if app.show_help {
        match key.code {
            KeyCode::Char('?') | KeyCode::Esc => app.toggle_help(),
            _ => {}
        }
        return Ok(());
    }

    // Context menu
    if app.context_menu.is_some() {
        handle_context_menu_input(key, app, claude_process)?;
        return Ok(());
    }

    // Wizard
    if app.is_wizard_active() {
        handle_wizard_input(app, key, claude_process);
        return Ok(());
    }

    // Run command overlays (picker and dialog intercept all input)
    if app.run_command_picker.is_some() {
        handle_run_command_picker_input(key, app)?;
        return Ok(());
    }
    if app.run_command_dialog.is_some() {
        handle_run_command_dialog_input(key, app)?;
        return Ok(());
    }

    // Mode-specific keybindings (scroll handlers use cached viewport heights from last render)
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
