//! Filesystem watcher for session files and worktree directories
//!
//! Uses the `notify` crate for kernel-level filesystem notifications (kqueue on
//! macOS, inotify on Linux, ReadDirectoryChangesW on Windows). A background
//! thread owns the watcher and forwards classified events to the main thread
//! via mpsc channels — same pattern as RenderThread and SttHandle.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use notify::{Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};

/// Events sent from watcher thread → main thread
pub enum WatchEvent {
    /// The watched session JSONL file was modified (append, write, etc.)
    SessionFileChanged,
    /// A file in the watched worktree was created/modified/deleted/renamed
    WorktreeChanged,
    /// The notify watcher failed — main thread should fall back to polling
    WatcherFailed(String),
}

/// Commands sent from main thread → watcher thread
pub enum WatchCommand {
    /// Watch a session JSONL file (replaces any previous session watch)
    WatchSessionFile(PathBuf),
    /// Watch a worktree directory recursively (replaces any previous worktree watch)
    WatchWorktree(PathBuf),
    /// Clear all watches (e.g., during session switch before re-registering)
    ClearAll,
}

/// Handle held by the main thread. Dropping it disconnects the command
/// channel, which causes the watcher thread to exit.
pub struct FileWatcher {
    /// Send commands to the watcher thread (watch/unwatch/clear)
    cmd_tx: mpsc::Sender<WatchCommand>,
    /// Receive filesystem events from the watcher thread
    event_rx: mpsc::Receiver<WatchEvent>,
    /// Kept alive so the thread doesn't get orphaned
    _handle: thread::JoinHandle<()>,
}

impl FileWatcher {
    /// Spawn the watcher thread. Returns None if the notify backend fails to
    /// initialize (e.g., OS limits exceeded). The caller should fall back to
    /// polling in that case.
    pub fn spawn() -> Option<Self> {
        let (cmd_tx, cmd_rx) = mpsc::channel::<WatchCommand>();
        let (event_tx, event_rx) = mpsc::channel::<WatchEvent>();

        // Internal channel for notify's callback → watcher thread
        let (notify_tx, notify_rx) = mpsc::channel::<notify::Result<Event>>();

        // Try creating the watcher — if this fails (OS limits, unsupported
        // platform, etc.), return None so the caller falls back to polling.
        let watcher = RecommendedWatcher::new(
            move |res| { let _ = notify_tx.send(res); },
            Config::default(),
        );
        let watcher = match watcher {
            Ok(w) => w,
            Err(e) => {
                tracing::warn!("Failed to create file watcher: {e}");
                return None;
            }
        };

        let handle = thread::Builder::new()
            .name("file-watcher".into())
            .spawn(move || {
                watcher_loop(watcher, cmd_rx, notify_rx, event_tx);
            })
            .expect("failed to spawn file-watcher thread");

        Some(Self { cmd_tx, event_rx, _handle: handle })
    }

    /// Send a command to the watcher thread (non-blocking)
    pub fn send(&self, cmd: WatchCommand) {
        let _ = self.cmd_tx.send(cmd);
    }

    /// Non-blocking poll for filesystem events. Returns None if no events
    /// are available. Call in a loop to drain all pending events.
    pub fn try_recv(&self) -> Option<WatchEvent> {
        self.event_rx.try_recv().ok()
    }
}

/// Paths that generate noise we don't care about — filtered in the watcher
/// thread so the main thread never sees them.
const NOISY_SEGMENTS: &[&str] = &[
    "target", ".git", "node_modules", ".DS_Store",
];

/// File extensions that are typically editor swap/backup files
const NOISY_EXTENSIONS: &[&str] = &["swp", "swo", "swn"];

/// Returns true if the path should be ignored (build artifacts, VCS
/// internals, editor swap files, etc.)
fn is_noisy_path(path: &Path) -> bool {
    // Check path segments for noisy directories
    for component in path.components() {
        if let std::path::Component::Normal(s) = component {
            let s = s.to_string_lossy();
            if NOISY_SEGMENTS.iter().any(|&n| s == n) { return true; }
        }
    }
    // Check extension for swap files
    if let Some(ext) = path.extension() {
        let ext = ext.to_string_lossy();
        if NOISY_EXTENSIONS.iter().any(|&e| ext == e) { return true; }
    }
    // Backup files ending with ~
    if path.to_string_lossy().ends_with('~') { return true; }
    false
}

