//! Input handler for the Git Actions panel (Shift+G).
//!
//! Full-screen modal overlay — consumes ALL input when active, dispatched via
//! the centralized keybinding system (lookup_git_actions_action in keybindings.rs).
//! Context-aware actions: main branch gets l=pull, c=commit, P=push;
//! feature branches get m=squash-merge, c=commit, P=push.
//! File list section: j/k navigate, Enter/d opens file diff in viewer.
//!
//! Split into focused submodules:
//! - `diff_viewer`: File/commit diff loading into inline viewer
//! - `operations`: Git commands (pull, push, rebase, squash-merge, commit, refresh)
//! - `commit_overlay`: Commit message editing overlay
//! - `conflict_resolution`: Conflict overlay and RCR Claude spawning
//! - `auto_resolve_overlay`: Auto-resolve file list settings overlay

mod auto_resolve_overlay;
mod commit_overlay;
mod conflict_resolution;
mod diff_viewer;
mod operations;

// Re-export pub(crate) items for external consumers
pub(crate) use operations::{exec_rebase_inner, RebaseOutcome, refresh_changed_files, refresh_commit_log};

use anyhow::Result;
use crossterm::event;
use crossterm::event::KeyModifiers;

use crate::app::App;
use crate::claude::ClaudeProcess;
use super::keybindings::{lookup_git_actions_action, Action};
use super::event_loop::copy_viewer_selection;

use diff_viewer::{open_file_diff_inline, load_file_diff_inline, load_commit_diff_inline};
use operations::{exec_squash_merge, exec_rebase, exec_pull, exec_push, exec_commit_start};
use commit_overlay::handle_commit_overlay;
use conflict_resolution::handle_conflict_overlay;
use auto_resolve_overlay::handle_auto_resolve_overlay;

/// Action count depends on context: main=3 (pull, commit, push),
/// feature=4 (squash-merge, rebase, commit, push)
fn action_count(is_on_main: bool) -> usize { if is_on_main { 3 } else { 4 } }

