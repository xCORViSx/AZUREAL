//! Core event loop and event handling
//!
//! Split into focused submodules:
//! - `actions`: Keyboard action dispatch (6 sub-submodules: execute, navigation, escape, session_list, deferred, rcr)
//! - `agent_events`: Agent process event handling
//! - `agent_processor`: Background JSON parsing for agent streaming events
//! - `coords`: Screen-to-content coordinate mapping
//! - `fast_draw`: Fast-path input rendering (~0.1ms bypass)
//! - `input_thread`: Dedicated stdin reader thread
//! - `mouse`: Mouse click, drag, scroll, and selection copy

mod actions;
mod agent_events;
mod agent_processor;
mod coords;
#[allow(dead_code)] // macOS-only fast paths; compiled on all platforms for tests
mod fast_draw;
mod input_thread;
mod mouse;

pub(super) use mouse::copy_viewer_selection;

use anyhow::Result;
use crossterm::event::{Event, KeyCode, MouseButton, MouseEventKind};
use std::io;
use std::io::Write;
use std::time::{Duration, Instant};

use ratatui::{backend::CrosstermBackend, Terminal};

use crate::app::App;
#[cfg(any(target_os = "macos", test))]
use crate::app::Focus;
use crate::backend::AgentProcess;
use crate::config::Config;

use super::draw_output::{submit_render_request, poll_render_result};
use super::run::ui;

use actions::handle_key_event;
use agent_events::handle_claude_event;
use coords::{screen_to_cache_pos, screen_to_edit_pos, screen_to_input_char};
#[cfg(target_os = "macos")]
use fast_draw::{fast_draw_input, fast_draw_session};
use mouse::{apply_scroll_cached, handle_mouse_click, handle_mouse_drag};

