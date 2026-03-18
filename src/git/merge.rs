//! Git squash-merge operations
//!
//! Squash-merge a feature branch into main, with conflict detection and cleanup.

use anyhow::{Context, Result};
use std::path::Path;
use std::process::Command;

use super::{Git, SquashMergeResult};

impl Git {
    /// Squash-merge a feature branch into main:
    /// 1. Pull main from remote (so we merge onto the latest upstream)
    /// 2. Squash-merge the branch (collapses all commits into one staged changeset)
    /// 3. On success: commit with a clean message → `SquashMergeResult::Success`
    /// 4. On conflict: return structured conflict info → `SquashMergeResult::Conflict`
    /// Push happens automatically after success (callers call `Git::push()`).
    /// Runs from the repo root (main worktree, already on main branch).
    pub fn squash_merge_into_main(
        repo_root: &Path,
        branch_name: &str,
    ) -> Result<SquashMergeResult> {
        // Pre-flight: clean up any leftover merge/rebase state on main from
        // a previous operation that was interrupted (app crash, force-quit, etc.).
        // Without this, `git merge --squash` fails with "unmerged files" errors.
        // Only act when actual merge/rebase state exists (MERGE_HEAD, rebase-merge/,
        // rebase-apply/) — UU files alone could be legitimate in-progress work.
        let git_dir = repo_root.join(".git");
        let has_merge_state = git_dir.join("MERGE_HEAD").exists();
        let has_rebase_state =
            git_dir.join("rebase-merge").exists() || git_dir.join("rebase-apply").exists();
        if has_merge_state {
            let _ = Command::new("git")
                .args(["merge", "--abort"])
                .current_dir(repo_root)
                .output();
        }
        if has_rebase_state {
            let _ = Command::new("git")
                .args(["rebase", "--abort"])
                .current_dir(repo_root)
                .output();
        }
        // Remove stale SQUASH_MSG from a previous failed squash merge —
        // git leaves this file behind even when there was nothing to commit
        let _ = std::fs::remove_file(git_dir.join("SQUASH_MSG"));

        // Check for unmerged files that block `git merge --squash`.
        // Covers ALL unmerged porcelain patterns: UU, AA, DD, AU, UA, DU, UD.
        // Uses `git reset --hard HEAD` which is safe because we're about to
        // overwrite main with the squash merge anyway, and local changes get
        // stashed in the next step.
        if Self::has_unmerged_files(repo_root) {
            let _ = Command::new("git")
                .args(["reset", "--hard", "HEAD"])
                .current_dir(repo_root)
                .output();
        }

        // Step 0: stash any dirty working tree on main (e.g. .DS_Store, editor
        // swap files) so `git merge --squash` doesn't fail with "your local
        // changes would be overwritten". Pop unconditionally after merge/commit.
        let stash_out = Command::new("git")
            .args(["stash", "--include-untracked"])
            .current_dir(repo_root)
            .output();
        let did_stash = stash_out
            .as_ref()
            .ok()
            .map(|o| {
                o.status.success()
                    && !String::from_utf8_lossy(&o.stdout).contains("No local changes")
            })
            .unwrap_or(false);

        // Step 1: pull main so we're merging onto the latest upstream.
        // --ff-only prevents accidental merge commits on main itself.
        // Always non-fatal — the feature branch was already rebased onto main
        // by exec_squash_merge(), so even if pull fails (offline, diverged local
        // main from unpushed merges, no remote), the squash merge will still work.
        let pull_out = Command::new("git")
            .args(["pull", "--ff-only"])
            .current_dir(repo_root)
            .output();
        let pull_note = match pull_out {
            Ok(ref o) if !o.status.success() => " (pull skipped)",
            Err(_) => " (pull skipped)",
            _ => "",
        };

        // Collect individual commit messages before squash (they'll be lost after).
        // `git log main..branch --reverse --format="- %s"` gives each commit as a bullet.
        let commit_log = Command::new("git")
            .args([
                "log",
                &format!("HEAD..{}", branch_name),
                "--reverse",
                "--format=- %s",
            ])
            .current_dir(repo_root)
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .unwrap_or_default();

        // Step 2: squash-merge stages all changes without committing
        let merge_out = Command::new("git")
            .args(["merge", "--squash", branch_name])
            .current_dir(repo_root)
            .output()
            .context("Failed to squash merge")?;
        let combined = format!(
            "{}{}",
            String::from_utf8_lossy(&merge_out.stdout),
            String::from_utf8_lossy(&merge_out.stderr)
        );
        let text = combined.trim();

        // Conflict detected — return structured info instead of bailing
        if !merge_out.status.success() {
            let mut conflicted = Vec::new();
            let mut auto_merged = Vec::new();
            for line in text.lines() {
                if line.starts_with("CONFLICT") {
                    // Extract file path from various CONFLICT formats:
                    // "CONFLICT (content): Merge conflict in <path>"
                    // "CONFLICT (add/add): Merge conflict in <path>"
                    if let Some(path) = line.rsplit("Merge conflict in ").next() {
                        conflicted.push(path.trim().to_string());
                    } else {
                        conflicted.push(line.to_string());
                    }
                } else if let Some(path) = line.strip_prefix("Auto-merging ") {
                    auto_merged.push(path.trim().to_string());
                }
            }
            // If we parsed CONFLICT lines, return structured result.
            // Don't pop stash here — merge state is dirty; stash pop would
            // conflict. The stash survives merge_abort() and gets popped by
            // whatever resolves the conflict (or by the user manually).
            if !conflicted.is_empty() {
                return Ok(SquashMergeResult::Conflict {
                    conflicted,
                    auto_merged,
                    _raw_output: text.to_string(),
                });
            }
            // Non-conflict failure — restore stash before bailing
            if did_stash {
                let _ = Command::new("git")
                    .args(["stash", "pop"])
                    .current_dir(repo_root)
                    .output();
            }
            anyhow::bail!("{}", text);
        }

        // Step 3: commit the squashed changes with a rich message.
        // Summary line + individual commit messages as bullet points.
        let display = crate::models::strip_branch_prefix(branch_name);
        let message = if commit_log.is_empty() {
            format!("feat: merge {} into main", display)
        } else {
            format!("feat: merge {} into main\n\n{}", display, commit_log)
        };
        let commit_out = Command::new("git")
            .args(["commit", "-m", &message])
            .current_dir(repo_root)
            .output()
            .context("Failed to commit squash merge")?;
        if !commit_out.status.success() {
            // git commit prints "nothing to commit" to STDOUT, not stderr
            let out = String::from_utf8_lossy(&commit_out.stdout);
            let err = String::from_utf8_lossy(&commit_out.stderr);
            if did_stash {
                let _ = Command::new("git")
                    .args(["stash", "pop"])
                    .current_dir(repo_root)
                    .output();
            }
            if out.contains("nothing to commit") || err.contains("nothing to commit") {
                // Clean up the SQUASH_MSG that git leaves behind
                let _ = std::fs::remove_file(repo_root.join(".git/SQUASH_MSG"));
                return Ok(SquashMergeResult::Success(
                    "Already up to date — nothing to merge".into(),
                ));
            }
            anyhow::bail!("Squash merge staged but commit failed: {}", err.trim());
        }

        // Restore any stashed changes now that merge+commit is complete
        if did_stash {
            let _ = Command::new("git")
                .args(["stash", "pop"])
                .current_dir(repo_root)
                .output();
        }

        let out = String::from_utf8_lossy(&commit_out.stdout)
            .trim()
            .to_string();
        let first = out.lines().next().unwrap_or(&out);
        Ok(SquashMergeResult::Success(format!(
            "Merged: {}{}",
            first, pull_note
        )))
    }

