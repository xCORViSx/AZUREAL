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
    WatcherFailed(#[allow(dead_code)] String),
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
            move |res| {
                let _ = notify_tx.send(res);
            },
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

        Some(Self {
            cmd_tx,
            event_rx,
            _handle: handle,
        })
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
const NOISY_SEGMENTS: &[&str] = &["target", ".git", "node_modules", ".DS_Store"];

/// File extensions that are typically editor swap/backup files
const NOISY_EXTENSIONS: &[&str] = &["swp", "swo", "swn"];

/// Returns true if the path should be ignored (build artifacts, VCS
/// internals, editor swap files, etc.)
fn is_noisy_path(path: &Path) -> bool {
    // Check path segments for noisy directories
    for component in path.components() {
        if let std::path::Component::Normal(s) = component {
            let s = s.to_string_lossy();
            if NOISY_SEGMENTS.iter().any(|&n| s == n) {
                return true;
            }
        }
    }
    // Check extension for swap files
    if let Some(ext) = path.extension() {
        let ext = ext.to_string_lossy();
        if NOISY_EXTENSIONS.iter().any(|&e| ext == e) {
            return true;
        }
    }
    // Backup files ending with ~
    if path.to_string_lossy().ends_with('~') {
        return true;
    }
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
                    &event_result,
                    &session_path,
                    &mut saw_session,
                    &mut saw_worktree,
                );
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => return,
        }

        // Drain any additional queued events (non-blocking) to coalesce
        while let Ok(event_result) = notify_rx.try_recv() {
            classify_event(
                &event_result,
                &session_path,
                &mut saw_session,
                &mut saw_worktree,
            );
        }

        // --- Phase 3: Forward coalesced events to main thread ---
        if saw_session {
            if event_tx.send(WatchEvent::SessionFileChanged).is_err() {
                return;
            }
        }
        if saw_worktree {
            if event_tx.send(WatchEvent::WorktreeChanged).is_err() {
                return;
            }
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
        if !paths_seen.insert(path) {
            continue;
        }

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

#[cfg(test)]
mod tests {
    use super::*;
    use notify::event::{AccessKind, CreateKind, ModifyKind, RemoveKind};

    // ── Helper to build a notify::Event with paths ──────────────────────
    fn make_event(kind: EventKind, paths: Vec<PathBuf>) -> Event {
        let mut e = Event::new(kind);
        e.paths = paths;
        e
    }

    // =====================================================================
    // is_noisy_path — NOISY_SEGMENTS
    // =====================================================================

    #[test]
    fn noisy_target_dir() {
        assert!(is_noisy_path(Path::new("/project/target/debug/build")));
    }

    #[test]
    fn noisy_target_as_only_segment() {
        assert!(is_noisy_path(Path::new("target")));
    }

    #[test]
    fn noisy_git_dir() {
        assert!(is_noisy_path(Path::new("/repo/.git/objects/pack")));
    }

    #[test]
    fn noisy_git_root_segment() {
        assert!(is_noisy_path(Path::new(".git")));
    }

    #[test]
    fn noisy_node_modules() {
        assert!(is_noisy_path(Path::new(
            "/app/node_modules/lodash/index.js"
        )));
    }

    #[test]
    fn noisy_ds_store_segment() {
        assert!(is_noisy_path(Path::new("/folder/.DS_Store")));
    }

    #[test]
    fn noisy_ds_store_bare() {
        assert!(is_noisy_path(Path::new(".DS_Store")));
    }

    #[test]
    fn noisy_target_nested_deep() {
        assert!(is_noisy_path(Path::new("/a/b/c/target/d/e/f.rs")));
    }

    #[test]
    fn noisy_node_modules_nested() {
        assert!(is_noisy_path(Path::new(
            "src/node_modules/.package-lock.json"
        )));
    }

    // =====================================================================
    // is_noisy_path — NOISY_EXTENSIONS
    // =====================================================================

    #[test]
    fn noisy_swp_extension() {
        assert!(is_noisy_path(Path::new("/src/main.rs.swp")));
    }

    #[test]
    fn noisy_swo_extension() {
        assert!(is_noisy_path(Path::new("/src/lib.rs.swo")));
    }

    #[test]
    fn noisy_swn_extension() {
        assert!(is_noisy_path(Path::new("config.toml.swn")));
    }

    #[test]
    fn noisy_swap_bare_filename() {
        assert!(is_noisy_path(Path::new("file.swp")));
    }

    // =====================================================================
    // is_noisy_path — backup files ending with ~
    // =====================================================================

    #[test]
    fn noisy_backup_tilde() {
        assert!(is_noisy_path(Path::new("/src/main.rs~")));
    }

    #[test]
    fn noisy_backup_tilde_bare() {
        assert!(is_noisy_path(Path::new("file~")));
    }

    #[test]
    fn noisy_backup_tilde_deep_path() {
        assert!(is_noisy_path(Path::new("/a/b/c/d.txt~")));
    }

    // =====================================================================
    // is_noisy_path — clean paths (should NOT be noisy)
    // =====================================================================

    #[test]
    fn clean_rust_source() {
        assert!(!is_noisy_path(Path::new("/src/main.rs")));
    }

    #[test]
    fn clean_toml_config() {
        assert!(!is_noisy_path(Path::new("Cargo.toml")));
    }

    #[test]
    fn clean_json_file() {
        assert!(!is_noisy_path(Path::new("/project/package.json")));
    }

    #[test]
    fn clean_nested_source() {
        assert!(!is_noisy_path(Path::new("/workspace/src/app/mod.rs")));
    }

    #[test]
    fn clean_readme() {
        assert!(!is_noisy_path(Path::new("README.md")));
    }

    #[test]
    fn clean_lock_file() {
        assert!(!is_noisy_path(Path::new("Cargo.lock")));
    }

    #[test]
    fn clean_hidden_non_git() {
        assert!(!is_noisy_path(Path::new("/home/.bashrc")));
    }

    #[test]
    fn clean_txt_file() {
        assert!(!is_noisy_path(Path::new("/docs/notes.txt")));
    }

    #[test]
    fn clean_empty_path() {
        assert!(!is_noisy_path(Path::new("")));
    }

    #[test]
    fn clean_root_path() {
        assert!(!is_noisy_path(Path::new("/")));
    }

    #[test]
    fn clean_relative_source() {
        assert!(!is_noisy_path(Path::new("src/lib.rs")));
    }

    #[test]
    fn clean_path_with_target_substring_not_segment() {
        // "targeted" contains "target" but is not the segment "target"
        assert!(!is_noisy_path(Path::new("/project/targeted/file.rs")));
    }

    #[test]
    fn clean_path_with_git_substring_not_segment() {
        // ".github" is not ".git"
        assert!(!is_noisy_path(Path::new(
            "/project/.github/workflows/ci.yml"
        )));
    }

    #[test]
    fn clean_path_with_gitignore() {
        assert!(!is_noisy_path(Path::new("/project/.gitignore")));
    }

    #[test]
    fn clean_unicode_filename() {
        assert!(!is_noisy_path(Path::new("/docs/日本語.txt")));
    }

    #[test]
    fn clean_spaces_in_path() {
        assert!(!is_noisy_path(Path::new("/my project/src/main.rs")));
    }

    #[test]
    fn clean_dots_in_filename() {
        assert!(!is_noisy_path(Path::new("/src/file.test.rs")));
    }

    #[test]
    fn clean_no_extension() {
        assert!(!is_noisy_path(Path::new("/bin/myapp")));
    }

    // =====================================================================
    // classify_event — error results
    // =====================================================================

    #[test]
    fn classify_error_result_sets_nothing() {
        let err: notify::Result<Event> = Err(notify::Error::generic("test error"));
        let session: Option<PathBuf> = None;
        let (mut saw_session, mut saw_worktree) = (false, false);
        classify_event(&err, &session, &mut saw_session, &mut saw_worktree);
        assert!(!saw_session);
        assert!(!saw_worktree);
    }

    // =====================================================================
    // classify_event — ignored EventKind variants (Access, Any, Other)
    // =====================================================================

    #[test]
    fn classify_access_event_ignored() {
        let event = make_event(
            EventKind::Access(AccessKind::Any),
            vec![PathBuf::from("/src/main.rs")],
        );
        let session: Option<PathBuf> = None;
        let (mut s, mut w) = (false, false);
        classify_event(&Ok(event), &session, &mut s, &mut w);
        assert!(!s);
        assert!(!w);
    }

    #[test]
    fn classify_any_event_ignored() {
        let event = make_event(EventKind::Any, vec![PathBuf::from("/src/main.rs")]);
        let session: Option<PathBuf> = None;
        let (mut s, mut w) = (false, false);
        classify_event(&Ok(event), &session, &mut s, &mut w);
        assert!(!s);
        assert!(!w);
    }

    #[test]
    fn classify_other_event_ignored() {
        let event = make_event(EventKind::Other, vec![PathBuf::from("/src/main.rs")]);
        let session: Option<PathBuf> = None;
        let (mut s, mut w) = (false, false);
        classify_event(&Ok(event), &session, &mut s, &mut w);
        assert!(!s);
        assert!(!w);
    }

    // =====================================================================
    // classify_event — session file detection
    // =====================================================================

    #[test]
    fn classify_session_file_create() {
        let sp = PathBuf::from("/tmp/session.jsonl");
        let event = make_event(EventKind::Create(CreateKind::File), vec![sp.clone()]);
        let (mut s, mut w) = (false, false);
        classify_event(&Ok(event), &Some(sp), &mut s, &mut w);
        assert!(s, "should detect session file");
        assert!(!w, "session file should not be counted as worktree");
    }

    #[test]
    fn classify_session_file_modify() {
        let sp = PathBuf::from("/sessions/abc.jsonl");
        let event = make_event(EventKind::Modify(ModifyKind::Any), vec![sp.clone()]);
        let (mut s, mut w) = (false, false);
        classify_event(&Ok(event), &Some(sp), &mut s, &mut w);
        assert!(s);
        assert!(!w);
    }

    #[test]
    fn classify_session_file_remove() {
        let sp = PathBuf::from("/data/session.jsonl");
        let event = make_event(EventKind::Remove(RemoveKind::File), vec![sp.clone()]);
        let (mut s, mut w) = (false, false);
        classify_event(&Ok(event), &Some(sp), &mut s, &mut w);
        assert!(s);
        assert!(!w);
    }

    #[test]
    fn classify_session_no_watch_set() {
        let event = make_event(
            EventKind::Modify(ModifyKind::Any),
            vec![PathBuf::from("/tmp/session.jsonl")],
        );
        let (mut s, mut w) = (false, false);
        classify_event(&Ok(event), &None, &mut s, &mut w);
        assert!(!s, "no session path set means no session match");
        assert!(w, "clean path should be seen as worktree change");
    }

    // =====================================================================
    // classify_event — worktree changes (clean paths)
    // =====================================================================

    #[test]
    fn classify_worktree_clean_file_create() {
        let event = make_event(
            EventKind::Create(CreateKind::File),
            vec![PathBuf::from("/project/src/lib.rs")],
        );
        let (mut s, mut w) = (false, false);
        classify_event(&Ok(event), &None, &mut s, &mut w);
        assert!(!s);
        assert!(w);
    }

    #[test]
    fn classify_worktree_clean_file_modify() {
        let event = make_event(
            EventKind::Modify(ModifyKind::Any),
            vec![PathBuf::from("/project/Cargo.toml")],
        );
        let (mut s, mut w) = (false, false);
        classify_event(&Ok(event), &None, &mut s, &mut w);
        assert!(w);
    }

    #[test]
    fn classify_worktree_clean_file_remove() {
        let event = make_event(
            EventKind::Remove(RemoveKind::File),
            vec![PathBuf::from("/project/old.rs")],
        );
        let (mut s, mut w) = (false, false);
        classify_event(&Ok(event), &None, &mut s, &mut w);
        assert!(w);
    }

    // =====================================================================
    // classify_event — worktree changes filtered by noisy paths
    // =====================================================================

    #[test]
    fn classify_noisy_target_filtered() {
        let event = make_event(
            EventKind::Create(CreateKind::File),
            vec![PathBuf::from("/project/target/debug/deps/foo.d")],
        );
        let (mut s, mut w) = (false, false);
        classify_event(&Ok(event), &None, &mut s, &mut w);
        assert!(!w, "target dir should be filtered");
    }

    #[test]
    fn classify_noisy_git_filtered() {
        let event = make_event(
            EventKind::Modify(ModifyKind::Any),
            vec![PathBuf::from("/project/.git/index")],
        );
        let (mut s, mut w) = (false, false);
        classify_event(&Ok(event), &None, &mut s, &mut w);
        assert!(!w, ".git dir should be filtered");
    }

    #[test]
    fn classify_noisy_swap_filtered() {
        let event = make_event(
            EventKind::Create(CreateKind::File),
            vec![PathBuf::from("/project/src/.main.rs.swp")],
        );
        let (mut s, mut w) = (false, false);
        classify_event(&Ok(event), &None, &mut s, &mut w);
        assert!(!w, ".swp should be filtered");
    }

    #[test]
    fn classify_noisy_backup_filtered() {
        let event = make_event(
            EventKind::Modify(ModifyKind::Any),
            vec![PathBuf::from("/project/src/main.rs~")],
        );
        let (mut s, mut w) = (false, false);
        classify_event(&Ok(event), &None, &mut s, &mut w);
        assert!(!w, "backup files (~) should be filtered");
    }

    #[test]
    fn classify_noisy_node_modules_filtered() {
        let event = make_event(
            EventKind::Create(CreateKind::File),
            vec![PathBuf::from("/app/node_modules/react/index.js")],
        );
        let (mut s, mut w) = (false, false);
        classify_event(&Ok(event), &None, &mut s, &mut w);
        assert!(!w, "node_modules should be filtered");
    }

    // =====================================================================
    // classify_event — multi-path events
    // =====================================================================

    #[test]
    fn classify_multi_path_session_and_worktree() {
        let sp = PathBuf::from("/tmp/session.jsonl");
        let event = make_event(
            EventKind::Modify(ModifyKind::Any),
            vec![sp.clone(), PathBuf::from("/project/src/lib.rs")],
        );
        let (mut s, mut w) = (false, false);
        classify_event(&Ok(event), &Some(sp), &mut s, &mut w);
        assert!(s, "session file should be detected");
        assert!(w, "clean worktree file should be detected");
    }

    #[test]
    fn classify_multi_path_all_noisy() {
        let event = make_event(
            EventKind::Modify(ModifyKind::Any),
            vec![
                PathBuf::from("/project/target/foo"),
                PathBuf::from("/project/.git/index"),
                PathBuf::from("/project/node_modules/x"),
            ],
        );
        let (mut s, mut w) = (false, false);
        classify_event(&Ok(event), &None, &mut s, &mut w);
        assert!(!s);
        assert!(!w, "all paths are noisy, worktree should be false");
    }

    #[test]
    fn classify_multi_path_mixed_noisy_and_clean() {
        let event = make_event(
            EventKind::Create(CreateKind::Any),
            vec![
                PathBuf::from("/project/target/debug/foo"),
                PathBuf::from("/project/src/clean.rs"),
            ],
        );
        let (mut s, mut w) = (false, false);
        classify_event(&Ok(event), &None, &mut s, &mut w);
        assert!(w, "at least one clean path should set worktree");
    }

    #[test]
    fn classify_duplicate_paths_deduplicated() {
        let path = PathBuf::from("/project/src/main.rs");
        let event = make_event(
            EventKind::Modify(ModifyKind::Any),
            vec![path.clone(), path.clone()],
        );
        let (mut s, mut w) = (false, false);
        classify_event(&Ok(event), &None, &mut s, &mut w);
        // Should still work — dedup doesn't break anything
        assert!(w);
    }

    #[test]
    fn classify_empty_paths() {
        let event = make_event(EventKind::Create(CreateKind::Any), vec![]);
        let (mut s, mut w) = (false, false);
        classify_event(&Ok(event), &None, &mut s, &mut w);
        assert!(!s);
        assert!(!w, "empty paths means no events");
    }

    // =====================================================================
    // classify_event — preserves existing flag values
    // =====================================================================

    #[test]
    fn classify_preserves_saw_session_true() {
        let event = make_event(
            EventKind::Create(CreateKind::File),
            vec![PathBuf::from("/project/src/lib.rs")],
        );
        let (mut s, mut w) = (true, false);
        classify_event(&Ok(event), &None, &mut s, &mut w);
        assert!(s, "saw_session should remain true");
        assert!(w);
    }

    #[test]
    fn classify_preserves_saw_worktree_true() {
        // A noisy event should not reset an already-true saw_worktree
        let event = make_event(
            EventKind::Create(CreateKind::File),
            vec![PathBuf::from("/project/target/debug/foo")],
        );
        let (mut s, mut w) = (false, true);
        classify_event(&Ok(event), &None, &mut s, &mut w);
        assert!(w, "saw_worktree should remain true");
    }

    // =====================================================================
    // is_noisy_path — edge cases with similar names
    // =====================================================================

    #[test]
    fn noisy_ds_store_not_partial_match() {
        // ".DS_Store_backup" is NOT the segment ".DS_Store", but the
        // extension test doesn't match either (extension = "DS_Store_backup")
        // Actually the segment IS ".DS_Store_backup" which != ".DS_Store"
        assert!(!is_noisy_path(Path::new("/project/.DS_Store_backup")));
    }

    #[test]
    fn noisy_target_exact_segment_only() {
        // "my-target" has segment "my-target" which is not "target"
        assert!(!is_noisy_path(Path::new("/project/my-target/file")));
    }

    #[test]
    fn noisy_swp_only_as_extension() {
        // "file.swp" is noisy; "swp" without extension is not
        assert!(!is_noisy_path(Path::new("/project/swp")));
    }

    #[test]
    fn clean_tilde_in_middle_of_path() {
        // Tilde only matters at the end; ~/ in the middle is just a dir name
        assert!(!is_noisy_path(Path::new("/home/user~/project/main.rs")));
    }

    #[test]
    fn noisy_deeply_nested_git() {
        assert!(is_noisy_path(Path::new("/a/b/c/d/e/.git/HEAD")));
    }

    #[test]
    fn clean_path_with_special_chars() {
        assert!(!is_noisy_path(Path::new(
            "/project/src/file-with-dashes.rs"
        )));
    }

    #[test]
    fn clean_path_with_underscores() {
        assert!(!is_noisy_path(Path::new("/project/src/my_module.rs")));
    }

    #[test]
    fn clean_path_emoji_in_name() {
        assert!(!is_noisy_path(Path::new("/docs/notes_\u{1F600}.md")));
    }

    #[test]
    fn noisy_git_worktree_hook() {
        assert!(is_noisy_path(Path::new("/repo/.git/hooks/pre-commit")));
    }
}
