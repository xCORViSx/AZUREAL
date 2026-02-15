//! Input handler for the Git Actions panel (Shift+G).
//!
//! Full-screen modal overlay — consumes ALL input when active, dispatched via
//! the centralized keybinding system (lookup_git_actions_action in keybindings.rs).
//! Actions section (Tab to switch): r=rebase from main, m=merge to main, f=fetch, l=pull, P=push.
//! File list section: j/k navigate, Enter/d opens file diff in viewer.

use anyhow::Result;
use crossterm::event;

use crate::app::App;
use crate::app::types::{GitActionsPanel, GitChangedFile};
use crate::git::Git;
use crate::models::RebaseResult;
use super::keybindings::{lookup_git_actions_action, Action};

/// Total number of action items displayed in the actions section
const ACTION_COUNT: usize = 5;

/// Handle all keyboard input while the Git Actions panel is open.
/// Returns Ok(()) — the panel intercepts everything (no fallthrough).
pub fn handle_git_actions_input(key: event::KeyEvent, app: &mut App) -> Result<()> {
    let panel = match app.git_actions_panel.as_mut() {
        Some(p) => p,
        None => return Ok(()),
    };

    // Clear stale result message on any non-nav key
    let is_nav = matches!(action_is_nav(key.modifiers, key.code, panel.actions_focused),
        Some(true));
    if !is_nav { panel.result_message = None; }

    let actions_focused = panel.actions_focused;

    // Resolve key → action via centralized binding arrays
    let Some(action) = lookup_git_actions_action(actions_focused, key.modifiers, key.code) else {
        return Ok(());
    };

    match action {
        Action::Escape => { app.close_git_actions_panel(); }
        Action::GitToggleFocus => {
            if let Some(ref mut p) = app.git_actions_panel { p.actions_focused = !p.actions_focused; }
        }
        Action::NavDown => {
            if let Some(ref mut p) = app.git_actions_panel {
                if p.actions_focused {
                    if p.selected_action + 1 < ACTION_COUNT { p.selected_action += 1; }
                } else if !p.changed_files.is_empty() && p.selected_file + 1 < p.changed_files.len() {
                    p.selected_file += 1;
                }
            }
        }
        Action::NavUp => {
            if let Some(ref mut p) = app.git_actions_panel {
                if p.actions_focused {
                    if p.selected_action > 0 { p.selected_action -= 1; }
                } else if p.selected_file > 0 {
                    p.selected_file -= 1;
                }
            }
        }
        Action::GoToTop => {
            if let Some(ref mut p) = app.git_actions_panel {
                if p.actions_focused { p.selected_action = 0; }
                else { p.selected_file = 0; p.file_scroll = 0; }
            }
        }
        Action::GoToBottom => {
            if let Some(ref mut p) = app.git_actions_panel {
                if p.actions_focused { p.selected_action = ACTION_COUNT.saturating_sub(1); }
                else if !p.changed_files.is_empty() { p.selected_file = p.changed_files.len() - 1; }
            }
        }

        // ── Git operations (only fire when actions_focused, enforced by lookup guard) ──
        Action::GitRebase => { exec_rebase(app); }
        Action::GitMerge => { exec_merge(app); }
        Action::GitFetch => { exec_fetch(app); }
        Action::GitPull => { exec_pull(app); }
        Action::GitPush => { exec_push(app); }

        // ── Enter/d: execute action by index (when focused) or open diff (file list) ──
        Action::Confirm => {
            let (focused, idx) = match app.git_actions_panel.as_ref() {
                Some(p) => (p.actions_focused, p.selected_action),
                None => return Ok(()),
            };
            if focused {
                match idx {
                    0 => exec_rebase(app),
                    1 => exec_merge(app),
                    2 => exec_fetch(app),
                    3 => exec_pull(app),
                    4 => exec_push(app),
                    _ => {}
                }
            } else {
                open_file_diff(app);
            }
        }
        Action::GitViewDiff => { open_file_diff(app); }

        Action::GitRefresh => {
            if let Some(ref mut p) = app.git_actions_panel {
                refresh_changed_files(p);
                p.result_message = Some(("Refreshed".into(), false));
            }
        }
        _ => {}
    }
    Ok(())
}

/// Quick check if a key is a nav key (used to preserve result_message during scrolling)
fn action_is_nav(modifiers: crossterm::event::KeyModifiers, code: crossterm::event::KeyCode, _actions_focused: bool) -> Option<bool> {
    use crossterm::event::{KeyCode, KeyModifiers};
    Some(matches!((modifiers, code),
        (KeyModifiers::NONE, KeyCode::Char('j')) | (KeyModifiers::NONE, KeyCode::Char('k'))
        | (KeyModifiers::NONE, KeyCode::Down) | (KeyModifiers::NONE, KeyCode::Up)
        | (KeyModifiers::ALT, KeyCode::Up) | (KeyModifiers::ALT, KeyCode::Down)
    ))
}