/// Main loop for the watcher thread. Blocks on notify events with a 200ms
/// timeout, drains commands each wakeup, classifies and coalesces filesystem
/// events, and forwards them to the main thread.
fn watcher_loop(
    mut watcher: RecommendedWatcher,
    cmd_rx: mpsc::Receiver<WatchCommand>,
    notify_rx: mpsc::Receiver<notify::Result<Event>>,
    event_tx: mpsc::Sender<WatchEvent>,
) {
    // Currently watched paths so we can unwatch them on ClearAll / replacement
    let mut session_path: Option<PathBuf> = None;
    let mut worktree_path: Option<PathBuf> = None;

    loop {
        // --- Phase 1: Drain commands (non-blocking) ---
        // If the command channel is disconnected (main thread dropped FileWatcher), exit.
        loop {
            match cmd_rx.try_recv() {
                Ok(WatchCommand::WatchSessionFile(path)) => {
                    // Unwatch previous session file
                    if let Some(ref old) = session_path {
                        let _ = watcher.unwatch(old);
                    }
                    // Watch new session file (non-recursive — it's a single file)
                    if let Err(e) = watcher.watch(&path, RecursiveMode::NonRecursive) {
                        tracing::warn!("Failed to watch session file {}: {e}", path.display());
                        let _ = event_tx.send(WatchEvent::WatcherFailed(format!("{e}")));
                    }
                    session_path = Some(path);
                }
                Ok(WatchCommand::WatchWorktree(path)) => {
                    // Unwatch previous worktree
                    if let Some(ref old) = worktree_path {
                        let _ = watcher.unwatch(old);
                    }
                    // Watch new worktree recursively (FSEvents on macOS for efficiency)
                    if let Err(e) = watcher.watch(&path, RecursiveMode::Recursive) {
                        tracing::warn!("Failed to watch worktree {}: {e}", path.display());
                        // Don't send WatcherFailed for worktree — session watching
                        // might still work fine. Just log it.
                    }
                    worktree_path = Some(path);
                }
                Ok(WatchCommand::ClearAll) => {
                    if let Some(ref old) = session_path {
                        let _ = watcher.unwatch(old);
                    }
                    if let Some(ref old) = worktree_path {
                        let _ = watcher.unwatch(old);
                    }
                    session_path = None;
                    worktree_path = None;
                }
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => return, // main thread gone
            }
        }

        // --- Phase 2: Wait for filesystem events (200ms timeout) ---
        // The timeout ensures commands are processed within 200ms even
        // when no filesystem events are arriving.
        let mut saw_session = false;
        let mut saw_worktree = false;

        // Block up to 200ms for the first event
        match notify_rx.recv_timeout(Duration::from_millis(200)) {
            Ok(event_result) => {
                classify_event(
                    &event_result, &session_path, &mut saw_session, &mut saw_worktree,
                );
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => return,
        }

        // Drain any additional queued events (non-blocking) to coalesce
        while let Ok(event_result) = notify_rx.try_recv() {
            classify_event(
                &event_result, &session_path, &mut saw_session, &mut saw_worktree,
            );
        }

        // --- Phase 3: Forward coalesced events to main thread ---
        if saw_session {
            if event_tx.send(WatchEvent::SessionFileChanged).is_err() { return; }
        }
        if saw_worktree {
            if event_tx.send(WatchEvent::WorktreeChanged).is_err() { return; }
        }
    }
}

/// Classify a single notify event as session-file or worktree change.
/// Sets the corresponding flag to true. Filters out noisy paths.
fn classify_event(
    event_result: &notify::Result<Event>,
    session_path: &Option<PathBuf>,
    saw_session: &mut bool,
    saw_worktree: &mut bool,
) {
    let event = match event_result {
        Ok(e) => e,
        Err(_) => return, // notify error — ignore silently
    };

    // Only care about content changes (create, modify, remove, rename)
    match event.kind {
        EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_) => {}
        _ => return,
    }

    // Collect unique affected paths to avoid double-counting
    let mut paths_seen = HashSet::new();
    for path in &event.paths {
        if !paths_seen.insert(path) { continue; }

        // Is this the session file?
        if let Some(ref sp) = session_path {
            if path == sp {
                *saw_session = true;
                continue;
            }
        }

        // Worktree change — filter noise
        if !is_noisy_path(path) {
            *saw_worktree = true;
        }
    }
}
