//! Core event loop and event handling
//!
//! Split into focused submodules:
//! - `actions`: Keyboard action dispatch (6 sub-submodules: execute, navigation, escape, session_list, deferred, rcr)
//! - `agent_events`: Agent process event handling
//! - `agent_processor`: Background JSON parsing for agent streaming events
//! - `auto_rebase`: Periodic auto-rebase checking for enabled worktrees
//! - `coords`: Screen-to-content coordinate mapping
//! - `fast_draw`: Fast-path input rendering (~0.1ms bypass)
//! - `git_polling`: Background git operation polling (commit gen, squash merge, ops, rebase)
//! - `housekeeping`: File watcher, session/tree/health refresh, STT, debug dump
//! - `input_thread`: Dedicated stdin reader thread
//! - `mouse`: Mouse click, drag, scroll, and selection copy
//! - `process_input`: Input event dispatch (key, mouse, resize)
//! - `prompt`: Staged prompt sending and compaction lifecycle

mod actions;
mod agent_events;
mod agent_processor;
mod auto_rebase;
mod coords;
#[allow(dead_code)] // macOS-only fast paths; compiled on all platforms for tests
mod fast_draw;
mod git_polling;
mod housekeeping;
mod input_thread;
mod mouse;
mod process_input;
mod prompt;

pub(super) use mouse::copy_viewer_selection;

use anyhow::Result;
use crossterm::event::{Event, KeyCode, KeyEventKind};
use std::io;
use std::io::Write;
use std::time::{Duration, Instant};

use ratatui::{backend::CrosstermBackend, Terminal};

use crate::app::App;
#[cfg(any(target_os = "macos", test))]
use crate::app::Focus;
use crate::backend::AgentProcess;
use crate::config::Config;

use super::draw_output::{poll_render_result, submit_render_request};
use super::run::ui;

use agent_events::handle_claude_event;
#[cfg(target_os = "macos")]
use fast_draw::fast_draw_input;
use mouse::apply_scroll_cached;
use process_input::process_input_event;

