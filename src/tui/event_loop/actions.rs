//! Keyboard action dispatch
//!
//! Routes resolved keybinding actions to the correct pane handler.
//! Contains handle_key_event (the top-level key processor), execute_action
//! (the action dispatcher), navigation dispatch, and escape dispatch.

use anyhow::Result;
use crossterm::event::{self, KeyCode};

use crate::app::{App, Focus};
use crate::claude::ClaudeProcess;

use super::super::keybindings::{Action, KeyContext, lookup_action};
use super::super::input_dialogs::{handle_branch_dialog_input, handle_context_menu_input, handle_run_command_picker_input, handle_run_command_dialog_input};
use super::super::input_file_tree::handle_file_tree_input;
use super::super::input_god_files::handle_god_files_input;
use super::super::input_output::handle_output_input;
use super::super::input_worktrees::handle_worktrees_input;
use super::super::input_terminal::{handle_input_mode, handle_worktree_creation_input};
use super::super::input_viewer::handle_viewer_input;
use super::super::input_projects::handle_projects_input;
use super::super::input_wizard::handle_wizard_input;

use super::mouse::{copy_viewer_selection, copy_output_selection};

/// Handle keyboard input events.
/// All key → action resolution goes through lookup_action() in keybindings.rs.
/// Modal overlays (help, context menu, wizard, dialogs) bypass this and consume
/// all input directly. Focus-specific handlers only see keys that lookup_action()
/// didn't resolve (text input, dialog nav, etc.).
pub fn handle_key_event(key: event::KeyEvent, app: &mut App, claude_process: &ClaudeProcess) -> Result<()> {
    // Bare modifier presses (Shift, Ctrl, Alt) arrive via Kitty protocol — ignore globally
    if matches!(key.code, KeyCode::Modifier(_)) { return Ok(()); }

    // --- Modal overlays consume ALL input (bypass keybinding system) ---

    // Help overlay: only ? and Esc close it, everything else ignored
    if app.show_help {
        match key.code {
            KeyCode::Char('?') | KeyCode::Esc => app.toggle_help(),
            _ => {}
        }
        return Ok(());
    }

    // Full-screen modals that intercept everything
    if app.context_menu.is_some() { return handle_context_menu_input(key, app, claude_process); }
    if app.is_projects_panel_active() { return handle_projects_input(key, app); }
    if app.is_wizard_active() { handle_wizard_input(app, key, claude_process); return Ok(()); }
    if app.god_file_panel.is_some() { return handle_god_files_input(key, app, claude_process); }
    if app.run_command_picker.is_some() { return handle_run_command_picker_input(key, app); }
    if app.run_command_dialog.is_some() { return handle_run_command_dialog_input(key, app, &claude_process); }

    // Convo search modal: typing search text bypasses keybinding system
    if app.convo_search_active { return handle_output_input(key, app); }

    // Session list overlay: bypass keybinding system so Up/Down/j/k navigate the list
    // instead of being intercepted as JumpNextBubble/JumpPrevBubble
    if app.show_session_list { return handle_output_input(key, app); }

    // --- Centralized keybinding resolution ---
    // Build context from app state, resolve key once, dispatch action.
    // Input/terminal handlers and dialog handlers own their own key execution —
    // lookup_action() resolves their bindings for help/title display, but the
    // actual execution stays in the handlers (Submit needs claude_process, text
    // editing is tightly coupled, etc.). Only global + navigation + focus-specific
    // COMMAND bindings go through execute_action().
    let ctx = KeyContext::from_app(app);
    if let Some(action) = lookup_action(&ctx, key.modifiers, key.code) {
        // Input-specific actions: let the input handler execute them (it has
        // the full context: claude_process, plan approval state, etc.)
        let is_input_action = matches!(action,
            Action::Submit | Action::InsertNewline | Action::ExitPromptMode
            | Action::WordLeft | Action::WordRight | Action::DeleteWord
            | Action::HistoryPrev | Action::HistoryNext
            | Action::ToggleStt | Action::EnterTerminalType
        ) && matches!(app.focus, Focus::Input);
        if !is_input_action {
            return execute_action(action, app, claude_process);
        }
    }

    // --- Fallthrough: focus-specific handlers for text input / unresolved keys ---
    // Input handlers also process their own resolved bindings (Submit, word nav, etc.)
    match app.focus {
        Focus::Worktrees => handle_worktrees_input(key, app)?,
        Focus::FileTree => handle_file_tree_input(key, app)?,
        Focus::Viewer => handle_viewer_input(key, app)?,
        Focus::Output => handle_output_input(key, app)?,
        Focus::Input => handle_input_mode(key, app, claude_process)?,
        Focus::WorktreeCreation => handle_worktree_creation_input(key, app, claude_process)?,
        Focus::BranchDialog => handle_branch_dialog_input(key, app)?,
    }

    Ok(())
}

