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
use super::super::input_dialogs::{handle_branch_dialog_input, handle_run_command_picker_input, handle_run_command_dialog_input, handle_preset_prompt_picker_input, handle_preset_prompt_dialog_input};
use super::super::input_file_tree::handle_file_tree_input;
use super::super::input_git_actions::handle_git_actions_input;
use super::super::input_health::handle_health_input;
use super::super::input_output::handle_output_input;
use super::super::input_worktrees::handle_worktrees_input;
use super::super::input_terminal::{handle_input_mode, handle_worktree_creation_input};
use super::super::input_viewer::handle_viewer_input;
use super::super::input_projects::handle_projects_input;
use super::super::input_wizard::handle_wizard_input;

use super::mouse::{copy_viewer_selection, copy_output_selection};

/// Handle keyboard input events.
/// All key → action resolution goes through lookup_action() in keybindings.rs.
/// Modal overlays (help, wizard, dialogs) bypass this and consume
/// all input directly. Focus-specific handlers only see keys that lookup_action()
/// didn't resolve (text input, dialog nav, etc.).
pub fn handle_key_event(key: event::KeyEvent, app: &mut App, claude_process: &ClaudeProcess) -> Result<()> {
    // Bare modifier presses (Shift, Ctrl, Alt) arrive via Kitty protocol — ignore globally
    if matches!(key.code, KeyCode::Modifier(_)) { return Ok(()); }

    // ⌃q quits from ANYWHERE — no modal should block this
    if key.modifiers.contains(event::KeyModifiers::CONTROL) && key.code == KeyCode::Char('q') {
        app.should_quit = true;
        return Ok(());
    }

    // --- Modal overlays consume ALL input (bypass keybinding system) ---

    // MCR approval dialog — highest priority modal (conflict resolution decision)
    if let Some(ref mcr) = app.mcr_session {
        if mcr.approval_pending {
            match key.code {
                KeyCode::Char('y') | KeyCode::Enter => { accept_mcr(app); }
                KeyCode::Char('n') => {
                    // Abort the merge entirely — reset main to pre-merge HEAD,
                    // undoing any commits Claude made during resolution
                    let mcr = app.mcr_session.take().unwrap();
                    if let Some(ref sid) = mcr.session_id {
                        if let Some(path) = crate::config::claude_session_file(&mcr.repo_root, sid) {
                            let _ = std::fs::remove_file(path);
                        }
                    }
                    if !mcr.pre_merge_head.is_empty() {
                        let _ = std::process::Command::new("git")
                            .args(["reset", "--hard", &mcr.pre_merge_head])
                            .current_dir(&mcr.repo_root)
                            .output();
                    }
                    // Pop any stash that squash_merge_into_main() pushed
                    let _ = std::process::Command::new("git")
                        .args(["stash", "pop"])
                        .current_dir(&mcr.repo_root)
                        .output();
                    app.load_session_output();
                    app.update_title_session_name();
                    app.set_status(format!("MCR cancelled — merge aborted for {}", mcr.display_name));
                }
                KeyCode::Esc => {
                    // Dismiss dialog — user wants to review the convo first.
                    // ⌃a re-shows the dialog when they're ready to accept.
                    if let Some(ref mut m) = app.mcr_session {
                        m.approval_pending = false;
                    }
                }
                _ => {}
            }
            return Ok(());
        }
    }

    // ⌃a re-shows the MCR approval dialog after dismissing with 'n'
    // Only active when MCR session exists, Claude isn't running, and dialog isn't shown
    if key.modifiers.contains(event::KeyModifiers::CONTROL) && key.code == KeyCode::Char('a') {
        if let Some(ref mcr) = app.mcr_session {
            if !mcr.approval_pending && !app.running_sessions.contains(&mcr.slot_id) {
                if let Some(ref mut m) = app.mcr_session {
                    m.approval_pending = true;
                }
                return Ok(());
            }
        }
    }

    // Post-merge dialog — keep/archive/delete worktree after squash merge
    if app.post_merge_dialog.is_some() {
        match key.code {
            KeyCode::Char('j') | KeyCode::Down => {
                if let Some(ref mut d) = app.post_merge_dialog { if d.selected < 2 { d.selected += 1; } }
            }
            KeyCode::Char('k') | KeyCode::Up => {
                if let Some(ref mut d) = app.post_merge_dialog { if d.selected > 0 { d.selected -= 1; } }
            }
            KeyCode::Enter => {
                let d = app.post_merge_dialog.take().unwrap();
                match d.selected {
                    0 => {
                        // Keep — rebase worktree onto main
                        let _ = std::process::Command::new("git")
                            .args(["rebase", "main"])
                            .current_dir(&d.worktree_path)
                            .output();
                        app.set_status(format!("{} — rebased onto main", d.display_name));
                    }
                    1 => {
                        // Archive — remove worktree, keep branch
                        if let Some(project) = &app.project {
                            let _ = crate::git::Git::remove_worktree(&project.path, &d.worktree_path);
                        }
                        app.set_status(format!("{} — archived", d.display_name));
                        let _ = app.refresh_worktrees();
                    }
                    2 => {
                        // Delete — remove worktree + delete branch
                        if let Some(project) = &app.project {
                            let _ = crate::git::Git::remove_worktree(&project.path, &d.worktree_path);
                            let _ = crate::git::Git::delete_branch(&project.path, &d.branch);
                        }
                        app.set_status(format!("{} — deleted", d.display_name));
                        let _ = app.refresh_worktrees();
                    }
                    _ => {}
                }
            }
            KeyCode::Esc => { app.post_merge_dialog = None; }
            _ => {}
        }
        return Ok(());
    }

    // Help overlay: only ? and Esc close it, everything else ignored
    if app.show_help {
        match key.code {
            KeyCode::Char('?') | KeyCode::Esc => app.toggle_help(),
            _ => {}
        }
        return Ok(());
    }

    // Full-screen modals that intercept everything
    if app.is_projects_panel_active() { return handle_projects_input(key, app); }
    if app.is_wizard_active() { handle_wizard_input(app, key, claude_process); return Ok(()); }
    if app.health_panel.is_some() && !app.god_file_filter_mode { return handle_health_input(key, app, claude_process); }
    if app.git_actions_panel.is_some() { return handle_git_actions_input(key, app, claude_process); }
    if app.run_command_picker.is_some() { return handle_run_command_picker_input(key, app); }
    if app.run_command_dialog.is_some() { return handle_run_command_dialog_input(key, app, &claude_process); }
    // Dialog checked before picker — dialog is spawned on top of picker (e/a keys)
    if app.preset_prompt_dialog.is_some() { return handle_preset_prompt_dialog_input(key, app); }
    if app.preset_prompt_picker.is_some() { return handle_preset_prompt_picker_input(key, app); }

    // FileTree options overlay: intercept all keys before keybinding resolution
    if app.file_tree_options_mode { return handle_file_tree_input(key, app); }

    // Debug dump naming dialog: text input for optional dump file suffix
    if app.debug_dump_naming.is_some() {
        match key.code {
            KeyCode::Enter => {
                // Transition to "saving" state — draw shows the dialog, dump runs next frame
                let name = app.debug_dump_naming.take().unwrap_or_default();
                app.debug_dump_saving = Some(name);
            }
            KeyCode::Esc => { app.debug_dump_naming = None; }
            KeyCode::Backspace => { if let Some(ref mut s) = app.debug_dump_naming { s.pop(); } }
            KeyCode::Char(c) if !key.modifiers.contains(event::KeyModifiers::CONTROL) => {
                if let Some(ref mut s) = app.debug_dump_naming { s.push(c); }
            }
            _ => {}
        }
        return Ok(());
    }

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
        Action::DumpDebug => { app.debug_dump_naming = Some(String::new()); }
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
        Action::ToggleFileTree if !app.god_file_filter_mode => {
            if app.current_worktree().and_then(|s| s.worktree_path.as_ref()).is_some() {
                app.show_file_tree = true;
                app.focus = Focus::FileTree;
                app.load_file_tree();
                app.invalidate_file_tree();
            } else {
                app.set_status("No worktree path available");
            }
        }
        Action::BrowseMain => {
            if app.browsing_main { app.exit_main_browse(); }
            else { app.enter_main_browse(); }
        }
        Action::ReturnToWorktrees if !app.god_file_filter_mode => {
            if app.browsing_main { app.exit_main_browse(); }
            else {
                app.show_file_tree = false;
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
        Action::SearchFilter => {
            app.sidebar_filter_active = true;
            app.sidebar_filter.clear();
            app.invalidate_sidebar();
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
        Action::RunCommand => { app.open_run_command_picker(); }
        Action::AddRunCommand => { app.open_run_command_dialog(); }
        Action::ArchiveWorktree => {
            if let Err(e) = app.archive_current_worktree() {
                app.set_status(format!("Failed to archive: {}", e));
            }
        }
        Action::UnarchiveWorktree => {
            if let Err(e) = app.unarchive_current_worktree() {
                app.set_status(format!("Failed to unarchive: {}", e));
            }
        }
        Action::StartResume => {
            start_or_resume(app);
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
            if app.view_mode == crate::app::ViewMode::Output { app.jump_to_next_bubble(true); }
        }
        Action::JumpPrevBubble => {
            if app.view_mode == crate::app::ViewMode::Output { app.jump_to_prev_bubble(true); }
        }
        // Shift+Up/Down: jump to user prompts only (skip assistant responses)
        Action::JumpNextMessage => {
            if app.view_mode == crate::app::ViewMode::Output { app.jump_to_next_bubble(false); }
        }
        Action::JumpPrevMessage => {
            if app.view_mode == crate::app::ViewMode::Output { app.jump_to_prev_bubble(false); }
        }
        Action::SearchConvo => {
            // Activate the convo search bar — clears previous query and matches
            app.convo_search_active = true;
            app.convo_search.clear();
            app.convo_search_matches.clear();
            app.convo_search_current = 0;
        }

        // --- Input/Terminal actions: handled by their own handlers (skip here) ---
        // These are filtered out in handle_key_event() and fall through to
        // handle_input_mode(). Listed here for exhaustive match.
        Action::Submit | Action::InsertNewline | Action::ExitPromptMode
        | Action::WordLeft | Action::WordRight | Action::DeleteWord
        | Action::HistoryPrev | Action::HistoryNext
        | Action::EnterTerminalType => {}

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
        Focus::Worktrees => app.select_next_session(),
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
        Focus::Worktrees => app.select_prev_session(),
        Focus::FileTree => app.file_tree_prev(),
        Focus::Input if app.terminal_mode && !app.prompt_mode => {
            app.scroll_terminal_up(1);
        }
        _ => {}
    }
}

fn dispatch_nav_left(app: &mut App) {
    match app.focus {
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
        Focus::Worktrees => app.select_first_session(),
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
        Focus::Worktrees => app.select_last_session(),
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

/// Open session list overlay — scoped to the currently selected worktree only.
/// Phase 1: show the overlay + loading indicator, refresh file list (fast).
/// Phase 2 (finish_session_list_load) runs on the next event loop iteration
/// so the loading dialog renders before the expensive message count I/O starts.
fn open_session_list(app: &mut App) {
    app.show_session_list = true;
    app.session_list_loading = true;
    app.session_list_selected = 0;
    app.session_list_scroll = 0;
    // Refresh file list immediately (cheap directory listing)
    if let Some(session) = app.current_worktree() {
        let branch = session.branch_name.clone();
        if let Some(ref wt_path) = app.worktrees[app.selected_worktree.unwrap()].worktree_path {
            let files = crate::config::list_claude_sessions(wt_path);
            app.session_files.insert(branch, files);
        }
    }
}

/// Phase 2 of session list loading — compute message counts (expensive I/O).
/// Called from event loop after the loading dialog has had a chance to render.
pub fn finish_session_list_load(app: &mut App) {
    if let Some(session) = app.current_worktree() {
        let branch = session.branch_name.clone();
        if let Some(files) = app.session_files.get(&branch) {
            for (session_id, path, _) in files.iter() {
                let file_size = path.metadata().map(|m| m.len()).unwrap_or(0);
                if let Some(&(_, cached_size)) = app.session_msg_counts.get(session_id.as_str()) {
                    if cached_size == file_size { continue; }
                }
                let count = count_messages_in_jsonl(path);
                app.session_msg_counts.insert(session_id.clone(), (count, file_size));
            }
        }
    }
    app.session_list_loading = false;
}

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
                        super::super::input_git_actions::refresh_changed_files(p);
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
                        super::super::input_git_actions::refresh_changed_files(p);
                    }
                    Err(e) => { p.result_message = Some((format!("{}", e), true)); }
                }
            }
        }
    }
}

