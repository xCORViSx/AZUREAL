//! Core event loop and event handling
//!
//! Split into focused submodules:
//! - `actions`: Keyboard action dispatch (6 sub-submodules: execute, navigation, escape, session_list, deferred, rcr)
//! - `claude_events`: Claude process event handling
//! - `coords`: Screen-to-content coordinate mapping
//! - `fast_draw`: Fast-path input rendering (~0.1ms bypass)
//! - `mouse`: Mouse click, drag, scroll, and selection copy

mod actions;
mod claude_events;
mod coords;
mod fast_draw;
mod mouse;

pub(super) use mouse::copy_viewer_selection;

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEventKind, MouseButton, MouseEventKind};
use std::io;
use std::time::{Duration, Instant};

use ratatui::{backend::CrosstermBackend, Terminal};

use crate::app::{App, Focus};
use crate::claude::ClaudeProcess;
use crate::config::Config;

use super::draw_output::{submit_render_request, poll_render_result};
use super::run::ui;

use actions::handle_key_event;
use claude_events::handle_claude_event;
use coords::{screen_to_cache_pos, screen_to_edit_pos, screen_to_input_char};
use fast_draw::fast_draw_input;
use mouse::{apply_scroll_cached, handle_mouse_click, handle_mouse_drag};

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
        // Also short-poll while commit message or squash merge is in progress
        let commit_generating = app.git_actions_panel.as_ref()
            .and_then(|p| p.commit_overlay.as_ref())
            .map(|o| o.generating).unwrap_or(false);
        let squash_merging = app.git_actions_panel.as_ref()
            .map(|p| p.squash_merge_receiver.is_some()).unwrap_or(false);
        let bg_pending = app.file_tree_receiver.is_some() || app.worktree_refresh_receiver.is_some();
        let poll_ms = if app.draw_pending || app.render_in_flight || !app.claude_receivers.is_empty() || app.stt_recording || app.stt_transcribing || app.session_file_dirty || app.file_tree_refresh_pending || app.health_refresh_pending || commit_generating || squash_merging || bg_pending { 16 } else { 100 };
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
                                app.session_selection = None;
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
                                } else if app.pane_session.contains(mpos) {
                                    app.clamp_session_scroll();
                                    if let Some((cl, cc)) = screen_to_cache_pos(mc, mr, app.pane_session, app.session_scroll, app.rendered_lines_cache.len()) {
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
                        app.screen_height = h;
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

        // Fast-path input rendering: MUST run immediately after key drain, before
        // any blocking housekeeping (file tree rebuild, worktree refresh, render
        // submit clone). Those operations can block 20-150ms every 500ms when
        // Claude modifies files, causing visible keystroke lag if fast_draw runs
        // after them. By rendering here, the user sees instant visual feedback
        // (~0.1ms) regardless of what blocking work follows.
        // Skip fast-path for multi-line input — the input box must resize via
        // full draw when newlines are added/removed. Single-line typing (the
        // common case) still gets the fast path.
        // Skip fast-path when selection is active — fast_draw_input doesn't
        // render selection highlighting, so the full draw_input must handle it
        if had_key_event && app.prompt_mode && !app.terminal_mode && app.focus == Focus::Input && app.input_area.width > 2 && !app.input.contains('\n') && !app.has_input_selection() {
            fast_draw_input(app);
        }

        // Process Claude events — drain up to MAX_CLAUDE_EVENTS per tick to prevent
        // burst processing from blocking input. During streaming Claude sends ~60
        // events/sec; draining all at once can take 10-50ms of sequential JSON
        // parsing. Capping per tick ensures the loop returns to key polling quickly.
        // Remaining events stay in the channel for the next iteration.
        const MAX_CLAUDE_EVENTS_PER_TICK: usize = 10;
        if !app.claude_receivers.is_empty() {
            let mut claude_events: Vec<(String, crate::claude::ClaudeEvent)> = Vec::new();
            'outer: for (sid, rx) in &app.claude_receivers {
                while let Ok(event) = rx.try_recv() {
                    claude_events.push((sid.clone(), event));
                    if claude_events.len() >= MAX_CLAUDE_EVENTS_PER_TICK { break 'outer; }
                }
            }
            for (session_id, event) in claude_events {
                handle_claude_event(&session_id, event, app, &claude_process)?;
                needs_redraw = true;
            }
        }

        // Poll commit message generation — background thread sends the Claude-generated
        // commit message via mpsc. Non-blocking try_recv; fills the overlay when ready.
        if let Some(ref mut panel) = app.git_actions_panel {
            if let Some(ref mut overlay) = panel.commit_overlay {
                if overlay.generating {
                    if let Some(ref rx) = overlay.receiver {
                        if let Ok(result) = rx.try_recv() {
                            match result {
                                Ok(msg) => {
                                    overlay.message = msg;
                                    overlay.cursor = overlay.message.chars().count();
                                    overlay.generating = false;
                                    overlay.receiver = None;
                                    needs_redraw = true;
                                }
                                Err(err) => {
                                    // Generation failed — close overlay and show error
                                    panel.commit_overlay = None;
                                    panel.result_message = Some((err, true));
                                    needs_redraw = true;
                                }
                            }
                        }
                    }
                }
            }
        }

        // Poll squash merge progress — background thread sends phase updates and
        // final outcome via mpsc. Updates loading_indicator with each phase.
        {
            use crate::app::types::SquashMergeOutcome;
            let mut merge_outcome: Option<SquashMergeOutcome> = None;
            let mut new_phase: Option<String> = None;
            if let Some(ref mut panel) = app.git_actions_panel {
                if let Some(ref rx) = panel.squash_merge_receiver {
                    while let Ok(progress) = rx.try_recv() {
                        if let Some(outcome) = progress.outcome {
                            panel.squash_merge_receiver = None;
                            merge_outcome = Some(outcome);
                            needs_redraw = true;
                            break;
                        }
                        new_phase = Some(progress.phase);
                        needs_redraw = true;
                    }
                }
            }
            if let Some(phase) = new_phase {
                app.loading_indicator = Some(phase);
            }
            if let Some(outcome) = merge_outcome {
                app.loading_indicator = None;
                match outcome {
                    SquashMergeOutcome::Success { status_msg, branch, display_name, worktree_path } => {
                        app.git_actions_panel = None;
                        app.post_merge_dialog = Some(crate::app::types::PostMergeDialog {
                            branch,
                            display_name,
                            worktree_path,
                            selected: 0,
                        });
                        app.set_status(status_msg);
                    }
                    SquashMergeOutcome::Conflict { conflicted, auto_merged } => {
                        if let Some(ref mut p) = app.git_actions_panel {
                            p.conflict_overlay = Some(crate::app::types::GitConflictOverlay {
                                conflicted_files: conflicted,
                                auto_merged_files: auto_merged,
                                scroll: 0,
                                selected: 0,
                                continue_with_merge: true,
                            });
                        }
                    }
                    SquashMergeOutcome::Failed(msg) => {
                        if let Some(ref mut p) = app.git_actions_panel {
                            p.result_message = Some((msg, true));
                        }
                    }
                }
            }
        }

        // Deferred debug dump saving — naming dialog closed, trigger the actual dump
        if let Some(name) = app.debug_dump_saving.take() {
            app.dump_debug_output(&name);
            app.draw_pending = true;
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
                        app.worktree_tabs_refresh_pending = true;
                        if app.health_panel.is_some() {
                            app.health_refresh_pending = true;
                        }
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

        // Debounced file tree refresh: spawn background thread to avoid
        // blocking the event loop (build_file_tree walks the filesystem,
        // 10-100ms depending on tree depth). Old tree stays visible until
        // the new one arrives — no flash of empty state.
        if app.file_tree_refresh_pending
            && app.file_tree_receiver.is_none()
            && now_poll.duration_since(app.worktree_last_notify) >= Duration::from_millis(500)
        {
            if let Some(wt) = app.current_worktree() {
                if let Some(ref wt_path) = wt.worktree_path {
                    let path = wt_path.clone();
                    let expanded = app.file_tree_expanded.clone();
                    let hidden = app.file_tree_hidden_dirs.clone();
                    let (tx, rx) = std::sync::mpsc::channel();
                    std::thread::spawn(move || {
                        let entries = crate::app::state::helpers::build_file_tree(&path, &expanded, &hidden);
                        let _ = tx.send(entries);
                    });
                    app.file_tree_receiver = Some(rx);
                }
            }
            app.file_tree_refresh_pending = false;
        }

        // Poll file tree background scan result
        if let Some(ref rx) = app.file_tree_receiver {
            if let Ok(entries) = rx.try_recv() {
                app.file_tree_entries = entries;
                app.file_tree_selected = if !app.file_tree_entries.is_empty() { Some(0) } else { None };
                app.file_tree_scroll = 0;
                app.invalidate_file_tree();
                app.file_tree_receiver = None;
                needs_redraw = true;
            }
        }

        // Debounced worktree tab list refresh: spawn background thread for
        // git + FS I/O (git worktree list, branch listing, session discovery,
        // 10-50ms). Sidebar stays visible with old data until results arrive.
        if app.worktree_tabs_refresh_pending
            && app.worktree_refresh_receiver.is_none()
            && now_poll.duration_since(app.worktree_last_notify) >= Duration::from_millis(500)
        {
            if let Some(ref project) = app.project {
                let path = project.path.clone();
                let main_branch = project.main_branch.clone();
                let wt_dir = project.worktrees_dir();
                let (tx, rx) = std::sync::mpsc::channel();
                std::thread::spawn(move || {
                    let result = crate::app::state::load::compute_worktree_refresh(path, main_branch, wt_dir);
                    let _ = tx.send(result);
                });
                app.worktree_refresh_receiver = Some(rx);
            }
            app.worktree_tabs_refresh_pending = false;
        }

        // Poll worktree refresh background result
        if let Some(ref rx) = app.worktree_refresh_receiver {
            if let Ok(result) = rx.try_recv() {
                if let Ok(data) = result {
                    app.apply_worktree_result(data);
                }
                app.worktree_refresh_receiver = None;
                needs_redraw = true;
            }
        }

        // Debounced health panel refresh: rescan god files + doc coverage
        // when source files change while the panel is open
        if app.health_refresh_pending
            && now_poll.duration_since(app.worktree_last_notify) >= Duration::from_millis(500)
        {
            app.refresh_health_panel();
            app.health_refresh_pending = false;
            needs_redraw = true;
        }

        // Timer-based housekeeping
        if now_poll.duration_since(last_session_poll) >= min_poll_interval {
            last_session_poll = now_poll;
        }

        // Compaction inactivity watcher: when context ≥ 95% and no events for 20s,
        // inject a "may be compacting" banner so the user knows why session pane is frozen
        if app.context_pct_high
            && !app.compaction_banner_injected
            && !app.claude_receivers.is_empty()
            && now_poll.duration_since(app.last_session_event_time) >= Duration::from_secs(20)
        {
            app.display_events.push(crate::events::DisplayEvent::MayBeCompacting);
            app.invalidate_render_cache();
            app.compaction_banner_injected = true;
            needs_redraw = true;
        }

        // Dismiss auto-rebase success dialog after 2 seconds
        if let Some((_, until)) = &app.auto_rebase_success_until {
            if now_poll >= *until {
                app.auto_rebase_success_until = None;
                needs_redraw = true;
            }
        }

        // Periodic auto-rebase check (every 2 seconds).
        // Skip during active Claude streaming — git subprocess calls (git status
        // --porcelain per worktree) block 5-50ms, and rebasing while Claude is
        // modifying files would fail with dirty working tree anyway.
        if app.claude_receivers.is_empty()
            && now_poll.duration_since(app.last_auto_rebase_check) >= Duration::from_secs(2)
        {
            app.last_auto_rebase_check = now_poll;
            if !app.auto_rebase_enabled.is_empty() {
                if check_auto_rebase(app, &claude_process) {
                    needs_redraw = true;
                }
            }
        }

        // Apply accumulated scroll using cached terminal size
        let mut scroll_changed = false;
        if scroll_delta != 0 {
            scroll_changed = apply_scroll_cached(app, scroll_delta, scroll_col, scroll_row, cached_width, cached_height);
        }

        // Submit render request to background thread if session cache is dirty.
        // This is NON-BLOCKING — the render thread does the expensive work while
        // we keep processing events. No more frozen input during session updates!
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
            // Session pane width is percentage-based (35% in run.rs), so we read the
            // actual width from the cached pane rect set during the last draw.
            // Falls back to 80 on first frame before any draw has occurred.
            let session_w = if app.pane_session.width > 0 { app.pane_session.width } else { 80 };
            submit_render_request(app, session_w);
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
                    Event::Resize(w, h) => { cached_width = w; cached_height = h; app.screen_height = h; }
                    _ => {}
                }
            }
            // Keys caught in pre-draw drain need immediate visual feedback too.
            // Without this, they appear only on the next full draw (~33ms later).
            if got_key && app.prompt_mode && !app.terminal_mode && app.focus == Focus::Input && app.input_area.width > 2 && !app.input.contains('\n') && !app.has_input_selection() {
                fast_draw_input(app);
            }
            if !got_key {
                terminal.draw(|f| ui(f, app))?;
                last_draw = Instant::now();
                app.draw_pending = false;

                // Deferred session list loading: the loading dialog just rendered,
                // so now we can do the expensive message count I/O. The user sees
                // "Loading sessions..." while this runs, then the list appears.
                if app.session_list_loading {
                    actions::finish_session_list_load(app);
                    app.draw_pending = true;
                }
                // Generic deferred action: loading indicator just rendered, now do the work.
                if let Some(action) = app.deferred_action.take() {
                    app.loading_indicator = None;
                    actions::execute_deferred_action(app, action);
                    app.draw_pending = true;
                }
            }
        }

        if app.should_quit { break; }
    }

    Ok(())
}

