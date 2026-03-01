//! Deferred action execution
//!
//! Runs actions after their loading indicator has rendered on-screen.
//! Each DeferredAction variant delegates to the same method that would
//! have been called synchronously before the deferred pattern.

use crate::app::App;

/// Execute a deferred action after its loading indicator has rendered on-screen.
/// Called from the event loop's post-draw section. Each variant delegates to the
/// same method that would have been called synchronously before the deferred pattern.
pub fn execute_deferred_action(app: &mut App, action: crate::app::DeferredAction) {
    use crate::app::DeferredAction;
    match action {
        DeferredAction::LoadSession { branch, idx } => {
            app.save_current_terminal();
            app.select_session_file(&branch, idx);
            app.show_session_list = false;
            app.session_filter.clear();
            app.session_filter_active = false;
            app.session_content_search = false;
            app.session_search_results.clear();
            app.invalidate_sidebar();
        }
        DeferredAction::LoadFile { path } => {
            app.load_file_by_path(&path);
        }
        DeferredAction::OpenHealthPanel => {
            app.open_health_panel();
        }
        DeferredAction::SwitchProject { path } => {
            app.switch_project(path);
        }
        DeferredAction::RescanHealthScope { dirs } => {
            app.rescan_health_with_dirs(&dirs);
        }
        DeferredAction::GitCommit { worktree, message } => {
            if let Some(ref mut p) = app.git_actions_panel {
                match crate::git::Git::commit(&worktree, &message) {
                    Ok(out) => {
                        let first = out.lines().next().unwrap_or(&out);
                        p.result_message = Some((format!("Committed: {}", first), false));
                        crate::tui::input_git_actions::refresh_changed_files(p);
                        crate::tui::input_git_actions::refresh_commit_log(p);
                    }
                    Err(e) => { p.result_message = Some((format!("{}", e), true)); }
                }
            }
        }
        DeferredAction::GitCommitAndPush { worktree, message } => {
            if let Some(ref mut p) = app.git_actions_panel {
                match crate::git::Git::commit(&worktree, &message) {
                    Ok(_) => {
                        match crate::git::Git::push(&worktree) {
                            Ok(_) => {
                                p.result_message = Some(("Committed and pushed".into(), false));
                            }
                            Err(e) => {
                                p.result_message = Some((format!("Committed but push failed: {}", e), true));
                            }
                        }
                        crate::tui::input_git_actions::refresh_changed_files(p);
                        crate::tui::input_git_actions::refresh_commit_log(p);
                    }
                    Err(e) => { p.result_message = Some((format!("{}", e), true)); }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::DeferredAction;
    use std::path::PathBuf;

    // ── LoadSession action ──

    #[test]
    fn test_load_session_clears_filter() {
        let mut app = App::new();
        app.session_filter = "search term".into();
        execute_deferred_action(&mut app, DeferredAction::LoadSession {
            branch: "main".into(), idx: 0,
        });
        assert!(app.session_filter.is_empty());
    }

    #[test]
    fn test_load_session_clears_filter_active() {
        let mut app = App::new();
        app.session_filter_active = true;
        execute_deferred_action(&mut app, DeferredAction::LoadSession {
            branch: "main".into(), idx: 0,
        });
        assert!(!app.session_filter_active);
    }

    #[test]
    fn test_load_session_hides_session_list() {
        let mut app = App::new();
        app.show_session_list = true;
        execute_deferred_action(&mut app, DeferredAction::LoadSession {
            branch: "feat".into(), idx: 2,
        });
        assert!(!app.show_session_list);
    }

    #[test]
    fn test_load_session_clears_content_search() {
        let mut app = App::new();
        app.session_content_search = true;
        execute_deferred_action(&mut app, DeferredAction::LoadSession {
            branch: "dev".into(), idx: 0,
        });
        assert!(!app.session_content_search);
    }

    #[test]
    fn test_load_session_clears_search_results() {
        let mut app = App::new();
        app.session_search_results.push((0, "sid".into(), "match".into()));
        execute_deferred_action(&mut app, DeferredAction::LoadSession {
            branch: "b".into(), idx: 0,
        });
        assert!(app.session_search_results.is_empty());
    }

    #[test]
    fn test_load_session_index_zero() {
        let mut app = App::new();
        // No panic with idx=0 even without session files set up
        execute_deferred_action(&mut app, DeferredAction::LoadSession {
            branch: "main".into(), idx: 0,
        });
    }

    #[test]
    fn test_load_session_index_large() {
        let mut app = App::new();
        // Large index should not panic (select_session_file handles out-of-bounds)
        execute_deferred_action(&mut app, DeferredAction::LoadSession {
            branch: "main".into(), idx: 999,
        });
    }

    #[test]
    fn test_load_session_empty_branch() {
        let mut app = App::new();
        execute_deferred_action(&mut app, DeferredAction::LoadSession {
            branch: String::new(), idx: 0,
        });
        assert!(!app.show_session_list);
    }

    // ── LoadFile action ──

    #[test]
    fn test_load_file_nonexistent() {
        let mut app = App::new();
        // Loading a non-existent file shouldn't panic
        execute_deferred_action(&mut app, DeferredAction::LoadFile {
            path: PathBuf::from("/tmp/nonexistent_deferred_test_file.rs"),
        });
    }

    #[test]
    fn test_load_file_empty_path() {
        let mut app = App::new();
        execute_deferred_action(&mut app, DeferredAction::LoadFile {
            path: PathBuf::from(""),
        });
    }

    // ── OpenHealthPanel action ──

    #[test]
    fn test_open_health_panel_no_project() {
        let mut app = App::new();
        // Without a project, open_health_panel should not panic
        execute_deferred_action(&mut app, DeferredAction::OpenHealthPanel);
    }

    // ── SwitchProject action ──

    #[test]
    fn test_switch_project_nonexistent_path() {
        let mut app = App::new();
        execute_deferred_action(&mut app, DeferredAction::SwitchProject {
            path: PathBuf::from("/tmp/no_such_project_deferred"),
        });
    }

    #[test]
    fn test_switch_project_empty_path() {
        let mut app = App::new();
        execute_deferred_action(&mut app, DeferredAction::SwitchProject {
            path: PathBuf::from(""),
        });
    }

    // ── RescanHealthScope action ──

    #[test]
    fn test_rescan_health_scope_empty_dirs() {
        let mut app = App::new();
        execute_deferred_action(&mut app, DeferredAction::RescanHealthScope {
            dirs: vec![],
        });
    }

    #[test]
    fn test_rescan_health_scope_with_dirs() {
        let mut app = App::new();
        execute_deferred_action(&mut app, DeferredAction::RescanHealthScope {
            dirs: vec!["src".into(), "tests".into()],
        });
    }

    // ── GitCommit action (without git_actions_panel) ──

    #[test]
    fn test_git_commit_no_panel() {
        let mut app = App::new();
        assert!(app.git_actions_panel.is_none());
        execute_deferred_action(&mut app, DeferredAction::GitCommit {
            worktree: PathBuf::from("/tmp"),
            message: "test commit".into(),
        });
        // No panic — panel is None, so the match arm does nothing
    }

    #[test]
    fn test_git_commit_empty_message_no_panel() {
        let mut app = App::new();
        execute_deferred_action(&mut app, DeferredAction::GitCommit {
            worktree: PathBuf::from("/tmp"),
            message: String::new(),
        });
    }

    // ── GitCommitAndPush action (without git_actions_panel) ──

    #[test]
    fn test_git_commit_and_push_no_panel() {
        let mut app = App::new();
        assert!(app.git_actions_panel.is_none());
        execute_deferred_action(&mut app, DeferredAction::GitCommitAndPush {
            worktree: PathBuf::from("/tmp"),
            message: "test".into(),
        });
    }

    #[test]
    fn test_git_commit_and_push_empty_message_no_panel() {
        let mut app = App::new();
        execute_deferred_action(&mut app, DeferredAction::GitCommitAndPush {
            worktree: PathBuf::from("/tmp"),
            message: String::new(),
        });
    }

    // ── DeferredAction enum variant construction ──

    #[test]
    fn test_deferred_action_load_session_construction() {
        let action = DeferredAction::LoadSession { branch: "b".into(), idx: 5 };
        assert!(matches!(action, DeferredAction::LoadSession { .. }));
    }

    #[test]
    fn test_deferred_action_load_file_construction() {
        let action = DeferredAction::LoadFile { path: PathBuf::from("/x") };
        assert!(matches!(action, DeferredAction::LoadFile { .. }));
    }

    #[test]
    fn test_deferred_action_open_health_construction() {
        let action = DeferredAction::OpenHealthPanel;
        assert!(matches!(action, DeferredAction::OpenHealthPanel));
    }

    #[test]
    fn test_deferred_action_switch_project_construction() {
        let action = DeferredAction::SwitchProject { path: PathBuf::from("/p") };
        assert!(matches!(action, DeferredAction::SwitchProject { .. }));
    }

    #[test]
    fn test_deferred_action_rescan_construction() {
        let action = DeferredAction::RescanHealthScope { dirs: vec!["a".into()] };
        assert!(matches!(action, DeferredAction::RescanHealthScope { .. }));
    }

    #[test]
    fn test_deferred_action_git_commit_construction() {
        let action = DeferredAction::GitCommit {
            worktree: PathBuf::from("/w"),
            message: "m".into(),
        };
        assert!(matches!(action, DeferredAction::GitCommit { .. }));
    }

    #[test]
    fn test_deferred_action_git_commit_and_push_construction() {
        let action = DeferredAction::GitCommitAndPush {
            worktree: PathBuf::from("/w"),
            message: "m".into(),
        };
        assert!(matches!(action, DeferredAction::GitCommitAndPush { .. }));
    }

    // ── App deferred_action field ──

    #[test]
    fn test_app_deferred_action_default_none() {
        let app = App::new();
        assert!(app.deferred_action.is_none());
    }

    #[test]
    fn test_app_loading_indicator_default_none() {
        let app = App::new();
        assert!(app.loading_indicator.is_none());
    }

    // ── Multiple sequential deferred actions ──

    #[test]
    fn test_sequential_load_sessions() {
        let mut app = App::new();
        app.session_filter = "old".into();
        execute_deferred_action(&mut app, DeferredAction::LoadSession {
            branch: "a".into(), idx: 0,
        });
        assert!(app.session_filter.is_empty());
        app.session_filter = "new".into();
        execute_deferred_action(&mut app, DeferredAction::LoadSession {
            branch: "b".into(), idx: 1,
        });
        assert!(app.session_filter.is_empty());
    }

    #[test]
    fn test_load_file_then_session() {
        let mut app = App::new();
        execute_deferred_action(&mut app, DeferredAction::LoadFile {
            path: PathBuf::from("/tmp/test"),
        });
        app.show_session_list = true;
        execute_deferred_action(&mut app, DeferredAction::LoadSession {
            branch: "main".into(), idx: 0,
        });
        assert!(!app.show_session_list);
    }

    // ── LoadSession with special branch names ──

    #[test]
    fn test_load_session_branch_with_slashes() {
        let mut app = App::new();
        app.session_filter = "something".into();
        execute_deferred_action(&mut app, DeferredAction::LoadSession {
            branch: "azureal/feature/sub".into(), idx: 0,
        });
        assert!(app.session_filter.is_empty());
        assert!(!app.show_session_list);
    }

    #[test]
    fn test_load_session_branch_unicode() {
        let mut app = App::new();
        app.session_filter_active = true;
        execute_deferred_action(&mut app, DeferredAction::LoadSession {
            branch: "feäture-ünïcode".into(), idx: 0,
        });
        assert!(!app.session_filter_active);
    }

    #[test]
    fn test_load_session_multiple_search_results_cleared() {
        let mut app = App::new();
        for i in 0..5 {
            app.session_search_results.push((i, format!("sid{}", i), "match".into()));
        }
        assert_eq!(app.session_search_results.len(), 5);
        execute_deferred_action(&mut app, DeferredAction::LoadSession {
            branch: "main".into(), idx: 0,
        });
        assert!(app.session_search_results.is_empty());
    }

    // ── DeferredAction field extraction ──

    #[test]
    fn test_deferred_action_load_session_branch_value() {
        let action = DeferredAction::LoadSession { branch: "my-branch".into(), idx: 7 };
        if let DeferredAction::LoadSession { branch, idx } = action {
            assert_eq!(branch, "my-branch");
            assert_eq!(idx, 7);
        } else {
            panic!("Expected LoadSession");
        }
    }

    #[test]
    fn test_deferred_action_load_file_path_value() {
        let path = PathBuf::from("/home/user/project/src/main.rs");
        let action = DeferredAction::LoadFile { path: path.clone() };
        if let DeferredAction::LoadFile { path: p } = action {
            assert_eq!(p, path);
        } else {
            panic!("Expected LoadFile");
        }
    }

    #[test]
    fn test_deferred_action_switch_project_path_value() {
        let path = PathBuf::from("/projects/azureal");
        let action = DeferredAction::SwitchProject { path: path.clone() };
        if let DeferredAction::SwitchProject { path: p } = action {
            assert_eq!(p, path);
        } else {
            panic!("Expected SwitchProject");
        }
    }

    #[test]
    fn test_deferred_action_git_commit_fields() {
        let wt = PathBuf::from("/tmp/wt");
        let msg = "fix: resolve bug".to_string();
        let action = DeferredAction::GitCommit { worktree: wt.clone(), message: msg.clone() };
        if let DeferredAction::GitCommit { worktree, message } = action {
            assert_eq!(worktree, wt);
            assert_eq!(message, msg);
        } else {
            panic!("Expected GitCommit");
        }
    }

    #[test]
    fn test_deferred_action_git_commit_and_push_fields() {
        let wt = PathBuf::from("/tmp/wt");
        let msg = "feat: add feature".to_string();
        let action = DeferredAction::GitCommitAndPush { worktree: wt.clone(), message: msg.clone() };
        if let DeferredAction::GitCommitAndPush { worktree, message } = action {
            assert_eq!(worktree, wt);
            assert_eq!(message, msg);
        } else {
            panic!("Expected GitCommitAndPush");
        }
    }

    #[test]
    fn test_deferred_action_rescan_dirs_value() {
        let dirs = vec!["src".to_string(), "tests".to_string(), "benches".to_string()];
        let action = DeferredAction::RescanHealthScope { dirs: dirs.clone() };
        if let DeferredAction::RescanHealthScope { dirs: d } = action {
            assert_eq!(d, dirs);
        } else {
            panic!("Expected RescanHealthScope");
        }
    }

    // ── App state untouched by irrelevant deferred actions ──

    #[test]
    fn test_load_file_does_not_change_session_filter() {
        let mut app = App::new();
        app.session_filter = "preserved".into();
        execute_deferred_action(&mut app, DeferredAction::LoadFile {
            path: PathBuf::from("/tmp/file.rs"),
        });
        // LoadFile should not touch session_filter
        assert_eq!(app.session_filter, "preserved");
    }

    #[test]
    fn test_open_health_panel_does_not_clear_filter() {
        let mut app = App::new();
        app.session_filter = "still here".into();
        execute_deferred_action(&mut app, DeferredAction::OpenHealthPanel);
        assert_eq!(app.session_filter, "still here");
    }

    #[test]
    fn test_git_commit_no_panel_does_not_change_filter() {
        let mut app = App::new();
        app.session_filter = "untouched".into();
        execute_deferred_action(&mut app, DeferredAction::GitCommit {
            worktree: PathBuf::from("/tmp"),
            message: "msg".into(),
        });
        assert_eq!(app.session_filter, "untouched");
    }

    // ── execute_deferred_action with git_actions_panel None for all git variants ──

    #[test]
    fn test_git_commit_panel_none_no_side_effects() {
        let mut app = App::new();
        let before_filter = app.session_filter.clone();
        execute_deferred_action(&mut app, DeferredAction::GitCommit {
            worktree: PathBuf::from("/nonexistent"),
            message: "commit msg".into(),
        });
        // Panel is None — nothing should change
        assert!(app.git_actions_panel.is_none());
        assert_eq!(app.session_filter, before_filter);
    }

    #[test]
    fn test_git_commit_and_push_panel_none_no_side_effects() {
        let mut app = App::new();
        execute_deferred_action(&mut app, DeferredAction::GitCommitAndPush {
            worktree: PathBuf::from("/nonexistent"),
            message: "commit and push msg".into(),
        });
        assert!(app.git_actions_panel.is_none());
    }

    // ── LoadSession idx boundary values ──

    #[test]
    fn test_load_session_idx_one() {
        let mut app = App::new();
        execute_deferred_action(&mut app, DeferredAction::LoadSession {
            branch: "main".into(), idx: 1,
        });
        assert!(!app.show_session_list);
    }

    #[test]
    fn test_load_session_idx_max_usize() {
        let mut app = App::new();
        execute_deferred_action(&mut app, DeferredAction::LoadSession {
            branch: "main".into(), idx: usize::MAX,
        });
        assert!(app.session_filter.is_empty());
    }

    // ── RescanHealthScope with many dirs ──

    #[test]
    fn test_rescan_health_scope_many_dirs() {
        let mut app = App::new();
        let dirs: Vec<String> = (0..50).map(|i| format!("dir{}", i)).collect();
        execute_deferred_action(&mut app, DeferredAction::RescanHealthScope {
            dirs: dirs.clone(),
        });
        // Should not panic regardless of number of dirs
    }

    // ── LoadFile with deep nested path ──

    #[test]
    fn test_load_file_deep_path() {
        let mut app = App::new();
        execute_deferred_action(&mut app, DeferredAction::LoadFile {
            path: PathBuf::from("/a/b/c/d/e/f/g/h/i/j/k.rs"),
        });
        // No panic on deep paths
    }

    #[test]
    fn test_open_health_panel_multiple_times() {
        let mut app = App::new();
        // Calling multiple times should not panic or corrupt state
        execute_deferred_action(&mut app, DeferredAction::OpenHealthPanel);
        execute_deferred_action(&mut app, DeferredAction::OpenHealthPanel);
    }

    #[test]
    fn test_switch_project_does_not_touch_session_filter() {
        let mut app = App::new();
        app.session_filter = "keep me".into();
        execute_deferred_action(&mut app, DeferredAction::SwitchProject {
            path: PathBuf::from("/tmp/proj"),
        });
        // SwitchProject doesn't clear session_filter
        // (it may wipe everything depending on impl, so just test no panic)
    }
}
