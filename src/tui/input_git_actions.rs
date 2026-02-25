//! Input handler for the Git Actions panel (Shift+G).
//!
//! Full-screen modal overlay — consumes ALL input when active, dispatched via
//! the centralized keybinding system (lookup_git_actions_action in keybindings.rs).
//! Context-aware actions: main branch gets l=pull, c=commit, P=push;
//! feature branches get m=squash-merge, c=commit, P=push.
//! File list section: j/k navigate, Enter/d opens file diff in viewer.

use anyhow::Result;
use crossterm::event;

use crate::app::{App, Focus};
use crate::app::types::{GitActionsPanel, GitChangedFile, GitCommitOverlay, GitConflictOverlay, RcrSession, PostMergeDialog};
use crate::claude::ClaudeProcess;
use crate::git::{Git, SquashMergeResult};
use super::keybindings::{lookup_git_actions_action, Action};

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

    // Clear stale result message on any non-nav key
    let is_nav = matches!(action_is_nav(key.modifiers, key.code, panel.actions_focused),
        Some(true));
    if !is_nav { panel.result_message = None; }

    let actions_focused = panel.actions_focused;
    let is_on_main = panel.is_on_main;

    // Resolve key → action via centralized binding arrays
    let Some(action) = lookup_git_actions_action(actions_focused, is_on_main, key.modifiers, key.code) else {
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
                    if p.selected_action + 1 < action_count(p.is_on_main) { p.selected_action += 1; }
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
                if p.actions_focused { p.selected_action = action_count(p.is_on_main).saturating_sub(1); }
                else if !p.changed_files.is_empty() { p.selected_file = p.changed_files.len() - 1; }
            }
        }

        // ── Git operations (only fire when actions_focused, enforced by lookup guard) ──
        Action::GitSquashMerge => { exec_squash_merge(app); }
        Action::GitRebase => { exec_rebase(app); }
        Action::GitPull => { exec_pull(app); }
        Action::GitCommit => { exec_commit_start(app); }
        Action::GitPush => { exec_push(app); }

        // ── Enter/d: execute action by index (when focused) or open diff (file list) ──
        // Index mapping: main=[pull,commit,push], feature=[squash-merge,rebase,commit,push]
        Action::Confirm => {
            let (focused, idx, on_main) = match app.git_actions_panel.as_ref() {
                Some(p) => (p.actions_focused, p.selected_action, p.is_on_main),
                None => return Ok(()),
            };
            if focused {
                if on_main {
                    match idx { 0 => exec_pull(app), 1 => exec_commit_start(app), 2 => exec_push(app), _ => {} }
                } else {
                    match idx { 0 => exec_squash_merge(app), 1 => exec_rebase(app), 2 => exec_commit_start(app), 3 => exec_push(app), _ => {} }
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

/// Squash-merge the current worktree's branch into main. Rebases the feature
/// branch onto main first to ensure a clean, linear merge. On success, shows
/// result message. On conflict (rebase or merge), opens the conflict overlay.
fn exec_squash_merge(app: &mut App) {
    let (repo_root, branch, wt_path, main_branch) = match app.git_actions_panel.as_ref() {
        Some(p) => (p.repo_root.clone(), p.worktree_name.clone(), p.worktree_path.clone(), p.main_branch.clone()),
        None => return,
    };
    // Block squash merge when the feature branch has uncommitted changes —
    // those changes won't be included in the squash and could be lost
    if let Some(ref mut p) = app.git_actions_panel {
        if !p.changed_files.is_empty() {
            p.result_message = Some(("Commit your changes first (c) before squash merging".into(), true));
            return;
        }
    }

    // Rebase feature branch onto main BEFORE merging. This ensures the squash
    // merge is clean and linear — conflicts are resolved here, not during merge.
    match exec_rebase_inner(&wt_path, &main_branch) {
        RebaseOutcome::Conflict { conflicted, auto_merged, .. } => {
            // Show conflict overlay — rebase in progress, RCR can resolve.
            // continue_with_merge=true so squash merge auto-proceeds after resolution.
            if let Some(ref mut p) = app.git_actions_panel {
                p.conflict_overlay = Some(GitConflictOverlay {
                    conflicted_files: conflicted,
                    auto_merged_files: auto_merged,
                    scroll: 0,
                    selected: 0,
                    continue_with_merge: true,
                });
            }
            return;
        }
        RebaseOutcome::Failed(msg) => {
            if let Some(ref mut p) = app.git_actions_panel {
                p.result_message = Some((format!("Rebase failed: {}", msg), true));
            }
            return;
        }
        RebaseOutcome::Rebased | RebaseOutcome::UpToDate => {} // proceed to merge
    }

    match Git::squash_merge_into_main(&repo_root, &branch) {
        Ok(SquashMergeResult::Success(msg)) => {
            let display = branch.strip_prefix("azureal/").unwrap_or(&branch).to_string();
            app.git_actions_panel = None;
            app.post_merge_dialog = Some(PostMergeDialog {
                branch: branch.clone(),
                display_name: display,
                worktree_path: wt_path,
                selected: 0,
            });
            app.set_status(msg);
        }
        Ok(SquashMergeResult::Conflict { conflicted, auto_merged, .. }) => {
            if let Some(ref mut p) = app.git_actions_panel {
                p.conflict_overlay = Some(GitConflictOverlay {
                    conflicted_files: conflicted,
                    auto_merged_files: auto_merged,
                    scroll: 0,
                    selected: 0,
                    continue_with_merge: true,
                });
            }
        }
        Err(e) => {
            if let Some(ref mut p) = app.git_actions_panel {
                p.result_message = Some((format!("{}", e), true));
            }
        }
    }
}

/// Rebase outcome for the UI to display
enum RebaseOutcome {
    Rebased,
    UpToDate,
    /// Conflict — rebase is left in progress (NOT aborted) so RCR can resolve.
    /// Contains (conflicted_files, auto_merged_files, raw_output).
    Conflict { conflicted: Vec<String>, auto_merged: Vec<String>, _raw_output: String },
    Failed(String),
}

/// Inner rebase logic — used by both manual rebase (r) and pre-merge rebase.
/// No git fetch — caller is responsible for ensuring main is current.
/// On conflict, leaves rebase in progress so RCR can resolve it.
fn exec_rebase_inner(worktree_path: &std::path::Path, main_branch: &str) -> RebaseOutcome {
    // Check if already up to date
    let base = std::process::Command::new("git")
        .args(["merge-base", "HEAD", main_branch])
        .current_dir(worktree_path)
        .output();
    let tip = std::process::Command::new("git")
        .args(["rev-parse", main_branch])
        .current_dir(worktree_path)
        .output();
    if let (Ok(b), Ok(t)) = (&base, &tip) {
        if b.stdout == t.stdout { return RebaseOutcome::UpToDate; }
    }

    let output = match std::process::Command::new("git")
        .args(["rebase", main_branch])
        .current_dir(worktree_path)
        .output()
    {
        Ok(o) => o,
        Err(e) => return RebaseOutcome::Failed(e.to_string()),
    };

    if output.status.success() { return RebaseOutcome::Rebased; }

    let combined = format!("{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
    let text = combined.trim();
    if text.contains("CONFLICT") || text.contains("could not apply") {
        // Parse conflict info — leave rebase in progress for RCR resolution
        let mut conflicted = Vec::new();
        let mut auto_merged = Vec::new();
        for line in text.lines() {
            if line.starts_with("CONFLICT") {
                if let Some(path) = line.rsplit("Merge conflict in ").next() {
                    conflicted.push(path.trim().to_string());
                } else {
                    conflicted.push(line.to_string());
                }
            } else if let Some(path) = line.strip_prefix("Auto-merging ") {
                auto_merged.push(path.trim().to_string());
            }
        }
        // If we couldn't parse specific files, get them from git diff
        if conflicted.is_empty() {
            if let Ok(diff) = Git::get_conflicted_files(worktree_path) {
                conflicted = diff;
            }
        }
        return RebaseOutcome::Conflict { conflicted, auto_merged, _raw_output: text.to_string() };
    }

    RebaseOutcome::Failed(text.to_string())
}

/// Manual rebase action — rebase this worktree onto main (feature branches only).
/// On conflict, shows the conflict overlay with RCR option (rebase stays in progress).
fn exec_rebase(app: &mut App) {
    let (wt_path, main_branch) = match app.git_actions_panel.as_ref() {
        Some(p) => (p.worktree_path.clone(), p.main_branch.clone()),
        None => return,
    };
    if let Some(ref p) = app.git_actions_panel {
        if !p.changed_files.is_empty() {
            if let Some(ref mut p) = app.git_actions_panel {
                p.result_message = Some(("Commit or stash changes first before rebasing".into(), true));
            }
            return;
        }
    }
    match exec_rebase_inner(&wt_path, &main_branch) {
        RebaseOutcome::Rebased => {
            if let Some(ref mut p) = app.git_actions_panel {
                refresh_changed_files(p);
                p.result_message = Some(("Rebased onto main".to_string(), false));
            }
        }
        RebaseOutcome::UpToDate => {
            if let Some(ref mut p) = app.git_actions_panel {
                p.result_message = Some(("Already up to date with main".to_string(), false));
            }
        }
        RebaseOutcome::Conflict { conflicted, auto_merged, .. } => {
            // Show conflict overlay — rebase is still in progress, RCR can resolve.
            // continue_with_merge=false since this was a manual rebase, not squash merge.
            if let Some(ref mut p) = app.git_actions_panel {
                p.conflict_overlay = Some(GitConflictOverlay {
                    conflicted_files: conflicted,
                    auto_merged_files: auto_merged,
                    scroll: 0,
                    selected: 0,
                    continue_with_merge: false,
                });
            }
        }
        RebaseOutcome::Failed(e) => {
            if let Some(ref mut p) = app.git_actions_panel {
                p.result_message = Some((format!("Rebase failed: {}", e), true));
            }
        }
    }
}

/// Pull latest changes from remote (for main branch)
fn exec_pull(app: &mut App) {
    let wt = match app.git_actions_panel.as_ref() {
        Some(p) => p.worktree_path.clone(),
        None => return,
    };
    let msg = match Git::pull(&wt) {
        Ok(m) => {
            let summary = m.lines().next().unwrap_or(&m);
            (format!("Pulled: {}", summary), false)
        }
        Err(e) => (format!("{}", e), true),
    };
    if let Some(ref mut p) = app.git_actions_panel {
        p.result_message = Some(msg);
        refresh_changed_files(p);
    }
}

/// Push the current worktree branch to remote
fn exec_push(app: &mut App) {
    let wt = match app.git_actions_panel.as_ref() {
        Some(p) => p.worktree_path.clone(),
        None => return,
    };
    let msg = match Git::push(&wt) {
        Ok(m) => {
            let summary = m.lines().next().unwrap_or(&m);
            (format!("Pushed: {}", summary), false)
        }
        Err(e) => (format!("{}", e), true),
    };
    if let Some(ref mut p) = app.git_actions_panel {
        p.result_message = Some(msg);
    }
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

/// Handle input while the conflict resolution overlay is open.
/// j/k or Up/Down navigate between "Resolve with Claude" and "Abort rebase".
/// Enter/y resolves, n/Esc aborts the rebase and closes the overlay.
fn handle_conflict_overlay(key: event::KeyEvent, app: &mut App, claude_process: &ClaudeProcess) -> Result<()> {
    use crossterm::event::{KeyCode, KeyModifiers};

    // Extract what we need before the mutable borrow dance
    let (sel, wt_path, repo_root, branch, conflicted, auto_merged, continue_merge) = match app.git_actions_panel.as_ref() {
        Some(p) => match p.conflict_overlay.as_ref() {
            Some(ov) => (
                ov.selected, p.worktree_path.clone(), p.repo_root.clone(),
                p.worktree_name.clone(), ov.conflicted_files.clone(), ov.auto_merged_files.clone(),
                ov.continue_with_merge,
            ),
            None => return Ok(()),
        },
        None => return Ok(()),
    };

    match (key.modifiers, key.code) {
        // Navigate between the two options
        (KeyModifiers::NONE, KeyCode::Char('j')) | (KeyModifiers::NONE, KeyCode::Down) => {
            if let Some(ref mut p) = app.git_actions_panel {
                if let Some(ref mut ov) = p.conflict_overlay {
                    if ov.selected < 1 { ov.selected = 1; }
                }
            }
        }
        (KeyModifiers::NONE, KeyCode::Char('k')) | (KeyModifiers::NONE, KeyCode::Up) => {
            if let Some(ref mut p) = app.git_actions_panel {
                if let Some(ref mut ov) = p.conflict_overlay {
                    if ov.selected > 0 { ov.selected = 0; }
                }
            }
        }

        // Enter — execute selected action
        (KeyModifiers::NONE, KeyCode::Enter) => {
            if sel == 0 {
                spawn_conflict_claude(app, claude_process, &wt_path, &repo_root, &branch, &conflicted, &auto_merged, continue_merge);
            } else {
                abort_rebase(app, &wt_path);
            }
        }

        // y — quick shortcut to resolve with Claude
        (KeyModifiers::NONE, KeyCode::Char('y')) => {
            spawn_conflict_claude(app, claude_process, &wt_path, &repo_root, &branch, &conflicted, &auto_merged, continue_merge);
        }

        // n or Esc — abort rebase and close overlay
        (KeyModifiers::NONE, KeyCode::Char('n')) | (KeyModifiers::NONE, KeyCode::Esc) => {
            abort_rebase(app, &wt_path);
        }

        _ => {}
    }
    Ok(())
}

/// Abort an in-progress rebase on the feature branch, close the overlay
fn abort_rebase(app: &mut App, wt_path: &std::path::Path) {
    let _ = Git::rebase_abort(wt_path);
    if let Some(ref mut p) = app.git_actions_panel {
        p.conflict_overlay = None;
        p.result_message = Some(("Rebase aborted".into(), false));
        refresh_changed_files(p);
    }
}

/// Spawn a streaming Claude session to resolve rebase conflicts on the
/// feature branch worktree. Claude runs in the worktree directory and uses
/// `git add` + `git rebase --continue` to complete the rebase.
fn spawn_conflict_claude(
    app: &mut App,
    claude_process: &ClaudeProcess,
    wt_path: &std::path::Path,
    repo_root: &std::path::Path,
    branch: &str,
    conflicted: &[String],
    auto_merged: &[String],
    continue_with_merge: bool,
) {
    // Build a prompt describing the rebase conflict state
    let display = branch.strip_prefix("azureal/").unwrap_or(branch);
    let mut prompt = format!(
        "Rebasing branch '{}' onto main produced merge conflicts.\n\
         Git left the working directory in a partially-rebased state.\n\n",
        display
    );
    prompt.push_str(&format!("Conflicted files ({}):\n", conflicted.len()));
    for f in conflicted { prompt.push_str(&format!("  - {}\n", f)); }
    if !auto_merged.is_empty() {
        prompt.push_str(&format!("\nAuto-merged cleanly ({}):\n", auto_merged.len()));
        for f in auto_merged { prompt.push_str(&format!("  - {}\n", f)); }
    }
    prompt.push_str(
        "\nResolve all conflicts:\n\
         1. Read each conflicted file — look for <<<<<<< / ======= / >>>>>>> markers\n\
         2. Edit each file to keep the correct combined content, removing all markers\n\
         3. Stage resolved files: git add <files>\n\
         4. Continue the rebase: git rebase --continue\n\
         5. If more conflicts appear, repeat steps 1-4 until the rebase completes\n\
         6. Verify with: git status\n\n\
         Ask me if any conflict is ambiguous."
    );

    match claude_process.spawn(wt_path, &prompt, None) {
        Ok((rx, pid)) => {
            let slot = pid.to_string();
            app.pending_session_names.push((slot.clone(), format!("[RCR] {}", display)));
            // Register under feature branch so output appears in the current view
            app.register_claude(branch.to_string(), pid, rx);
            // Enter RCR mode — green borders, routed prompts, approval dialog on exit
            app.rcr_session = Some(RcrSession {
                branch: branch.to_string(),
                display_name: display.to_string(),
                worktree_path: wt_path.to_path_buf(),
                repo_root: repo_root.to_path_buf(),
                slot_id: slot,
                session_id: None,
                approval_pending: false,
                continue_with_merge,
            });
            app.title_session_name = format!("[RCR] {}", display);
            // Clear convo pane so RCR starts as a visually fresh session
            app.display_events.clear();
            app.output_lines.clear();
            app.output_buffer.clear();
            app.output_scroll = usize::MAX;
            app.session_file_parse_offset = 0;
            app.rendered_events_count = 0;
            app.rendered_content_line_count = 0;
            app.rendered_events_start = 0;
            app.event_parser = crate::events::EventParser::new();
            app.selected_event = None;
            app.pending_tool_calls.clear();
            app.failed_tool_calls.clear();
            app.session_tokens = None;
            app.model_context_window = None;
            app.token_badge_cache = None;
            app.current_todos.clear();
            app.subagent_todos.clear();
            app.active_task_tool_ids.clear();
            app.subagent_parent_idx = None;
            app.awaiting_ask_user_question = false;
            app.ask_user_questions_cache = None;
            app.invalidate_render_cache();
            // Close the git panel and focus on output so user sees the convo
            app.git_actions_panel = None;
            app.focus = Focus::Output;
        }
        Err(e) => {
            if let Some(ref mut p) = app.git_actions_panel {
                p.conflict_overlay = None;
                p.result_message = Some((format!("Failed to spawn Claude: {}", e), true));
            }
        }
    }
}
