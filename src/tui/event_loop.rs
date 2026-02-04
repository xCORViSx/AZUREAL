//! Core event loop and event handling

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyModifiers, MouseEventKind};
use std::io;
use std::time::{Duration, Instant};

use ratatui::{backend::CrosstermBackend, Terminal};

use crate::app::{App, Focus};
use crate::claude::{ClaudeEvent, ClaudeProcess};
use crate::config::Config;

use super::input_dialogs::{handle_branch_dialog_input, handle_context_menu_input};
use super::input_file_tree::handle_file_tree_input;
use super::input_output::handle_output_input;
use super::input_worktrees::handle_worktrees_input;
use super::input_terminal::{handle_input_mode, handle_worktree_creation_input};
use super::input_viewer::handle_viewer_input;
use super::input_wizard::handle_wizard_input;
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
    let min_draw_interval = Duration::from_millis(100); // Max 10fps for scroll
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

        // First wait for at least one event
        if event::poll(Duration::from_millis(100))? {
            // Drain all available events without blocking
            loop {
                match event::read()? {
                    Event::Key(key) => {
                        handle_key_event(key, app, &claude_process)?;
                        had_key_event = true;
                    }
                    Event::Mouse(mouse) => {
                        // Accumulate scroll events, discard motion/clicks
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
                            _ => {} // Discard motion, clicks instantly
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

        // Poll session file for changes (throttled - max 2x/sec for large files)
        let now_poll = Instant::now();
        if now_poll.duration_since(last_session_poll) >= min_poll_interval {
            if app.poll_session_file() {
                needs_redraw = true;
            }
            if app.poll_interactive_sessions() {
                needs_redraw = true;
            }
            last_session_poll = now_poll;
        }

        // Debug dump removed - too expensive for every redraw

        // Apply accumulated scroll using cached terminal size
        let mut scroll_changed = false;
        if scroll_delta != 0 {
            scroll_changed = apply_scroll_cached(app, scroll_delta, scroll_col, scroll_row, cached_width, cached_height);
        }

        // Key events, Claude events, terminal output: redraw immediately
        // Scroll events: throttle to max 20fps
        let now = Instant::now();
        let should_draw = if had_key_event || needs_redraw {
            true
        } else { scroll_changed && now.duration_since(last_draw) >= min_draw_interval };

        if should_draw {
            terminal.draw(|f| ui(f, app))?;
            last_draw = now;
        }

        if app.should_quit { break; }
    }

    Ok(())
}

/// Apply accumulated scroll to the appropriate panel (uses cached terminal size)
/// Layout: Sessions(40) | FileTree(40) | Viewer(50%) | Convo(50%)
fn apply_scroll_cached(app: &mut App, delta: i32, col: u16, row: u16, term_width: u16, term_height: u16) -> bool {
    let sessions_width = 40u16;
    let file_tree_width = 40u16;
    let remaining_width = term_width.saturating_sub(sessions_width + file_tree_width);
    let viewer_width = remaining_width / 2;

    let input_height = if app.terminal_mode { app.terminal_height + 2 } else { 3u16 };
    let content_height = term_height.saturating_sub(input_height + 1);

    let in_sessions = col < sessions_width && row < content_height;
    let in_file_tree = col >= sessions_width && col < sessions_width + file_tree_width && row < content_height;
    let in_viewer = col >= sessions_width + file_tree_width && col < sessions_width + file_tree_width + viewer_width && row < content_height;
    let in_output = col >= sessions_width + file_tree_width + viewer_width && row < content_height;
    let in_terminal = app.terminal_mode && row >= content_height && row < term_height - 1;

    if in_sessions {
        let old = app.selected_session;
        if delta > 0 { for _ in 0..delta.abs() { app.select_next_session(); } }
        else { for _ in 0..delta.abs() { app.select_prev_session(); } }
        app.selected_session != old
    } else if in_file_tree {
        let old = app.file_tree_selected;
        if delta > 0 { for _ in 0..delta.abs() { app.file_tree_next(); } }
        else { for _ in 0..delta.abs() { app.file_tree_prev(); } }
        app.file_tree_selected != old
    } else if in_viewer {
        if delta > 0 { app.scroll_viewer_down(delta as usize) }
        else { app.scroll_viewer_up((-delta) as usize) }
    } else if in_terminal {
        if delta > 0 { app.scroll_terminal_down(delta as usize); }
        else { app.scroll_terminal_up((-delta) as usize); }
        true // Terminal scroll doesn't have boundary check yet
    } else if in_output {
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
    // D key (uppercase, i.e. Shift+D) when not in insert mode - Debug dump
    if !app.insert_mode && key.modifiers == KeyModifiers::SHIFT && key.code == KeyCode::Char('D') {
        app.dump_debug_output();
        return Ok(());
    }

    // Global keybindings
    match (key.modifiers, key.code) {
        (KeyModifiers::CONTROL, KeyCode::Char('c')) | (KeyModifiers::CONTROL, KeyCode::Char('q')) => {
            app.should_quit = true;
            return Ok(());
        }
        (KeyModifiers::CONTROL, KeyCode::Char('x')) => {
            // Cancel running Claude response
            app.cancel_current_claude();
            return Ok(());
        }
        (KeyModifiers::NONE, KeyCode::Char('i')) if !app.insert_mode && !app.show_help && app.context_menu.is_none() && !app.is_wizard_active() => {
            app.focus = Focus::Input;
            app.insert_mode = true;
            return Ok(());
        }
        (KeyModifiers::NONE, KeyCode::Char('t')) if !app.insert_mode && !app.show_help && app.context_menu.is_none() && !app.is_wizard_active() => {
            app.toggle_terminal();
            app.focus = Focus::Input;
            return Ok(());
        }
        (KeyModifiers::NONE, KeyCode::Char('+')) | (KeyModifiers::SHIFT, KeyCode::Char('+')) if !app.insert_mode && app.terminal_mode => {
            app.adjust_terminal_height(2);
            return Ok(());
        }
        (KeyModifiers::NONE, KeyCode::Char('-')) if !app.insert_mode && app.terminal_mode => {
            app.adjust_terminal_height(-2);
            return Ok(());
        }
        (KeyModifiers::NONE, KeyCode::Char('?')) if !app.insert_mode => {
            app.toggle_help();
            return Ok(());
        }
        (KeyModifiers::NONE, KeyCode::Tab) => {
            // Cycle focus (works in both insert and command mode)
            // Skip when wizard is active (wizard uses Tab for field cycling)
            if !app.show_help && !app.is_wizard_active() {
                app.insert_mode = false; // Exit insert mode when tabbing away
                app.focus_next();
                return Ok(());
            }
        }
        (KeyModifiers::SHIFT, KeyCode::BackTab) => {
            // Cycle focus backwards (works in both insert and command mode)
            // Skip when wizard is active
            if !app.show_help && !app.is_wizard_active() {
                app.insert_mode = false;
                app.focus_prev();
                return Ok(());
            }
        }
        _ => {}
    }

    // Help overlay is open
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