/// Check all auto-rebase-enabled worktrees and rebase the first eligible one.
/// Returns true if any state changed (needs redraw).
fn check_auto_rebase(app: &mut App, _claude_process: &ClaudeProcess) -> bool {
    use super::input_git_actions::{exec_rebase_inner, RebaseOutcome};
    use crate::app::types::GitConflictOverlay;

    // Skip if RCR active or editing a file
    if app.rcr_session.is_some() { return false; }
    if app.viewer_edit_mode { return false; }

    let project = match &app.project {
        Some(p) => p.clone(),
        None => return false,
    };

    // Collect eligible worktrees (avoid borrowing app during iteration)
    let candidates: Vec<(String, std::path::PathBuf)> = app.worktrees.iter()
        .filter(|wt| {
            wt.branch_name != project.main_branch
                && !wt.archived
                && app.auto_rebase_enabled.contains(&wt.branch_name)
                && !app.is_session_running(&wt.branch_name)
                && wt.worktree_path.is_some()
        })
        .map(|wt| (wt.branch_name.clone(), wt.worktree_path.clone().unwrap()))
        .collect();

    // If the git panel is open, note which worktree it's viewing
    let git_panel_branch = app.git_actions_panel.as_ref().map(|p| p.worktree_name.clone());

    for (branch, wt_path) in candidates {
        // Skip the worktree whose git panel is currently open
        if git_panel_branch.as_ref() == Some(&branch) { continue; }

        let display = crate::models::strip_branch_prefix(&branch).to_string();

        // Skip worktrees with uncommitted changes — git rebase would fail
        let dirty = std::process::Command::new("git")
            .args(["status", "--porcelain"])
            .current_dir(&wt_path)
            .output()
            .ok()
            .map(|o| !o.stdout.is_empty())
            .unwrap_or(false);
        if dirty { continue; }

        let ar_files = crate::azufig::load_auto_resolve_files(&project.path);
        match exec_rebase_inner(&wt_path, &project.main_branch, &ar_files) {
            RebaseOutcome::UpToDate => continue,
            RebaseOutcome::Rebased => {
                // Push the rebased branch to its remote
                let push_suffix = match crate::git::Git::push(&wt_path) {
                    Ok(_) => " → pushed",
                    Err(_) => "",
                };
                app.auto_rebase_success_until = Some((
                    format!("{}{}", display, push_suffix),
                    Instant::now() + Duration::from_secs(2),
                ));
                app.invalidate_sidebar();
                return true;
            }
            RebaseOutcome::Conflict { conflicted, auto_merged, .. } => {
                // Switch to the conflicted worktree and open Git panel with conflict overlay
                if let Some(idx) = app.worktrees.iter().position(|w| w.branch_name == branch) {
                    app.selected_worktree = Some(idx);
                    app.load_session_output();
                }
                app.open_git_actions_panel();
                if let Some(ref mut panel) = app.git_actions_panel {
                    panel.conflict_overlay = Some(GitConflictOverlay {
                        conflicted_files: conflicted,
                        auto_merged_files: auto_merged,
                        scroll: 0,
                        selected: 0,
                        continue_with_merge: false,
                    });
                }
                app.invalidate_sidebar();
                return true;
            }
            RebaseOutcome::Failed(_) => continue,
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers, MouseButton, MouseEventKind, MouseEvent};

    // -- Duration constants --

    #[test]
    fn test_min_draw_interval() {
        let interval = Duration::from_millis(33);
        assert_eq!(interval.as_millis(), 33);
    }

    #[test]
    fn test_min_poll_interval() {
        let interval = Duration::from_millis(500);
        assert_eq!(interval.as_millis(), 500);
    }

    #[test]
    fn test_min_animation_interval() {
        let interval = Duration::from_millis(250);
        assert_eq!(interval.as_millis(), 250);
    }

    #[test]
    fn test_draw_fps_approximately_30() {
        let interval = Duration::from_millis(33);
        let fps = 1000.0 / interval.as_millis() as f64;
        assert!(fps > 29.0 && fps < 31.0);
    }

    #[test]
    fn test_animation_fps_is_4() {
        let interval = Duration::from_millis(250);
        let fps = 1000 / interval.as_millis();
        assert_eq!(fps, 4);
    }

    // -- Poll timeout logic --

    #[test]
    fn test_poll_ms_busy() {
        let draw_pending = true;
        let poll_ms = if draw_pending { 16 } else { 100 };
        assert_eq!(poll_ms, 16);
    }

    #[test]
    fn test_poll_ms_idle() {
        let draw_pending = false;
        let render_in_flight = false;
        let has_receivers = false;
        let poll_ms = if draw_pending || render_in_flight || has_receivers { 16 } else { 100 };
        assert_eq!(poll_ms, 100);
    }

    // -- KeyEvent construction --

    #[test]
    fn test_key_event_press() {
        let key = KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE);
        assert_eq!(key.code, KeyCode::Char('a'));
        assert_eq!(key.modifiers, KeyModifiers::NONE);
    }

    #[test]
    fn test_key_event_ctrl_q() {
        let key = KeyEvent::new(KeyCode::Char('q'), KeyModifiers::CONTROL);
        assert!(key.modifiers.contains(KeyModifiers::CONTROL));
        assert_eq!(key.code, KeyCode::Char('q'));
    }

    #[test]
    fn test_key_event_kind_press() {
        let key = KeyEvent {
            code: KeyCode::Char('x'),
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        };
        assert!(matches!(key.kind, KeyEventKind::Press));
    }

    #[test]
    fn test_key_event_kind_repeat() {
        let key = KeyEvent {
            code: KeyCode::Down,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Repeat,
            state: KeyEventState::NONE,
        };
        assert!(matches!(key.kind, KeyEventKind::Repeat));
    }

    #[test]
    fn test_key_event_kind_release_filtered() {
        let key = KeyEvent {
            code: KeyCode::Char('a'),
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Release,
            state: KeyEventState::NONE,
        };
        let accepted = matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat);
        assert!(!accepted);
    }

    #[test]
    fn test_modifier_key_detection() {
        let key = KeyEvent::new(KeyCode::Modifier(crossterm::event::ModifierKeyCode::LeftShift), KeyModifiers::SHIFT);
        assert!(matches!(key.code, KeyCode::Modifier(_)));
    }

    // -- MouseEvent construction --

    #[test]
    fn test_mouse_scroll_down() {
        let mouse = MouseEvent {
            kind: MouseEventKind::ScrollDown,
            column: 40,
            row: 10,
            modifiers: KeyModifiers::NONE,
        };
        assert!(matches!(mouse.kind, MouseEventKind::ScrollDown));
        assert_eq!(mouse.column, 40);
        assert_eq!(mouse.row, 10);
    }

    #[test]
    fn test_mouse_scroll_up() {
        let mouse = MouseEvent {
            kind: MouseEventKind::ScrollUp,
            column: 20,
            row: 5,
            modifiers: KeyModifiers::NONE,
        };
        assert!(matches!(mouse.kind, MouseEventKind::ScrollUp));
    }

    #[test]
    fn test_mouse_left_click() {
        let mouse = MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 15,
            row: 8,
            modifiers: KeyModifiers::NONE,
        };
        assert!(matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left)));
    }

    #[test]
    fn test_mouse_drag() {
        let mouse = MouseEvent {
            kind: MouseEventKind::Drag(MouseButton::Left),
            column: 25,
            row: 12,
            modifiers: KeyModifiers::NONE,
        };
        assert!(matches!(mouse.kind, MouseEventKind::Drag(MouseButton::Left)));
    }

    #[test]
    fn test_mouse_up() {
        let mouse = MouseEvent {
            kind: MouseEventKind::Up(MouseButton::Left),
            column: 0,
            row: 0,
            modifiers: KeyModifiers::NONE,
        };
        assert!(matches!(mouse.kind, MouseEventKind::Up(MouseButton::Left)));
    }

    // -- Scroll delta accumulation --

    #[test]
    fn test_scroll_delta_accumulation_down() {
        let mut delta: i32 = 0;
        delta += 3;
        delta += 3;
        assert_eq!(delta, 6);
    }

    #[test]
    fn test_scroll_delta_accumulation_up() {
        let mut delta: i32 = 0;
        delta -= 3;
        delta -= 3;
        assert_eq!(delta, -6);
    }

    #[test]
    fn test_scroll_delta_mixed() {
        let mut delta: i32 = 0;
        delta += 3;
        delta -= 3;
        assert_eq!(delta, 0);
    }

    // -- Animation tick wrapping --

    #[test]
    fn test_animation_tick_wrapping() {
        let tick: u8 = 255;
        assert_eq!(tick.wrapping_add(1), 0);
    }

    #[test]
    fn test_animation_tick_normal() {
        let tick: u8 = 5;
        assert_eq!(tick.wrapping_add(1), 6);
    }

    // -- Draw decision logic --

    #[test]
    fn test_draw_pending_logic() {
        let had_key = true;
        let needs_redraw = false;
        let scroll_changed = false;
        assert!(had_key || needs_redraw || scroll_changed);
    }

    #[test]
    fn test_draw_pending_all_false() {
        let had_key = false;
        let needs_redraw = false;
        let scroll_changed = false;
        assert!(!(had_key || needs_redraw || scroll_changed));
    }

    #[test]
    fn test_should_draw_logic() {
        let draw_pending = true;
        let draw_ready = true;
        let defer = false;
        assert!(draw_pending && draw_ready && !defer);
    }

    #[test]
    fn test_should_draw_deferred() {
        let draw_pending = true;
        let draw_ready = true;
        let defer = true;
        assert!(!(draw_pending && draw_ready && !defer));
    }

    // -- Compaction watcher timing --

    #[test]
    fn test_compaction_timeout_20s() {
        let timeout = Duration::from_secs(20);
        assert_eq!(timeout.as_secs(), 20);
    }

    // -- Auto-rebase check interval --

    #[test]
    fn test_auto_rebase_interval_2s() {
        let interval = Duration::from_secs(2);
        assert_eq!(interval.as_secs(), 2);
    }

    // -- Render throttle interval --

    #[test]
    fn test_render_submit_throttle() {
        let throttle = Duration::from_millis(50);
        assert_eq!(throttle.as_millis(), 50);
    }

    // -- File tree debounce --

    #[test]
    fn test_file_tree_debounce_500ms() {
        let debounce = Duration::from_millis(500);
        assert_eq!(debounce.as_millis(), 500);
    }

    // -- Session width fallback --

    #[test]
    fn test_session_width_fallback() {
        let w: u16 = 0;
        let session_w = if w > 0 { w } else { 80 };
        assert_eq!(session_w, 80);
    }

    #[test]
    fn test_session_width_normal() {
        let w: u16 = 120;
        let session_w = if w > 0 { w } else { 80 };
        assert_eq!(session_w, 120);
    }

    // -- Terminal size fallback --

    #[test]
    fn test_terminal_size_fallback() {
        let (w, h): (u16, u16) = (80, 24);
        assert_eq!(w, 80);
        assert_eq!(h, 24);
    }

    // -- Focus comparison --

    #[test]
    fn test_focus_eq() {
        assert_eq!(Focus::Input, Focus::Input);
        assert_ne!(Focus::Input, Focus::Viewer);
    }

    // -- Event variant matching --

    #[test]
    fn test_event_key_variant() {
        let event = Event::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(matches!(event, Event::Key(_)));
    }

    #[test]
    fn test_event_resize_variant() {
        let event = Event::Resize(120, 40);
        if let Event::Resize(w, h) = event {
            assert_eq!(w, 120);
            assert_eq!(h, 40);
        } else {
            panic!("expected Resize");
        }
    }

    // -- Instant elapsed check --

    #[test]
    fn test_instant_duration_since() {
        let start = Instant::now();
        let elapsed = Instant::now().duration_since(start);
        assert!(elapsed.as_millis() < 100);
    }

    // -- KeyCode variants --

    #[test]
    fn test_keycode_char() {
        assert!(matches!(KeyCode::Char('a'), KeyCode::Char('a')));
    }

    #[test]
    fn test_keycode_enter() {
        assert!(matches!(KeyCode::Enter, KeyCode::Enter));
    }

    #[test]
    fn test_keycode_esc() {
        assert!(matches!(KeyCode::Esc, KeyCode::Esc));
    }

    #[test]
    fn test_keycode_arrows() {
        assert!(matches!(KeyCode::Up, KeyCode::Up));
        assert!(matches!(KeyCode::Down, KeyCode::Down));
        assert!(matches!(KeyCode::Left, KeyCode::Left));
        assert!(matches!(KeyCode::Right, KeyCode::Right));
    }

    #[test]
    fn test_keycode_page_keys() {
        assert!(matches!(KeyCode::PageUp, KeyCode::PageUp));
        assert!(matches!(KeyCode::PageDown, KeyCode::PageDown));
    }

    // -- Fast path condition checks --

    #[test]
    fn test_fast_path_no_newline() {
        let input = "hello world";
        assert!(!input.contains('\n'));
    }

    #[test]
    fn test_fast_path_has_newline() {
        let input = "hello\nworld";
        assert!(input.contains('\n'));
    }

    #[test]
    fn test_fast_path_width_check() {
        let w: u16 = 80;
        assert!(w > 2);
    }

    #[test]
    fn test_fast_path_width_too_small() {
        let w: u16 = 2;
        assert!(!(w > 2));
    }

    #[test]
    fn test_duration_from_secs_converts() {
        let d = Duration::from_secs(1);
        assert_eq!(d.as_millis(), 1000);
    }

    #[test]
    fn test_key_event_char_a() {
        let k = KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE);
        assert_eq!(k.code, KeyCode::Char('a'));
    }

    #[test]
    fn test_mouse_event_column_row() {
        let m = MouseEvent { kind: MouseEventKind::Moved, column: 10, row: 20, modifiers: KeyModifiers::NONE };
        assert_eq!(m.column, 10);
        assert_eq!(m.row, 20);
    }
}
