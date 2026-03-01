//! Git operations executed from the Git Actions panel.
//!
//! Pull, push, rebase, squash-merge, commit start, and data refresh functions.
//! Each operation updates the panel's result_message on success/failure.
//! Auto-resolve logic uses union merge for configured files.

use crate::app::App;
use crate::app::types::{GitActionsPanel, GitChangedFile, GitCommitOverlay, GitConflictOverlay, PostMergeDialog};
use crate::git::{Git, SquashMergeResult};

/// Rebase outcome for the UI to display
pub(crate) enum RebaseOutcome {
    Rebased,
    UpToDate,
    /// Conflict — rebase is left in progress (NOT aborted) so RCR can resolve.
    /// Contains (conflicted_files, auto_merged_files, raw_output).
    Conflict { conflicted: Vec<String>, auto_merged: Vec<String>, _raw_output: String },
    Failed(String),
}

/// Parse conflict and auto-merge file lists from git rebase/merge output.
fn parse_conflict_files(text: &str, worktree_path: &std::path::Path) -> (Vec<String>, Vec<String>) {
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
    if conflicted.is_empty() {
        if let Ok(diff) = Git::get_conflicted_files(worktree_path) {
            conflicted = diff;
        }
    }
    (conflicted, auto_merged)
}

/// Resolve a single conflicted file using `git merge-file --union`.
/// Extracts the 3 index stages (base, ours, theirs), runs union merge which
/// keeps BOTH sides' changes with no conflict markers, then stages the result.
fn union_merge_file(worktree_path: &std::path::Path, file: &str) -> bool {
    let tmp = std::env::temp_dir();
    let base_p = tmp.join("azureal_base");
    let ours_p = tmp.join("azureal_ours");
    let theirs_p = tmp.join("azureal_theirs");

    // Extract the 3 stages: :1 = base, :2 = ours (onto target), :3 = theirs (replayed commit)
    let base = std::process::Command::new("git")
        .args(["show", &format!(":1:{}", file)]).current_dir(worktree_path).output();
    let ours = std::process::Command::new("git")
        .args(["show", &format!(":2:{}", file)]).current_dir(worktree_path).output();
    let theirs = std::process::Command::new("git")
        .args(["show", &format!(":3:{}", file)]).current_dir(worktree_path).output();

    let (Ok(base), Ok(ours), Ok(theirs)) = (base, ours, theirs) else { return false };
    if !base.status.success() || !ours.status.success() || !theirs.status.success() { return false; }

    if std::fs::write(&base_p, &base.stdout).is_err() { return false; }
    if std::fs::write(&ours_p, &ours.stdout).is_err() { return false; }
    if std::fs::write(&theirs_p, &theirs.stdout).is_err() { return false; }

    // Union merge — modifies ours_p in place, exit 0 = clean, 1 = overlaps resolved
    let merge = std::process::Command::new("git")
        .args(["merge-file", "--union",
            ours_p.to_str().unwrap_or(""),
            base_p.to_str().unwrap_or(""),
            theirs_p.to_str().unwrap_or("")])
        .output();

    let ok = match merge {
        Ok(ref o) => o.status.code().map(|c| c <= 1).unwrap_or(false),
        Err(_) => false,
    };

    if ok {
        if let Ok(result) = std::fs::read(&ours_p) {
            let file_path = worktree_path.join(file);
            let _ = std::fs::write(&file_path, result);
            let _ = std::process::Command::new("git")
                .args(["add", "--", file]).current_dir(worktree_path).output();
        }
    }

    let _ = std::fs::remove_file(&base_p);
    let _ = std::fs::remove_file(&ours_p);
    let _ = std::fs::remove_file(&theirs_p);
    ok
}