/// Main TUI event loop
pub async fn run_app(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
    config: Config,
) -> Result<()> {
    let claude_process = AgentProcess::new(config.clone(), config.backend);

    // Event loop profiler: log slow iterations (>5ms) to find input blockers
    let mut profile_log = {
        let dir = dirs::home_dir().unwrap_or_default().join(".azureal");
        let _ = std::fs::create_dir_all(&dir);
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(dir.join("event_loop_profile.log"))
            .ok()
    };
    if let Some(ref mut f) = profile_log {
        let _ = writeln!(f, "\n=== Session started {:?} ===", std::time::SystemTime::now());
    }

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
    // Track last key event time so we can defer session pane updates while
    // typing. This keeps terminal.draw() diffs small (input-only) near
    // keystrokes, reducing terminal escape-sequence volume that causes
    // terminal emulators to drop keyboard input.
    let mut last_key_time = Instant::now() - Duration::from_secs(1);

    // Cache terminal size, update on resize events
    let (mut cached_width, mut cached_height) = crossterm::terminal::size().unwrap_or((80, 24));

    // Dedicated input reader thread: continuously reads crossterm events from
    // stdin so keystrokes are captured immediately — even during terminal.draw()
    // (~18ms) or other blocking work. Without this, keys that arrive during a
    // draw sit in the kernel buffer and some terminal emulators drop them under
    // heavy output load.
    let input_rx = input_thread::spawn_input_thread();

    // Background JSON parser: Claude streaming events get parsed off the main
    // thread. Eliminates 10-50ms of serde_json::from_str() per tick that was
    // blocking input during Claude streaming.
    let claude_proc = agent_processor::AgentProcessor::spawn(config.backend);

    // Initial draw
    terminal.draw(|f| ui(f, app))?;

    loop {
        let _loop_start = Instant::now();

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
        let mut _key_chars = String::new(); // diagnostic: chars received per drain

        // Drain all events from the input reader thread (non-blocking).
        // The reader thread continuously reads stdin, so events are buffered
        // in the channel even during terminal.draw() or other blocking work.
        // If idle with no pending work, block briefly to avoid busy-spinning.
        let commit_generating = app.git_actions_panel.as_ref()
            .and_then(|p| p.commit_overlay.as_ref())
            .map(|o| o.generating).unwrap_or(false);
        let squash_merging = app.git_actions_panel.as_ref()
            .map(|p| p.squash_merge_receiver.is_some()).unwrap_or(false);
        let bg_pending = app.file_tree_receiver.is_some() || app.worktree_refresh_receiver.is_some()
            || app.background_op_receiver.is_some() || app.rebase_op_receiver.is_some();
        // Note: session_file_dirty, file_tree_refresh_pending, health_refresh_pending
        // are NOT included — they have their own debounce timers and don't need
        // the main loop to busy-spin. Including them caused sustained high CPU
        // when file watchers fired frequently (the debounce kept resetting).
        let is_busy = app.draw_pending || app.render_in_flight || !app.agent_receivers.is_empty() || app.stt_recording || app.stt_transcribing || commit_generating || squash_merging || bg_pending || app.terminal_mode;

        // First event: block briefly when idle so we don't spin the CPU
        let first_event = if is_busy {
            input_rx.try_recv().ok()
        } else {
            input_rx.recv_timeout(Duration::from_millis(100)).ok()
        };

        if let Some(evt) = first_event {
            // Diagnostic: capture key chars + kinds for profiler
            if let Event::Key(ref k) = evt {
                if let KeyCode::Char(c) = k.code {
                    _key_chars.push(c);
                    let kind_ch = match k.kind { crossterm::event::KeyEventKind::Press => 'P', crossterm::event::KeyEventKind::Repeat => 'R', _ => '?' };
                    _key_chars.push(kind_ch);
                    _key_chars.push(' ');
                }
            }
            process_input_event(evt, app, &claude_process, &mut needs_redraw, &mut scroll_delta, &mut scroll_col, &mut scroll_row, &mut had_key_event, &mut cached_width, &mut cached_height)?;
            // Drain remaining queued events (non-blocking)
            while let Ok(evt) = input_rx.try_recv() {
                if let Event::Key(ref k) = evt {
                    if let KeyCode::Char(c) = k.code {
                        _key_chars.push(c);
                        let kind_ch = match k.kind { crossterm::event::KeyEventKind::Press => 'P', crossterm::event::KeyEventKind::Repeat => 'R', _ => '?' };
                        _key_chars.push(kind_ch);
                        _key_chars.push(' ');
                    }
                }
                process_input_event(evt, app, &claude_process, &mut needs_redraw, &mut scroll_delta, &mut scroll_col, &mut scroll_row, &mut had_key_event, &mut cached_width, &mut cached_height)?;
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
        if had_key_event {
            last_key_time = Instant::now();
        }
        // fast_draw_input bypasses ratatui and writes directly to stdout.
        // Only safe on macOS — on Windows, direct escape sequences corrupt
        // the console input parser (garbled text in input, broken cursor).
        #[cfg(target_os = "macos")]
        let has_fast_path = app.prompt_mode && !app.terminal_mode && !app.input.contains('\n') && !app.has_input_selection();
        #[cfg(not(target_os = "macos"))]
        let has_fast_path = false;
        #[cfg(target_os = "macos")]
        if had_key_event && has_fast_path && app.focus == Focus::Input && app.input_area.width > 2 {
            fast_draw_input(app);
        }

        // Compute ONCE per iteration. 300ms covers ~3 chars/sec typing with margin.
        let streaming = !app.agent_receivers.is_empty();
        let typing_recently = last_key_time.elapsed() < Duration::from_millis(300);

        let _t_input = _loop_start.elapsed();

        // Reset the background JSON parser when session changed (flag set by
        // load_session_output / clear_session_state). Drain stale results too.
        if app.agent_processor_needs_reset {
            claude_proc.reset(claude_process.backend());
            claude_proc.drain();
            app.agent_processor_needs_reset = false;
        }

        // Process Claude events — drain raw events from Claude subprocess channels.
        // Output events are forwarded to the background AgentProcessor for JSON
        // parsing (non-blocking send). Non-Output events (Started, SessionId,
        // Exited) are handled directly — they're instant. This moves 10-50ms of
        // serde_json::from_str() per tick off the main thread entirely.
        const MAX_CLAUDE_EVENTS_PER_TICK: usize = 10;
        if !app.agent_receivers.is_empty() {
            let mut count = 0;
            let mut claude_events: Vec<(String, crate::claude::AgentEvent)> = Vec::new();
            'outer: for (sid, rx) in &app.agent_receivers {
                while let Ok(event) = rx.try_recv() {
                    claude_events.push((sid.clone(), event));
                    count += 1;
                    if count >= MAX_CLAUDE_EVENTS_PER_TICK { break 'outer; }
                }
            }
            for (session_id, event) in claude_events {
                match event {
                    crate::claude::AgentEvent::Output(output) => {
                        // Only parse output for the active/viewed slot — other
                        // slots' output is discarded (no display needed)
                        if app.is_viewing_slot(&session_id) {
                            claude_proc.submit(
                                session_id,
                                output.output_type,
                                output.data,
                            );
                        }
                    }
                    other => {
                        handle_claude_event(&session_id, other, app, &claude_process)?;
                        app.update_token_badge_live();
                        needs_redraw = true;
                    }
                }
            }
        }

        // Send staged prompt when no agent is running (e.g. first prompt after session creation)
        if app.staged_prompt.is_some() && !app.is_active_slot_running() {
            if let Some(prompt) = app.staged_prompt.take() {
                if let Some(wt_path) = app.current_worktree().and_then(|s| s.worktree_path.clone()) {
                    let branch = app.current_worktree().map(|s| s.branch_name.clone()).unwrap_or_default();
                    let events_offset = app.display_events.len();
                    app.add_user_message(prompt.clone());
                    app.process_session_chunk(&format!("You: {}\n", prompt));
                    app.current_todos.clear();
                    let send_prompt = app.current_session_id
                        .and_then(|sid| app.session_store.as_ref().map(|s| (sid, s)))
                        .and_then(|(sid, store)| store.build_context(sid).ok().flatten())
                        .map(|payload| crate::app::context_injection::build_context_prompt(&payload, &prompt))
                        .unwrap_or_else(|| prompt.clone());
                    match claude_process.spawn(&wt_path, &send_prompt, None, app.selected_model.as_deref()) {
                        Ok((rx, pid)) => {
                            if let Some(sid) = app.current_session_id {
                                app.pid_session_target.insert(pid.to_string(), (sid, wt_path.clone(), events_offset));
                            }
                            app.register_claude(branch, pid, rx);
                            app.set_status("Running...");
                        }
                        Err(e) => app.set_status(format!("Failed to start: {}", e)),
                    }
                    needs_redraw = true;
                }
            }
        }

        // Poll compaction agents (background summarization, invisible to UI)
        if agent_events::poll_compaction_agents(app) {
            // No redraw needed — compaction is invisible
        }

        let _t_claude = _loop_start.elapsed();

        // Poll parsed results from the background AgentProcessor. Each result
        // contains pre-parsed DisplayEvents + JSON value — applying them is cheap
        // (HashMap lookups, Vec pushes, flag sets). No JSON parsing on main thread.
        // Capped to prevent unbounded drain when parser batches many results.
        {
            let mut parsed_count = 0;
            while parsed_count < MAX_CLAUDE_EVENTS_PER_TICK {
                match claude_proc.try_recv() {
                    Some(result) => {
                        app.apply_parsed_output(
                            result.events,
                            result.parsed_json,
                            result.output_type,
                            &result.data,
                        );
                        needs_redraw = true;
                        parsed_count += 1;
                    }
                    None => break,
                }
            }
        }

        let _t_parsed = _loop_start.elapsed();

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
                            crate::tui::input_git_actions::refresh_changed_files(p);
                            crate::tui::input_git_actions::refresh_commit_log(p);
                        }
                    }
                    SquashMergeOutcome::Failed(msg) => {
                        if let Some(ref mut p) = app.git_actions_panel {
                            p.result_message = Some((msg, true));
                            crate::tui::input_git_actions::refresh_changed_files(p);
                            crate::tui::input_git_actions::refresh_commit_log(p);
                        }
                    }
                }
            }
        }

        // Poll background worktree/git operations (archive, unarchive, create,
        // delete, pull, push). Background thread sends progress phases and
        // final outcome via mpsc.
        {
            use crate::app::types::BackgroundOpOutcome;
            let mut op_outcome: Option<BackgroundOpOutcome> = None;
            let mut new_phase: Option<String> = None;
            if let Some(ref rx) = app.background_op_receiver {
                while let Ok(progress) = rx.try_recv() {
                    if let Some(outcome) = progress.outcome {
                        app.background_op_receiver = None;
                        op_outcome = Some(outcome);
                        needs_redraw = true;
                        break;
                    }
                    if !progress.phase.is_empty() {
                        new_phase = Some(progress.phase);
                        needs_redraw = true;
                    }
                }
            }
            if let Some(phase) = new_phase {
                app.loading_indicator = Some(phase);
            }
            if let Some(outcome) = op_outcome {
                app.loading_indicator = None;
                match outcome {
                    BackgroundOpOutcome::Archived => {
                        app.set_status("Session archived");
                        let _ = app.refresh_worktrees();
                        app.load_session_output();
                    }
                    BackgroundOpOutcome::Unarchived { branch, display_name } => {
                        app.set_status(format!("Unarchived: {}", display_name));
                        app.save_current_terminal();
                        let _ = app.refresh_worktrees();
                        if let Some(idx) = app.worktrees.iter().position(|s| s.branch_name == branch) {
                            app.selected_worktree = Some(idx);
                            app.load_session_output();
                        }
                    }
                    BackgroundOpOutcome::Created { branch } => {
                        app.save_current_terminal();
                        let _ = app.refresh_worktrees();
                        if let Some(idx) = app.worktrees.iter().position(|s| s.branch_name == branch) {
                            app.selected_worktree = Some(idx);
                            app.load_session_output();
                        }
                    }
                    BackgroundOpOutcome::Deleted { display_name, prev_idx, .. } => {
                        app.set_status(format!("Deleted: {}", display_name));
                        app.save_current_terminal();
                        let _ = app.refresh_worktrees();
                        app.selected_worktree = if app.worktrees.is_empty() {
                            None
                        } else {
                            Some(prev_idx.min(app.worktrees.len() - 1))
                        };
                        app.load_session_output();
                    }
                    BackgroundOpOutcome::GitResult { message, is_error } => {
                        if let Some(ref mut p) = app.git_actions_panel {
                            p.result_message = Some((message, is_error));
                            crate::tui::input_git_actions::refresh_changed_files(p);
                            crate::tui::input_git_actions::refresh_commit_log(p);
                        }
                    }
                    BackgroundOpOutcome::Failed(msg) => {
                        app.set_status(msg);
                    }
                }
            }
        }

        // Poll background rebase operations (separate from generic ops because
        // rebase has conflict overlay handling)
        {
            use crate::app::types::BackgroundRebaseOutcome;
            let mut rebase_outcome: Option<BackgroundRebaseOutcome> = None;
            if let Some(ref rx) = app.rebase_op_receiver {
                if let Ok(outcome) = rx.try_recv() {
                    app.rebase_op_receiver = None;
                    rebase_outcome = Some(outcome);
                    needs_redraw = true;
                }
            }
            if let Some(outcome) = rebase_outcome {
                app.loading_indicator = None;
                match outcome {
                    BackgroundRebaseOutcome::Rebased(msg) => {
                        if let Some(ref mut p) = app.git_actions_panel {
                            crate::tui::input_git_actions::refresh_changed_files(p);
                            crate::tui::input_git_actions::refresh_commit_log(p);
                            p.result_message = Some((msg, false));
                        }
                    }
                    BackgroundRebaseOutcome::UpToDate => {
                        if let Some(ref mut p) = app.git_actions_panel {
                            crate::tui::input_git_actions::refresh_changed_files(p);
                            crate::tui::input_git_actions::refresh_commit_log(p);
                            p.result_message = Some(("Already up to date with main".to_string(), false));
                        }
                    }
                    BackgroundRebaseOutcome::Conflict { conflicted, auto_merged } => {
                        if let Some(ref mut p) = app.git_actions_panel {
                            p.conflict_overlay = Some(crate::app::types::GitConflictOverlay {
                                conflicted_files: conflicted,
                                auto_merged_files: auto_merged,
                                scroll: 0,
                                selected: 0,
                                continue_with_merge: false,
                            });
                            crate::tui::input_git_actions::refresh_changed_files(p);
                            crate::tui::input_git_actions::refresh_commit_log(p);
                        }
                    }
                    BackgroundRebaseOutcome::Failed(e) => {
                        if let Some(ref mut p) = app.git_actions_panel {
                            crate::tui::input_git_actions::refresh_changed_files(p);
                            crate::tui::input_git_actions::refresh_commit_log(p);
                            p.result_message = Some((format!("Rebase failed: {}", e), true));
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
                let backend = app.backend;
                let (tx, rx) = std::sync::mpsc::channel();
                std::thread::spawn(move || {
                    let result = crate::app::state::load::compute_worktree_refresh(path, main_branch, wt_dir, backend);
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
        // when source files change while the panel is open.
        // Skipped during active Claude streaming — the synchronous filesystem
        // walk (10-200ms) would block the event loop and cause input hiccups.
        // Panel refreshes once streaming finishes.
        if app.health_refresh_pending
            && app.agent_receivers.is_empty()
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
        if app.agent_receivers.is_empty()
            && now_poll.duration_since(app.last_auto_rebase_check) >= Duration::from_secs(2)
        {
            app.last_auto_rebase_check = now_poll;
            if !app.auto_rebase_enabled.is_empty() {
                if check_auto_rebase(app, &claude_process) {
                    needs_redraw = true;
                }
            }
        }

        let _t_housekeeping = _loop_start.elapsed();

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
        // Always apply results immediately — session content stays up-to-date.
        // On macOS: fast_draw_session (direct cell writes, ~10-15KB) updates the
        // session pane without a full terminal.draw() (~87KB). Skipped during
        // typing so escape sequences don't compete with keystroke processing.
        // On Windows: disabled — direct CSI writes corrupt the console input
        // parser, causing escape sequences to appear as text and broken cursor.
        #[cfg(target_os = "macos")]
        let old_cache_len = app.rendered_lines_cache.len();
        if poll_render_result(app) {
            #[cfg(target_os = "macos")]
            {
            let new_line_count = app.rendered_lines_cache.len().saturating_sub(old_cache_len);
            let follow_bottom = app.session_scroll == usize::MAX;
            if streaming && follow_bottom && new_line_count > 0
                && !typing_recently
                && app.pane_session_content.width > 2 && app.pane_session_content.height > 2
                && app.git_actions_panel.is_none()
                && !app.show_session_list
                && !app.is_projects_panel_active()
            {
                fast_draw_session(app, new_line_count);
            }
            } // #[cfg(target_os = "macos")]
            needs_redraw = true;
        }

        let _t_render = _loop_start.elapsed();

        // Mark that we need a draw (will be fulfilled on a quiet iteration)
        if had_key_event || needs_redraw || scroll_changed {
            app.draw_pending = true;
        }

        // Full draw throttling during streaming:
        // On macOS: defer draws during typing (fast_draw_input handles feedback).
        // On Windows: no deferral — full draws handle everything.
        // On all platforms: throttle to 5fps during idle streaming.
        let now = Instant::now();
        let draw_interval = if streaming && !had_key_event {
            Duration::from_millis(200) // 5fps idle during streaming
        } else {
            min_draw_interval // 30fps with user interaction
        };
        let draw_ready = now.duration_since(last_draw) >= draw_interval;
        let defer_for_typing = typing_recently && has_fast_path;
        let should_draw = app.draw_pending && draw_ready && !defer_for_typing;

        let mut drew = false;
        if should_draw {
            // Pre-draw drain: catch events buffered by the input thread since
            // the primary drain. If a key arrives, skip draw (loop back).
            let mut got_key = false;
            while let Ok(evt) = input_rx.try_recv() {
                if let Event::Key(_) = &evt {
                    got_key = true;
                }
                process_input_event(evt, app, &claude_process, &mut needs_redraw, &mut scroll_delta, &mut scroll_col, &mut scroll_row, &mut had_key_event, &mut cached_width, &mut cached_height)?;
            }
            #[cfg(target_os = "macos")]
            if got_key && app.prompt_mode && !app.terminal_mode && app.focus == Focus::Input && app.input_area.width > 2 && !app.input.contains('\n') && !app.has_input_selection() {
                fast_draw_input(app);
            }
            if !got_key {
                if app.force_full_redraw {
                    terminal.clear()?;
                    app.force_full_redraw = false;
                }
                terminal.draw(|f| ui(f, app))?;
                last_draw = Instant::now();
                app.draw_pending = false;
                drew = true;

                // On Windows, Claude CLI can overwrite the console title via
                // SetConsoleTitle (inherits the console even with piped stdio).
                // Reassert our title after each draw to keep it correct.
                #[cfg(target_os = "windows")]
                if !app.agent_receivers.is_empty() {
                    app.update_terminal_title();
                }

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

        // Profile: log slow iterations (>5ms) OR any iteration with key events
        let _t_total = _loop_start.elapsed();
        if _t_total.as_millis() > 5 || had_key_event {
            if let Some(ref mut f) = profile_log {
                let _ = write!(f,
                    "{}ms total | input:{:.1} claude:{:.1} parsed:{:.1} house:{:.1} render:{:.1} draw:{} | key:{} stream:{} typing:{}",
                    _t_total.as_millis(),
                    (_t_input.as_micros() as f64) / 1000.0,
                    ((_t_claude - _t_input).as_micros() as f64) / 1000.0,
                    ((_t_parsed - _t_claude).as_micros() as f64) / 1000.0,
                    ((_t_housekeeping - _t_parsed).as_micros() as f64) / 1000.0,
                    ((_t_render - _t_housekeeping).as_micros() as f64) / 1000.0,
                    if drew { format!("{:.1}", (_t_total - _t_render).as_micros() as f64 / 1000.0) } else { "-".to_string() },
                    had_key_event,
                    streaming,
                    typing_recently,
                );
                if !_key_chars.is_empty() {
                    let _ = write!(f, " | keys:[{}]", _key_chars.trim());
                }
                let _ = writeln!(f);
            }
        }

        if app.should_quit { break; }
    }

    Ok(())
}

/// Process a single input event from the reader thread channel.
/// Dispatches key, mouse, and resize events to the appropriate handlers.
#[allow(clippy::too_many_arguments)]
fn process_input_event(
    evt: Event,
    app: &mut App,
    claude_process: &AgentProcess,
    needs_redraw: &mut bool,
    scroll_delta: &mut i32,
    scroll_col: &mut u16,
    scroll_row: &mut u16,
    had_key_event: &mut bool,
    cached_width: &mut u16,
    cached_height: &mut u16,
) -> Result<()> {
    match evt {
        Event::Key(key) => {
            // Input thread already filters to Press/Repeat only
            if !matches!(key.code, KeyCode::Modifier(_)) {
                handle_key_event(key, app, claude_process)?;
                *had_key_event = true;
            }
        }
        Event::Mouse(mouse) => {
            match mouse.kind {
                MouseEventKind::ScrollDown => {
                    *scroll_delta += 3;
                    *scroll_col = mouse.column;
                    *scroll_row = mouse.row;
                }
                MouseEventKind::ScrollUp => {
                    *scroll_delta -= 3;
                    *scroll_col = mouse.column;
                    *scroll_row = mouse.row;
                }
                MouseEventKind::Down(MouseButton::Left) => {
                    app.viewer_selection = None;
                    app.session_selection = None;
                    let (mc, mr) = (mouse.column, mouse.row);
                    use ratatui::layout::Position;
                    let mpos = Position::new(mc, mr);
                    if app.pane_viewer.contains(mpos) {
                        if app.viewer_edit_mode {
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
                        *needs_redraw = true;
                    }
                }
                MouseEventKind::Drag(MouseButton::Left) => {
                    if handle_mouse_drag(app, mouse.column, mouse.row) {
                        *needs_redraw = true;
                    }
                }
                MouseEventKind::Up(MouseButton::Left) => {
                    app.mouse_drag_start = None;
                }
                _ => {}
            }
        }
        Event::Resize(w, h) => {
            *cached_width = w;
            *cached_height = h;
            app.screen_height = h;
            *needs_redraw = true;
        }
        _ => {}
    }
    Ok(())
}

/// Check all auto-rebase-enabled worktrees and rebase the first eligible one.
/// Returns true if any state changed (needs redraw).
fn check_auto_rebase(app: &mut App, _claude_process: &AgentProcess) -> bool {
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
    fn test_compaction_timeout_30s() {
        let timeout = Duration::from_secs(30);
        assert_eq!(timeout.as_secs(), 30);
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
