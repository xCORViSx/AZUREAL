//! Git operations executed from the Git Actions panel.
//!
//! Pull, push, rebase, squash-merge, commit start, and data refresh functions.
//! Each operation updates the panel's result_message on success/failure.
//! Auto-resolve logic uses union merge for configured files.

use crate::app::App;
use crate::app::types::{BackgroundRebaseOutcome, GitActionsPanel, GitChangedFile, GitCommitOverlay};
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

    // Stash any dirty working tree (.DS_Store, swap files, etc.) so rebase
    // doesn't fail with "You have unstaged changes".
    let stashed = std::process::Command::new("git")
        .args(["stash", "--include-untracked"])
        .current_dir(worktree_path)
        .output()
        .ok()
        .map(|o| {
            let msg = String::from_utf8_lossy(&o.stdout);
            o.status.success() && !msg.contains("No local changes")
        })
        .unwrap_or(false);

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
            Err(e) => {
                if stashed { let _ = std::process::Command::new("git").args(["stash", "pop"]).current_dir(worktree_path).output(); }
                return RebaseOutcome::Failed(e.to_string());
            }
        }
    } else {
        match std::process::Command::new("git")
            .args(["rebase", main_branch])
            .current_dir(worktree_path)
            .output()
        {
            Ok(o) => o,
            Err(e) => {
                if stashed { let _ = std::process::Command::new("git").args(["stash", "pop"]).current_dir(worktree_path).output(); }
                return RebaseOutcome::Failed(e.to_string());
            }
        }
    };

    if output.status.success() {
        if stashed { let _ = std::process::Command::new("git").args(["stash", "pop"]).current_dir(worktree_path).output(); }
        return RebaseOutcome::Rebased;
    }

    let combined = format!("{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
    let text = combined.trim();
    if text.contains("CONFLICT") || text.contains("could not apply") {
        let (conflicted, auto_merged) = parse_conflict_files(text, worktree_path);

        // Auto-resolve via union merge if ALL conflicts are in the auto-resolve list
        if let Some(outcome) = try_auto_resolve_conflicts(worktree_path, &conflicted, auto_resolve_files) {
            if stashed { let _ = std::process::Command::new("git").args(["stash", "pop"]).current_dir(worktree_path).output(); }
            return outcome;
        }

        // Non-auto-resolve conflicts — leave rebase in progress for RCR resolution.
        // Stash stays; popped after RCR accept/abort.
        return RebaseOutcome::Conflict { conflicted, auto_merged, _raw_output: text.to_string() };
    }

    if stashed { let _ = std::process::Command::new("git").args(["stash", "pop"]).current_dir(worktree_path).output(); }
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
    let (tx, rx) = std::sync::mpsc::channel();
    app.loading_indicator = Some("Rebasing onto main...".into());
    app.rebase_op_receiver = Some(rx);
    std::thread::spawn(move || {
        let outcome = match exec_rebase_inner(&wt_path, &main_branch, &ar_files) {
            RebaseOutcome::Rebased => {
                let push_note = match Git::push(&wt_path) {
                    Ok(_) => " → pushed".to_string(),
                    Err(e) => format!(" (push failed: {})", e),
                };
                BackgroundRebaseOutcome::Rebased(format!("Rebased onto main{}", push_note))
            }
            RebaseOutcome::UpToDate => BackgroundRebaseOutcome::UpToDate,
            RebaseOutcome::Conflict { conflicted, auto_merged, .. } => {
                BackgroundRebaseOutcome::Conflict { conflicted, auto_merged }
            }
            RebaseOutcome::Failed(e) => BackgroundRebaseOutcome::Failed(e),
        };
        let _ = tx.send(outcome);
    });
}