/// Execute a resolved keybinding action. Called by handle_key_event() after
/// lookup_action() identifies WHAT to do. This function handles the HOW.
fn execute_action(action: Action, app: &mut App, _claude_process: &ClaudeProcess) -> Result<()> {
    match action {
        // --- Global actions ---
        Action::Quit => { app.should_quit = true; }
        Action::Restart => { app.should_restart = true; app.should_quit = true; }
        Action::DumpDebug => { app.dump_debug_output(); }
        Action::CancelClaude => { app.cancel_current_claude(); }
        Action::CopySelection => {
            // Copy from whichever pane has an active selection
            if app.prompt_mode && app.has_input_selection() {
                app.input_copy();
            } else if app.viewer_selection.is_some() {
                copy_viewer_selection(app);
            } else if app.output_selection.is_some() {
                copy_output_selection(app);
            }
        }
        Action::ToggleHelp => { app.toggle_help(); }
        Action::EnterPromptMode => {
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
            // Clear sidebar filter on focus change
            if app.sidebar_filter_active || !app.sidebar_filter.is_empty() {
                app.sidebar_filter.clear();
                app.sidebar_filter_active = false;
                app.invalidate_sidebar();
            }
            app.prompt_mode = false;
            app.viewer_selection = None;
            app.output_selection = None;
            app.focus_next();
        }
        Action::CycleFocusBackward => {
            if app.sidebar_filter_active || !app.sidebar_filter.is_empty() {
                app.sidebar_filter.clear();
                app.sidebar_filter_active = false;
                app.invalidate_sidebar();
            }
            app.prompt_mode = false;
            app.viewer_selection = None;
            app.output_selection = None;
            app.focus_prev();
        }

        // --- Wizard actions ---
        Action::WizardNextTab => {
            if let Some(wizard) = app.creation_wizard.as_mut() { wizard.next_tab(); }
        }
        Action::WizardPrevTab => {
            if let Some(wizard) = app.creation_wizard.as_mut() { wizard.prev_tab(); }
        }
        // WizardNextField: wizard intercepts all input before lookup_action() runs,
        // so this arm never fires. Exists only for help text generation.
        Action::WizardNextField => {}

        // --- Terminal resize (global when terminal is open) ---
        Action::ResizeUp => { app.adjust_terminal_height(2); }
        Action::ResizeDown => { app.adjust_terminal_height(-2); }

        // --- All other actions are focus-specific; dispatch inline ---
        // Worktrees
        Action::ToggleFileTree => {
            if app.current_session().and_then(|s| s.worktree_path.as_ref()).is_some() {
                app.show_file_tree = true;
                app.focus = Focus::FileTree;
                app.load_file_tree();
                app.invalidate_file_tree();
            } else {
                app.set_status("No worktree path available");
            }
        }
        Action::EnterInputMode => {
            if app.is_current_session_running() {
                app.focus = Focus::Input;
                app.set_status("Enter input to send to Claude:");
            } else {
                app.set_status("No Claude running in this session");
            }
        }
        Action::ReturnToWorktrees => {
            app.show_file_tree = false;
            app.focus = Focus::Worktrees;
            app.invalidate_sidebar();
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
        Action::ViewerNextTab => { app.viewer_next_tab(); }
        Action::ViewerPrevTab => { app.viewer_prev_tab(); }
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
        Action::EnterEditMode => {
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
        Action::SearchFilter => {
            app.sidebar_filter_active = true;
            app.sidebar_filter.clear();
            app.invalidate_sidebar();
        }
        // Project jumping — currently single-project mode, no-op until multi-project navigation exists
        Action::SelectNextProject | Action::SelectPrevProject => {}
        Action::OpenContextMenu => {
            app.open_context_menu();
        }
        Action::NewWorktree => {
            app.start_wizard();
        }
        Action::BrowseBranches => {
            if let Some(project) = app.current_project() {
                match crate::git::Git::list_available_branches(&project.path) {
                    Ok(branches) => app.open_branch_dialog(branches),
                    Err(e) => app.set_status(format!("Failed to list branches: {}", e)),
                }
            }
        }
        Action::ViewDiff => {
            if let Err(e) = app.load_diff() {
                app.set_status(format!("Failed to get diff: {}", e));
            } else if app.focus == Focus::Output {
                app.diff_scroll = 0;
            }
        }
        Action::RunCommand => { app.open_run_command_picker(); }
        Action::AddRunCommand => { app.open_run_command_dialog(); }
        Action::RebaseOntoMain => {
            rebase_current(app);
        }
        Action::ArchiveWorktree => {
            if let Err(e) = app.archive_current_session() {
                app.set_status(format!("Failed to archive: {}", e));
            }
        }
        Action::StartResume => {
            start_or_resume(app);
        }
        Action::OpenProjects => {
            app.open_projects_panel();
        }
        Action::OpenGodFiles => {
            app.open_god_file_panel();
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
                if let Some(entry) = app.file_tree_entries.get(idx) {
                    if entry.is_dir {
                        app.toggle_file_tree_dir();
                    } else {
                        app.load_file_into_viewer();
                        app.focus = Focus::Viewer;
                    }
                }
            }
        }
        Action::AddFile => {
            app.file_tree_action = Some(crate::app::types::FileTreeAction::Add(String::new()));
        }
        Action::DeleteFile => {
            if app.file_tree_selected.is_some() {
                app.file_tree_action = Some(crate::app::types::FileTreeAction::Delete);
            }
        }
        Action::RenameFile => {
            if let Some(idx) = app.file_tree_selected {
                if let Some(entry) = app.file_tree_entries.get(idx) {
                    app.file_tree_action = Some(crate::app::types::FileTreeAction::Rename(entry.name.clone()));
                }
            }
        }
        Action::CopyFile => {
            if let Some(idx) = app.file_tree_selected {
                if let Some(entry) = app.file_tree_entries.get(idx) {
                    app.file_tree_action = Some(crate::app::types::FileTreeAction::Copy(entry.path.clone()));
                    app.set_status("Copy: select target dir, Enter to paste");
                    app.invalidate_file_tree();
                }
            }
        }
        Action::MoveFile => {
            if let Some(idx) = app.file_tree_selected {
                if let Some(entry) = app.file_tree_entries.get(idx) {
                    app.file_tree_action = Some(crate::app::types::FileTreeAction::Move(entry.path.clone()));
                    app.set_status("Move: select target dir, Enter to paste");
                    app.invalidate_file_tree();
                }
            }
        }

        // --- Output/Convo ---
        Action::JumpNextBubble => {
            if app.view_mode == crate::app::ViewMode::Output { app.jump_to_next_bubble(false); }
        }
        Action::JumpPrevBubble => {
            if app.view_mode == crate::app::ViewMode::Output { app.jump_to_prev_bubble(false); }
        }
        Action::JumpNextMessage => {
            if app.view_mode == crate::app::ViewMode::Output { app.jump_to_next_bubble(true); }
        }
        Action::JumpPrevMessage => {
            if app.view_mode == crate::app::ViewMode::Output { app.jump_to_prev_bubble(true); }
        }
        Action::SearchConvo => {
            // Activate the convo search bar — clears previous query and matches
            app.convo_search_active = true;
            app.convo_search.clear();
            app.convo_search_matches.clear();
            app.convo_search_current = 0;
        }
        Action::SwitchToOutput => {
            app.view_mode = crate::app::ViewMode::Output;
            app.output_scroll = usize::MAX;
        }
        Action::RebaseStatus => {
            if let Some(session) = app.current_session() {
                if let Some(ref wt_path) = session.worktree_path {
                    if crate::git::Git::is_rebase_in_progress(wt_path) {
                        if let Ok(status) = crate::git::Git::get_rebase_status(wt_path) {
                            app.set_rebase_status(status);
                        }
                    }
                }
            }
        }

        // --- Input/Terminal actions: handled by their own handlers (skip here) ---
        // These are filtered out in handle_key_event() and fall through to
        // handle_input_mode(). Listed here for exhaustive match.
        Action::Submit | Action::InsertNewline | Action::ExitPromptMode
        | Action::WordLeft | Action::WordRight | Action::DeleteWord
        | Action::HistoryPrev | Action::HistoryNext
        | Action::ToggleStt | Action::EnterTerminalType => {}

        // --- Generic escape: context-dependent close/back ---
        Action::Escape => {
            dispatch_escape(app);
        }

        // --- Dialog actions (not reached here — modals intercept above) ---
        Action::Confirm | Action::Cancel | Action::DeleteSelected | Action::EditSelected => {}
    }

    Ok(())
}

/// Navigation dispatch — routes NavDown to the correct pane handler
fn dispatch_nav_down(app: &mut App) {
    match app.focus {
        Focus::Viewer => { app.scroll_viewer_down(1); }
        Focus::Output => {
            match app.view_mode {
                crate::app::ViewMode::Output => { app.scroll_output_down(1); }
                crate::app::ViewMode::Diff => { app.scroll_diff_down(1); }
                _ => {}
            }
        }
        Focus::Worktrees => {
            if app.is_current_worktree_expanded() { app.session_file_next(); }
            else { app.select_next_session(); }
        }
        Focus::FileTree => app.file_tree_next(),
        Focus::Input if app.terminal_mode && !app.prompt_mode => {
            app.scroll_terminal_down(1);
        }
        _ => {}
    }
}

fn dispatch_nav_up(app: &mut App) {
    match app.focus {
        Focus::Viewer => { app.scroll_viewer_up(1); }
        Focus::Output => {
            match app.view_mode {
                crate::app::ViewMode::Output => { app.scroll_output_up(1); }
                crate::app::ViewMode::Diff => { app.scroll_diff_up(1); }
                _ => {}
            }
        }
        Focus::Worktrees => {
            if app.is_current_worktree_expanded() { app.session_file_prev(); }
            else { app.select_prev_session(); }
        }
        Focus::FileTree => app.file_tree_prev(),
        Focus::Input if app.terminal_mode && !app.prompt_mode => {
            app.scroll_terminal_up(1);
        }
        _ => {}
    }
}

fn dispatch_nav_left(app: &mut App) {
    match app.focus {
        Focus::Worktrees => {
            if app.is_current_worktree_expanded() {
                if let Some(session) = app.current_session() {
                    let branch = session.branch_name.clone();
                    app.collapse_worktree(&branch);
                }
            }
        }
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

fn dispatch_nav_right(app: &mut App) {
    match app.focus {
        Focus::Worktrees => {
            if !app.is_current_worktree_expanded() {
                if let Some(session) = app.current_session() {
                    let branch = session.branch_name.clone();
                    app.expand_worktree(&branch);
                }
            }
        }
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

fn dispatch_page_down(app: &mut App) {
    match app.focus {
        Focus::Viewer => { app.scroll_viewer_down(app.viewer_viewport_height.saturating_sub(2)); }
        Focus::Output => {
            let page = app.output_viewport_height.saturating_sub(2);
            match app.view_mode {
                crate::app::ViewMode::Output => { app.scroll_output_down(page); }
                crate::app::ViewMode::Diff => { app.scroll_diff_down(page); }
                _ => {}
            }
        }
        Focus::Input if app.terminal_mode && !app.prompt_mode => {
            app.scroll_terminal_down((app.terminal_height as usize).saturating_sub(2));
        }
        _ => {}
    }
}

fn dispatch_page_up(app: &mut App) {
    match app.focus {
        Focus::Viewer => { app.scroll_viewer_up(app.viewer_viewport_height.saturating_sub(2)); }
        Focus::Output => {
            let page = app.output_viewport_height.saturating_sub(2);
            match app.view_mode {
                crate::app::ViewMode::Output => { app.scroll_output_up(page); }
                crate::app::ViewMode::Diff => { app.scroll_diff_up(page); }
                _ => {}
            }
        }
        Focus::Input if app.terminal_mode && !app.prompt_mode => {
            app.scroll_terminal_up((app.terminal_height as usize).saturating_sub(2));
        }
        _ => {}
    }
}

fn dispatch_go_to_top(app: &mut App) {
    match app.focus {
        Focus::Viewer => app.viewer_scroll = 0,
        Focus::Output => {
            match app.view_mode {
                crate::app::ViewMode::Output => app.output_scroll = 0,
                crate::app::ViewMode::Diff => app.diff_scroll = 0,
                _ => {}
            }
        }
        Focus::Worktrees => {
            if app.is_current_worktree_expanded() { app.session_file_first(); }
            else { app.select_first_session(); }
        }
        Focus::FileTree => app.file_tree_first_sibling(),
        Focus::Input if app.terminal_mode && !app.prompt_mode => {
            app.terminal_scroll = 0;
        }
        _ => {}
    }
}

fn dispatch_go_to_bottom(app: &mut App) {
    match app.focus {
        Focus::Viewer => app.scroll_viewer_to_bottom(),
        Focus::Output => {
            match app.view_mode {
                crate::app::ViewMode::Output => app.scroll_output_to_bottom(),
                crate::app::ViewMode::Diff => app.scroll_diff_to_bottom(),
                _ => {}
            }
        }
        Focus::Worktrees => {
            if app.is_current_worktree_expanded() { app.session_file_last(); }
            else { app.select_last_session(); }
        }
        Focus::FileTree => app.file_tree_last_sibling(),
        Focus::Input if app.terminal_mode && !app.prompt_mode => {
            app.scroll_terminal_to_bottom();
        }
        _ => {}
    }
}

/// Escape dispatch — context-dependent close/back
fn dispatch_escape(app: &mut App) {
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
        Focus::FileTree => {
            app.show_file_tree = false;
            app.focus = Focus::Worktrees;
            app.invalidate_sidebar();
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
        app.output_viewport_scroll = usize::MAX;
        app.load_file_with_edit_diff(&file_path, &old_str, &new_str);
        app.output_scroll = line_idx.saturating_sub(3);
    }
}

/// Open session list overlay
fn open_session_list(app: &mut App) {
    app.show_session_list = true;
    app.session_list_selected = 0;
    app.session_list_scroll = 0;
    for session in &app.sessions {
        if !app.session_files.contains_key(&session.branch_name) {
            if let Some(ref wt_path) = session.worktree_path {
                let files = crate::config::list_claude_sessions(wt_path);
                app.session_files.insert(session.branch_name.clone(), files);
            }
        }
    }
    for files in app.session_files.values() {
        for (session_id, path, _) in files.iter() {
            if !app.session_msg_counts.contains_key(session_id) {
                let count = count_messages_in_jsonl(path);
                app.session_msg_counts.insert(session_id.clone(), count);
            }
        }
    }
}

/// Count human+assistant messages in a JSONL session file
fn count_messages_in_jsonl(path: &std::path::Path) -> usize {
    let Ok(content) = std::fs::read_to_string(path) else { return 0; };
    content.lines().filter(|line| {
        line.contains("\"type\":\"human\"") || line.contains("\"type\":\"assistant\"")
            || line.contains("\"type\": \"human\"") || line.contains("\"type\": \"assistant\"")
    }).count()
}

/// Rebase current worktree onto main
fn rebase_current(app: &mut App) {
    use crate::models::RebaseResult;
    if let Some(session) = app.current_session() {
        if let (Some(ref wt_path), Some(project)) = (&session.worktree_path, app.current_project()) {
            let wt = wt_path.clone();
            let main_branch = project.main_branch.clone();
            match crate::git::Git::rebase_onto_main(&wt, &main_branch) {
                Ok(RebaseResult::Success) => {
                    app.set_status("Rebase completed successfully");
                    app.clear_rebase_status();
                }
                Ok(RebaseResult::UpToDate) => app.set_status("Already up to date"),
                Ok(RebaseResult::Conflicts(status)) => {
                    let n = status.conflicted_files.len();
                    app.set_rebase_status(status);
                    app.set_status(format!("Rebase conflicts: {} file(s)", n));
                }
                Ok(RebaseResult::Aborted) => {
                    app.set_status("Rebase was aborted");
                    app.clear_rebase_status();
                }
                Ok(RebaseResult::Failed(e)) => app.set_status(format!("Rebase failed: {}", e)),
                Err(e) => app.set_status(format!("Rebase error: {}", e)),
            }
        } else {
            app.set_status("No worktree path available");
        }
    }
}

/// Start or resume a Claude session from worktrees Enter key
fn start_or_resume(app: &mut App) {
    use crate::models::SessionStatus;
    let is_expanded = app.is_current_worktree_expanded();
    if is_expanded {
        if let Some(session) = app.current_session() {
            let branch = session.branch_name.clone();
            let idx = *app.session_selected_file_idx.get(&branch).unwrap_or(&0);
            app.select_session_file(&branch, idx);
            app.collapse_worktree(&branch);
            app.set_status("Loaded selected session file");
        }
    } else if let Some(session) = app.current_session() {
        let status = session.status(&app.running_sessions);
        if matches!(status, SessionStatus::Pending | SessionStatus::Stopped
            | SessionStatus::Completed | SessionStatus::Failed | SessionStatus::Waiting)
        {
            app.focus = Focus::Input;
            app.prompt_mode = true;
            app.set_status("Type your prompt and press Enter to send");
        }
    }
}
