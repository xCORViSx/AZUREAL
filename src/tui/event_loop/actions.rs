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
use super::super::input_terminal::{handle_input_mode, handle_worktree_creation_input};
use super::super::input_viewer::handle_viewer_input;
use super::super::input_azureal::handle_azureal_input;
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

    // ⌃q quits from ANYWHERE — no modal should block this
    if key.modifiers.contains(event::KeyModifiers::CONTROL) && key.code == KeyCode::Char('q') {
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
                    app.sidebar_dirty = true; // Update R indicator color
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
    if app.azureal_panel.is_some() { return handle_azureal_input(key, app); }
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
    if matches!(app.focus, Focus::WorktreeCreation | Focus::BranchDialog) {
        match app.focus {
            Focus::WorktreeCreation => return handle_worktree_creation_input(key, app, claude_process),
            Focus::BranchDialog => return handle_branch_dialog_input(key, app),
            _ => unreachable!(),
        }
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
        Focus::WorktreeCreation => handle_worktree_creation_input(key, app, claude_process)?,
        Focus::BranchDialog => handle_branch_dialog_input(key, app)?,
    }

    Ok(())
}