/// Handle all keyboard input while the Git Actions panel is open.
/// Returns Ok(()) — the panel intercepts everything (no fallthrough).
pub fn handle_git_actions_input(key: event::KeyEvent, app: &mut App, claude_process: &ClaudeProcess) -> Result<()> {
    let panel = match app.git_actions_panel.as_mut() {
        Some(p) => p,
        None => return Ok(()),
    };

    // Conflict overlay intercepts all input when open (resolve/abort actions)
    if panel.conflict_overlay.is_some() {
        return handle_conflict_overlay(key, app, claude_process);
    }

    // Commit overlay intercepts all input when open (text editing + actions)
    if panel.commit_overlay.is_some() {
        return handle_commit_overlay(key, app);
    }

    // Auto-resolve overlay intercepts all input when open (file list editing)
    if panel.auto_resolve_overlay.is_some() {
        return handle_auto_resolve_overlay(key, app);
    }

    // Cmd+C (copy) and Cmd+A (select all) — global actions that must work in git mode
    if key.modifiers.contains(KeyModifiers::SUPER) {
        match key.code {
            event::KeyCode::Char('c') => {
                if app.viewer_selection.is_some() {
                    copy_viewer_selection(app);
                } else if app.git_status_selected {
                    if let Some(ref p) = app.git_actions_panel {
                        if let Some((ref msg, _)) = p.result_message {
                            let text = msg.clone();
                            if let Ok(mut cb) = arboard::Clipboard::new() { let _ = cb.set_text(&text); }
                            app.clipboard = text;
                            app.set_status("Copied to clipboard");
                        }
                    }
                } else if let Some(ref p) = app.git_actions_panel {
                    if let Some((ref msg, _)) = p.result_message {
                        let text = msg.clone();
                        if let Ok(mut cb) = arboard::Clipboard::new() { let _ = cb.set_text(&text); }
                        app.clipboard = text;
                        app.set_status("Copied to clipboard");
                    }
                }
                return Ok(());
            }
            event::KeyCode::Char('a') => {
                if app.viewer_lines_cache.is_empty() {
                    // No viewer content — select the status box message
                    app.git_status_selected = app.git_actions_panel.as_ref()
                        .and_then(|p| p.result_message.as_ref()).is_some();
                } else {
                    app.git_status_selected = false;
                    let last = app.viewer_lines_cache.len().saturating_sub(1);
                    let last_col = app.viewer_lines_cache.last()
                        .map(|l| l.spans.iter().map(|s| s.content.chars().count()).sum::<usize>())
                        .unwrap_or(0);
                    app.viewer_selection = Some((0, 0, last, last_col));
                }
                return Ok(());
            }
            _ => {}
        }
    }

    // Shift+J/K and PageDown/PageUp — scroll the diff viewer
    match key.code {
        event::KeyCode::Char('J') if key.modifiers.contains(KeyModifiers::SHIFT) => {
            app.scroll_viewer_down(app.viewer_viewport_height.saturating_sub(2));
            return Ok(());
        }
        event::KeyCode::Char('K') if key.modifiers.contains(KeyModifiers::SHIFT) => {
            app.scroll_viewer_up(app.viewer_viewport_height.saturating_sub(2));
            return Ok(());
        }
        event::KeyCode::PageDown => {
            app.scroll_viewer_down(app.viewer_viewport_height.saturating_sub(2));
            return Ok(());
        }
        event::KeyCode::PageUp => {
            app.scroll_viewer_up(app.viewer_viewport_height.saturating_sub(2));
            return Ok(());
        }
        _ => {}
    }

    // Clear stale result message on any non-nav key
    let is_nav = matches!(action_is_nav(key.modifiers, key.code, panel.focused_pane),
        Some(true));
    if !is_nav { panel.result_message = None; app.git_status_selected = false; }

    let focused_pane = panel.focused_pane;
    let is_on_main = panel.is_on_main;

    // Resolve key → action via centralized binding arrays
    let Some(action) = lookup_git_actions_action(focused_pane, is_on_main, key.modifiers, key.code) else {
        return Ok(());
    };

    match action {
        Action::Escape => { app.close_git_actions_panel(); }
        Action::GitToggleFocus => {
            if let Some(ref mut p) = app.git_actions_panel {
                p.focused_pane = (p.focused_pane + 1) % 3;
            }
        }
        Action::NavDown => {
            if let Some(ref mut p) = app.git_actions_panel {
                match p.focused_pane {
                    0 => { if p.selected_action + 1 < action_count(p.is_on_main) { p.selected_action += 1; } }
                    1 => {
                        if !p.changed_files.is_empty() && p.selected_file + 1 < p.changed_files.len() {
                            p.selected_file += 1;
                        }
                        load_file_diff_inline(p);
                    }
                    2 => {
                        if !p.commits.is_empty() && p.selected_commit + 1 < p.commits.len() {
                            p.selected_commit += 1;
                        }
                        load_commit_diff_inline(p);
                    }
                    _ => {}
                }
            }
        }
        Action::NavUp => {
            if let Some(ref mut p) = app.git_actions_panel {
                match p.focused_pane {
                    0 => { if p.selected_action > 0 { p.selected_action -= 1; } }
                    1 => {
                        if p.selected_file > 0 { p.selected_file -= 1; }
                        load_file_diff_inline(p);
                    }
                    2 => {
                        if p.selected_commit > 0 { p.selected_commit -= 1; }
                        load_commit_diff_inline(p);
                    }
                    _ => {}
                }
            }
        }
        Action::GoToTop => {
            if let Some(ref mut p) = app.git_actions_panel {
                match p.focused_pane {
                    0 => { p.selected_action = 0; }
                    1 => { p.selected_file = 0; p.file_scroll = 0; load_file_diff_inline(p); }
                    2 => { p.selected_commit = 0; p.commit_scroll = 0; load_commit_diff_inline(p); }
                    _ => {}
                }
            }
        }
        Action::GoToBottom => {
            if let Some(ref mut p) = app.git_actions_panel {
                match p.focused_pane {
                    0 => { p.selected_action = action_count(p.is_on_main).saturating_sub(1); }
                    1 => { if !p.changed_files.is_empty() { p.selected_file = p.changed_files.len() - 1; load_file_diff_inline(p); } }
                    2 => { if !p.commits.is_empty() { p.selected_commit = p.commits.len() - 1; load_commit_diff_inline(p); } }
                    _ => {}
                }
            }
        }

        // ── Git operations (only fire when focused_pane==0, enforced by lookup guard) ──
        Action::GitSquashMerge => { exec_squash_merge(app); }
        Action::GitRebase => { exec_rebase(app); }
        Action::GitPull => { exec_pull(app); }
        Action::GitCommit => { exec_commit_start(app); }
        Action::GitPush => { exec_push(app); }
        Action::GitAutoRebase => {
            if let Some(ref p) = app.git_actions_panel {
                let branch = p.worktree_name.clone();
                let repo_root = p.repo_root.clone();
                let enabled = !app.auto_rebase_enabled.contains(&branch);
                if enabled {
                    app.auto_rebase_enabled.insert(branch.clone());
                } else {
                    app.auto_rebase_enabled.remove(&branch);
                }
                crate::azufig::set_auto_rebase(&repo_root, &branch, enabled);
                app.invalidate_sidebar();
                if let Some(ref mut p) = app.git_actions_panel {
                    p.result_message = Some((
                        if enabled { "Auto-rebase enabled".into() } else { "Auto-rebase disabled".into() },
                        false,
                    ));
                }
            }
        }

        Action::GitAutoResolveSettings => {
            if let Some(ref mut p) = app.git_actions_panel {
                let files: Vec<(String, bool)> = p.auto_resolve_files.iter()
                    .map(|f| (f.clone(), true))
                    .collect();
                p.auto_resolve_overlay = Some(crate::app::types::AutoResolveOverlay {
                    files,
                    selected: 0,
                    adding: false,
                    input_buffer: String::new(),
                    input_cursor: 0,
                });
            }
        }

        // ── Enter/d: execute action by index (when focused) or open diff (file list) ──
        // Index mapping: main=[pull,commit,push], feature=[squash-merge,rebase,commit,push]
        Action::Confirm => {
            let (pane, idx, on_main) = match app.git_actions_panel.as_ref() {
                Some(p) => (p.focused_pane, p.selected_action, p.is_on_main),
                None => return Ok(()),
            };
            match pane {
                0 => {
                    if on_main {
                        match idx { 0 => exec_pull(app), 1 => exec_commit_start(app), 2 => exec_push(app), _ => {} }
                    } else {
                        match idx { 0 => exec_squash_merge(app), 1 => exec_rebase(app), 2 => exec_commit_start(app), 3 => exec_push(app), _ => {} }
                    }
                }
                1 => { open_file_diff_inline(app); }
                2 => {
                    if let Some(ref mut p) = app.git_actions_panel {
                        load_commit_diff_inline(p);
                    }
                }
                _ => {}
            }
        }
        Action::GitViewDiff => { open_file_diff_inline(app); }

        Action::GitRefresh => {
            if let Some(ref mut p) = app.git_actions_panel {
                refresh_changed_files(p);
                refresh_commit_log(p);
                p.result_message = Some(("Refreshed".into(), false));
            }
        }
        Action::GitPrevWorktree => { switch_git_panel_worktree(app, false); }
        Action::GitNextWorktree => { switch_git_panel_worktree(app, true); }
        Action::GitPrevPage => { switch_git_panel_page(app, false); }
        Action::GitNextPage => { switch_git_panel_page(app, true); }
        _ => {}
    }
    Ok(())
}

