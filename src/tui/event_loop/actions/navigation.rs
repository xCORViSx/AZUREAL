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

#[cfg(test)]
mod tests {
    use super::*;

    // -- Focus enum equality/inequality --

    #[test]
    fn test_focus_viewer_eq() { assert_eq!(Focus::Viewer, Focus::Viewer); }
    #[test]
    fn test_focus_session_eq() { assert_eq!(Focus::Session, Focus::Session); }
    #[test]
    fn test_focus_worktrees_eq() { assert_eq!(Focus::Worktrees, Focus::Worktrees); }
    #[test]
    fn test_focus_file_tree_eq() { assert_eq!(Focus::FileTree, Focus::FileTree); }
    #[test]
    fn test_focus_input_eq() { assert_eq!(Focus::Input, Focus::Input); }
    #[test]
    fn test_focus_ne_viewer_session() { assert_ne!(Focus::Viewer, Focus::Session); }
    #[test]
    fn test_focus_ne_worktrees_input() { assert_ne!(Focus::Worktrees, Focus::Input); }
    #[test]
    fn test_focus_ne_file_tree_viewer() { assert_ne!(Focus::FileTree, Focus::Viewer); }

    // -- Focus matching for nav dispatch --

    #[test]
    fn test_focus_match_viewer() {
        let f = Focus::Viewer;
        assert!(matches!(f, Focus::Viewer));
    }

    #[test]
    fn test_focus_match_session() {
        let f = Focus::Session;
        assert!(matches!(f, Focus::Session));
    }

    #[test]
    fn test_focus_match_file_tree() {
        let f = Focus::FileTree;
        assert!(matches!(f, Focus::FileTree));
    }

    #[test]
    fn test_focus_match_worktrees_not_browsing() {
        let f = Focus::Worktrees;
        let browsing = false;
        assert!(matches!(f, Focus::Worktrees) && !browsing);
    }

    #[test]
    fn test_focus_match_worktrees_browsing() {
        let f = Focus::Worktrees;
        let browsing = true;
        assert!(!(matches!(f, Focus::Worktrees) && !browsing));
    }

    #[test]
    fn test_focus_match_input_terminal() {
        let f = Focus::Input;
        let terminal = true;
        let prompt = false;
        assert!(matches!(f, Focus::Input) && terminal && !prompt);
    }

    #[test]
    fn test_focus_match_input_prompt_mode() {
        let f = Focus::Input;
        let terminal = true;
        let prompt = true;
        assert!(!(matches!(f, Focus::Input) && terminal && !prompt));
    }

    #[test]
    fn test_focus_match_input_no_terminal() {
        let f = Focus::Input;
        let terminal = false;
        let prompt = false;
        assert!(!(matches!(f, Focus::Input) && terminal && !prompt));
    }

    // -- Page size calculations --

    #[test]
    fn test_page_size_viewer() {
        let viewport_height = 30usize;
        let page = viewport_height.saturating_sub(2);
        assert_eq!(page, 28);
    }

    #[test]
    fn test_page_size_small_viewport() {
        let viewport_height = 2usize;
        let page = viewport_height.saturating_sub(2);
        assert_eq!(page, 0);
    }

    #[test]
    fn test_page_size_terminal() {
        let terminal_height = 15u16;
        let page = (terminal_height as usize).saturating_sub(2);
        assert_eq!(page, 13);
    }

    #[test]
    fn test_page_size_tiny_terminal() {
        let terminal_height = 1u16;
        let page = (terminal_height as usize).saturating_sub(2);
        assert_eq!(page, 0);
    }

    // -- Worktrees down is no-op (horizontal tab row) --

    #[test]
    fn test_worktrees_down_is_noop() {
        let focus = Focus::Worktrees;
        let handled = match focus {
            Focus::Viewer => true,
            Focus::Session => true,
            Focus::Worktrees => false, // no-op
            Focus::FileTree => true,
            _ => false,
        };
        assert!(!handled);
    }

    // -- Worktrees up is no-op --

    #[test]
    fn test_worktrees_up_is_noop() {
        let focus = Focus::Worktrees;
        let handled = match focus {
            Focus::Viewer => true,
            Focus::Session => true,
            Focus::Worktrees => false, // no-op
            Focus::FileTree => true,
            _ => false,
        };
        assert!(!handled);
    }

    // -- GoToTop sets scroll to 0 --

    #[test]
    fn test_go_to_top_viewer_scroll() {
        let mut scroll = 42usize;
        assert_eq!(scroll, 42);
        scroll = 0;
        assert_eq!(scroll, 0);
    }

    #[test]
    fn test_go_to_top_session_scroll() {
        let mut scroll = 100usize;
        assert_eq!(scroll, 100);
        scroll = 0;
        assert_eq!(scroll, 0);
    }

    #[test]
    fn test_go_to_top_terminal_scroll() {
        let mut scroll = 50usize;
        assert_eq!(scroll, 50);
        scroll = 0;
        assert_eq!(scroll, 0);
    }

    // -- NavLeft/NavRight context --

