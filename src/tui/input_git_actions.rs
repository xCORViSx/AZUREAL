//! Input handler for the Git Actions panel (Shift+G).
//!
//! Full-screen modal overlay — consumes ALL input when active.
//! Actions section (Tab to switch): r=rebase, m=merge, f=fetch, l=pull, P=push.
//! File list section: j/k navigate, Enter/d opens file diff in viewer.

use anyhow::Result;
use crossterm::event::{self, KeyCode, KeyModifiers};

use crate::app::App;
use crate::app::types::{GitActionsPanel, GitChangedFile};
use crate::git::Git;
use crate::models::RebaseResult;

/// Total number of action items displayed in the actions section
const ACTION_COUNT: usize = 5;

/// Handle all keyboard input while the Git Actions panel is open.
/// Returns Ok(()) — the panel intercepts everything (no fallthrough).
pub fn handle_git_actions_input(key: event::KeyEvent, app: &mut App) -> Result<()> {
    // Safety: caller checks is_some() before calling us
    let panel = match app.git_actions_panel.as_mut() {
        Some(p) => p,
        None => return Ok(()),
    };

    // Clear stale result message on any key except navigation
    let is_nav = matches!(key.code, KeyCode::Char('j') | KeyCode::Char('k') | KeyCode::Up | KeyCode::Down);
    if !is_nav { panel.result_message = None; }

    match (key.modifiers, key.code) {
        // ── Close ──
        (KeyModifiers::NONE, KeyCode::Esc) => { app.close_git_actions_panel(); }

        // ── Tab: toggle focus between actions and file list ──
        (KeyModifiers::NONE, KeyCode::Tab) => { panel.actions_focused = !panel.actions_focused; }

        // ── Navigation (j/k/Up/Down work in both sections) ──
        (KeyModifiers::NONE, KeyCode::Char('j')) | (KeyModifiers::NONE, KeyCode::Down) => {
            if panel.actions_focused {
                if panel.selected_action + 1 < ACTION_COUNT { panel.selected_action += 1; }
            } else if !panel.changed_files.is_empty() && panel.selected_file + 1 < panel.changed_files.len() {
                panel.selected_file += 1;
            }
        }
        (KeyModifiers::NONE, KeyCode::Char('k')) | (KeyModifiers::NONE, KeyCode::Up) => {
            if panel.actions_focused {
                if panel.selected_action > 0 { panel.selected_action -= 1; }
            } else if panel.selected_file > 0 {
                panel.selected_file -= 1;
            }
        }

        // ── Jump to top/bottom ──
        (KeyModifiers::ALT, KeyCode::Up) => {
            if panel.actions_focused { panel.selected_action = 0; }
            else { panel.selected_file = 0; panel.file_scroll = 0; }
        }
        (KeyModifiers::ALT, KeyCode::Down) => {
            if panel.actions_focused { panel.selected_action = ACTION_COUNT.saturating_sub(1); }
            else if !panel.changed_files.is_empty() { panel.selected_file = panel.changed_files.len() - 1; }
        }

        // ── Git actions (only fire when actions section is focused) ──

        // [r] Rebase from main
        (KeyModifiers::NONE, KeyCode::Char('r')) if panel.actions_focused => {
            let wt = panel.worktree_path.clone();
            let main = panel.main_branch.clone();
            match Git::rebase_onto_main(&wt, &main) {
                Ok(RebaseResult::Success) => panel.result_message = Some(("Rebase completed".into(), false)),
                Ok(RebaseResult::UpToDate) => panel.result_message = Some(("Already up to date".into(), false)),
                Ok(RebaseResult::Conflicts(s)) => panel.result_message = Some((format!("Conflicts: {} files", s.conflicted_files.len()), true)),
                Ok(RebaseResult::Aborted) => panel.result_message = Some(("Rebase aborted".into(), true)),
                Ok(RebaseResult::Failed(e)) => panel.result_message = Some((e, true)),
                Err(e) => panel.result_message = Some((format!("{}", e), true)),
            }
            refresh_changed_files(panel);
        }
        // [m] Merge from main
        (KeyModifiers::NONE, KeyCode::Char('m')) if panel.actions_focused => {
            let wt = panel.worktree_path.clone();
            let main = panel.main_branch.clone();
            match Git::merge_from_main(&wt, &main) {
                Ok(msg) => { panel.result_message = Some((msg, false)); refresh_changed_files(panel); }
                Err(e) => panel.result_message = Some((format!("{}", e), true)),
            }
        }
        // [f] Fetch
        (KeyModifiers::NONE, KeyCode::Char('f')) if panel.actions_focused => {
            let wt = panel.worktree_path.clone();
            match Git::fetch(&wt) {
                Ok(msg) => panel.result_message = Some((if msg.is_empty() { "Fetched".into() } else { msg }, false)),
                Err(e) => panel.result_message = Some((format!("{}", e), true)),
            }
        }
        // [l] Pull (mnemonic: puLl — 'p' conflicts with global prompt mode)
        (KeyModifiers::NONE, KeyCode::Char('l')) if panel.actions_focused => {
            let wt = panel.worktree_path.clone();
            match Git::pull(&wt) {
                Ok(msg) => { panel.result_message = Some((msg, false)); refresh_changed_files(panel); }
                Err(e) => panel.result_message = Some((format!("{}", e), true)),
            }
        }
        // [P] Push (uppercase to avoid conflicts, signals "action with consequences")
        (KeyModifiers::NONE, KeyCode::Char('P')) if panel.actions_focused => {
            let wt = panel.worktree_path.clone();
            match Git::push(&wt) {
                Ok(msg) => panel.result_message = Some((if msg.is_empty() { "Pushed".into() } else { msg }, false)),
                Err(e) => panel.result_message = Some((format!("{}", e), true)),
            }
        }
        // ── Enter on actions: execute the selected action by index ──
        (KeyModifiers::NONE, KeyCode::Enter) if panel.actions_focused => {
            // Fire the action corresponding to selected_action index
            let wt = panel.worktree_path.clone();
            let main = panel.main_branch.clone();
            match panel.selected_action {
                0 => { // Rebase
                    match Git::rebase_onto_main(&wt, &main) {
                        Ok(RebaseResult::Success) => panel.result_message = Some(("Rebase completed".into(), false)),
                        Ok(RebaseResult::UpToDate) => panel.result_message = Some(("Already up to date".into(), false)),
                        Ok(RebaseResult::Conflicts(s)) => panel.result_message = Some((format!("Conflicts: {} files", s.conflicted_files.len()), true)),
                        Ok(RebaseResult::Aborted) => panel.result_message = Some(("Rebase aborted".into(), true)),
                        Ok(RebaseResult::Failed(e)) => panel.result_message = Some((e, true)),
                        Err(e) => panel.result_message = Some((format!("{}", e), true)),
                    }
                    refresh_changed_files(panel);
                }
                1 => { // Merge
                    match Git::merge_from_main(&wt, &main) {
                        Ok(msg) => panel.result_message = Some((msg, false)),
                        Err(e) => panel.result_message = Some((format!("{}", e), true)),
                    }
                    refresh_changed_files(panel);
                }
                2 => { // Fetch
                    match Git::fetch(&wt) {
                        Ok(msg) => panel.result_message = Some((if msg.is_empty() { "Fetched".into() } else { msg }, false)),
                        Err(e) => panel.result_message = Some((format!("{}", e), true)),
                    }
                }
                3 => { // Pull
                    match Git::pull(&wt) {
                        Ok(msg) => panel.result_message = Some((msg, false)),
                        Err(e) => panel.result_message = Some((format!("{}", e), true)),
                    }
                    refresh_changed_files(panel);
                }
                4 => { // Push
                    match Git::push(&wt) {
                        Ok(msg) => panel.result_message = Some((if msg.is_empty() { "Pushed".into() } else { msg }, false)),
                        Err(e) => panel.result_message = Some((format!("{}", e), true)),
                    }
                }
                _ => {}
            }
        }

        // ── Enter/d on file list: open file diff in viewer ──
        (KeyModifiers::NONE, KeyCode::Enter) | (KeyModifiers::NONE, KeyCode::Char('d')) if !panel.actions_focused => {
            if let Some(file) = panel.changed_files.get(panel.selected_file) {
                let wt = panel.worktree_path.clone();
                let main = panel.main_branch.clone();
                let path = file.path.clone();
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
        }

        // ── R: refresh file list ──
        (KeyModifiers::NONE, KeyCode::Char('R')) => {
            refresh_changed_files(panel);
            panel.result_message = Some(("Refreshed".into(), false));
        }

        // Everything else consumed (modal eats all input)
        _ => {}
    }
    Ok(())
}

/// Re-scan changed files from git diff (called after operations that change working tree)
fn refresh_changed_files(panel: &mut GitActionsPanel) {
    match Git::get_diff_files(&panel.worktree_path, &panel.main_branch) {
        Ok(files) => {
            panel.changed_files = files.into_iter().map(|(path, status, add, del)| {
                GitChangedFile { path, status, additions: add, deletions: del }
            }).collect();
            // Clamp selection if files were removed
            if panel.selected_file >= panel.changed_files.len() {
                panel.selected_file = panel.changed_files.len().saturating_sub(1);
            }
        }
        Err(_) => { panel.changed_files.clear(); panel.selected_file = 0; }
    }
}