/// Auto-resolve conflicts via union merge for files in the auto-resolve list.
/// Union merge keeps BOTH sides' changes — no content is lost, no conflict
/// markers are produced. Loops through subsequent commits that also have
/// auto-resolvable-only conflicts. Returns `Some(outcome)` if handled,
/// `None` if conflicts include files not in the auto-resolve list.
fn try_auto_resolve_conflicts(
    worktree_path: &std::path::Path,
    conflicted: &[String],
    auto_resolve_files: &[String],
) -> Option<RebaseOutcome> {
    let all_resolvable = !conflicted.is_empty()
        && conflicted.iter().all(|f| auto_resolve_files.iter().any(|af| af == f));
    if !all_resolvable { return None; }

    for file in conflicted {
        if !union_merge_file(worktree_path, file) { return None; }
    }

    // Continue rebase — loop in case the next commit also has auto-resolvable conflicts
    loop {
        let cont = std::process::Command::new("git")
            .args(["rebase", "--continue"])
            .env("GIT_EDITOR", "true")
            .current_dir(worktree_path)
            .output();
        match cont {
            Ok(o) if o.status.success() => return Some(RebaseOutcome::Rebased),
            Ok(o) => {
                let text = format!(
                    "{}{}",
                    String::from_utf8_lossy(&o.stdout),
                    String::from_utf8_lossy(&o.stderr),
                );
                let text = text.trim();
                if !text.contains("CONFLICT") && !text.contains("could not apply") {
                    return Some(RebaseOutcome::Failed(text.to_string()));
                }
                let (new_conflicts, new_auto) = parse_conflict_files(text, worktree_path);
                let still_resolvable = !new_conflicts.is_empty()
                    && new_conflicts.iter().all(|f| auto_resolve_files.iter().any(|af| af == f));
                if !still_resolvable {
                    return Some(RebaseOutcome::Conflict {
                        conflicted: new_conflicts,
                        auto_merged: new_auto,
                        _raw_output: text.to_string(),
                    });
                }
                for file in &new_conflicts {
                    if !union_merge_file(worktree_path, file) {
                        return Some(RebaseOutcome::Conflict {
                            conflicted: new_conflicts,
                            auto_merged: new_auto,
                            _raw_output: text.to_string(),
                        });
                    }
                }
            }
            Err(e) => return Some(RebaseOutcome::Failed(e.to_string())),
        }
    }
}