/// Open the selected file's diff in the viewer pane
fn open_file_diff(app: &mut App) {
    let (wt, main, path) = match app.git_actions_panel.as_ref() {
        Some(p) => {
            if let Some(file) = p.changed_files.get(p.selected_file) {
                (p.worktree_path.clone(), p.main_branch.clone(), file.path.clone())
            } else { return; }
        }
        None => return,
    };
    match Git::get_file_diff(&wt, &main, &path) {
        Ok(diff) => {
            let title = format!("diff: {}", path);
            app.load_diff_into_viewer(&diff, Some(title));
            app.close_git_actions_panel();
        }
        Err(e) => {
            if let Some(ref mut p) = app.git_actions_panel {
                p.result_message = Some((format!("{}", e), true));
            }
        }
    }
}

/// Execute rebase and set result message
fn exec_rebase(app: &mut App) {
    let (wt, main) = match app.git_actions_panel.as_ref() {
        Some(p) => (p.worktree_path.clone(), p.main_branch.clone()),
        None => return,
    };
    let msg = match Git::rebase_onto_main(&wt, &main) {
        Ok(RebaseResult::Success) => ("Rebase completed".into(), false),
        Ok(RebaseResult::UpToDate) => ("Already up to date".into(), false),
        Ok(RebaseResult::Conflicts(s)) => (format!("Conflicts: {} files", s.conflicted_files.len()), true),
        Ok(RebaseResult::Aborted) => ("Rebase aborted".into(), true),
        Ok(RebaseResult::Failed(e)) => (e, true),
        Err(e) => (format!("{}", e), true),
    };
    if let Some(ref mut p) = app.git_actions_panel {
        p.result_message = Some(msg);
        refresh_changed_files(p);
    }
}

/// Merge the current worktree's branch into main. Runs from repo root
/// (which is always checked out on main) so no checkout is needed.
fn exec_merge(app: &mut App) {
    let (repo_root, branch) = match app.git_actions_panel.as_ref() {
        Some(p) => (p.repo_root.clone(), p.worktree_name.clone()),
        None => return,
    };
    let msg = match Git::merge_into_main(&repo_root, &branch) {
        Ok(m) => (m, false),
        Err(e) => (format!("{}", e), true),
    };
    if let Some(ref mut p) = app.git_actions_panel {
        p.result_message = Some(msg);
        refresh_changed_files(p);
    }
}

fn exec_fetch(app: &mut App) {
    let wt = match app.git_actions_panel.as_ref() {
        Some(p) => p.worktree_path.clone(),
        None => return,
    };
    let msg = match Git::fetch(&wt) {
        Ok(m) => (if m.is_empty() { "Fetched".into() } else { m }, false),
        Err(e) => (format!("{}", e), true),
    };
    if let Some(ref mut p) = app.git_actions_panel { p.result_message = Some(msg); }
}

fn exec_pull(app: &mut App) {
    let wt = match app.git_actions_panel.as_ref() {
        Some(p) => p.worktree_path.clone(),
        None => return,
    };
    let msg = match Git::pull(&wt) {
        Ok(m) => (m, false),
        Err(e) => (format!("{}", e), true),
    };
    if let Some(ref mut p) = app.git_actions_panel {
        p.result_message = Some(msg);
        refresh_changed_files(p);
    }
}

fn exec_push(app: &mut App) {
    let wt = match app.git_actions_panel.as_ref() {
        Some(p) => p.worktree_path.clone(),
        None => return,
    };
    let msg = match Git::push(&wt) {
        Ok(m) => (if m.is_empty() { "Pushed".into() } else { m }, false),
        Err(e) => (format!("{}", e), true),
    };
    if let Some(ref mut p) = app.git_actions_panel { p.result_message = Some(msg); }
}

/// Re-scan changed files from git diff (called after operations that change working tree)
fn refresh_changed_files(panel: &mut GitActionsPanel) {
    match Git::get_diff_files(&panel.worktree_path, &panel.main_branch) {
        Ok(files) => {
            panel.changed_files = files.into_iter().map(|(path, status, add, del)| {
                GitChangedFile { path, status, additions: add, deletions: del }
            }).collect();
            if panel.selected_file >= panel.changed_files.len() {
                panel.selected_file = panel.changed_files.len().saturating_sub(1);
            }
        }
        Err(_) => { panel.changed_files.clear(); panel.selected_file = 0; }
    }
}