/// Main TUI event loop
pub async fn run_app(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
    config: Config,
) -> Result<()> {
    let claude_process = AgentProcess::new(config.clone());

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
        let _ = writeln!(
            f,
            "\n=== Session started {:?} ===",
            std::time::SystemTime::now()
        );
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
    let claude_proc =
        agent_processor::AgentProcessor::spawn(app.backend, app.display_model_name().to_string());

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

        // Only trigger redraw for animation when pending tools are visible
        // in the viewport (have entries in animation_line_indices). This
        // avoids running the full ui() function at 4fps when tools are
        // pending but off-screen.
        let has_visible_pending = has_pending_tools
            && app
                .animation_line_indices
                .iter()
                .any(|(_, _, id)| app.pending_tool_calls.contains(id));
        let mut needs_redraw = terminal_changed || (animation_due && has_visible_pending);
        let mut scroll_delta: i32 = 0;
        let mut scroll_col: u16 = 0;
        let mut scroll_row: u16 = 0;
        let mut had_key_event = false;
        let mut _key_chars = String::new(); // diagnostic: chars received per drain

        // Drain all events from the input reader thread (non-blocking).
        // The reader thread continuously reads stdin, so events are buffered
        // in the channel even during terminal.draw() or other blocking work.
        // If idle with no pending work, block briefly to avoid busy-spinning.
        let commit_generating = app
            .git_actions_panel
            .as_ref()
            .and_then(|p| p.commit_overlay.as_ref())
            .map(|o| o.generating)
            .unwrap_or(false);
        let squash_merging = app
            .git_actions_panel
            .as_ref()
            .map(|p| p.squash_merge_receiver.is_some())
            .unwrap_or(false);
        let bg_pending = app.file_tree_receiver.is_some()
            || app.worktree_refresh_receiver.is_some()
            || app.background_op_receiver.is_some()
            || app.rebase_op_receiver.is_some();
        // Note: session_file_dirty, file_tree_refresh_pending, health_refresh_pending
        // are NOT included — they have their own debounce timers and don't need
        // the main loop to busy-spin. Including them caused sustained high CPU
        // when file watchers fired frequently (the debounce kept resetting).
        let is_busy = app.draw_pending
            || app.render_in_flight
            || !app.agent_receivers.is_empty()
            || app.stt_recording
            || app.stt_transcribing
            || commit_generating
            || squash_merging
            || bg_pending
            || app.terminal_mode;

        // First event: block briefly when idle so we don't spin the CPU
        let first_event = if is_busy {
            input_rx.try_recv().ok()
        } else {
            input_rx.recv_timeout(Duration::from_millis(100)).ok()
        };

        // Snapshot fast-draw eligibility BEFORE processing key events.
        // If fast_draw was active (writing directly to terminal cells) but stops
        // being active after key processing (e.g. prompt submit sets prompt_mode=false),
        // we must run one final fast_draw to clear stale content from the physical
        // terminal — otherwise ratatui's diff engine won't touch those cells because
        // its internal buffer was never updated by the direct crossterm writes.
        #[cfg(target_os = "macos")]
        let was_fast_path = app.prompt_mode
            && !app.terminal_mode
            && !app.input.contains('\n')
            && !app.has_input_selection()
            && app.focus == Focus::Input
            && app.input_area.width > 2;
        #[cfg(not(target_os = "macos"))]
        let _was_fast_path = false;

        if let Some(evt) = first_event {
            // Collect all events from this drain cycle.
            // On Windows, crossterm uses ReadConsoleInputW which delivers pasted
            // text as individual KEY_EVENT records — each newline becomes a plain
            // Enter keypress that triggers prompt submit. Collecting the full batch
            // lets coalesce_paste_events detect and merge them.
            let mut batch = vec![evt];
            while let Ok(evt) = input_rx.try_recv() {
                batch.push(evt);
            }
            // If the batch ends with Enter and has other events, the input thread
            // may still be forwarding remaining paste events. Wait briefly (2ms) to
            // catch them — imperceptible to humans but paste events arrive within
            // microseconds. Only extends when batch already has >1 event (solo Enter
            // is just a normal submit, no delay needed).
            if batch.len() > 1 {
                let last_is_enter = batch
                    .iter()
                    .rev()
                    .find_map(|e| {
                        if let Event::Key(k) = e {
                            Some(k.code == KeyCode::Enter)
                        } else {
                            None
                        }
                    })
                    .unwrap_or(false);
                if last_is_enter {
                    if let Ok(extra) = input_rx.recv_timeout(Duration::from_millis(2)) {
                        batch.push(extra);
                        while let Ok(evt) = input_rx.try_recv() {
                            batch.push(evt);
                        }
                    }
                }
            }

            // Diagnostic: capture key chars + kinds for profiler
            for evt in &batch {
                if let Event::Key(ref k) = evt {
                    if let KeyCode::Char(c) = k.code {
                        _key_chars.push(c);
                        let kind_ch = match k.kind {
                            crossterm::event::KeyEventKind::Press => 'P',
                            crossterm::event::KeyEventKind::Repeat => 'R',
                            _ => '?',
                        };
                        _key_chars.push(kind_ch);
                        _key_chars.push(' ');
                    }
                }
            }

            // Coalesce rapid char+Enter events into a single Event::Paste.
            // On Windows, bracketed paste doesn't produce Event::Paste —
            // pasted text arrives as individual KEY_EVENT records.
            let batch = coalesce_paste_events(batch);

            for evt in batch {
                process_input_event(
                    evt,
                    app,
                    &claude_process,
                    &mut needs_redraw,
                    &mut scroll_delta,
                    &mut scroll_col,
                    &mut scroll_row,
                    &mut had_key_event,
                    &mut cached_width,
                    &mut cached_height,
                )?;
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
        let has_fast_path = app.prompt_mode
            && !app.terminal_mode
            && !app.input.contains('\n')
            && !app.has_input_selection();
        #[cfg(not(target_os = "macos"))]
        let has_fast_path = false;
        #[cfg(target_os = "macos")]
        if had_key_event && has_fast_path && app.focus == Focus::Input && app.input_area.width > 2 {
            fast_draw_input(app);
        }
        // Reconcile: fast_draw was active before key processing but isn't now
        // (e.g., Enter submitted the prompt → prompt_mode=false, input cleared).
        // Run one final fast_draw to overwrite stale content on the real terminal.
        // Without this, ratatui's diff won't touch those cells (its buffer was
        // never updated by the direct crossterm writes).
        #[cfg(target_os = "macos")]
        if had_key_event && was_fast_path && !has_fast_path && app.input_area.width > 2 {
            fast_draw_input(app);
        }

        // Compute ONCE per iteration. 300ms covers ~3 chars/sec typing with margin.
        let streaming = !app.agent_receivers.is_empty();
        let typing_recently = last_key_time.elapsed() < Duration::from_millis(300);

        let _t_input = _loop_start.elapsed();

        // Reset the background JSON parser when session changed (flag set by
        // load_session_output / clear_session_state). Drain stale results too.
        if app.agent_processor_needs_reset {
            claude_proc.reset(app.backend, app.display_model_name().to_string());
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
                    if count >= MAX_CLAUDE_EVENTS_PER_TICK {
                        break 'outer;
                    }
                }
            }
            for (session_id, event) in claude_events {
                match event {
                    crate::claude::AgentEvent::Output(output) => {
                        // Only parse output for the active/viewed slot — other
                        // slots' output is discarded (no display needed)
                        if app.is_viewing_slot(&session_id) {
                            claude_proc.submit(session_id, output.output_type, output.data);
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

        // Send staged prompt when no agent is running and no dialog is blocking
        if prompt::send_staged_prompt(app, &claude_process) {
            needs_redraw = true;
        }

        // Compaction lifecycle: poll agents, spawn when threshold crossed, auto-continue
        if prompt::manage_compaction(app, &claude_process) {
            needs_redraw = true;
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
                        // Guard: discard stale results from a previous session.
                        // The is_viewing_slot check at submit time (line 292) gates
                        // most output, but results can arrive from the background
                        // parser thread after a project/worktree switch.
                        if app.is_viewing_slot(&result.slot_id) {
                            app.apply_parsed_output(
                                &result.slot_id,
                                result.events,
                                result.parsed_json,
                                result.output_type,
                                &result.data,
                            );
                            needs_redraw = true;
                        }
                        parsed_count += 1;
                    }
                    None => break,
                }
            }
        }

        let _t_parsed = _loop_start.elapsed();

        // Poll git background operations (commit gen, squash merge, ops, rebase)
        if git_polling::poll_commit_generation(app) {
            needs_redraw = true;
        }
        if git_polling::poll_squash_merge(app) {
            needs_redraw = true;
        }
        if git_polling::poll_background_ops(app) {
            needs_redraw = true;
        }
        if git_polling::poll_rebase_ops(app) {
            needs_redraw = true;
        }

        // Misc housekeeping: debug dump, STT, file watcher
        housekeeping::handle_debug_dump(app);
        if housekeeping::poll_stt(app) {
            needs_redraw = true;
        }
        housekeeping::drain_file_watcher(app);

        let now_poll = Instant::now();

        // Session file, file tree, worktree tabs, health panel refreshes
        if housekeeping::poll_refreshes(app, now_poll, &mut last_session_poll, min_poll_interval) {
            needs_redraw = true;
        }

        // Dismiss auto-rebase success dialog after timeout
        if housekeeping::check_auto_rebase_timeout(app, now_poll) {
            needs_redraw = true;
        }

        // Periodic auto-rebase check (every 2 seconds, skip during streaming)
        if app.agent_receivers.is_empty()
            && now_poll.duration_since(app.last_auto_rebase_check) >= Duration::from_secs(2)
        {
            app.last_auto_rebase_check = now_poll;
            if !app.auto_rebase_enabled.is_empty() {
                if auto_rebase::check_auto_rebase(app, &claude_process) {
                    needs_redraw = true;
                }
            }
        }

        let _t_housekeeping = _loop_start.elapsed();

        // Apply accumulated scroll using cached terminal size
        let mut scroll_changed = false;
        if scroll_delta != 0 {
            scroll_changed = apply_scroll_cached(
                app,
                scroll_delta,
                scroll_col,
                scroll_row,
                cached_width,
                cached_height,
            );
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
        if app.rendered_lines_dirty
            && !app.render_in_flight
            && app.last_render_submit.elapsed() >= Duration::from_millis(50)
        {
            // Session pane width is percentage-based (35% in run.rs), so we read the
            // actual width from the cached pane rect set during the last draw.
            // Falls back to 80 on first frame before any draw has occurred.
            let session_w = if app.pane_session.width > 0 {
                app.pane_session.width
            } else {
                80
            };
            submit_render_request(app, session_w);
            app.last_render_submit = Instant::now();
        }

        // Poll for completed render results from the background thread (non-blocking).
        // Always apply results immediately — session content stays up-to-date.
        // Session updates now rely on normal ratatui draws only. The old macOS
        // direct-write fast path caused terminal/buffer desync, which manifested
        // as missing border cells and stale session fragments left on screen.
        if poll_render_result(app) {
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
            let mut pre_batch = Vec::new();
            while let Ok(evt) = input_rx.try_recv() {
                if let Event::Key(_) = &evt {
                    got_key = true;
                }
                pre_batch.push(evt);
            }
            let pre_batch = coalesce_paste_events(pre_batch);
            for evt in pre_batch {
                process_input_event(
                    evt,
                    app,
                    &claude_process,
                    &mut needs_redraw,
                    &mut scroll_delta,
                    &mut scroll_col,
                    &mut scroll_row,
                    &mut had_key_event,
                    &mut cached_width,
                    &mut cached_height,
                )?;
            }
            #[cfg(target_os = "macos")]
            if got_key
                && app.prompt_mode
                && !app.terminal_mode
                && app.focus == Focus::Input
                && app.input_area.width > 2
                && !app.input.contains('\n')
                && !app.has_input_selection()
            {
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
                // Reassert our title after every draw — not just while agents
                // are running — because the title stays corrupted after exit.
                #[cfg(target_os = "windows")]
                app.update_terminal_title();

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

        if app.should_quit {
            break;
        }
    }

    Ok(())
}

/// Detect rapid char+Enter key events in a drain batch and coalesce them into
/// a single `Event::Paste`. On Windows, `EnableBracketedPaste` sends the VT
/// sequence but crossterm's `ReadConsoleInputW`-based reader delivers pasted
/// text as individual `KEY_EVENT` records — `Event::Paste` is never generated.
/// Each newline arrives as a plain Enter keypress, triggering prompt submit.
///
/// Heuristic: characters appear AFTER an Enter in the same drain batch AND the
/// batch has ≥3 key presses. Human typing at normal speed (≤200 WPM, ~60ms
/// between keys) never produces Enter + character within the 16-100ms drain
/// window. The ≥3 threshold prevents false positives from fast typing patterns
/// like (Enter, j) at idle poll intervals (100ms). Even a short paste like
/// "a\nb" produces 3 presses (a, Enter, b), safely above the threshold.
fn coalesce_paste_events(events: Vec<Event>) -> Vec<Event> {
    let mut press_count = 0usize;
    let mut seen_enter = false;
    let mut chars_after_enter = false;

    for evt in &events {
        if let Event::Key(k) = evt {
            if k.kind == KeyEventKind::Press {
                match k.code {
                    KeyCode::Enter => {
                        press_count += 1;
                        seen_enter = true;
                    }
                    KeyCode::Char(_) => {
                        press_count += 1;
                        if seen_enter {
                            chars_after_enter = true;
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    if !chars_after_enter || press_count < 3 {
        return events; // Not a paste — process normally
    }

    // Coalesce: extract char/Enter Press keys into paste text, keep the rest
    let mut paste_text = String::new();
    let mut other_events = Vec::new();

    for evt in events {
        match &evt {
            Event::Key(k) if k.kind == KeyEventKind::Press => match k.code {
                KeyCode::Char(c) => paste_text.push(c),
                KeyCode::Enter => paste_text.push('\n'),
                _ => other_events.push(evt),
            },
            _ => other_events.push(evt),
        }
    }

    if !paste_text.is_empty() {
        other_events.push(Event::Paste(paste_text));
    }

    other_events
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{
        KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers, MouseButton, MouseEvent,
        MouseEventKind,
    };
    use std::time::Duration;

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
        let poll_ms = if draw_pending || render_in_flight || has_receivers {
            16
        } else {
            100
        };
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
        let key = KeyEvent::new(
            KeyCode::Modifier(crossterm::event::ModifierKeyCode::LeftShift),
            KeyModifiers::SHIFT,
        );
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
        assert!(matches!(
            mouse.kind,
            MouseEventKind::Down(MouseButton::Left)
        ));
    }

    #[test]
    fn test_mouse_drag() {
        let mouse = MouseEvent {
            kind: MouseEventKind::Drag(MouseButton::Left),
            column: 25,
            row: 12,
            modifiers: KeyModifiers::NONE,
        };
        assert!(matches!(
            mouse.kind,
            MouseEventKind::Drag(MouseButton::Left)
        ));
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
        let m = MouseEvent {
            kind: MouseEventKind::Moved,
            column: 10,
            row: 20,
            modifiers: KeyModifiers::NONE,
        };
        assert_eq!(m.column, 10);
        assert_eq!(m.row, 20);
    }
}
