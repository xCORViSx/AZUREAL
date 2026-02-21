//! Input handler for the Git Actions panel (Shift+G).
//!
//! Full-screen modal overlay — consumes ALL input when active, dispatched via
//! the centralized keybinding system (lookup_git_actions_action in keybindings.rs).
//! Actions section (Tab to switch): r=rebase, m=merge, f=fetch, l=pull, P=push, c=commit.
//! File list section: j/k navigate, Enter/d opens file diff in viewer.

use anyhow::Result;
use crossterm::event;

use crate::app::App;
use crate::app::types::{GitActionsPanel, GitChangedFile, GitCommitOverlay};
use crate::git::Git;
use crate::models::RebaseResult;
use super::keybindings::{lookup_git_actions_action, Action};

/// Total number of action items in the actions section
/// (6 git ops: rebase/merge/fetch/pull/push/commit + 1 auto-rebase toggle)
const ACTION_COUNT: usize = 7;

/// Handle all keyboard input while the Git Actions panel is open.
/// Returns Ok(()) — the panel intercepts everything (no fallthrough).
pub fn handle_git_actions_input(key: event::KeyEvent, app: &mut App) -> Result<()> {
    let panel = match app.git_actions_panel.as_mut() {
        Some(p) => p,
        None => return Ok(()),
    };

    // Commit overlay intercepts all input when open (text editing + actions)
    if panel.commit_overlay.is_some() {
        return handle_commit_overlay(key, app);
    }

    // If auto-rebase scope picker is open, handle it before anything else
    if panel.autorebase_scope.is_some() {
        return handle_autorebase_scope(key, app);
    }

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
        Action::GitCommit => { exec_commit_start(app); }

        // Auto-rebase toggle: if currently ON → turn off, if OFF → open scope picker
        Action::GitAutoRebase => { exec_autorebase_toggle(app); }

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
                    5 => exec_commit_start(app),
                    6 => exec_autorebase_toggle(app),
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

/// Toggle auto-rebase: if yes → set no (direct), if no → open scope picker
fn exec_autorebase_toggle(app: &mut App) {
    let panel = match app.git_actions_panel.as_mut() {
        Some(p) => p,
        None => return,
    };
    if panel.autorebase_on {
        // Currently yes — disable for this worktree
        crate::azufig::set_autorebase(&panel.worktree_path, false);
        panel.autorebase_on = false;
        panel.result_message = Some(("Auto-rebase: No".into(), false));
    } else {
        // Currently no — open the scope picker (0 = this worktree, 1 = all)
        panel.autorebase_scope = Some(0);
    }
}

/// Handle input while the auto-rebase scope picker is open.
/// j/k or ↑/↓ to select, Enter to confirm, Esc to cancel.
fn handle_autorebase_scope(key: event::KeyEvent, app: &mut App) -> Result<()> {
    use crossterm::event::{KeyCode, KeyModifiers};
    let panel = match app.git_actions_panel.as_mut() {
        Some(p) => p,
        None => return Ok(()),
    };
    let sel = panel.autorebase_scope.unwrap_or(0);
    match (key.modifiers, key.code) {
        (KeyModifiers::NONE, KeyCode::Char('j')) | (KeyModifiers::NONE, KeyCode::Down) => {
            if sel < 1 { panel.autorebase_scope = Some(1); }
        }
        (KeyModifiers::NONE, KeyCode::Char('k')) | (KeyModifiers::NONE, KeyCode::Up) => {
            if sel > 0 { panel.autorebase_scope = Some(0); }
        }
        (KeyModifiers::NONE, KeyCode::Enter) => {
            if sel == 0 {
                // This worktree only — write to this worktree's config
                let wt = panel.worktree_path.clone();
                crate::azufig::set_autorebase(&wt, true);
                panel.autorebase_on = true;
                panel.result_message = Some(("Auto-rebase: Yes (this worktree)".into(), false));
            } else {
                // All worktrees — write to each worktree's config individually
                let paths: Vec<_> = app.worktrees.iter()
                    .filter_map(|s| s.worktree_path.clone())
                    .collect();
                for wt in &paths { crate::azufig::set_autorebase(wt, true); }
                if let Some(ref mut p) = app.git_actions_panel {
                    p.autorebase_on = true;
                    p.result_message = Some((format!("Auto-rebase: Yes ({} worktrees)", paths.len()), false));
                }
            }
            if let Some(ref mut p) = app.git_actions_panel { p.autorebase_scope = None; }
        }
        (KeyModifiers::NONE, KeyCode::Esc) => {
            panel.autorebase_scope = None;
        }
        _ => {}
    }
    Ok(())
}

/// Re-scan changed files from git diff (called after operations that change working tree)
pub(crate) fn refresh_changed_files(panel: &mut GitActionsPanel) {
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

/// Start the commit flow: stage all changes, get the diff, spawn Claude one-shot
/// to generate a commit message, and open the commit overlay.
fn exec_commit_start(app: &mut App) {
    let wt = match app.git_actions_panel.as_ref() {
        Some(p) => p.worktree_path.clone(),
        None => return,
    };

    // Stage everything and check if there's anything to commit
    if let Err(e) = Git::stage_all(&wt) {
        if let Some(ref mut p) = app.git_actions_panel {
            p.result_message = Some((format!("Stage failed: {}", e), true));
        }
        return;
    }
    let diff = match Git::get_staged_diff(&wt) {
        Ok(d) if d.trim().is_empty() => {
            if let Some(ref mut p) = app.git_actions_panel {
                p.result_message = Some(("Nothing to commit".into(), false));
            }
            return;
        }
        Ok(d) => d,
        Err(e) => {
            if let Some(ref mut p) = app.git_actions_panel {
                p.result_message = Some((format!("Diff failed: {}", e), true));
            }
            return;
        }
    };

    // Also get --stat summary for a more compact prompt (Claude sees both)
    let stat = Git::get_staged_stat(&wt).unwrap_or_default();

    // Resolve the Claude binary path from config
    let claude_bin = crate::azufig::load_global_azufig()
        .config.claude_executable
        .unwrap_or_else(|| "claude".into());

    // Spawn background thread to run Claude one-shot for commit message generation.
    // Uses `claude -p` with a focused prompt — no session file, no streaming, just
    // stdout capture. The diff is piped in full so Claude has complete context.
    let (tx, rx) = std::sync::mpsc::channel();
    let wt_clone = wt.clone();
    std::thread::spawn(move || {
        // Truncate diff to ~30k chars to stay within reasonable prompt size.
        // The stat summary provides overview even if the diff is truncated.
        let max_diff = 30_000;
        let diff_trimmed = if diff.len() > max_diff { &diff[..max_diff] } else { &diff };
        let prompt = format!(
            "Write a conventional commit message for this diff. Format: type: short description (under 72 chars) on the first line, then a blank line, then optional bullet points for details. Types: feat, fix, refactor, docs, test, chore. Output ONLY the commit message, nothing else.\n\n--- stat ---\n{}\n--- diff ---\n{}",
            stat, diff_trimmed
        );
        // --no-session-persistence prevents Claude from saving a .jsonl session
        // file for this throwaway one-shot invocation (no session to resume)
        let result = std::process::Command::new(&claude_bin)
            .args(["-p", "--no-session-persistence", &prompt])
            .current_dir(&wt_clone)
            .output();

        match result {
            Ok(output) if output.status.success() => {
                // Strip markdown code fences Claude sometimes wraps the message in
                let raw = String::from_utf8_lossy(&output.stdout).trim().to_string();
                let msg = raw.strip_prefix("```").unwrap_or(&raw);
                let msg = msg.strip_suffix("```").unwrap_or(msg).trim().to_string();
                let _ = tx.send(Ok(msg));
            }
            Ok(output) => {
                let err = String::from_utf8_lossy(&output.stderr).trim().to_string();
                let _ = tx.send(Err(format!("Claude failed: {}", err)));
            }
            Err(e) => { let _ = tx.send(Err(format!("Failed to run claude: {}", e))); }
        }
    });

    // Open the overlay in "generating" state — message will be filled by the
    // event loop polling the receiver once Claude returns
    if let Some(ref mut p) = app.git_actions_panel {
        p.commit_overlay = Some(GitCommitOverlay {
            message: String::new(),
            cursor: 0,
            generating: true,
            scroll: 0,
            receiver: Some(rx),
        });
    }
}

/// Handle input while the commit message overlay is open.
/// Supports text editing (type/backspace/arrows), Enter to commit, p to commit+push, Esc to cancel.
fn handle_commit_overlay(key: event::KeyEvent, app: &mut App) -> Result<()> {
    use crossterm::event::{KeyCode, KeyModifiers};
    let panel = match app.git_actions_panel.as_mut() {
        Some(p) => p,
        None => return Ok(()),
    };
    let overlay = match panel.commit_overlay.as_mut() {
        Some(o) => o,
        None => return Ok(()),
    };

    // Block editing while Claude is still generating
    let generating = overlay.generating;

    match (key.modifiers, key.code) {
        // Esc — cancel and close overlay
        (KeyModifiers::NONE, KeyCode::Esc) => {
            panel.commit_overlay = None;
        }

        // Enter — commit with current message (deferred so loading indicator renders first)
        (KeyModifiers::NONE, KeyCode::Enter) if !generating && !overlay.message.trim().is_empty() => {
            let msg = overlay.message.clone();
            let wt = panel.worktree_path.clone();
            panel.commit_overlay = None;
            app.loading_indicator = Some("Committing...".into());
            app.deferred_action = Some(crate::app::DeferredAction::GitCommit {
                worktree: wt, message: msg,
            });
        }

        // ⌘P — commit + push (deferred so loading indicator renders first)
        (m, KeyCode::Char('p')) if m.contains(KeyModifiers::SUPER) && !generating && !overlay.message.trim().is_empty() => {
            let msg = overlay.message.clone();
            let wt = panel.worktree_path.clone();
            panel.commit_overlay = None;
            app.loading_indicator = Some("Committing and pushing...".into());
            app.deferred_action = Some(crate::app::DeferredAction::GitCommitAndPush {
                worktree: wt, message: msg,
            });
        }

        // Backspace — delete char before cursor
        (KeyModifiers::NONE, KeyCode::Backspace) if !generating => {
            if overlay.cursor > 0 {
                let byte_pos = overlay.message.char_indices()
                    .nth(overlay.cursor - 1)
                    .map(|(i, _)| i)
                    .unwrap_or(0);
                let next_byte = overlay.message.char_indices()
                    .nth(overlay.cursor)
                    .map(|(i, _)| i)
                    .unwrap_or(overlay.message.len());
                overlay.message.replace_range(byte_pos..next_byte, "");
                overlay.cursor -= 1;
            }
        }

        // Delete — delete char at cursor
        (KeyModifiers::NONE, KeyCode::Delete) if !generating => {
            let char_count = overlay.message.chars().count();
            if overlay.cursor < char_count {
                let byte_pos = overlay.message.char_indices()
                    .nth(overlay.cursor)
                    .map(|(i, _)| i)
                    .unwrap_or(overlay.message.len());
                let next_byte = overlay.message.char_indices()
                    .nth(overlay.cursor + 1)
                    .map(|(i, _)| i)
                    .unwrap_or(overlay.message.len());
                overlay.message.replace_range(byte_pos..next_byte, "");
            }
        }

        // Left/Right arrow — move cursor within message
        (KeyModifiers::NONE, KeyCode::Left) if !generating => {
            if overlay.cursor > 0 { overlay.cursor -= 1; }
        }
        (KeyModifiers::NONE, KeyCode::Right) if !generating => {
            let char_count = overlay.message.chars().count();
            if overlay.cursor < char_count { overlay.cursor += 1; }
        }

        // Home/End — jump to start/end of current line
        (KeyModifiers::NONE, KeyCode::Home) if !generating => { overlay.cursor = 0; }
        (KeyModifiers::NONE, KeyCode::End) if !generating => {
            overlay.cursor = overlay.message.chars().count();
        }

        // Up/Down — move cursor to previous/next logical line (auto-scroll follows)
        (KeyModifiers::NONE, KeyCode::Up) if !generating => {
            // Find start of current line, then move to same column on previous line
            let chars: Vec<char> = overlay.message.chars().collect();
            let mut line_start = overlay.cursor;
            while line_start > 0 && chars.get(line_start - 1) != Some(&'\n') { line_start -= 1; }
            if line_start > 0 {
                // There's a previous line — find its start
                let prev_end = line_start - 1; // the '\n' before current line
                let mut prev_start = prev_end;
                while prev_start > 0 && chars.get(prev_start - 1) != Some(&'\n') { prev_start -= 1; }
                let col = overlay.cursor - line_start;
                let prev_len = prev_end - prev_start;
                overlay.cursor = prev_start + col.min(prev_len);
            }
        }
        (KeyModifiers::NONE, KeyCode::Down) if !generating => {
            let chars: Vec<char> = overlay.message.chars().collect();
            let mut line_start = overlay.cursor;
            while line_start > 0 && chars.get(line_start - 1) != Some(&'\n') { line_start -= 1; }
            let col = overlay.cursor - line_start;
            // Find end of current line (the '\n' or message end)
            let mut line_end = overlay.cursor;
            while line_end < chars.len() && chars[line_end] != '\n' { line_end += 1; }
            if line_end < chars.len() {
                // There's a next line
                let next_start = line_end + 1;
                let mut next_end = next_start;
                while next_end < chars.len() && chars[next_end] != '\n' { next_end += 1; }
                let next_len = next_end - next_start;
                overlay.cursor = next_start + col.min(next_len);
            }
        }

        // Shift+Enter — insert newline (Enter alone commits)
        (m, KeyCode::Enter) if m.contains(KeyModifiers::SHIFT) && !generating => {
            let byte_pos = overlay.message.char_indices()
                .nth(overlay.cursor)
                .map(|(i, _)| i)
                .unwrap_or(overlay.message.len());
            overlay.message.insert(byte_pos, '\n');
            overlay.cursor += 1;
        }

        // Regular char — insert at cursor
        (m, KeyCode::Char(c)) if !generating && !m.contains(KeyModifiers::CONTROL) => {
            let byte_pos = overlay.message.char_indices()
                .nth(overlay.cursor)
                .map(|(i, _)| i)
                .unwrap_or(overlay.message.len());
            overlay.message.insert(byte_pos, c);
            overlay.cursor += 1;
        }

        _ => {}
    }
    Ok(())
}

/// Extract the first line from a multi-line string (for result messages)
fn first_line(s: &str) -> &str {
    s.lines().next().unwrap_or(s)
}