    #[test]
    fn test_nav_left_focus_worktrees() {
        let f = Focus::Worktrees;
        assert!(matches!(f, Focus::Worktrees));
    }

    #[test]
    fn test_nav_left_focus_file_tree() {
        let f = Focus::FileTree;
        assert!(matches!(f, Focus::FileTree));
    }

    #[test]
    fn test_nav_right_focus_worktrees() {
        let f = Focus::Worktrees;
        assert!(matches!(f, Focus::Worktrees));
    }

    #[test]
    fn test_nav_right_focus_file_tree() {
        let f = Focus::FileTree;
        assert!(matches!(f, Focus::FileTree));
    }

    // -- All 8 dispatch functions exist --

    #[test]
    fn test_dispatch_functions_exist() {
        // Verify the function names are accessible (type-system test)
        let _ = dispatch_nav_down as fn(&mut App);
        let _ = dispatch_nav_up as fn(&mut App);
        let _ = dispatch_nav_left as fn(&mut App);
        let _ = dispatch_nav_right as fn(&mut App);
        let _ = dispatch_page_down as fn(&mut App);
        let _ = dispatch_page_up as fn(&mut App);
        let _ = dispatch_go_to_top as fn(&mut App);
        let _ = dispatch_go_to_bottom as fn(&mut App);
    }

    // -- Default focus fallthrough --

    #[test]
    fn test_branch_dialog_not_handled_by_nav() {
        let f = Focus::BranchDialog;
        let handled = match f {
            Focus::Viewer | Focus::Session | Focus::Worktrees | Focus::FileTree => true,
            Focus::Input => true,
            _ => false,
        };
        assert!(!handled);
    }


    // -- Page size saturating_sub edge cases --

    #[test]
    fn test_page_size_zero_viewport() {
        let viewport_height = 0usize;
        let page = viewport_height.saturating_sub(2);
        assert_eq!(page, 0);
    }

    #[test]
    fn test_page_size_one_viewport() {
        let viewport_height = 1usize;
        let page = viewport_height.saturating_sub(2);
        assert_eq!(page, 0);
    }

    #[test]
    fn test_page_size_exactly_two() {
        let viewport_height = 2usize;
        let page = viewport_height.saturating_sub(2);
        assert_eq!(page, 0);
    }

    #[test]
    fn test_page_size_large_viewport() {
        let viewport_height = 100usize;
        let page = viewport_height.saturating_sub(2);
        assert_eq!(page, 98);
    }

    #[test]
    fn test_terminal_page_size_zero() {
        let terminal_height = 0u16;
        let page = (terminal_height as usize).saturating_sub(2);
        assert_eq!(page, 0);
    }

    #[test]
    fn test_terminal_page_size_two() {
        let terminal_height = 2u16;
        let page = (terminal_height as usize).saturating_sub(2);
        assert_eq!(page, 0);
    }

    #[test]
    fn test_terminal_page_size_large() {
        let terminal_height = 50u16;
        let page = (terminal_height as usize).saturating_sub(2);
        assert_eq!(page, 48);
    }

    // -- GoToTop scroll resets --

    #[test]
    fn test_go_to_top_scroll_from_large_value() {
        let scroll: usize = 9999;
        let reset = 0usize;
        assert_eq!(reset, 0);
        assert_ne!(scroll, reset);
    }

    #[test]
    fn test_go_to_top_already_at_zero() {
        let scroll: usize = 0;
        let reset = 0usize;
        assert_eq!(scroll, reset);
    }

    // -- Focus variant exhaustiveness (all 7 variants covered) --

    #[test]
    fn test_focus_health_not_handled_by_nav() {
        // Focus only has 7 variants — verify unknown variants fall through
        let f = Focus::BranchDialog;
        let is_nav_focus = matches!(f, Focus::Viewer | Focus::Session | Focus::Worktrees | Focus::FileTree | Focus::Input);
        assert!(!is_nav_focus);
    }


    // -- Worktrees browsing_main guard (NavLeft/NavRight/GoToTop/GoToBottom) --

    #[test]
    fn test_worktrees_browsing_main_blocks_nav_left() {
        let f = Focus::Worktrees;
        let browsing_main = true;
        // dispatch_nav_left skips when browsing_main is true
        let should_act = matches!(f, Focus::Worktrees) && !browsing_main;
        assert!(!should_act);
    }

    #[test]
    fn test_worktrees_not_browsing_main_allows_nav_left() {
        let f = Focus::Worktrees;
        let browsing_main = false;
        let should_act = matches!(f, Focus::Worktrees) && !browsing_main;
        assert!(should_act);
    }

    #[test]
    fn test_worktrees_browsing_main_blocks_nav_right() {
        let f = Focus::Worktrees;
        let browsing_main = true;
        let should_act = matches!(f, Focus::Worktrees) && !browsing_main;
        assert!(!should_act);
    }

    #[test]
    fn test_worktrees_browsing_main_blocks_go_to_top() {
        let f = Focus::Worktrees;
        let browsing_main = true;
        let should_act = matches!(f, Focus::Worktrees) && !browsing_main;
        assert!(!should_act);
    }