/// Squash-merge the current worktree's branch into main. Validates synchronously,
/// then spawns a background thread for the heavy work (rebase, push, merge, push).
/// Progress updates flow through `panel.squash_merge_receiver` and are polled
/// in the event loop to show real-time phase messages in the loading dialog.
pub(super) fn exec_squash_merge(app: &mut App) {
    use std::sync::mpsc;
    use crate::app::types::{SquashMergeProgress, SquashMergeOutcome};

    let (repo_root, branch, wt_path, main_branch, ar_files) = match app.git_actions_panel.as_ref() {
        Some(p) => (p.repo_root.clone(), p.worktree_name.clone(), p.worktree_path.clone(), p.main_branch.clone(), p.auto_resolve_files.clone()),
        None => return,
    };
    // Dirty check is fast — keep synchronous
    if let Some(ref mut p) = app.git_actions_panel {
        if !p.changed_files.is_empty() {
            p.result_message = Some(("Commit your changes first (c) before squash merging".into(), true));
            return;
        }
    }

    let (tx, rx) = mpsc::channel::<SquashMergeProgress>();
    if let Some(ref mut p) = app.git_actions_panel {
        p.squash_merge_receiver = Some(rx);
    }
    app.loading_indicator = Some("Rebasing onto main...".into());

    let display = crate::models::strip_branch_prefix(&branch).to_string();
    std::thread::spawn(move || {
        // Phase 1: Rebase
        let _ = tx.send(SquashMergeProgress {
            phase: "Rebasing onto main...".into(),
            outcome: None,
        });

        match exec_rebase_inner(&wt_path, &main_branch, &ar_files) {
            RebaseOutcome::Conflict { conflicted, auto_merged, .. } => {
                let _ = tx.send(SquashMergeProgress {
                    phase: String::new(),
                    outcome: Some(SquashMergeOutcome::Conflict { conflicted, auto_merged }),
                });
                return;
            }
            RebaseOutcome::Failed(msg) => {
                let _ = tx.send(SquashMergeProgress {
                    phase: String::new(),
                    outcome: Some(SquashMergeOutcome::Failed(format!("Rebase failed: {}", msg))),
                });
                return;
            }
            RebaseOutcome::Rebased | RebaseOutcome::UpToDate => {}
        }

        // Phase 2: Push rebased branch
        let _ = tx.send(SquashMergeProgress {
            phase: "Pushing rebased branch...".into(),
            outcome: None,
        });
        let branch_push_note = match Git::push(&wt_path) {
            Ok(_) => String::new(),
            Err(e) => format!(" (branch push failed: {})", e),
        };

        // Phase 3: Squash merge into main
        let _ = tx.send(SquashMergeProgress {
            phase: "Merging into main...".into(),
            outcome: None,
        });
        match Git::squash_merge_into_main(&repo_root, &branch) {
            Ok(SquashMergeResult::Success(msg)) => {
                // Phase 4: Push main
                let _ = tx.send(SquashMergeProgress {
                    phase: "Pushing to remote...".into(),
                    outcome: None,
                });
                let main_push_note = match Git::push(&repo_root) {
                    Ok(_) => " → pushed".to_string(),
                    Err(e) => format!(" (main push failed: {})", e),
                };
                // Fast-forward feature branch to main so divergence indicators reset
                let _ = std::process::Command::new("git")
                    .args(["reset", "--hard", &main_branch])
                    .current_dir(&wt_path)
                    .output();
                let _ = tx.send(SquashMergeProgress {
                    phase: String::new(),
                    outcome: Some(SquashMergeOutcome::Success {
                        status_msg: format!("{}{}{}", msg, main_push_note, branch_push_note),
                        branch: branch.clone(),
                        display_name: display,
                        worktree_path: wt_path,
                    }),
                });
            }
            Ok(SquashMergeResult::Conflict { conflicted, auto_merged, .. }) => {
                let _ = tx.send(SquashMergeProgress {
                    phase: String::new(),
                    outcome: Some(SquashMergeOutcome::Conflict { conflicted, auto_merged }),
                });
            }
            Err(e) => {
                let _ = tx.send(SquashMergeProgress {
                    phase: String::new(),
                    outcome: Some(SquashMergeOutcome::Failed(format!("{}", e))),
                });
            }
        }
    });
}