/// Count message bubbles in a JSONL session file for the session list [N msgs] badge.
/// Uses fast string scanning (no JSON parsing) — "type":"user" and "type":"assistant"
/// have zero false positives in Claude Code's compact JSON output.
/// Skips isMeta, tool_result arrays, command hooks, and compaction summaries.
/// ParentUuid dedup skipped for speed (rare rewind case, off by ≤2).
fn count_messages_in_jsonl(path: &std::path::Path) -> usize {
    let Ok(content) = std::fs::read_to_string(path) else { return 0; };
    let mut count = 0usize;
    for line in content.lines() {
        if line.contains("\"type\":\"user\"") {
            // Skip system-generated meta messages
            if line.contains("\"isMeta\":true") { continue; }
            // Skip tool_result lines — only string content creates bubbles
            // Tool result user lines contain {"type":"tool_result",...} blocks
            if line.contains("\"type\":\"tool_result\"") { continue; }
            // Skip non-bubble user events the parser also skips
            if line.contains("<local-command-caveat>") { continue; }
            if line.contains("<local-command-stdout>") { continue; }
            if line.contains("<command-name>") { continue; }
            if line.contains("This session is being continued from a previous conversation") { continue; }
            count += 1;
        } else if line.contains("\"type\":\"assistant\"") {
            // Only count lines with a text content block (those become AssistantText bubbles)
            if line.contains("\"type\":\"text\"") { count += 1; }
        }
    }
    count
}