/// Inner rebase logic — used by both manual rebase (r), pre-merge rebase,
/// and auto-rebase. No git fetch — caller ensures main is current.
///
/// Conflicts in auto-resolve files are resolved via union merge (keeps both
/// sides' changes). Only non-auto-resolve conflicts require user intervention.
pub(crate) fn exec_rebase_inner(
    worktree_path: &std::path::Path,
    main_branch: &str,
    auto_resolve_files: &[String],
) -> RebaseOutcome {
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

    // --onto with explicit fork point replays ONLY branch-specific commits.
    // Plain `git rebase main` can replay squash merge commits from other
    // branches that ended up in this branch's history, causing hangs.
    let fork_point = base.as_ref().ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default();
    let output = if !fork_point.is_empty() {
        match std::process::Command::new("git")
            .args(["rebase", "--onto", main_branch, &fork_point])
            .current_dir(worktree_path)
            .output()
        {
            Ok(o) => o,
            Err(e) => return RebaseOutcome::Failed(e.to_string()),
        }
    } else {
        match std::process::Command::new("git")
            .args(["rebase", main_branch])
            .current_dir(worktree_path)
            .output()
        {
            Ok(o) => o,
            Err(e) => return RebaseOutcome::Failed(e.to_string()),
        }
    };

    if output.status.success() { return RebaseOutcome::Rebased; }

    let combined = format!("{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
    let text = combined.trim();
    if text.contains("CONFLICT") || text.contains("could not apply") {
        let (conflicted, auto_merged) = parse_conflict_files(text, worktree_path);

        // Auto-resolve via union merge if ALL conflicts are in the auto-resolve list
        if let Some(outcome) = try_auto_resolve_conflicts(worktree_path, &conflicted, auto_resolve_files) {
            return outcome;
        }

        // Non-auto-resolve conflicts — leave rebase in progress for RCR resolution
        return RebaseOutcome::Conflict { conflicted, auto_merged, _raw_output: text.to_string() };
    }

    RebaseOutcome::Failed(text.to_string())
}

/// Manual rebase action — rebase this worktree onto main (feature branches only).
/// On conflict, shows the conflict overlay with RCR option (rebase stays in progress).
pub(super) fn exec_rebase(app: &mut App) {
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
    let ar_files = match app.git_actions_panel.as_ref() {
        Some(p) => p.auto_resolve_files.clone(),
        None => return,
    };
    match exec_rebase_inner(&wt_path, &main_branch, &ar_files) {
        RebaseOutcome::Rebased => {
            // Push the rebased branch to its remote
            let push_note = match Git::push(&wt_path) {
                Ok(_) => " → pushed".to_string(),
                Err(e) => format!(" (push failed: {})", e),
            };
            if let Some(ref mut p) = app.git_actions_panel {
                super::refresh_changed_files(p);
                super::refresh_commit_log(p);
                p.result_message = Some((format!("Rebased onto main{}", push_note), false));
            }
        }
        RebaseOutcome::UpToDate => {
            if let Some(ref mut p) = app.git_actions_panel {
                p.result_message = Some(("Already up to date with main".to_string(), false));
            }
        }
        RebaseOutcome::Conflict { conflicted, auto_merged, .. } => {
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

/// Squash-merge the current worktree's branch into main. Rebases the feature
/// branch onto main first to ensure a clean, linear merge. On success, shows
/// result message. On conflict (rebase or merge), opens the conflict overlay.
pub(super) fn exec_squash_merge(app: &mut App) {
    let (repo_root, branch, wt_path, main_branch, ar_files) = match app.git_actions_panel.as_ref() {
        Some(p) => (p.repo_root.clone(), p.worktree_name.clone(), p.worktree_path.clone(), p.main_branch.clone(), p.auto_resolve_files.clone()),
        None => return,
    };
    if let Some(ref mut p) = app.git_actions_panel {
        if !p.changed_files.is_empty() {
            p.result_message = Some(("Commit your changes first (c) before squash merging".into(), true));
            return;
        }
    }

    match exec_rebase_inner(&wt_path, &main_branch, &ar_files) {
        RebaseOutcome::Conflict { conflicted, auto_merged, .. } => {
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
        RebaseOutcome::Rebased | RebaseOutcome::UpToDate => {}
    }

    // Push the rebased feature branch to its remote before merging
    let branch_push_note = match Git::push(&wt_path) {
        Ok(_) => String::new(),
        Err(e) => format!(" (branch push failed: {})", e),
    };

    match Git::squash_merge_into_main(&repo_root, &branch) {
        Ok(SquashMergeResult::Success(msg)) => {
            // Auto-push main to remote after successful squash merge
            let main_push_note = match Git::push(&repo_root) {
                Ok(_) => " → pushed".to_string(),
                Err(e) => format!(" (main push failed: {})", e),
            };
            // Fast-forward feature branch to main so divergence indicators reset
            // (squash merge creates a different commit, leaving the branch "ahead")
            let _ = std::process::Command::new("git")
                .args(["reset", "--hard", &main_branch])
                .current_dir(&wt_path)
                .output();
            let display = crate::models::strip_branch_prefix(&branch).to_string();
            app.git_actions_panel = None;
            app.post_merge_dialog = Some(PostMergeDialog {
                branch: branch.clone(),
                display_name: display,
                worktree_path: wt_path,
                selected: 0,
            });
            app.set_status(format!("{}{}{}", msg, main_push_note, branch_push_note));
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

/// Pull latest changes from remote (for main branch)
pub(super) fn exec_pull(app: &mut App) {
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
        super::refresh_changed_files(p);
    }
}

/// Push the current worktree branch to remote
pub(super) fn exec_push(app: &mut App) {
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

/// Start the commit flow: stage all changes, get the diff, spawn Claude one-shot
/// to generate a commit message, and open the commit overlay.
pub(super) fn exec_commit_start(app: &mut App) {
    let wt = match app.git_actions_panel.as_ref() {
        Some(p) => p.worktree_path.clone(),
        None => return,
    };

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

    let stat = Git::get_staged_stat(&wt).unwrap_or_default();

    let claude_bin = crate::azufig::load_global_azufig()
        .config.claude_executable
        .unwrap_or_else(|| "claude".into());

    let (tx, rx) = std::sync::mpsc::channel();
    let wt_clone = wt.clone();
    std::thread::spawn(move || {
        let max_diff = 30_000;
        let diff_trimmed = if diff.len() > max_diff { &diff[..max_diff] } else { &diff };
        let prompt = format!(
            "Write a conventional commit message for this diff. Format: type: short description (under 72 chars) on the first line, then a blank line, then optional bullet points for details. Types: feat, fix, refactor, docs, test, chore. Output ONLY the commit message, nothing else.\n\n--- stat ---\n{}\n--- diff ---\n{}",
            stat, diff_trimmed
        );
        let result = std::process::Command::new(&claude_bin)
            .args(["-p", "--no-session-persistence", &prompt])
            .current_dir(&wt_clone)
            .output();

        match result {
            Ok(output) if output.status.success() => {
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

/// Re-fetch the commit log from git (called after commit/push operations)
pub(crate) fn refresh_commit_log(panel: &mut GitActionsPanel) {
    let log_main = if panel.is_on_main { None } else { Some(panel.main_branch.as_str()) };
    match Git::get_commit_log(&panel.worktree_path, 200, log_main) {
        Ok(entries) => {
            panel.commits = entries.into_iter().map(|(hash, full_hash, subject, is_pushed)| {
                crate::app::types::GitCommit { hash, full_hash, subject, is_pushed }
            }).collect();
            if panel.selected_commit >= panel.commits.len() {
                panel.selected_commit = panel.commits.len().saturating_sub(1);
            }
        }
        Err(_) => { panel.commits.clear(); panel.selected_commit = 0; }
    }
    if !panel.is_on_main {
        let (behind, ahead) = Git::get_main_divergence(&panel.worktree_path, &panel.main_branch);
        panel.commits_behind_main = behind;
        panel.commits_ahead_main = ahead;
    }
    let (rb, ra) = Git::get_remote_divergence(&panel.worktree_path);
    panel.commits_behind_remote = rb;
    panel.commits_ahead_remote = ra;
}
