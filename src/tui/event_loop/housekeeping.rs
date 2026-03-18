//! Background housekeeping tasks
//!
//! Polls file watchers, session file changes, file tree refresh, worktree tab
//! refresh, health panel refresh, speech-to-text, and debug dump saving.

use std::time::{Duration, Instant};

use crate::app::App;

/// Drain kernel-level notify events from the file watcher (non-blocking).
/// Sets dirty flags on App for debounced processing downstream.
pub fn drain_file_watcher(app: &mut App) {
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
}

/// Poll session file and file tree/worktree/health background refreshes.
/// `min_poll_interval` controls fallback stat()-based session polling frequency.
/// Returns true if anything changed (needs redraw).
pub fn poll_refreshes(
    app: &mut App,
    now: Instant,
    last_session_poll: &mut Instant,
    min_poll_interval: Duration,
) -> bool {
    let mut redraw = false;

    // Parse session file when dirty (set by watcher or fallback polling)
    if app.session_file_dirty {
        if app.poll_session_file() {
            redraw = true;
        }
    }

    // Fallback: stat() polling when watcher is unavailable
    if app.file_watcher.is_none()
        && now.duration_since(*last_session_poll) >= min_poll_interval
    {
        app.check_session_file();
        if app.poll_session_file() {
            redraw = true;
        }
    }

    // Debounced file tree refresh: spawn background thread to avoid
    // blocking the event loop (build_file_tree walks the filesystem,
    // 10-100ms depending on tree depth). Old tree stays visible until
    // the new one arrives — no flash of empty state.
    if app.file_tree_refresh_pending
        && app.file_tree_receiver.is_none()
        && now.duration_since(app.worktree_last_notify) >= Duration::from_millis(500)
    {
        if let Some(wt) = app.current_worktree() {
            if let Some(ref wt_path) = wt.worktree_path {
                let path = wt_path.clone();
                let expanded = app.file_tree_expanded.clone();
                let hidden = app.file_tree_hidden_dirs.clone();
                let (tx, rx) = std::sync::mpsc::channel();
                std::thread::spawn(move || {
                    let entries =
                        crate::app::state::helpers::build_file_tree(&path, &expanded, &hidden);
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
            app.file_tree_selected = if !app.file_tree_entries.is_empty() {
                Some(0)
            } else {
                None
            };
            app.file_tree_scroll = 0;
            app.invalidate_file_tree();
            app.file_tree_receiver = None;
            redraw = true;
        }
    }

    // Debounced worktree tab list refresh: spawn background thread for
    // git + FS I/O (git worktree list, branch listing, session discovery,
    // 10-50ms). Sidebar stays visible with old data until results arrive.
    if app.worktree_tabs_refresh_pending
        && app.worktree_refresh_receiver.is_none()
        && now.duration_since(app.worktree_last_notify) >= Duration::from_millis(500)
    {
        if let Some(ref project) = app.project {
            let path = project.path.clone();
            let main_branch = project.main_branch.clone();
            let wt_dir = project.worktrees_dir();
            let backend = app.backend;
            let (tx, rx) = std::sync::mpsc::channel();
            std::thread::spawn(move || {
                let result = crate::app::state::load::compute_worktree_refresh(
                    path,
                    main_branch,
                    wt_dir,
                    backend,
                );
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
            redraw = true;
        }
    }

    // Debounced health panel refresh: rescan god files + doc coverage
    // when source files change while the panel is open.
    // Skipped during active Claude streaming — the synchronous filesystem
    // walk (10-200ms) would block the event loop and cause input hiccups.
    // Panel refreshes once streaming finishes.
    if app.health_refresh_pending
        && app.agent_receivers.is_empty()
        && now.duration_since(app.worktree_last_notify) >= Duration::from_millis(500)
    {
        app.refresh_health_panel();
        app.health_refresh_pending = false;
        redraw = true;
    }

    // Timer-based housekeeping
    if now.duration_since(*last_session_poll) >= min_poll_interval {
        *last_session_poll = now;
    }

    redraw
}

/// Dismiss auto-rebase success dialog after timeout. Returns true if changed.
pub fn check_auto_rebase_timeout(app: &mut App, now: Instant) -> bool {
    if let Some((_, until)) = &app.auto_rebase_success_until {
        if now >= *until {
            app.auto_rebase_success_until = None;
            return true;
        }
    }
    false
}

/// Handle deferred debug dump saving. Returns true if needs redraw.
pub fn handle_debug_dump(app: &mut App) -> bool {
    if let Some(name) = app.debug_dump_saving.take() {
        app.dump_debug_output(&name);
        app.draw_pending = true;
        return true;
    }
    false
}

/// Poll speech-to-text events. Returns true if needs redraw.
pub fn poll_stt(app: &mut App) -> bool {
    if app.stt_handle.is_some() {
        return app.poll_stt();
    }
    false
}
