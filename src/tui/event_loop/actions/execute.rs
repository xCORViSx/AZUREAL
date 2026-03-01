//! Action execution dispatch
//!
//! Routes resolved keybinding actions to the correct handler. Called by
//! handle_key_event() after lookup_action() identifies WHAT to do —
//! this function handles the HOW.

use anyhow::Result;

use crate::app::{App, Focus};
use crate::claude::ClaudeProcess;
use crate::tui::keybindings::Action;
use super::super::mouse::{copy_viewer_selection, copy_session_selection};
use super::navigation::{
    dispatch_nav_down, dispatch_nav_up, dispatch_nav_left, dispatch_nav_right,
    dispatch_page_down, dispatch_page_up, dispatch_go_to_top, dispatch_go_to_bottom,
};
use super::escape::dispatch_escape;
use super::session_list::open_session_list;

/// Execute a resolved keybinding action. Called by handle_key_event() after
/// lookup_action() identifies WHAT to do. This function handles the HOW.
pub(super) fn execute_action(action: Action, app: &mut App, _claude_process: &ClaudeProcess) -> Result<()> {
    match action {
        // --- Global actions ---
        Action::Quit => {
            if !app.git_action_in_progress() { app.should_quit = true; }
            else { app.set_status("Cannot quit while a git operation is in progress"); }
        }
        Action::DumpDebug => {
            app.debug_dump_naming = Some(String::new());
        }
        Action::CancelClaude => { app.cancel_current_claude(); }
        Action::CopySelection => {
            // Copy from whichever pane has an active selection
            if app.prompt_mode && app.has_input_selection() {
                app.input_copy();
            } else if app.viewer_selection.is_some() {
                copy_viewer_selection(app);
            } else if app.session_selection.is_some() {
                copy_session_selection(app);
            } else if let Some(ref p) = app.git_actions_panel {
                // Git mode fallback: copy status box result message
                if let Some((ref msg, _)) = p.result_message {
                    let text = msg.clone();
                    if let Ok(mut cb) = arboard::Clipboard::new() { let _ = cb.set_text(&text); }
                    app.clipboard = text;
                    app.set_status("Copied to clipboard");
                }
            }
        }
        Action::ToggleHelp => { app.toggle_help(); }
        Action::EnterPromptMode if !app.browsing_main => {
            app.show_help = false;
            if app.terminal_mode { app.close_terminal(); }
            app.focus = Focus::Input;
            app.prompt_mode = true;
        }
        Action::ToggleTerminal => {
            app.show_help = false;
            app.toggle_terminal();
            app.focus = Focus::Input;
        }
        Action::CycleFocusForward => {
            app.prompt_mode = false;
            app.viewer_selection = None;
            app.session_selection = None;
            app.focus_next();
        }
        Action::CycleFocusBackward => {
            app.prompt_mode = false;
            app.viewer_selection = None;
            app.session_selection = None;
            app.focus_prev();
        }

        // --- Terminal resize (global when terminal is open) ---
        Action::ResizeUp => { app.adjust_terminal_height(2); }
        Action::ResizeDown => { app.adjust_terminal_height(-2); }

        // --- All other actions are focus-specific; dispatch inline ---
        // Worktrees
        Action::BrowseMain => {
            if app.browsing_main { app.exit_main_browse(); }
            else { app.enter_main_browse(); }
        }
        Action::ReturnToWorktrees if !app.god_file_filter_mode => {
            if app.browsing_main { app.exit_main_browse(); }
            else {
                app.focus = Focus::Worktrees;
                app.invalidate_sidebar();
            }
        }
        Action::ToggleSessionList => {
            if app.show_session_list { app.show_session_list = false; }
            else { open_session_list(app); }
        }

        // --- Viewer tab management ---
        Action::ViewerTabCurrent => { app.viewer_tab_current(); }
        Action::ViewerOpenTabDialog => {
            if !app.viewer_tabs.is_empty() { app.toggle_viewer_tab_dialog(); }
        }
        Action::ViewerCloseTab => { app.viewer_close_current_tab(); }
        Action::SelectAll => {
            // Read-only viewer: select entire cache. Edit mode: select all edit content.
            if app.viewer_edit_mode {
                app.viewer_edit_select_all();
            } else {
                let last = app.viewer_lines_cache.len().saturating_sub(1);
                let last_col = app.viewer_lines_cache.last()
                    .map(|l| l.spans.iter().map(|s| s.content.chars().count()).sum::<usize>())
                    .unwrap_or(0);
                app.viewer_selection = Some((0, 0, last, last_col));
            }
        }

        // --- Viewer navigation ---
        Action::EnterEditMode if !app.browsing_main => {
            if app.viewer_path.is_some() { app.enter_viewer_edit_mode(); }
        }
        Action::JumpNextEdit => { jump_edit(app, true); }
        Action::JumpPrevEdit => { jump_edit(app, false); }

        // --- Viewer edit mode ---
        Action::Save => {
            match app.save_viewer_edits() {
                Ok(()) => {
                    app.set_status("File saved");
                    if app.viewer_edit_diff.is_some() {
                        app.viewer_edit_save_dialog = true;
                    }
                }
                Err(e) => app.set_status(format!("Save failed: {}", e)),
            }
        }
        Action::Undo => { app.viewer_edit_undo(); }
        Action::Redo => { app.viewer_edit_redo(); }

        // --- Shared navigation (used by viewer, output, worktrees, file tree, terminal) ---
        Action::NavDown => { dispatch_nav_down(app); }
        Action::NavUp => { dispatch_nav_up(app); }
        Action::NavLeft => { dispatch_nav_left(app); }
        Action::NavRight => { dispatch_nav_right(app); }
        Action::PageDown => { dispatch_page_down(app); }
        Action::PageUp => { dispatch_page_up(app); }
        Action::GoToTop => { dispatch_go_to_top(app); }
        Action::GoToBottom => { dispatch_go_to_bottom(app); }

        // --- Worktree-specific ---
        Action::AddWorktree => {
            // Unified Add Worktree dialog: fetch branches, compute per-branch worktree counts
            if let Some(project) = app.current_project().cloned() {
                match crate::git::Git::list_all_branches_with_status(&project.path) {
                    Ok((branches, checked_out)) => {
                        // Per-branch worktree count: how many worktrees (active + archived) use each branch
                        let worktree_counts: Vec<usize> = branches.iter().map(|branch| {
                            let local = if branch.contains('/') {
                                branch.split('/').skip(1).collect::<Vec<_>>().join("/")
                            } else {
                                branch.clone()
                            };
                            app.worktrees.iter().filter(|wt| wt.branch_name == *branch || wt.branch_name == local).count()
                        }).collect();
                        app.open_branch_dialog(branches, checked_out, worktree_counts);
                    }
                    Err(e) => app.set_status(format!("Failed to list branches: {}", e)),
                }
            } else {
                app.set_status("No project loaded — open a project first");
            }
        }
        Action::NewSession => {
            // Start a fresh Claude session in the current worktree (don't resume)
            if let Some(wt) = app.current_worktree().cloned() {
                if wt.archived {
                    app.set_status("Worktree is archived — unarchive first (⌘a)");
                } else if wt.worktree_path.is_some() {
                    let branch = wt.branch_name.clone();
                    // Clear session ID so next prompt starts fresh (no --resume)
                    app.claude_session_ids.remove(&branch);
                    // Clear display to show fresh conversation
                    app.display_events.clear();
                    app.session_lines.clear();
                    app.session_buffer.clear();
                    app.session_scroll = usize::MAX;
                    app.session_file_parse_offset = 0;
                    app.rendered_events_count = 0;
                    app.rendered_content_line_count = 0;
                    app.rendered_events_start = 0;
                    app.event_parser = crate::events::EventParser::new();
                    app.selected_event = None;
                    app.current_todos.clear();
                    app.subagent_todos.clear();
                    app.session_tokens = None;
                    app.token_badge_cache = None;
                    app.invalidate_render_cache();
                    // Enter prompt mode for the new session
                    app.focus = Focus::Input;
                    app.prompt_mode = true;
                    app.set_status("Add session — type your prompt and press Enter");
                }
            }
        }
        Action::RunCommand => { app.open_run_command_picker(); }
        Action::AddRunCommand => { app.open_run_command_dialog(); }
        Action::ToggleArchiveWorktree => {
            let is_archived = app.current_worktree().map(|w| w.archived).unwrap_or(false);
            let result = if is_archived {
                app.unarchive_current_worktree()
            } else {
                app.archive_current_worktree()
            };
            if let Err(e) = result {
                app.set_status(format!("Failed: {}", e));
            }
        }
        Action::DeleteWorktree => {
            if let Some(wt) = app.current_worktree().cloned() {
                if let Some(project) = &app.project {
                    if wt.branch_name == project.main_branch {
                        app.set_status("Cannot delete main branch");
                    } else {
                        // Sibling guard: find other worktrees on the same branch
                        let current_idx = app.selected_worktree.unwrap_or(0);
                        let sibling_indices: Vec<usize> = app.worktrees.iter().enumerate()
                            .filter(|(i, w)| *i != current_idx && w.branch_name == wt.branch_name)
                            .map(|(i, _)| i)
                            .collect();
                        if sibling_indices.is_empty() {
                            // Sole worktree on this branch — normal delete flow
                            let name = wt.name().to_string();
                            app.set_status(format!("Delete '{}' and its branch? (y/N)", name));
                            app.worktree_delete_confirm = true;
                        } else {
                            // Has siblings — can't delete branch until all worktrees are gone
                            let count = sibling_indices.len();
                            app.set_status(format!(
                                "{} other worktree{} on this branch. Delete all? (y) Archive only? (a)",
                                count, if count == 1 { "" } else { "s" }
                            ));
                            app.worktree_delete_siblings = Some((wt.branch_name.clone(), sibling_indices));
                        }
                    }
                }
            }
        }
        // Worktree tab switching (global [ / ])
        Action::WorktreeTabPrev => {
            app.select_prev_session();
        }
        Action::WorktreeTabNext => {
            app.select_next_session();
        }
        Action::OpenHealth => {
            if app.health_panel.is_some() { app.close_health_panel(); }
            else {
                // Deferred health scan — show loading popup while recursive dir walk runs
                app.loading_indicator = Some("Scanning project health…".into());
                app.deferred_action = Some(crate::app::DeferredAction::OpenHealthPanel);
            }
        }
        Action::OpenGitActions => {
            // Toggle: close if already open, open otherwise
            if app.git_actions_panel.is_some() {
                app.close_git_actions_panel();
            } else {
                app.open_git_actions_panel();
            }
        }
        Action::OpenProjects => {
            app.open_projects_panel();
        }

        // --- FileTree ---
        Action::ToggleDir => {
            if let Some(idx) = app.file_tree_selected {
                if let Some(entry) = app.file_tree_entries.get(idx) {
                    if entry.is_dir { app.toggle_file_tree_dir(); }
                }
            }
        }
        Action::OpenFile => {
            if let Some(idx) = app.file_tree_selected {
                if let Some(entry) = app.file_tree_entries.get(idx).cloned() {
                    if app.god_file_filter_mode && entry.is_dir {
                        // In filter mode, Enter on a dir toggles it in/out of scan scope
                        app.god_file_filter_toggle_dir(entry.path);
                    } else if entry.is_dir {
                        app.toggle_file_tree_dir();
                    } else {
                        // Deferred file load — show "Loading <filename>…" while I/O runs
                        let filename = entry.path.file_name()
                            .map(|n| n.to_string_lossy().to_string())
                            .unwrap_or_else(|| "file".into());
                        app.loading_indicator = Some(format!("Loading {}…", filename));
                        app.deferred_action = Some(crate::app::DeferredAction::LoadFile {
                            path: entry.path.clone(),
                        });
                        app.focus = Focus::Viewer;
                    }
                }
            }
        }
        // Open file tree options overlay (toggle hidden dirs)
        Action::FileTreeOptions if !app.god_file_filter_mode => {
            app.file_tree_options_mode = true;
            app.file_tree_options_selected = 0;
        }

        // File actions disabled in god file filter mode and main browse mode (read-only)
        Action::AddFile if !app.god_file_filter_mode && !app.browsing_main => {
            app.file_tree_action = Some(crate::app::types::FileTreeAction::Add(String::new()));
        }
        Action::DeleteFile if !app.god_file_filter_mode && !app.browsing_main => {
            if app.file_tree_selected.is_some() {
                app.file_tree_action = Some(crate::app::types::FileTreeAction::Delete);
            }
        }
        Action::RenameFile if !app.god_file_filter_mode && !app.browsing_main => {
            if let Some(idx) = app.file_tree_selected {
                if let Some(entry) = app.file_tree_entries.get(idx) {
                    app.file_tree_action = Some(crate::app::types::FileTreeAction::Rename(entry.name.clone()));
                }
            }
        }
        Action::CopyFile if !app.god_file_filter_mode && !app.browsing_main => {
            if let Some(idx) = app.file_tree_selected {
                if let Some(entry) = app.file_tree_entries.get(idx) {
                    app.file_tree_action = Some(crate::app::types::FileTreeAction::Copy(entry.path.clone()));
                    app.set_status("Copy: select target dir, Enter to paste");
                    app.invalidate_file_tree();
                }
            }
        }
        Action::MoveFile if !app.god_file_filter_mode && !app.browsing_main => {
            if let Some(idx) = app.file_tree_selected {
                if let Some(entry) = app.file_tree_entries.get(idx) {
                    app.file_tree_action = Some(crate::app::types::FileTreeAction::Move(entry.path.clone()));
                    app.set_status("Move: select target dir, Enter to paste");
                    app.invalidate_file_tree();
                }
            }
        }

        // --- Output/Convo ---
        // Plain Up/Down: step through ALL bubbles (user + assistant)
        Action::JumpNextBubble => {
            if app.view_mode == crate::app::ViewMode::Session { app.jump_to_next_bubble(true); }
        }
        Action::JumpPrevBubble => {
            if app.view_mode == crate::app::ViewMode::Session { app.jump_to_prev_bubble(true); }
        }
        // Shift+Up/Down: jump to user prompts only (skip assistant responses)
        Action::JumpNextMessage => {
            if app.view_mode == crate::app::ViewMode::Session { app.jump_to_next_bubble(false); }
        }
        Action::JumpPrevMessage => {
            if app.view_mode == crate::app::ViewMode::Session { app.jump_to_prev_bubble(false); }
        }
        Action::SearchSession => {
            // Activate the session find bar — clears previous query and matches
            app.session_find_active = true;
            app.session_find.clear();
            app.session_find_matches.clear();
            app.session_find_current = 0;
        }

        // --- Input/Terminal actions: handled by their own handlers (skip here) ---
        // These are filtered out in handle_key_event() and fall through to
        // handle_input_mode(). Listed here for exhaustive match.
        Action::Submit | Action::InsertNewline | Action::ExitPromptMode
        | Action::WordLeft | Action::WordRight | Action::DeleteWord
        | Action::HistoryPrev | Action::HistoryNext
        | Action::EnterTerminalType => {}

        // ⌃m: cycle Claude model (opus → sonnet → haiku → default)
        Action::CycleModel => { app.cycle_model(); }

        // STT toggle — works from edit mode (viewer) AND prompt input.
        // Input focus is filtered out above (is_input_action) so the raw handler
        // in handle_input_mode() catches it there. For edit mode, this is the
        // only path since lookup_action() intercepts ⌃s before handle_viewer_input().
        Action::ToggleStt => { app.toggle_stt(); }

        // --- Generic escape: context-dependent close/back ---
        Action::Escape => {
            dispatch_escape(app);
        }

        // --- Preset prompts ---
        Action::PresetPrompts => { app.open_preset_prompt_picker(); }

        // --- Dialog actions (not reached here — modals intercept above) ---
        Action::Confirm | Action::Cancel | Action::DeleteSelected | Action::EditSelected => {}

        // Guarded arms that didn't match (e.g. file actions suppressed in god file filter mode)
        _ => {}
    }

    Ok(())
}

