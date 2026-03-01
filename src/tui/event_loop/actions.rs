//! Keyboard action dispatch
//!
//! Routes resolved keybinding actions to the correct pane handler.
//! Split into focused submodules:
//! - `execute`: Action execution dispatch (the main match on Action variants)
//! - `navigation`: Focus-aware navigation dispatch (up/down/left/right/page/top/bottom)
//! - `escape`: Context-dependent escape dispatch
//! - `session_list`: Session list overlay helpers and JSONL message counting
//! - `deferred`: Deferred action execution (post-loading-indicator dispatch)
//! - `rcr`: Rebase Conflict Resolution acceptance logic

mod deferred;
mod escape;
mod execute;
mod navigation;
mod rcr;
mod session_list;

// Re-export public API consumed by the event loop
pub use deferred::execute_deferred_action;
pub use session_list::finish_session_list_load;

use anyhow::Result;
use crossterm::event::{self, KeyCode};

use crate::app::{App, Focus};
use crate::claude::ClaudeProcess;

use super::super::keybindings::{Action, KeyContext, lookup_action};
use super::super::input_dialogs::{handle_branch_dialog_input, handle_run_command_picker_input, handle_run_command_dialog_input, handle_preset_prompt_picker_input, handle_preset_prompt_dialog_input};
use super::super::input_file_tree::handle_file_tree_input;
use super::super::input_git_actions::handle_git_actions_input;
use super::super::input_health::handle_health_input;
use super::super::input_output::handle_session_input;
use super::super::input_worktrees::handle_worktrees_input;
use super::super::input_terminal::handle_input_mode;
use super::super::input_viewer::handle_viewer_input;
use super::super::input_projects::handle_projects_input;

use execute::execute_action;
use rcr::accept_rcr;