/// Start or resume a Claude session from worktrees Enter key.
/// Archived sessions can't be started — user must press `u` to unarchive first.
fn start_or_resume(app: &mut App) {
    if app.browsing_main { app.set_status("Read-only: main branch"); return; }
    use crate::models::WorktreeStatus;
    let Some(session) = app.current_worktree() else { return };
    if session.archived {
        app.set_status("Session is archived — press u to unarchive first");
        return;
    }
    let status = session.status(app.is_session_running(&session.branch_name));
    if matches!(status, WorktreeStatus::Pending | WorktreeStatus::Stopped
        | WorktreeStatus::Completed | WorktreeStatus::Failed | WorktreeStatus::Waiting)
    {
        app.focus = Focus::Input;
        app.prompt_mode = true;
        app.set_status("Type your prompt and press Enter to send");
    }
}

/// Accept the MCR resolution — delete the temporary session file from
/// `~/.claude/projects/<main-encoded>/<session-id>.jsonl`, clear MCR state,
/// and restore normal convo pane borders + title.
fn accept_mcr(app: &mut App) {
    if let Some(mcr) = app.mcr_session.take() {
        // Delete the MCR session file so it doesn't pollute main's session list
        if let Some(ref sid) = mcr.session_id {
            if let Some(path) = crate::config::claude_session_file(&mcr.repo_root, sid) {
                let _ = std::fs::remove_file(path);
            }
        }
        // Restore convo pane, then show post-merge dialog
        app.load_session_output();
        app.update_title_session_name();
        let wt_path = app.current_worktree()
            .and_then(|w| w.worktree_path.clone())
            .unwrap_or_else(|| mcr.repo_root.clone());
        app.post_merge_dialog = Some(crate::app::types::PostMergeDialog {
            branch: mcr.branch.clone(),
            display_name: mcr.display_name.clone(),
            worktree_path: wt_path,
            repo_root: mcr.repo_root,
            selected: 0,
        });
        app.set_status(format!("MCR complete — {} merged", mcr.display_name));
    }
}