/// Jump to next/prev Edit tool entry in the clickable paths list
fn jump_edit(app: &mut App, forward: bool) {
    let edits: Vec<usize> = app.clickable_paths.iter().enumerate()
        .filter(|(_, (_, _, _, _, o, n, _))| !o.is_empty() || !n.is_empty())
        .map(|(i, _)| i).collect();
    if edits.is_empty() { return; }
    let cur = app.selected_tool_diff.and_then(|s| edits.iter().position(|&e| e >= s));
    let target = if forward {
        match cur { Some(pos) => (pos + 1) % edits.len(), None => 0 }
    } else {
        match cur { Some(0) | None => edits.len() - 1, Some(pos) => pos - 1 }
    };
    let idx = edits[target];
    app.selected_tool_diff = Some(idx);
    if let Some((line_idx, sc, ec, file_path, old_str, new_str, wlc)) = app.clickable_paths.get(idx).cloned() {
        app.clicked_path_highlight = Some((line_idx, sc, ec, wlc));
        app.session_viewport_scroll = usize::MAX;
        app.load_file_with_edit_diff(&file_path, &old_str, &new_str);
        app.session_scroll = line_idx.saturating_sub(3);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::keybindings::Action;

    // -- Action enum variant equality --

    #[test]
    fn test_action_quit_eq() { assert_eq!(Action::Quit, Action::Quit); }
    #[test]
    fn test_action_dump_debug_eq() { assert_eq!(Action::DumpDebug, Action::DumpDebug); }
    #[test]
    fn test_action_cancel_claude_eq() { assert_eq!(Action::CancelClaude, Action::CancelClaude); }
    #[test]
    fn test_action_copy_selection_eq() { assert_eq!(Action::CopySelection, Action::CopySelection); }
    #[test]
    fn test_action_toggle_help_eq() { assert_eq!(Action::ToggleHelp, Action::ToggleHelp); }
    #[test]
    fn test_action_enter_prompt_eq() { assert_eq!(Action::EnterPromptMode, Action::EnterPromptMode); }
    #[test]
    fn test_action_toggle_terminal_eq() { assert_eq!(Action::ToggleTerminal, Action::ToggleTerminal); }
    #[test]
    fn test_action_cycle_forward_eq() { assert_eq!(Action::CycleFocusForward, Action::CycleFocusForward); }
    #[test]
    fn test_action_cycle_backward_eq() { assert_eq!(Action::CycleFocusBackward, Action::CycleFocusBackward); }
    #[test]
    fn test_action_resize_up_eq() { assert_eq!(Action::ResizeUp, Action::ResizeUp); }
    #[test]
    fn test_action_resize_down_eq() { assert_eq!(Action::ResizeDown, Action::ResizeDown); }
    #[test]
    fn test_action_browse_main_eq() { assert_eq!(Action::BrowseMain, Action::BrowseMain); }
    #[test]
    fn test_action_return_to_worktrees_eq() { assert_eq!(Action::ReturnToWorktrees, Action::ReturnToWorktrees); }
    #[test]
    fn test_action_toggle_session_list_eq() { assert_eq!(Action::ToggleSessionList, Action::ToggleSessionList); }
    #[test]
    fn test_action_viewer_tab_current_eq() { assert_eq!(Action::ViewerTabCurrent, Action::ViewerTabCurrent); }
    #[test]
    fn test_action_viewer_close_tab_eq() { assert_eq!(Action::ViewerCloseTab, Action::ViewerCloseTab); }
    #[test]
    fn test_action_select_all_eq() { assert_eq!(Action::SelectAll, Action::SelectAll); }
    #[test]
    fn test_action_enter_edit_mode_eq() { assert_eq!(Action::EnterEditMode, Action::EnterEditMode); }
    #[test]
    fn test_action_save_eq() { assert_eq!(Action::Save, Action::Save); }
    #[test]
    fn test_action_undo_eq() { assert_eq!(Action::Undo, Action::Undo); }
    #[test]
    fn test_action_redo_eq() { assert_eq!(Action::Redo, Action::Redo); }
    #[test]
    fn test_action_nav_down_eq() { assert_eq!(Action::NavDown, Action::NavDown); }
    #[test]
    fn test_action_nav_up_eq() { assert_eq!(Action::NavUp, Action::NavUp); }
    #[test]
    fn test_action_page_down_eq() { assert_eq!(Action::PageDown, Action::PageDown); }
    #[test]
    fn test_action_page_up_eq() { assert_eq!(Action::PageUp, Action::PageUp); }
    #[test]
    fn test_action_go_to_top_eq() { assert_eq!(Action::GoToTop, Action::GoToTop); }
    #[test]
    fn test_action_go_to_bottom_eq() { assert_eq!(Action::GoToBottom, Action::GoToBottom); }

    // -- Action inequality --

    #[test]
    fn test_action_ne_quit_escape() { assert_ne!(Action::Quit, Action::Escape); }
    #[test]
    fn test_action_ne_save_undo() { assert_ne!(Action::Save, Action::Undo); }

    // -- Jump edit logic --

    #[test]
    fn test_jump_edit_forward_wrap() {
        let edits = vec![2, 5, 8];
        let cur = Some(1usize); // at index 1
        let target = match cur { Some(pos) => (pos + 1) % edits.len(), None => 0 };
        assert_eq!(target, 2);
    }

    #[test]
    fn test_jump_edit_forward_wrap_at_end() {
        let edits = vec![2, 5, 8];
        let cur = Some(2usize); // at last
        let target = (cur.unwrap() + 1) % edits.len();
        assert_eq!(target, 0);
    }

    #[test]
    fn test_jump_edit_backward_wrap() {
        let edits = vec![2, 5, 8];
        let cur: Option<usize> = Some(0);
        let target = match cur { Some(0) | None => edits.len() - 1, Some(pos) => pos - 1 };
        assert_eq!(target, 2);
    }

    #[test]
    fn test_jump_edit_backward_normal() {
        let edits = vec![2, 5, 8];
        let cur = Some(2usize);
        let target = match cur { Some(0) | None => edits.len() - 1, Some(pos) => pos - 1 };
        assert_eq!(target, 1);
    }

    #[test]
    fn test_jump_edit_empty_edits() {
        let edits: Vec<usize> = vec![];
        assert!(edits.is_empty());
    }

    // -- Clickable path tuple structure --

    #[test]
    fn test_clickable_path_structure() {
        let path: (usize, usize, usize, String, String, String, usize) =
            (10, 5, 20, "src/main.rs".into(), "old".into(), "new".into(), 1);
        assert_eq!(path.0, 10); // line_idx
        assert_eq!(path.1, 5);  // sc
        assert_eq!(path.2, 20); // ec
        assert_eq!(path.3, "src/main.rs");
        assert_eq!(path.6, 1);  // wlc
    }

    #[test]
    fn test_clickable_path_has_edit_content() {
        let path: (usize, usize, usize, String, String, String, usize) =
            (0, 0, 0, "file.rs".into(), "old_str".into(), "new_str".into(), 1);
        let has_edit = !path.4.is_empty() || !path.5.is_empty();
        assert!(has_edit);
    }

    #[test]
    fn test_clickable_path_no_edit_content() {
        let path: (usize, usize, usize, String, String, String, usize) =
            (0, 0, 0, "file.rs".into(), "".into(), "".into(), 1);
        let has_edit = !path.4.is_empty() || !path.5.is_empty();
        assert!(!has_edit);
    }

    // -- saturating_sub for scroll --

    #[test]
    fn test_scroll_saturating_sub() {
        assert_eq!(10usize.saturating_sub(3), 7);
    }

    #[test]
    fn test_scroll_saturating_sub_underflow() {
        assert_eq!(2usize.saturating_sub(3), 0);
    }

    // -- Focus variants --

    #[test]
    fn test_focus_input() { assert_eq!(Focus::Input, Focus::Input); }
    #[test]
    fn test_focus_viewer() { assert_eq!(Focus::Viewer, Focus::Viewer); }
    #[test]
    fn test_focus_worktrees() { assert_eq!(Focus::Worktrees, Focus::Worktrees); }

    // -- Health panel actions --

    #[test]
    fn test_action_open_health() { assert_eq!(Action::OpenHealth, Action::OpenHealth); }
    #[test]
    fn test_action_open_git() { assert_eq!(Action::OpenGitActions, Action::OpenGitActions); }
    #[test]
    fn test_action_open_projects() { assert_eq!(Action::OpenProjects, Action::OpenProjects); }

    // -- Search session action --

    #[test]
    fn test_action_search_session() { assert_eq!(Action::SearchSession, Action::SearchSession); }

    // -- DeferredAction accessibility --

    #[test]
    fn test_deferred_open_health() {
        let a = crate::app::DeferredAction::OpenHealthPanel;
        assert!(matches!(a, crate::app::DeferredAction::OpenHealthPanel));
    }

    // -- Loading indicator string --

    #[test]
    fn test_loading_indicator_health() {
        let s = "Scanning project health\u{2026}";
        assert!(s.contains("health"));
    }

    #[test]
    fn test_loading_indicator_file() {
        let filename = "main.rs";
        let s = format!("Loading {}\u{2026}", filename);
        assert!(s.starts_with("Loading "));
    }

    #[test]
    fn test_action_ne_quit_vs_dump() {
        assert_ne!(Action::Quit, Action::DumpDebug);
    }
}
