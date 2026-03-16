//! Session list overlay helpers
//!
//! Handles opening the session list overlay, computing message counts
//! (two-phase load), and fast JSONL message counting.

use crate::app::App;

/// Open session list overlay — scoped to the currently selected worktree only.
/// Phase 1: show the overlay + loading indicator, refresh file list (fast).
/// Phase 2 (finish_session_list_load) runs on the next event loop iteration
/// so the loading dialog renders before the expensive message count I/O starts.
pub(super) fn open_session_list(app: &mut App) {
    app.show_session_list = true;
    app.session_list_loading = true;
    app.session_list_selected = 0;
    app.session_list_scroll = 0;
    app.ensure_session_store();
    if let Some(session) = app.current_worktree() {
        let branch = session.branch_name.clone();
        let mut files = Vec::new();

        // Include SQLite store sessions (S-numbered) for this worktree
        if let Some(ref store) = app.session_store {
            if let Ok(sessions) = store.list_sessions(Some(&branch)) {
                for s in &sessions {
                    let key = s.id.to_string();
                    // Use integer ID as string key — never collides with UUIDs
                    files.push((key.clone(), std::path::PathBuf::new(), s.created.clone()));
                    // Pre-populate msg counts from store metadata (avoids JSONL I/O)
                    app.session_msg_counts
                        .insert(key.clone(), (s.message_count, 0));
                    // Pre-populate completion status (display-only, never in prompts)
                    if let Some(success) = s.completed {
                        app.session_completion.insert(
                            key,
                            (
                                success,
                                s.duration_ms.unwrap_or(0),
                                s.cost_usd.unwrap_or(0.0),
                            ),
                        );
                    }
                }
            }
        }

        app.session_files.insert(branch, files);
    }
}

/// Phase 2 of session list loading — message counts are already populated
/// from the store metadata in open_session_list. Just clear the loading flag.
pub fn finish_session_list_load(app: &mut App) {
    app.session_list_loading = false;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::App;

    #[test]
    fn test_open_session_list_sets_show() {
        let mut app = App::new();
        open_session_list(&mut app);
        assert!(app.show_session_list);
    }

    #[test]
    fn test_open_session_list_sets_loading() {
        let mut app = App::new();
        open_session_list(&mut app);
        assert!(app.session_list_loading);
    }

    #[test]
    fn test_open_session_list_resets_selected() {
        let mut app = App::new();
        app.session_list_selected = 5;
        open_session_list(&mut app);
        assert_eq!(app.session_list_selected, 0);
    }

    #[test]
    fn test_open_session_list_resets_scroll() {
        let mut app = App::new();
        app.session_list_scroll = 10;
        open_session_list(&mut app);
        assert_eq!(app.session_list_scroll, 0);
    }

    #[test]
    fn test_open_session_list_no_worktree_no_crash() {
        let mut app = App::new();
        open_session_list(&mut app);
        assert!(app.show_session_list);
    }

    #[test]
    fn test_finish_clears_loading() {
        let mut app = App::new();
        app.session_list_loading = true;
        finish_session_list_load(&mut app);
        assert!(!app.session_list_loading);
    }

    #[test]
    fn test_finish_no_worktree_no_crash() {
        let mut app = App::new();
        finish_session_list_load(&mut app);
        assert!(!app.session_list_loading);
    }

    #[test]
    fn test_open_session_list_twice_resets() {
        let mut app = App::new();
        open_session_list(&mut app);
        app.session_list_selected = 3;
        app.session_list_scroll = 5;
        open_session_list(&mut app);
        assert_eq!(app.session_list_selected, 0);
        assert_eq!(app.session_list_scroll, 0);
    }

    #[test]
    fn test_finish_then_open_loading_sequence() {
        let mut app = App::new();
        finish_session_list_load(&mut app);
        assert!(!app.session_list_loading);
        open_session_list(&mut app);
        assert!(app.session_list_loading);
    }

    #[test]
    fn test_open_when_already_open() {
        let mut app = App::new();
        app.show_session_list = true;
        app.session_list_selected = 7;
        open_session_list(&mut app);
        assert!(app.show_session_list);
        assert_eq!(app.session_list_selected, 0);
    }

    #[test]
    fn test_finish_already_not_loading() {
        let mut app = App::new();
        app.session_list_loading = false;
        finish_session_list_load(&mut app);
        assert!(!app.session_list_loading);
    }

    #[test]
    fn test_open_session_list_preserves_msg_counts() {
        let mut app = App::new();
        app.session_msg_counts
            .insert("sess1".to_string(), (10, 500));
        open_session_list(&mut app);
        assert_eq!(app.session_msg_counts.get("sess1"), Some(&(10, 500)));
    }

    #[test]
    fn test_finish_preserves_existing_msg_counts() {
        let mut app = App::new();
        app.session_msg_counts.insert("old".into(), (5, 100));
        app.session_list_loading = true;
        finish_session_list_load(&mut app);
        assert_eq!(app.session_msg_counts.get("old"), Some(&(5, 100)));
    }

    #[test]
    fn test_open_session_list_all_fields() {
        let mut app = App::new();
        app.session_list_selected = 10;
        app.session_list_scroll = 20;
        app.session_list_loading = false;
        app.show_session_list = false;
        open_session_list(&mut app);
        assert!(app.show_session_list);
        assert!(app.session_list_loading);
        assert_eq!(app.session_list_selected, 0);
        assert_eq!(app.session_list_scroll, 0);
    }

    #[test]
    fn test_open_finish_open_cycle() {
        let mut app = App::new();
        open_session_list(&mut app);
        assert!(app.session_list_loading);
        finish_session_list_load(&mut app);
        assert!(!app.session_list_loading);
        open_session_list(&mut app);
        assert!(app.session_list_loading);
    }
}
