//! Context-dependent escape dispatch
//!
//! Handles the Escape key action based on current focus and mode:
//! viewer edit mode, viewer, file tree, output, input/terminal.

use crate::app::{App, Focus};

/// Escape dispatch — context-dependent close/back
pub(super) fn dispatch_escape(app: &mut App) {
    match app.focus {
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
                // Exit scope mode — save scope (fast) and defer the expensive rescan
                if let Some(ref project) = app.project {
                    crate::app::save_health_scope(&project.path, &app.god_file_filter_dirs);
                }
                let dirs: Vec<String> = app.god_file_filter_dirs.iter()
                    .map(|p| p.to_string_lossy().to_string()).collect();
                app.god_file_filter_mode = false;
                app.god_file_filter_dirs.clear();
                app.show_file_tree = false;
                app.focus = crate::app::Focus::Worktrees;
                app.loading_indicator = Some("Rescanning health scope…".into());
                app.deferred_action = Some(crate::app::DeferredAction::RescanHealthScope { dirs });
            } else {
                app.show_file_tree = false;
                app.focus = Focus::Worktrees;
                app.invalidate_sidebar();
            }
        }
        Focus::Output => {
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