/// Handle keyboard input events.
/// All key → action resolution goes through lookup_action() in keybindings.rs.
/// Modal overlays (help, wizard, dialogs) bypass this and consume
/// all input directly. Focus-specific handlers only see keys that lookup_action()
/// didn't resolve (text input, dialog nav, etc.).
pub fn handle_key_event(key: event::KeyEvent, app: &mut App, claude_process: &ClaudeProcess) -> Result<()> {
    // Bare modifier presses (Shift, Ctrl, Alt) arrive via Kitty protocol — ignore globally
    if matches!(key.code, KeyCode::Modifier(_)) { return Ok(()); }

    // ⌃q quits from ANYWHERE — blocked only when a git operation is in progress
    if key.modifiers.contains(event::KeyModifiers::CONTROL) && key.code == KeyCode::Char('q') {
        if app.git_action_in_progress() {
            app.set_status("Cannot quit while a git operation is in progress");
            return Ok(());
        }
        app.should_quit = true;
        return Ok(());
    }

    // --- Modal overlays consume ALL input (bypass keybinding system) ---

    // RCR approval dialog — highest priority modal (conflict resolution decision)
    if let Some(ref rcr) = app.rcr_session {
        if rcr.approval_pending {
            match key.code {
                KeyCode::Char('y') | KeyCode::Enter => { accept_rcr(app); }
                KeyCode::Char('n') => {
                    // Abort the rebase on the feature branch worktree,
                    // restoring it to its pre-rebase state
                    app.invalidate_sidebar();
                    let rcr = app.rcr_session.take().unwrap();
                    if let Some(ref sid) = rcr.session_id {
                        if let Some(path) = crate::config::claude_session_file(&rcr.worktree_path, sid) {
                            let _ = std::fs::remove_file(path);
                        }
                    }
                    let _ = std::process::Command::new("git")
                        .args(["rebase", "--abort"])
                        .current_dir(&rcr.worktree_path)
                        .output();
                    app.load_session_output();
                    app.update_title_session_name();
                    app.set_status(format!("RCR cancelled — rebase aborted for {}", rcr.display_name));
                }
                KeyCode::Esc => {
                    // Dismiss dialog — user wants to review the session output first.
                    // ⌃a re-shows the dialog when they're ready to accept.
                    if let Some(ref mut m) = app.rcr_session {
                        m.approval_pending = false;
                    }
                }
                _ => {}
            }
            return Ok(());
        }
    }

    // ⌃a re-shows the RCR approval dialog after dismissing with 'n'
    // Only active when RCR session exists, Claude isn't running, and dialog isn't shown
    if key.modifiers.contains(event::KeyModifiers::CONTROL) && key.code == KeyCode::Char('a') {
        if let Some(ref rcr) = app.rcr_session {
            if !rcr.approval_pending && !app.running_sessions.contains(&rcr.slot_id) {
                if let Some(ref mut m) = app.rcr_session {
                    m.approval_pending = true;
                }
                return Ok(());
            }
        }
    }

    // Worktree delete confirmation — y confirms, anything else cancels
    if app.worktree_delete_confirm {
        app.worktree_delete_confirm = false;
        if key.code == KeyCode::Char('y') || key.code == KeyCode::Char('Y') {
            if let Err(e) = app.delete_current_worktree() {
                app.set_status(format!("Delete failed: {}", e));
            }
        } else {
            app.set_status("Delete cancelled");
        }
        return Ok(());
    }

    // Sibling worktree delete: y = delete all siblings + current + branch, a = archive current only
    if let Some((ref _branch, ref _siblings)) = app.worktree_delete_siblings {
        let siblings_data = app.worktree_delete_siblings.take().unwrap();
        match key.code {
            KeyCode::Char('y') | KeyCode::Char('Y') => {
                let (branch, mut sibling_indices) = siblings_data;
                // Include current worktree index
                if let Some(current) = app.selected_worktree {
                    sibling_indices.push(current);
                }
                // Sort descending so we remove from the end first (indices stay valid)
                sibling_indices.sort_unstable();
                sibling_indices.dedup();
                sibling_indices.reverse();
                let project = app.project.clone();
                if let Some(project) = project {
                    for &idx in &sibling_indices {
                        if let Some(wt) = app.worktrees.get(idx) {
                            if let Some(ref wt_path) = wt.worktree_path {
                                let _ = crate::git::Git::remove_worktree(&project.path, wt_path);
                            }
                            app.auto_rebase_enabled.remove(&wt.branch_name);
                            crate::azufig::set_auto_rebase(&project.path, &wt.branch_name, false);
                        }
                    }
                    // All worktrees removed — now delete the branch
                    let _ = crate::git::Git::delete_branch(&project.path, &branch);
                }
                let _ = app.refresh_worktrees();
                // Clamp selection
                if app.worktrees.is_empty() {
                    app.selected_worktree = None;
                } else if let Some(sel) = app.selected_worktree {
                    if sel >= app.worktrees.len() {
                        app.selected_worktree = Some(app.worktrees.len() - 1);
                    }
                }
                app.set_status(format!("Deleted all worktrees on {} and branch", branch));
            }
            KeyCode::Char('a') | KeyCode::Char('A') => {
                // Archive current only — remove worktree dir, keep branch
                if let Some(project) = &app.project {
                    if let Some(wt) = app.current_worktree() {
                        if let Some(ref wt_path) = wt.worktree_path {
                            let _ = crate::git::Git::remove_worktree(&project.path, wt_path);
                        }
                        let name = wt.name().to_string();
                        let _ = app.refresh_worktrees();
                        app.set_status(format!("Archived '{}' — branch kept (other worktrees still active)", name));
                    }
                }
            }
            _ => {
                app.set_status("Delete cancelled");
            }
        }
        return Ok(());
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
                // Remember current selection so refresh doesn't reset to index 0
                let prev_branch = app.selected_worktree
                    .and_then(|i| app.worktrees.get(i))
                    .map(|w| w.branch_name.clone());
                let prev_idx = app.selected_worktree.unwrap_or(0);
                match d.selected {
                    0 => {
                        // Keep — worktree is already rebased (rebase happens before merge)
                        app.set_status(format!("{} — kept", d.display_name));
                    }
                    1 => {
                        // Archive — remove worktree, keep branch
                        if let Some(project) = &app.project {
                            let _ = crate::git::Git::remove_worktree(&project.path, &d.worktree_path);
                            // Clean up auto-rebase config for removed worktree
                            app.auto_rebase_enabled.remove(&d.branch);
                            crate::azufig::set_auto_rebase(&project.path, &d.branch, false);
                        }
                        app.set_status(format!("{} — archived", d.display_name));
                    }
                    2 => {
                        // Delete — remove worktree + delete branch
                        if let Some(project) = &app.project {
                            let _ = crate::git::Git::remove_worktree(&project.path, &d.worktree_path);
                            let _ = crate::git::Git::delete_branch(&project.path, &d.branch);
                            // Clean up auto-rebase config for deleted branch
                            app.auto_rebase_enabled.remove(&d.branch);
                            crate::azufig::set_auto_rebase(&project.path, &d.branch, false);
                        }
                        app.set_status(format!("{} — deleted", d.display_name));
                    }
                    _ => {}
                }
                let _ = app.refresh_worktrees();
                // Restore selection: find the same branch, or clamp to previous index
                app.selected_worktree = if app.worktrees.is_empty() {
                    None
                } else {
                    let by_branch = prev_branch.and_then(|b|
                        app.worktrees.iter().position(|w| w.branch_name == b));
                    Some(by_branch.unwrap_or_else(|| prev_idx.min(app.worktrees.len() - 1)))
                };
            }
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
    if app.health_panel.is_some() && !app.god_file_filter_mode { return handle_health_input(key, app, claude_process); }
    if app.git_actions_panel.is_some() { return handle_git_actions_input(key, app, claude_process); }

    // Debug dump naming dialog — text input for the dump file suffix
    if let Some(ref mut naming) = app.debug_dump_naming {
        match key.code {
            KeyCode::Enter => {
                let name = naming.clone();
                app.debug_dump_saving = Some(name);
                app.debug_dump_naming = None;
            }
            KeyCode::Esc => { app.debug_dump_naming = None; }
            KeyCode::Backspace => { naming.pop(); }
            KeyCode::Char(c) => { naming.push(c); }
            _ => {}
        }
        return Ok(());
    }

    if app.run_command_picker.is_some() { return handle_run_command_picker_input(key, app); }
    if app.run_command_dialog.is_some() { return handle_run_command_dialog_input(key, app, &claude_process); }
    // Dialog checked before picker — dialog is spawned on top of picker (e/a keys)
    if app.preset_prompt_dialog.is_some() { return handle_preset_prompt_dialog_input(key, app); }
    if app.preset_prompt_picker.is_some() { return handle_preset_prompt_picker_input(key, app); }

    // FileTree options overlay: intercept all keys before keybinding resolution
    if app.file_tree_options_mode { return handle_file_tree_input(key, app); }

    // Session find modal: typing search text bypasses keybinding system
    if app.session_find_active { return handle_session_input(key, app); }

    // Session list overlay: bypass keybinding system so Up/Down/j/k navigate the list
    // instead of being intercepted as JumpNextBubble/JumpPrevBubble
    if app.show_session_list { return handle_session_input(key, app); }

    // Text input modals bypass keybinding resolution entirely — they consume
    // all keypresses (including Shift+G, etc.) as literal text input.
    if app.focus == Focus::BranchDialog {
        return handle_branch_dialog_input(key, app);
    }

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
        Focus::Session => handle_session_input(key, app)?,
        Focus::Input => handle_input_mode(key, app, claude_process)?,
        Focus::BranchDialog => handle_branch_dialog_input(key, app)?,
    }

    Ok(())
}

