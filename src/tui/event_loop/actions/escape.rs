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
