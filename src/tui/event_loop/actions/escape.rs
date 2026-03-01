//! Context-dependent escape dispatch
//!
//! Handles the Escape key action based on current focus and mode:
//! viewer edit mode, viewer, file tree, output, input/terminal.

use crate::app::{App, Focus};

/// Escape dispatch — context-dependent close/back
pub(super) fn dispatch_escape(app: &mut App) {
    match app.focus {
        // Worktree tab row: exit BrowseMain if active, otherwise move focus to FileTree
        Focus::Worktrees if app.browsing_main => { app.exit_main_browse(); }
        Focus::Worktrees => { app.focus = Focus::FileTree; }
        Focus::Viewer if app.viewer_edit_mode => {
            if app.viewer_edit_dirty {
                app.viewer_edit_discard_dialog = true;
            } else {
                app.exit_viewer_edit_mode();
            }
        }
        Focus::Viewer => {
            // Close viewer / close diff overlay
            if app.viewer_edit_diff.is_some() {
                if let Some((prev_content, prev_path, prev_scroll)) = app.viewer_prev_state.take() {
                    app.viewer_content = prev_content;
                    app.viewer_path = prev_path;
                    app.viewer_scroll = prev_scroll;
                    app.viewer_mode = if app.viewer_content.is_some() {
                        crate::app::ViewerMode::File
                    } else {
                        crate::app::ViewerMode::Empty
                    };
                } else {
                    app.clear_viewer();
                }
                app.viewer_edit_diff = None;
                app.viewer_edit_diff_line = None;
                app.selected_tool_diff = None;
                app.viewer_lines_dirty = true;
                app.focus = Focus::FileTree;
            } else {
                app.clear_viewer();
                app.focus = Focus::FileTree;
            }
        }
        Focus::FileTree if app.browsing_main => {
            // Exit main browse mode — restore previous worktree selection
            app.exit_main_browse();
        }
        Focus::FileTree => {
            if app.god_file_filter_mode {
                // Exit scope mode — translate worktree paths back to project-root
                // paths before saving (scope is persisted relative to project root)
                // and passing to the rescan (which also uses project root).
                let project_root = app.project.as_ref().map(|p| p.path.clone());
                let wt_root = app.current_worktree()
                    .and_then(|wt| wt.worktree_path.clone());
                let translated: std::collections::HashSet<std::path::PathBuf> =
                    if let (Some(ref pr), Some(ref wr)) = (&project_root, &wt_root) {
                        if pr != wr {
                            app.god_file_filter_dirs.iter()
                                .map(|p| {
                                    if let Ok(rel) = p.strip_prefix(wr) {
                                        pr.join(rel)
                                    } else {
                                        p.clone()
                                    }
                                })
                                .collect()
                        } else {
                            app.god_file_filter_dirs.clone()
                        }
                    } else {
                        app.god_file_filter_dirs.clone()
                    };
                if let Some(ref pr) = project_root {
                    crate::app::save_health_scope(pr, &translated);
                }
                let dirs: Vec<String> = translated.iter()
                    .map(|p| p.to_string_lossy().to_string()).collect();
                app.god_file_filter_mode = false;
                app.god_file_filter_dirs.clear();
                app.invalidate_file_tree();
                app.focus = crate::app::Focus::Worktrees;
                app.loading_indicator = Some("Rescanning health scope…".into());
                app.deferred_action = Some(crate::app::DeferredAction::RescanHealthScope { dirs });
            } else {
                // FileTree is always visible; Esc just moves focus to Worktrees
                app.focus = Focus::Worktrees;
                app.invalidate_sidebar();
            }
        }
        Focus::Session => {
            if app.show_session_list { app.show_session_list = false; }
            else { app.focus = Focus::Worktrees; }
        }
        Focus::Input if app.terminal_mode && !app.prompt_mode => {
            app.close_terminal();
        }
        Focus::Input if app.prompt_mode => {
            app.prompt_mode = false;
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{App, Focus, ViewerMode};

    // ── 1. Worktrees focus — browsing_main exits main browse ──

    #[test]
    fn test_worktrees_browsing_main_exits_main_browse() {
        let mut app = App::new();
        app.focus = Focus::Worktrees;
        app.browsing_main = true;
        app.pre_main_browse_selection = Some(2);
        dispatch_escape(&mut app);
        assert!(!app.browsing_main);
    }

    #[test]
    fn test_worktrees_browsing_main_restores_selection() {
        let mut app = App::new();
        app.focus = Focus::Worktrees;
        app.browsing_main = true;
        app.pre_main_browse_selection = Some(3);
        dispatch_escape(&mut app);
        assert_eq!(app.selected_worktree, Some(3));
    }

    #[test]
    fn test_worktrees_browsing_main_sets_focus_worktrees() {
        let mut app = App::new();
        app.focus = Focus::Worktrees;
        app.browsing_main = true;
        dispatch_escape(&mut app);
        assert_eq!(app.focus, Focus::Worktrees);
    }

    // ── 2. Worktrees focus — not browsing_main moves to FileTree ──

    #[test]
    fn test_worktrees_not_browsing_moves_to_file_tree() {
        let mut app = App::new();
        app.focus = Focus::Worktrees;
        app.browsing_main = false;
        dispatch_escape(&mut app);
        assert_eq!(app.focus, Focus::FileTree);
    }

    // ── 3. Viewer edit mode — dirty shows discard dialog ──

    #[test]
    fn test_viewer_edit_dirty_shows_discard_dialog() {
        let mut app = App::new();
        app.focus = Focus::Viewer;
        app.viewer_edit_mode = true;
        app.viewer_edit_dirty = true;
        dispatch_escape(&mut app);
        assert!(app.viewer_edit_discard_dialog);
    }

    #[test]
    fn test_viewer_edit_dirty_stays_in_edit_mode() {
        let mut app = App::new();
        app.focus = Focus::Viewer;
        app.viewer_edit_mode = true;
        app.viewer_edit_dirty = true;
        dispatch_escape(&mut app);
        assert!(app.viewer_edit_mode);
    }

    // ── 4. Viewer edit mode — clean exits edit mode ──

    #[test]
    fn test_viewer_edit_clean_exits_edit_mode() {
        let mut app = App::new();
        app.focus = Focus::Viewer;
        app.viewer_edit_mode = true;
        app.viewer_edit_dirty = false;
        dispatch_escape(&mut app);
        assert!(!app.viewer_edit_mode);
    }

    #[test]
    fn test_viewer_edit_clean_no_discard_dialog() {
        let mut app = App::new();
        app.focus = Focus::Viewer;
        app.viewer_edit_mode = true;
        app.viewer_edit_dirty = false;
        dispatch_escape(&mut app);
        assert!(!app.viewer_edit_discard_dialog);
    }

    // ── 5. Viewer — with diff overlay, no prev_state ──

    #[test]
    fn test_viewer_diff_overlay_no_prev_state_clears_viewer() {
        let mut app = App::new();
        app.focus = Focus::Viewer;
        app.viewer_edit_diff = Some(("old".to_string(), "new".to_string()));
        app.viewer_prev_state = None;
        app.viewer_content = Some("content".into());
        dispatch_escape(&mut app);
        assert!(app.viewer_content.is_none());
        assert!(app.viewer_edit_diff.is_none());
        assert_eq!(app.focus, Focus::FileTree);
    }

    // ── 6. Viewer — with diff overlay, with prev_state restores ──

    #[test]
    fn test_viewer_diff_overlay_with_prev_state_restores_content() {
        let mut app = App::new();
        app.focus = Focus::Viewer;
        app.viewer_edit_diff = Some(("a".to_string(), "b".to_string()));
        app.viewer_prev_state = Some((
            Some("old content".to_string()),
            Some(std::path::PathBuf::from("/foo.rs")),
            42,
        ));
        dispatch_escape(&mut app);
        assert_eq!(app.viewer_content.as_deref(), Some("old content"));
        assert_eq!(app.viewer_path, Some(std::path::PathBuf::from("/foo.rs")));
        assert_eq!(app.viewer_scroll, 42);
    }

    #[test]
    fn test_viewer_diff_overlay_with_prev_state_restores_mode_file() {
        let mut app = App::new();
        app.focus = Focus::Viewer;
        app.viewer_edit_diff = Some(("a".to_string(), "b".to_string()));
        app.viewer_prev_state = Some((
            Some("content".to_string()),
            Some(std::path::PathBuf::from("/foo.rs")),
            0,
        ));
        dispatch_escape(&mut app);
        assert_eq!(app.viewer_mode, ViewerMode::File);
    }

    #[test]
    fn test_viewer_diff_overlay_with_prev_state_empty_content_sets_empty_mode() {
        let mut app = App::new();
        app.focus = Focus::Viewer;
        app.viewer_edit_diff = Some(("a".to_string(), "b".to_string()));
        app.viewer_prev_state = Some((None, None, 0));
        dispatch_escape(&mut app);
        assert_eq!(app.viewer_mode, ViewerMode::Empty);
    }

    #[test]
    fn test_viewer_diff_overlay_clears_diff_fields() {
        let mut app = App::new();
        app.focus = Focus::Viewer;
        app.viewer_edit_diff = Some(("a".to_string(), "b".to_string()));
        app.viewer_edit_diff_line = Some(5);
        app.selected_tool_diff = Some(0);
        dispatch_escape(&mut app);
        assert!(app.viewer_edit_diff.is_none());
        assert!(app.viewer_edit_diff_line.is_none());
        assert!(app.selected_tool_diff.is_none());
    }

    #[test]
    fn test_viewer_diff_overlay_sets_lines_dirty() {
        let mut app = App::new();
        app.focus = Focus::Viewer;
        app.viewer_edit_diff = Some(("a".to_string(), "b".to_string()));
        app.viewer_lines_dirty = false;
        dispatch_escape(&mut app);
        assert!(app.viewer_lines_dirty);
    }

    #[test]
    fn test_viewer_diff_overlay_moves_to_file_tree() {
        let mut app = App::new();
        app.focus = Focus::Viewer;
        app.viewer_edit_diff = Some(("a".to_string(), "b".to_string()));
        dispatch_escape(&mut app);
        assert_eq!(app.focus, Focus::FileTree);
    }

    // ── 7. Viewer — no diff overlay, clears viewer ──

    #[test]
    fn test_viewer_no_diff_clears_viewer() {
        let mut app = App::new();
        app.focus = Focus::Viewer;
        app.viewer_edit_diff = None;
        app.viewer_content = Some("file content".to_string());
        dispatch_escape(&mut app);
        assert!(app.viewer_content.is_none());
    }

    #[test]
    fn test_viewer_no_diff_moves_to_file_tree() {
        let mut app = App::new();
        app.focus = Focus::Viewer;
        app.viewer_edit_diff = None;
        dispatch_escape(&mut app);
        assert_eq!(app.focus, Focus::FileTree);
    }

    #[test]
    fn test_viewer_no_diff_resets_viewer_mode_to_empty() {
        let mut app = App::new();
        app.focus = Focus::Viewer;
        app.viewer_edit_diff = None;
        app.viewer_mode = ViewerMode::File;
        dispatch_escape(&mut app);
        assert_eq!(app.viewer_mode, ViewerMode::Empty);
    }

    // ── 8. FileTree — browsing_main exits ──

    #[test]
    fn test_file_tree_browsing_main_exits() {
        let mut app = App::new();
        app.focus = Focus::FileTree;
        app.browsing_main = true;
        app.pre_main_browse_selection = Some(1);
        dispatch_escape(&mut app);
        assert!(!app.browsing_main);
    }

    #[test]
    fn test_file_tree_browsing_main_restores_prev_selection() {
        let mut app = App::new();
        app.focus = Focus::FileTree;
        app.browsing_main = true;
        app.pre_main_browse_selection = Some(5);
        dispatch_escape(&mut app);
        assert_eq!(app.selected_worktree, Some(5));
    }

    // ── 9. FileTree — not browsing, not god_file_filter → Worktrees ──

    #[test]
    fn test_file_tree_normal_moves_to_worktrees() {
        let mut app = App::new();
        app.focus = Focus::FileTree;
        app.browsing_main = false;
        app.god_file_filter_mode = false;
        dispatch_escape(&mut app);
        assert_eq!(app.focus, Focus::Worktrees);
    }

    // ── 10. FileTree — god_file_filter_mode sets deferred action and exits filter ──

    #[test]
    fn test_file_tree_god_filter_mode_exits_filter() {
        let mut app = App::new();
        app.focus = Focus::FileTree;
        app.god_file_filter_mode = true;
        app.browsing_main = false;
        dispatch_escape(&mut app);
        assert!(!app.god_file_filter_mode);
    }

    #[test]
    fn test_file_tree_god_filter_mode_clears_dirs() {
        let mut app = App::new();
        app.focus = Focus::FileTree;
        app.god_file_filter_mode = true;
        app.browsing_main = false;
        app.god_file_filter_dirs.insert(std::path::PathBuf::from("/test"));
        dispatch_escape(&mut app);
        assert!(app.god_file_filter_dirs.is_empty());
    }

    #[test]
    fn test_file_tree_god_filter_sets_focus_worktrees() {
        let mut app = App::new();
        app.focus = Focus::FileTree;
        app.god_file_filter_mode = true;
        app.browsing_main = false;
        dispatch_escape(&mut app);
        assert_eq!(app.focus, Focus::Worktrees);
    }

    #[test]
    fn test_file_tree_god_filter_sets_loading_indicator() {
        let mut app = App::new();
        app.focus = Focus::FileTree;
        app.god_file_filter_mode = true;
        app.browsing_main = false;
        dispatch_escape(&mut app);
        assert!(app.loading_indicator.is_some());
    }

    #[test]
    fn test_file_tree_god_filter_sets_deferred_action() {
        let mut app = App::new();
        app.focus = Focus::FileTree;
        app.god_file_filter_mode = true;
        app.browsing_main = false;
        dispatch_escape(&mut app);
        assert!(app.deferred_action.is_some());
    }

    // ── 11. Session focus — with session list ──

    #[test]
    fn test_session_with_list_closes_list() {
        let mut app = App::new();
        app.focus = Focus::Session;
        app.show_session_list = true;
        dispatch_escape(&mut app);
        assert!(!app.show_session_list);
        // Focus should stay on Session (we just close the list)
        assert_eq!(app.focus, Focus::Session);
    }

    #[test]
    fn test_session_without_list_moves_to_worktrees() {
        let mut app = App::new();
        app.focus = Focus::Session;
        app.show_session_list = false;
        dispatch_escape(&mut app);
        assert_eq!(app.focus, Focus::Worktrees);
    }

    // ── 12. Input — terminal mode (not prompt) closes terminal ──

    #[test]
    fn test_input_terminal_mode_closes_terminal() {
        let mut app = App::new();
        app.focus = Focus::Input;
        app.terminal_mode = true;
        app.prompt_mode = false;
        dispatch_escape(&mut app);
        assert!(!app.terminal_mode);
    }

    #[test]
    fn test_input_terminal_mode_also_clears_prompt_mode() {
        let mut app = App::new();
        app.focus = Focus::Input;
        app.terminal_mode = true;
        app.prompt_mode = false;
        dispatch_escape(&mut app);
        assert!(!app.prompt_mode);
    }

    // ── 13. Input — prompt mode exits prompt ──

    #[test]
    fn test_input_prompt_mode_exits_prompt() {
        let mut app = App::new();
        app.focus = Focus::Input;
        app.prompt_mode = true;
        dispatch_escape(&mut app);
        assert!(!app.prompt_mode);
    }

    #[test]
    fn test_input_prompt_mode_stays_input_focus() {
        let mut app = App::new();
        app.focus = Focus::Input;
        app.prompt_mode = true;
        dispatch_escape(&mut app);
        // After exiting prompt_mode, the focus should still be Input
        // (the match arm doesn't change focus)
        assert_eq!(app.focus, Focus::Input);
    }

    // ── 14. Input — neither terminal nor prompt → no-op ──

    #[test]
    fn test_input_neither_terminal_nor_prompt_noop() {
        let mut app = App::new();
        app.focus = Focus::Input;
        app.terminal_mode = false;
        app.prompt_mode = false;
        let focus_before = app.focus;
        dispatch_escape(&mut app);
        assert_eq!(app.focus, focus_before);
    }

    // ── 15. Input — terminal + prompt: prompt takes priority ──

    #[test]
    fn test_input_terminal_and_prompt_exits_prompt_only() {
        let mut app = App::new();
        app.focus = Focus::Input;
        app.terminal_mode = true;
        app.prompt_mode = true;
        dispatch_escape(&mut app);
        // prompt_mode match arm fires first (it matches `Focus::Input if app.prompt_mode`)
        assert!(!app.prompt_mode);
        // terminal_mode should remain true
        assert!(app.terminal_mode);
    }

    // ── 16. WorktreeCreation focus — fallthrough (no-op) ──

    #[test]
    fn test_worktree_creation_focus_noop() {
        let mut app = App::new();
        app.focus = Focus::WorktreeCreation;
        dispatch_escape(&mut app);
        assert_eq!(app.focus, Focus::WorktreeCreation);
    }

    // ── 17. BranchDialog focus — fallthrough (no-op) ──

    #[test]
    fn test_branch_dialog_focus_noop() {
        let mut app = App::new();
        app.focus = Focus::BranchDialog;
        dispatch_escape(&mut app);
        assert_eq!(app.focus, Focus::BranchDialog);
    }

    // ── 18. Viewer edit mode flag isolation ──

    #[test]
    fn test_viewer_edit_mode_dispatch_does_not_change_focus_when_dirty() {
        let mut app = App::new();
        app.focus = Focus::Viewer;
        app.viewer_edit_mode = true;
        app.viewer_edit_dirty = true;
        dispatch_escape(&mut app);
        // Focus stays Viewer (discard dialog shown, not exited)
        assert_eq!(app.focus, Focus::Viewer);
    }

    // ── 19. Viewer non-edit, no diff — zero scroll after clear ──

    #[test]
    fn test_viewer_clear_resets_scroll() {
        let mut app = App::new();
        app.focus = Focus::Viewer;
        app.viewer_scroll = 100;
        dispatch_escape(&mut app);
        assert_eq!(app.viewer_scroll, 0);
    }

    // ── 20. Viewer path cleared on plain close ──

    #[test]
    fn test_viewer_plain_close_clears_path() {
        let mut app = App::new();
        app.focus = Focus::Viewer;
        app.viewer_path = Some(std::path::PathBuf::from("/test.rs"));
        dispatch_escape(&mut app);
        assert!(app.viewer_path.is_none());
    }

    // ── 21. Session escape with list, list_selected preserved ──

    #[test]
    fn test_session_escape_preserves_list_selected() {
        let mut app = App::new();
        app.focus = Focus::Session;
        app.show_session_list = true;
        app.session_list_selected = 5;
        dispatch_escape(&mut app);
        // session_list_selected is not reset by escape
        assert_eq!(app.session_list_selected, 5);
    }

    // ── 22. Multiple escapes cascade correctly ──

    #[test]
    fn test_double_escape_from_viewer_to_worktrees() {
        let mut app = App::new();
        app.focus = Focus::Viewer;
        dispatch_escape(&mut app);
        assert_eq!(app.focus, Focus::FileTree);
        // Second escape from FileTree → Worktrees
        dispatch_escape(&mut app);
        assert_eq!(app.focus, Focus::Worktrees);
    }

    #[test]
    fn test_triple_escape_from_viewer_to_file_tree_to_worktrees() {
        let mut app = App::new();
        app.focus = Focus::Viewer;
        dispatch_escape(&mut app);
        assert_eq!(app.focus, Focus::FileTree);
        dispatch_escape(&mut app);
        assert_eq!(app.focus, Focus::Worktrees);
        // Third escape from Worktrees → FileTree
        dispatch_escape(&mut app);
        assert_eq!(app.focus, Focus::FileTree);
    }

    // ── 23. Viewer lines_dirty set correctly on clear ──

    #[test]
    fn test_viewer_clear_sets_lines_dirty() {
        let mut app = App::new();
        app.focus = Focus::Viewer;
        app.viewer_lines_dirty = false;
        dispatch_escape(&mut app);
        assert!(app.viewer_lines_dirty);
    }

    // ── 24. Session escape without list goes to worktrees ──

    #[test]
    fn test_session_no_list_escape_to_worktrees() {
        let mut app = App::new();
        app.focus = Focus::Session;
        app.show_session_list = false;
        dispatch_escape(&mut app);
        assert_eq!(app.focus, Focus::Worktrees);
    }

    // ── 25. FileTree normal: browsing_main precedence ──

    #[test]
    fn test_file_tree_browsing_main_takes_priority_over_god_filter() {
        let mut app = App::new();
        app.focus = Focus::FileTree;
        app.browsing_main = true;
        app.god_file_filter_mode = true;
        // browsing_main match arm fires first
        dispatch_escape(&mut app);
        assert!(!app.browsing_main);
        // god_file_filter_mode should still be true (not reached)
        assert!(app.god_file_filter_mode);
    }

    // ── 26. Worktrees escape without browsing_main, focus becomes FileTree ──

    #[test]
    fn test_worktrees_escape_focus_is_file_tree() {
        let mut app = App::new();
        app.focus = Focus::Worktrees;
        dispatch_escape(&mut app);
        assert_eq!(app.focus, Focus::FileTree);
    }

    // ── 27. Viewer diff overlay: prev_state None path ──

    #[test]
    fn test_viewer_diff_prev_state_none_path() {
        let mut app = App::new();
        app.focus = Focus::Viewer;
        app.viewer_edit_diff = Some(("a".to_string(), "b".to_string()));
        app.viewer_prev_state = Some((Some("content".to_string()), None, 10));
        dispatch_escape(&mut app);
        assert_eq!(app.viewer_content.as_deref(), Some("content"));
        assert!(app.viewer_path.is_none());
        assert_eq!(app.viewer_scroll, 10);
    }

    // ── 28. Session list close does not change focus ──

    #[test]
    fn test_session_close_list_keeps_session_focus() {
        let mut app = App::new();
        app.focus = Focus::Session;
        app.show_session_list = true;
        dispatch_escape(&mut app);
        assert_eq!(app.focus, Focus::Session);
    }

    // ── 29. Viewer edit: clean exit clears undo/redo stacks ──

    #[test]
    fn test_viewer_edit_clean_exit_clears_undo_redo() {
        let mut app = App::new();
        app.focus = Focus::Viewer;
        app.viewer_edit_mode = true;
        app.viewer_edit_dirty = false;
        app.viewer_edit_undo.push(vec!["snapshot".to_string()]);
        app.viewer_edit_redo.push(vec!["redo".to_string()]);
        dispatch_escape(&mut app);
        assert!(app.viewer_edit_undo.is_empty());
        assert!(app.viewer_edit_redo.is_empty());
    }

    // ── 30. Input: terminal_mode true, prompt_mode true — prompt matched first ──

    #[test]
    fn test_input_prompt_priority_keeps_terminal_alive() {
        let mut app = App::new();
        app.focus = Focus::Input;
        app.terminal_mode = true;
        app.prompt_mode = true;
        dispatch_escape(&mut app);
        assert!(!app.prompt_mode);
        assert!(app.terminal_mode);
        assert_eq!(app.focus, Focus::Input);
    }

    // ── 31. FileTree escape without god filter or browse ──

    #[test]
    fn test_file_tree_plain_escape_no_side_effects() {
        let mut app = App::new();
        app.focus = Focus::FileTree;
        app.browsing_main = false;
        app.god_file_filter_mode = false;
        app.viewer_content = Some("untouched".to_string());
        dispatch_escape(&mut app);
        assert_eq!(app.focus, Focus::Worktrees);
        // viewer_content is unchanged by FileTree escape
        assert_eq!(app.viewer_content.as_deref(), Some("untouched"));
    }
}