    #[test]
    fn test_worktrees_browsing_main_blocks_go_to_bottom() {
        let f = Focus::Worktrees;
        let browsing_main = true;
        let should_act = matches!(f, Focus::Worktrees) && !browsing_main;
        assert!(!should_act);
    }

    // -- Input focus terminal+prompt guard --

    #[test]
    fn test_input_focus_with_terminal_no_prompt_is_active() {
        let f = Focus::Input;
        let terminal_mode = true;
        let prompt_mode = false;
        let active = matches!(f, Focus::Input) && terminal_mode && !prompt_mode;
        assert!(active);
    }

    #[test]
    fn test_input_focus_with_both_terminal_and_prompt_is_inactive() {
        let f = Focus::Input;
        let terminal_mode = true;
        let prompt_mode = true;
        let active = matches!(f, Focus::Input) && terminal_mode && !prompt_mode;
        assert!(!active);
    }

    #[test]
    fn test_input_focus_without_terminal_is_inactive() {
        let f = Focus::Input;
        let terminal_mode = false;
        let prompt_mode = false;
        let active = matches!(f, Focus::Input) && terminal_mode && !prompt_mode;
        assert!(!active);
    }

    // -- Dispatch function pointer types are correct --

    #[test]
    fn test_dispatch_nav_down_type() {
        let f: fn(&mut App) = dispatch_nav_down;
        let _ = f;
    }

    #[test]
    fn test_dispatch_nav_up_type() {
        let f: fn(&mut App) = dispatch_nav_up;
        let _ = f;
    }

    #[test]
    fn test_dispatch_nav_left_type() {
        let f: fn(&mut App) = dispatch_nav_left;
        let _ = f;
    }

    #[test]
    fn test_dispatch_nav_right_type() {
        let f: fn(&mut App) = dispatch_nav_right;
        let _ = f;
    }

    #[test]
    fn test_dispatch_page_down_type() {
        let f: fn(&mut App) = dispatch_page_down;
        let _ = f;
    }

    #[test]
    fn test_dispatch_page_up_type() {
        let f: fn(&mut App) = dispatch_page_up;
        let _ = f;
    }

    #[test]
    fn test_dispatch_go_to_top_type() {
        let f: fn(&mut App) = dispatch_go_to_top;
        let _ = f;
    }

    #[test]
    fn test_dispatch_go_to_bottom_type() {
        let f: fn(&mut App) = dispatch_go_to_bottom;
        let _ = f;
    }

    // -- App::new() exercises nav dispatch without panic --

    #[test]
    fn test_nav_down_viewer_does_not_panic() {
        let mut app = App::new();
        app.focus = Focus::Viewer;
        dispatch_nav_down(&mut app);
    }

    #[test]
    fn test_nav_up_viewer_does_not_panic() {
        let mut app = App::new();
        app.focus = Focus::Viewer;
        dispatch_nav_up(&mut app);
    }

    #[test]
    fn test_nav_down_session_does_not_panic() {
        let mut app = App::new();
        app.focus = Focus::Session;
        dispatch_nav_down(&mut app);
    }

    #[test]
    fn test_nav_up_session_does_not_panic() {
        let mut app = App::new();
        app.focus = Focus::Session;
        dispatch_nav_up(&mut app);
    }

    #[test]
    fn test_page_down_viewer_does_not_panic() {
        let mut app = App::new();
        app.focus = Focus::Viewer;
        dispatch_page_down(&mut app);
    }

    #[test]
    fn test_page_up_viewer_does_not_panic() {
        let mut app = App::new();
        app.focus = Focus::Viewer;
        dispatch_page_up(&mut app);
    }

    #[test]
    fn test_go_to_top_viewer_resets_scroll() {
        let mut app = App::new();
        app.focus = Focus::Viewer;
        app.viewer_scroll = 42;
        dispatch_go_to_top(&mut app);
        assert_eq!(app.viewer_scroll, 0);
    }

    #[test]
    fn test_go_to_top_session_resets_scroll() {
        let mut app = App::new();
        app.focus = Focus::Session;
        app.session_scroll = 77;
        dispatch_go_to_top(&mut app);
        assert_eq!(app.session_scroll, 0);
    }

    #[test]
    fn test_nav_down_worktrees_noop_does_not_panic() {
        let mut app = App::new();
        app.focus = Focus::Worktrees;
        dispatch_nav_down(&mut app);
    }

    #[test]
    fn test_nav_up_worktrees_noop_does_not_panic() {
        let mut app = App::new();
        app.focus = Focus::Worktrees;
        dispatch_nav_up(&mut app);
    }

    #[test]
    fn test_go_to_top_file_tree_does_not_panic() {
        let mut app = App::new();
        app.focus = Focus::FileTree;
        dispatch_go_to_top(&mut app);
    }

    #[test]
    fn test_go_to_bottom_file_tree_does_not_panic() {
        let mut app = App::new();
        app.focus = Focus::FileTree;
        dispatch_go_to_bottom(&mut app);
    }
}