/// Pull latest changes from remote (for main branch)
pub(super) fn exec_pull(app: &mut App) {
    use std::sync::mpsc;
    use crate::app::types::{BackgroundOpProgress, BackgroundOpOutcome};
    let wt = match app.git_actions_panel.as_ref() {
        Some(p) => p.worktree_path.clone(),
        None => return,
    };
    let (tx, rx) = mpsc::channel();
    app.loading_indicator = Some("Pulling from remote...".into());
    app.background_op_receiver = Some(rx);
    std::thread::spawn(move || {
        let (message, is_error) = match Git::pull(&wt) {
            Ok(m) => {
                let summary = m.lines().next().unwrap_or(&m).to_string();
                (format!("Pulled: {}", summary), false)
            }
            Err(e) => (format!("{}", e), true),
        };
        let _ = tx.send(BackgroundOpProgress {
            phase: String::new(),
            outcome: Some(BackgroundOpOutcome::GitResult { message, is_error }),
        });
    });
}

/// Push the current worktree branch to remote
pub(super) fn exec_push(app: &mut App) {
    use std::sync::mpsc;
    use crate::app::types::{BackgroundOpProgress, BackgroundOpOutcome};
    let wt = match app.git_actions_panel.as_ref() {
        Some(p) => p.worktree_path.clone(),
        None => return,
    };
    let (tx, rx) = mpsc::channel();
    app.loading_indicator = Some("Pushing to remote...".into());
    app.background_op_receiver = Some(rx);
    std::thread::spawn(move || {
        let (message, is_error) = match Git::push(&wt) {
            Ok(m) => {
                let summary = m.lines().next().unwrap_or(&m).to_string();
                (format!("Pushed: {}", summary), false)
            }
            Err(e) => (format!("{}", e), true),
        };
        let _ = tx.send(BackgroundOpProgress {
            phase: String::new(),
            outcome: Some(BackgroundOpOutcome::GitResult { message, is_error }),
        });
    });
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::types::PostMergeDialog;

    // ══════════════════════════════════════════════════════════════════
    //  RebaseOutcome enum
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn rebase_outcome_rebased() {
        let outcome = RebaseOutcome::Rebased;
        assert!(matches!(outcome, RebaseOutcome::Rebased));
    }

    #[test]
    fn rebase_outcome_up_to_date() {
        let outcome = RebaseOutcome::UpToDate;
        assert!(matches!(outcome, RebaseOutcome::UpToDate));
    }

    #[test]
    fn rebase_outcome_conflict() {
        let outcome = RebaseOutcome::Conflict {
            conflicted: vec!["a.rs".into()],
            auto_merged: vec!["b.rs".into()],
            _raw_output: "output".into(),
        };
        assert!(matches!(outcome, RebaseOutcome::Conflict { .. }));
    }

    #[test]
    fn rebase_outcome_failed() {
        let outcome = RebaseOutcome::Failed("error".into());
        assert!(matches!(outcome, RebaseOutcome::Failed(_)));
    }

    #[test]
    fn rebase_outcome_conflict_fields() {
        let outcome = RebaseOutcome::Conflict {
            conflicted: vec!["x.rs".into(), "y.rs".into()],
            auto_merged: vec![],
            _raw_output: "raw".into(),
        };
        if let RebaseOutcome::Conflict { conflicted, auto_merged, _raw_output } = outcome {
            assert_eq!(conflicted.len(), 2);
            assert!(auto_merged.is_empty());
            assert_eq!(_raw_output, "raw");
        }
    }

    #[test]
    fn rebase_outcome_failed_message() {
        let outcome = RebaseOutcome::Failed("fatal: invalid upstream".into());
        if let RebaseOutcome::Failed(msg) = outcome {
            assert!(msg.contains("fatal"));
        }
    }

    // ══════════════════════════════════════════════════════════════════
    //  parse_conflict_files
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn parse_conflict_files_basic_conflict() {
        let text = "CONFLICT (content): Merge conflict in src/main.rs\nAuto-merging Cargo.lock";
        let (conflicted, auto_merged) = parse_conflict_files(text, std::path::Path::new("/nonexistent"));
        assert_eq!(conflicted, vec!["src/main.rs"]);
        assert_eq!(auto_merged, vec!["Cargo.lock"]);
    }

    #[test]
    fn parse_conflict_files_no_conflicts() {
        // When no CONFLICT lines, parse_conflict_files returns empty (Git::get_conflicted_files
        // would fail on nonexistent path, but the Vec is still populated or empty)
        let text = "Auto-merging Cargo.lock\nAlready up to date.";
        let (_conflicted, auto_merged) = parse_conflict_files(text, std::path::Path::new("/nonexistent"));
        assert_eq!(auto_merged, vec!["Cargo.lock"]);
    }

    #[test]
    fn parse_conflict_files_multiple_conflicts() {
        let text = "CONFLICT (content): Merge conflict in a.rs\nCONFLICT (content): Merge conflict in b.rs";
        let (conflicted, _) = parse_conflict_files(text, std::path::Path::new("/nonexistent"));
        assert_eq!(conflicted, vec!["a.rs", "b.rs"]);
    }

    #[test]
    fn parse_conflict_files_multiple_auto_merged() {
        let text = "Auto-merging x.rs\nAuto-merging y.rs";
        let (_, auto_merged) = parse_conflict_files(text, std::path::Path::new("/nonexistent"));
        assert_eq!(auto_merged, vec!["x.rs", "y.rs"]);
    }

    #[test]
    fn parse_conflict_files_empty_text() {
        let text = "";
        let (conflicted, auto_merged) = parse_conflict_files(text, std::path::Path::new("/nonexistent"));
        // conflicted may be empty or populated by Git::get_conflicted_files fallback
        // auto_merged is definitely empty
        assert!(auto_merged.is_empty());
        // conflicted is empty from text parsing (fallback fails on nonexistent path)
        let _ = conflicted;
    }

    #[test]
    fn parse_conflict_files_conflict_without_merge_prefix() {
        // Lines starting with CONFLICT but without "Merge conflict in"
        let text = "CONFLICT (rename/delete): some_file.rs";
        let (conflicted, _) = parse_conflict_files(text, std::path::Path::new("/nonexistent"));
        // Should still capture something (the whole line as fallback or the rsplit result)
        assert!(!conflicted.is_empty());
    }

    // ══════════════════════════════════════════════════════════════════
    //  try_auto_resolve_conflicts — logic checks (no git)
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn auto_resolve_empty_conflicted_returns_none() {
        let conflicted: Vec<String> = vec![];
        let ar_files = vec!["Cargo.lock".to_string()];
        let all_resolvable = !conflicted.is_empty()
            && conflicted.iter().all(|f| ar_files.iter().any(|af| af == f));
        assert!(!all_resolvable);
    }

    #[test]
    fn auto_resolve_all_in_list() {
        let conflicted = vec!["Cargo.lock".to_string()];
        let ar_files = vec!["Cargo.lock".to_string()];
        let all_resolvable = !conflicted.is_empty()
            && conflicted.iter().all(|f| ar_files.iter().any(|af| af == f));
        assert!(all_resolvable);
    }

    #[test]
    fn auto_resolve_not_all_in_list() {
        let conflicted = vec!["Cargo.lock".to_string(), "src/main.rs".to_string()];
        let ar_files = vec!["Cargo.lock".to_string()];
        let all_resolvable = !conflicted.is_empty()
            && conflicted.iter().all(|f| ar_files.iter().any(|af| af == f));
        assert!(!all_resolvable);
    }

    #[test]
    fn auto_resolve_empty_ar_files() {
        let conflicted = vec!["a.rs".to_string()];
        let ar_files: Vec<String> = vec![];
        let all_resolvable = !conflicted.is_empty()
            && conflicted.iter().all(|f| ar_files.iter().any(|af| af == f));
        assert!(!all_resolvable);
    }

    // ══════════════════════════════════════════════════════════════════
    //  GitConflictOverlay construction
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn conflict_overlay_construction_rebase() {
        let ov = GitConflictOverlay {
            conflicted_files: vec!["a.rs".into()],
            auto_merged_files: vec![],
            scroll: 0, selected: 0, continue_with_merge: false,
        };
        assert!(!ov.continue_with_merge);
    }

    #[test]
    fn conflict_overlay_construction_merge() {
        let ov = GitConflictOverlay {
            conflicted_files: vec![], auto_merged_files: vec![],
            scroll: 0, selected: 0, continue_with_merge: true,
        };
        assert!(ov.continue_with_merge);
    }

    // ══════════════════════════════════════════════════════════════════
    //  GitCommitOverlay construction
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn commit_overlay_initial_state() {
        let ov = GitCommitOverlay {
            message: String::new(), cursor: 0, generating: true, scroll: 0, receiver: None,
        };
        assert!(ov.generating);
        assert!(ov.message.is_empty());
        assert!(ov.receiver.is_none());
    }

    // ══════════════════════════════════════════════════════════════════
    //  GitChangedFile construction
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn changed_file_from_tuple() {
        let (path, status, add, del) = ("src/lib.rs".to_string(), 'M', 5usize, 3usize);
        let f = GitChangedFile { path, status, additions: add, deletions: del };
        assert_eq!(f.path, "src/lib.rs");
        assert_eq!(f.additions, 5);
        assert_eq!(f.deletions, 3);
    }

    // ══════════════════════════════════════════════════════════════════
    //  Pull/Push message formatting
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn pull_message_summary_first_line() {
        let msg = "Already up to date.\nSome details";
        let summary = msg.lines().next().unwrap_or(msg);
        assert_eq!(summary, "Already up to date.");
    }

    #[test]
    fn push_message_summary_first_line() {
        let msg = "Everything up-to-date";
        let summary = msg.lines().next().unwrap_or(msg);
        assert_eq!(summary, "Everything up-to-date");
    }

    #[test]
    fn pull_message_format() {
        let summary = "Updated main";
        let result = format!("Pulled: {}", summary);
        assert_eq!(result, "Pulled: Updated main");
    }

    #[test]
    fn push_message_format() {
        let summary = "To origin";
        let result = format!("Pushed: {}", summary);
        assert_eq!(result, "Pushed: To origin");
    }

    // ══════════════════════════════════════════════════════════════════
    //  selected_file bounds adjustment
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn selected_file_clamp_when_list_shrinks() {
        let mut selected = 5usize;
        let new_len = 3usize;
        if selected >= new_len { selected = new_len.saturating_sub(1); }
        assert_eq!(selected, 2);
    }

    #[test]
    fn selected_file_stays_when_in_bounds() {
        let mut selected = 2usize;
        let new_len = 5usize;
        if selected >= new_len { selected = new_len.saturating_sub(1); }
        assert_eq!(selected, 2);
    }

    #[test]
    fn selected_file_empty_list_saturating() {
        let new_len = 0usize;
        let selected = new_len.saturating_sub(1);
        assert_eq!(selected, 0);
    }

    // ══════════════════════════════════════════════════════════════════
    //  Commit prompt diff trimming
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn diff_trimmed_under_max() {
        let diff = "short diff";
        let max = 30_000;
        let trimmed = if diff.len() > max { &diff[..max] } else { diff };
        assert_eq!(trimmed, "short diff");
    }

    #[test]
    fn diff_trimmed_over_max() {
        let diff = "x".repeat(40_000);
        let max = 30_000;
        let trimmed = if diff.len() > max { &diff[..max] } else { &diff };
        assert_eq!(trimmed.len(), 30_000);
    }

    // ══════════════════════════════════════════════════════════════════
    //  log_main computation for feature vs main
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn log_main_none_for_main_branch() {
        let is_on_main = true;
        let main_branch = "main";
        let log_main: Option<&str> = if is_on_main { None } else { Some(main_branch) };
        assert!(log_main.is_none());
    }

    #[test]
    fn log_main_some_for_feature_branch() {
        let is_on_main = false;
        let main_branch = "main";
        let log_main: Option<&str> = if is_on_main { None } else { Some(main_branch) };
        assert_eq!(log_main, Some("main"));
    }

    // ══════════════════════════════════════════════════════════════════
    //  PostMergeDialog construction
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn post_merge_dialog_construction() {
        let pmd = PostMergeDialog {
            branch: "feature/test".into(),
            display_name: "test".into(),
            worktree_path: std::path::PathBuf::from("/tmp/wt"),
            selected: 0,
        };
        assert_eq!(pmd.branch, "feature/test");
        assert_eq!(pmd.display_name, "test");
        assert_eq!(pmd.selected, 0);
    }

    // ══════════════════════════════════════════════════════════════════
    //  Result message formatting
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn rebase_success_message() {
        let push_note = " -> pushed";
        let msg = format!("Rebased onto main{}", push_note);
        assert_eq!(msg, "Rebased onto main -> pushed");
    }

    #[test]
    fn rebase_up_to_date_message() {
        let msg = "Already up to date with main".to_string();
        assert!(msg.contains("up to date"));
    }

    #[test]
    fn rebase_failed_message() {
        let err = "fatal: error";
        let msg = format!("Rebase failed: {}", err);
        assert!(msg.starts_with("Rebase failed:"));
    }

    // ══════════════════════════════════════════════════════════════════
    //  parse_conflict_files — additional edge cases
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn parse_conflict_files_mixed_lines() {
        let text = "Some output\nCONFLICT (content): Merge conflict in foo.rs\nAuto-merging bar.rs\nOther output";
        let (conflicted, auto_merged) = parse_conflict_files(text, std::path::Path::new("/nonexistent"));
        assert!(conflicted.contains(&"foo.rs".to_string()));
        assert!(auto_merged.contains(&"bar.rs".to_string()));
    }

    #[test]
    fn parse_conflict_files_only_auto_merged_no_conflict() {
        let text = "Auto-merging Cargo.lock\nAuto-merging Cargo.toml";
        let (_, auto_merged) = parse_conflict_files(text, std::path::Path::new("/nonexistent"));
        assert_eq!(auto_merged.len(), 2);
        assert!(auto_merged.contains(&"Cargo.lock".to_string()));
        assert!(auto_merged.contains(&"Cargo.toml".to_string()));
    }

    #[test]
    fn parse_conflict_files_auto_merge_trim_whitespace() {
        let text = "Auto-merging   spaced_file.rs  ";
        let (_, auto_merged) = parse_conflict_files(text, std::path::Path::new("/nonexistent"));
        // trim() is applied in the parse
        assert!(!auto_merged.is_empty());
    }

    #[test]
    fn parse_conflict_conflict_trim_whitespace() {
        let text = "CONFLICT (content): Merge conflict in   trimmed.rs  ";
        let (conflicted, _) = parse_conflict_files(text, std::path::Path::new("/nonexistent"));
        // rsplit("Merge conflict in ") gives the part after, then trim()
        assert!(!conflicted.is_empty());
        assert!(conflicted[0].contains("trimmed.rs"));
    }

    // ══════════════════════════════════════════════════════════════════
    //  Auto-resolve logic — boundary conditions
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn auto_resolve_multiple_all_in_list() {
        let conflicted = vec!["Cargo.lock".to_string(), "Cargo.toml".to_string()];
        let ar_files = vec!["Cargo.lock".to_string(), "Cargo.toml".to_string()];
        let all_resolvable = !conflicted.is_empty()
            && conflicted.iter().all(|f| ar_files.iter().any(|af| af == f));
        assert!(all_resolvable);
    }

    #[test]
    fn auto_resolve_single_extra_file_not_in_list() {
        let conflicted = vec!["Cargo.lock".to_string(), "src/lib.rs".to_string()];
        let ar_files = vec!["Cargo.lock".to_string()];
        let all_resolvable = !conflicted.is_empty()
            && conflicted.iter().all(|f| ar_files.iter().any(|af| af == f));
        assert!(!all_resolvable);
    }

    #[test]
    fn auto_resolve_exact_match_required() {
        let conflicted = vec!["Cargo.lock.bak".to_string()];
        let ar_files = vec!["Cargo.lock".to_string()];
        let all_resolvable = !conflicted.is_empty()
            && conflicted.iter().all(|f| ar_files.iter().any(|af| af == f));
        assert!(!all_resolvable);
    }

    // ══════════════════════════════════════════════════════════════════
    //  GitConflictOverlay — field-level tests
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn conflict_overlay_scroll_starts_at_zero() {
        let ov = GitConflictOverlay {
            conflicted_files: vec!["a.rs".into()],
            auto_merged_files: vec![],
            scroll: 0, selected: 0, continue_with_merge: false,
        };
        assert_eq!(ov.scroll, 0);
        assert_eq!(ov.selected, 0);
    }

    #[test]
    fn conflict_overlay_conflicted_files_list() {
        let files = vec!["src/a.rs".to_string(), "src/b.rs".to_string()];
        let ov = GitConflictOverlay {
            conflicted_files: files.clone(),
            auto_merged_files: vec![],
            scroll: 0, selected: 0, continue_with_merge: false,
        };
        assert_eq!(ov.conflicted_files, files);
    }

    #[test]
    fn conflict_overlay_auto_merged_files_list() {
        let files = vec!["Cargo.lock".to_string()];
        let ov = GitConflictOverlay {
            conflicted_files: vec![],
            auto_merged_files: files.clone(),
            scroll: 0, selected: 0, continue_with_merge: true,
        };
        assert_eq!(ov.auto_merged_files, files);
    }

    // ══════════════════════════════════════════════════════════════════
    //  GitCommitOverlay — cursor and scroll
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn commit_overlay_cursor_zero() {
        let ov = GitCommitOverlay {
            message: "feat: add feature".into(),
            cursor: 0,
            generating: false,
            scroll: 0,
            receiver: None,
        };
        assert_eq!(ov.cursor, 0);
    }

    #[test]
    fn commit_overlay_with_message() {
        let msg = "fix: resolve issue #42".to_string();
        let ov = GitCommitOverlay {
            message: msg.clone(),
            cursor: msg.len(),
            generating: false,
            scroll: 0,
            receiver: None,
        };
        assert_eq!(ov.message, msg);
        assert_eq!(ov.cursor, msg.len());
    }

    #[test]
    fn commit_overlay_not_generating() {
        let ov = GitCommitOverlay {
            message: "done".into(),
            cursor: 4,
            generating: false,
            scroll: 0,
            receiver: None,
        };
        assert!(!ov.generating);
    }

    // ══════════════════════════════════════════════════════════════════
    //  GitChangedFile — additional field tests
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn changed_file_added_status() {
        let f = GitChangedFile {
            path: "new_file.rs".into(),
            status: 'A',
            additions: 100,
            deletions: 0,
        };
        assert_eq!(f.status, 'A');
        assert_eq!(f.deletions, 0);
    }

    #[test]
    fn changed_file_deleted_status() {
        let f = GitChangedFile {
            path: "old_file.rs".into(),
            status: 'D',
            additions: 0,
            deletions: 50,
        };
        assert_eq!(f.status, 'D');
        assert_eq!(f.additions, 0);
    }

    // ══════════════════════════════════════════════════════════════════
    //  Commit prompt strip_prefix / strip_suffix markdown fence
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn commit_msg_strip_backtick_prefix() {
        let raw = "```feat: add thing\nbody here```";
        let msg = raw.strip_prefix("```").unwrap_or(raw);
        let msg = msg.strip_suffix("```").unwrap_or(msg).trim();
        assert_eq!(msg, "feat: add thing\nbody here");
    }

    #[test]
    fn commit_msg_no_backtick_unchanged() {
        let raw = "feat: add thing\nbody here";
        let msg = raw.strip_prefix("```").unwrap_or(raw);
        let msg = msg.strip_suffix("```").unwrap_or(msg).trim();
        assert_eq!(msg, "feat: add thing\nbody here");
    }

    // ══════════════════════════════════════════════════════════════════
    //  PostMergeDialog — selected index
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn post_merge_dialog_selected_starts_at_zero() {
        let pmd = PostMergeDialog {
            branch: "feat/x".into(),
            display_name: "x".into(),
            worktree_path: std::path::PathBuf::from("/tmp"),
            selected: 0,
        };
        assert_eq!(pmd.selected, 0);
    }

    #[test]
    fn post_merge_dialog_display_name() {
        let pmd = PostMergeDialog {
            branch: "feat/my-feature".into(),
            display_name: "my-feature".into(),
            worktree_path: std::path::PathBuf::from("/tmp/wt"),
            selected: 0,
        };
        assert_eq!(pmd.display_name, "my-feature");
    }

    // ══════════════════════════════════════════════════════════════════
    //  Divergence text formatting
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn squash_merge_push_note_empty() {
        let branch_push_note = String::new();
        assert!(branch_push_note.is_empty());
    }

    #[test]
    fn squash_merge_push_note_error() {
        let e = "Network unreachable";
        let note = format!(" (branch push failed: {})", e);
        assert!(note.contains("branch push failed"));
    }
}