/// Cycle the git panel to the prev/next active (non-archived) worktree without closing the panel.
/// Preserves `focused_pane` so the user stays in the same column after switching.
fn switch_git_panel_worktree(app: &mut App, forward: bool) {
    // Collect indices of all active worktrees (non-archived, have a real path)
    let active: Vec<usize> = app.worktrees.iter().enumerate()
        .filter(|(_, wt)| !wt.archived && wt.worktree_path.is_some())
        .map(|(i, _)| i)
        .collect();
    if active.len() <= 1 { return; }

    // Find current position — match against panel's worktree_name
    let current_name = match app.git_actions_panel.as_ref() {
        Some(p) => p.worktree_name.clone(),
        None => return,
    };
    let pos = active.iter().position(|&idx| {
        app.worktrees[idx].branch_name == current_name
    }).unwrap_or(0);

    let new_pos = if forward {
        (pos + 1) % active.len()
    } else {
        (pos + active.len() - 1) % active.len()
    };
    let new_idx = active[new_pos];

    // Preserve focused pane across the rebuild
    let focused_pane = app.git_actions_panel.as_ref().map(|p| p.focused_pane).unwrap_or(0);

    app.selected_worktree = Some(new_idx);
    app.load_session_output();
    app.open_git_actions_panel();

    if let Some(ref mut p) = app.git_actions_panel {
        p.focused_pane = focused_pane;
    }
}