    /// Check if a worktree has unmerged files in the index.
    /// Detects ALL porcelain v1 unmerged patterns: UU, AA, DD, AU, UA, DU, UD.
    pub fn has_unmerged_files(path: &Path) -> bool {
        let status = Command::new("git")
            .args(["status", "--porcelain"])
            .current_dir(path)
            .output()
            .ok();
        status
            .as_ref()
            .map(|o| {
                String::from_utf8_lossy(&o.stdout).lines().any(|l| {
                    let b = l.as_bytes();
                    // Unmerged entries have U in either column, or AA/DD
                    matches!(
                        b,
                        [b'U', _, ..] | [_, b'U', ..] | [b'A', b'A', ..] | [b'D', b'D', ..]
                    )
                })
            })
            .unwrap_or(false)
    }

    /// Clean up leftover squash merge state on main (or any worktree).
    /// `git merge --squash` does NOT create MERGE_HEAD, so `merge --abort`
    /// won't work. Instead, reset the index and working tree to HEAD to
    /// clear unmerged entries, then pop any stash that was pushed during
    /// `squash_merge_into_main()`.
    pub fn cleanup_squash_merge_state(repo_root: &Path) {
        if Self::has_unmerged_files(repo_root) {
            let _ = Command::new("git")
                .args(["reset", "--hard", "HEAD"])
                .current_dir(repo_root)
                .output();
            // Pop stash that was pushed at the start of squash_merge_into_main()
            let _ = Command::new("git")
                .args(["stash", "pop"])
                .current_dir(repo_root)
                .output();
        }
        // Clean up SQUASH_MSG file that git leaves behind
        let _ = std::fs::remove_file(repo_root.join(".git/SQUASH_MSG"));
    }
}
