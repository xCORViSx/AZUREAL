//! Focus-aware navigation dispatch
//!
//! Routes NavDown/NavUp/NavLeft/NavRight/PageDown/PageUp/GoToTop/GoToBottom
//! actions to the correct pane's scroll or selection method.

use crate::app::{App, Focus};

/// Route NavDown to the focused pane's down handler
pub(super) fn dispatch_nav_down(app: &mut App) {
    match app.focus {
        Focus::Viewer => { app.scroll_viewer_down(1); }
        Focus::Session => {
            app.scroll_session_down(1);
        }
        // Worktree tab row is horizontal — Down is a no-op
        Focus::Worktrees => {}
        Focus::FileTree => app.file_tree_next(),
        Focus::Input if app.terminal_mode && !app.prompt_mode => {
            app.scroll_terminal_down(1);
        }
        _ => {}
    }
}

/// Route NavUp to the focused pane's up handler
pub(super) fn dispatch_nav_up(app: &mut App) {
    match app.focus {
        Focus::Viewer => { app.scroll_viewer_up(1); }
        Focus::Session => {
            app.scroll_session_up(1);
        }
        // Worktree tab row is horizontal — Up is a no-op
        Focus::Worktrees => {}
        Focus::FileTree => app.file_tree_prev(),
        Focus::Input if app.terminal_mode && !app.prompt_mode => {
            app.scroll_terminal_up(1);
        }
        _ => {}
    }
}

/// Route NavLeft — FileTree collapses dirs, Worktrees cycles tabs backward
pub(super) fn dispatch_nav_left(app: &mut App) {
    match app.focus {
        // Worktree tab row: Left cycles to previous tab (skip when browsing main)
        Focus::Worktrees if !app.browsing_main => { app.select_prev_session(); }
        Focus::FileTree => {
            if let Some(idx) = app.file_tree_selected {
                if let Some(entry) = app.file_tree_entries.get(idx).cloned() {
                    if entry.is_dir && app.file_tree_expanded.contains(&entry.path) {
                        app.toggle_file_tree_dir();
                    } else if let Some(parent) = entry.path.parent() {
                        let parent_path = parent.to_path_buf();
                        if let Some(pi) = app.file_tree_entries.iter().position(|e| e.path == parent_path && e.is_dir) {
                            if app.file_tree_expanded.contains(&parent_path) {
                                app.file_tree_selected = Some(pi);
                                app.toggle_file_tree_dir();
                            }
                        }
                    }
                }
            }
        }
        _ => {}
    }
}

/// Route NavRight — FileTree expands dirs, Worktrees cycles tabs forward
pub(super) fn dispatch_nav_right(app: &mut App) {
    match app.focus {
        // Worktree tab row: Right cycles to next tab (skip when browsing main)
        Focus::Worktrees if !app.browsing_main => { app.select_next_session(); }
        Focus::FileTree => {
            if let Some(idx) = app.file_tree_selected {
                if let Some(entry) = app.file_tree_entries.get(idx).cloned() {
                    if entry.is_dir && !app.file_tree_expanded.contains(&entry.path) {
                        app.toggle_file_tree_dir();
                    }
                }
            }
        }
        _ => {}
    }
}

/// Route PageDown to the focused pane
pub(super) fn dispatch_page_down(app: &mut App) {
    match app.focus {
        Focus::Viewer => { app.scroll_viewer_down(app.viewer_viewport_height.saturating_sub(2)); }
        Focus::Session => {
            let page = app.session_viewport_height.saturating_sub(2);
            app.scroll_session_down(page);
        }
        Focus::Input if app.terminal_mode && !app.prompt_mode => {
            app.scroll_terminal_down((app.terminal_height as usize).saturating_sub(2));
        }
        _ => {}
    }
}

/// Route PageUp to the focused pane
pub(super) fn dispatch_page_up(app: &mut App) {
    match app.focus {
        Focus::Viewer => { app.scroll_viewer_up(app.viewer_viewport_height.saturating_sub(2)); }
        Focus::Session => {
            let page = app.session_viewport_height.saturating_sub(2);
            app.scroll_session_up(page);
        }
        Focus::Input if app.terminal_mode && !app.prompt_mode => {
            app.scroll_terminal_up((app.terminal_height as usize).saturating_sub(2));
        }
        _ => {}
    }
}

/// Route GoToTop to the focused pane
pub(super) fn dispatch_go_to_top(app: &mut App) {
    match app.focus {
        Focus::Viewer => app.viewer_scroll = 0,
        Focus::Session => { app.session_scroll = 0; }
        Focus::Worktrees if !app.browsing_main => app.select_first_session(),
        Focus::FileTree => app.file_tree_first_sibling(),
        Focus::Input if app.terminal_mode && !app.prompt_mode => {
            app.terminal_scroll = 0;
        }
        _ => {}
    }
}

/// Route GoToBottom to the focused pane
pub(super) fn dispatch_go_to_bottom(app: &mut App) {
    match app.focus {
        Focus::Viewer => app.scroll_viewer_to_bottom(),
        Focus::Session => { app.scroll_session_to_bottom(); }
        Focus::Worktrees if !app.browsing_main => app.select_last_session(),
        Focus::FileTree => app.file_tree_last_sibling(),
        Focus::Input if app.terminal_mode && !app.prompt_mode => {
            app.scroll_terminal_to_bottom();
        }
        _ => {}
    }
}