/// Jump to the first worktree on the prev/next tab-bar page.
/// Replicates the greedy tab packing from `draw_git_worktree_tabs` to find page boundaries,
/// then switches to the first worktree on the target page.
fn switch_git_panel_page(app: &mut App, forward: bool) {
    let panel = match app.git_actions_panel.as_ref() {
        Some(p) => p,
        None => return,
    };
    let active_branch = panel.worktree_name.clone();
    let tab_bar_width = app.pane_worktrees.width.saturating_add(
        app.pane_viewer.width).saturating_add(app.pane_session.width) as usize;
    if tab_bar_width == 0 { return; }

    // Collect active worktrees (same filter as draw_git_worktree_tabs)
    let active: Vec<(usize, &str)> = app.worktrees.iter().enumerate()
        .filter(|(_, wt)| !wt.archived && wt.worktree_path.is_some())
        .map(|(i, wt)| (i, wt.name()))
        .collect();
    if active.len() <= 1 { return; }

    // Tab display widths: " name " = name_cols + 2
    let tab_widths: Vec<usize> = active.iter()
        .map(|(_, name)| {
            name.chars()
                .map(|c| unicode_width::UnicodeWidthChar::width(c).unwrap_or(1))
                .sum::<usize>() + 2
        })
        .collect();

    // Greedy page packing (mirrors draw_git_worktree_tabs logic)
    let mut pages: Vec<Vec<usize>> = Vec::new();
    let mut cur: Vec<usize> = Vec::new();
    let mut cur_w: usize = 0;
    let mut active_page: usize = 0;

    for (i, &tw) in tab_widths.iter().enumerate() {
        let cost = if cur.is_empty() { tw } else { tw + 1 };
        if !cur.is_empty() && cur_w + cost > tab_bar_width {
            pages.push(std::mem::take(&mut cur));
            cur = vec![i];
            cur_w = tw;
        } else {
            cur.push(i);
            cur_w += cost;
        }
        if active[i].1 == active_branch { active_page = pages.len(); }
    }
    if !cur.is_empty() { pages.push(cur); }

    let total_pages = pages.len();
    if total_pages <= 1 { return; }

    let target_page = if forward {
        (active_page + 1) % total_pages
    } else {
        (active_page + total_pages - 1) % total_pages
    };

    // Jump to first worktree on the target page
    let first_on_page = pages[target_page][0];
    let new_idx = active[first_on_page].0;

    let focused_pane = app.git_actions_panel.as_ref().map(|p| p.focused_pane).unwrap_or(0);
    app.selected_worktree = Some(new_idx);
    app.load_session_output();
    app.open_git_actions_panel();
    if let Some(ref mut p) = app.git_actions_panel {
        p.focused_pane = focused_pane;
    }
}

/// Quick check if a key is a nav key (used to preserve result_message during scrolling)
fn action_is_nav(modifiers: crossterm::event::KeyModifiers, code: crossterm::event::KeyCode, _focused_pane: u8) -> Option<bool> {
    use crossterm::event::{KeyCode, KeyModifiers};
    Some(matches!((modifiers, code),
        (KeyModifiers::NONE, KeyCode::Char('j')) | (KeyModifiers::NONE, KeyCode::Char('k'))
        | (KeyModifiers::NONE, KeyCode::Down) | (KeyModifiers::NONE, KeyCode::Up)
        | (KeyModifiers::ALT, KeyCode::Up) | (KeyModifiers::ALT, KeyCode::Down)
        | (KeyModifiers::NONE, KeyCode::Char('[')) | (KeyModifiers::NONE, KeyCode::Char(']'))
        | (_, KeyCode::Char('{')) | (_, KeyCode::Char('}'))
    ))
}

