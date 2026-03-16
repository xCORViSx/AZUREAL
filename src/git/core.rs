//! Core Git operations
//!
//! Basic git operations like repo detection, branch info, and diffs.

use anyhow::{Context, Result};
use std::path::Path;
use std::process::Command;

use crate::models::DiffInfo;

/// Result of a squash merge attempt — distinguishes clean success from
/// partial-conflict scenarios that need interactive resolution.
pub enum SquashMergeResult {
    /// Clean merge completed and committed successfully
    Success(String),
    /// Conflicts detected — repo left in dirty merge state on main.
    /// User must resolve conflicts or abort before main is usable.
    Conflict {
        /// Files with CONFLICT markers that need manual resolution
        conflicted: Vec<String>,
        /// Files that git auto-merged without issues
        auto_merged: Vec<String>,
        /// Full raw git output from the merge command (populated by deserialization)
        _raw_output: String,
    },
}

/// Worktree info from git
#[derive(Debug, Clone)]
pub struct WorktreeInfo {
    pub path: std::path::PathBuf,
    pub branch: Option<String>,
    pub _commit: String,
    pub is_main: bool,
}

/// Git operations for worktree management
pub struct Git;

impl Git {
    /// Check if a directory is a git repository
    pub fn is_git_repo(path: &Path) -> bool {
        Command::new("git")
            .args(["rev-parse", "--git-dir"])
            .current_dir(path)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    /// Get the root path of the git repository
    pub fn repo_root(path: &Path) -> Result<std::path::PathBuf> {
        // Use --git-common-dir to resolve to the MAIN repo root, not the
        // worktree root. --show-toplevel returns the worktree's own directory
        // when run from a worktree, which breaks project/worktree discovery.
        let output = Command::new("git")
            .args(["rev-parse", "--path-format=absolute", "--git-common-dir"])
            .current_dir(path)
            .output()
            .context("Failed to get repo root")?;

        if output.status.success() {
            let git_dir = std::path::PathBuf::from(String::from_utf8_lossy(&output.stdout).trim());
            // --git-common-dir returns the .git directory; parent is the repo root
            Ok(git_dir.parent().unwrap_or(&git_dir).to_path_buf())
        } else {
            anyhow::bail!("Not in a git repository")
        }
    }

    /// List all prefixed branches (for archived session detection).
    /// Includes both local branches and remote branches (from origin).
    /// Remote branches appear as archived worktrees when no local checkout exists.
    pub fn list_azureal_branches(repo_path: &Path) -> Result<Vec<String>> {
        let pattern = format!("{}/*", crate::models::BRANCH_PREFIX);

        // Local branches: azureal/*
        let local_output = Command::new("git")
            .args(["branch", "--list", &pattern, "--format=%(refname:short)"])
            .current_dir(repo_path)
            .output()
            .context("Failed to list local branches")?;

        let mut branches: Vec<String> = String::from_utf8_lossy(&local_output.stdout)
            .lines()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();

        // Remote branches: origin/azureal/* (strip origin/ prefix to get branch name)
        let remote_pattern = format!("origin/{}/*", crate::models::BRANCH_PREFIX);
        let remote_output = Command::new("git")
            .args([
                "branch",
                "-r",
                "--list",
                &remote_pattern,
                "--format=%(refname:short)",
            ])
            .current_dir(repo_path)
            .output()
            .context("Failed to list remote branches")?;

        for line in String::from_utf8_lossy(&remote_output.stdout).lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            // Strip "origin/" prefix to get the bare branch name
            let branch_name = trimmed.strip_prefix("origin/").unwrap_or(trimmed);
            // Only add if not already present as a local branch
            if !branches.contains(&branch_name.to_string()) {
                branches.push(branch_name.to_string());
            }
        }

        Ok(branches)
    }

    /// Prune stale branch refs from other machines.
    /// 1. Prunes remote-tracking refs that no longer exist on origin.
    /// 2. Deletes local azureal/* branches that are fully merged to main
    ///    AND have no remote counterpart (deleted on another machine).
    /// Best-effort: silently ignored if offline or no remote configured.
    pub fn prune_remote_refs(repo_path: &Path) {
        // Prune stale origin/* refs
        let _ = Command::new("git")
            .args(["remote", "prune", "origin"])
            .current_dir(repo_path)
            .output();

        // Find local azureal/* branches with no remote counterpart
        let prefix = crate::models::BRANCH_PREFIX;
        let pattern = format!("{}/*", prefix);
        let local = Command::new("git")
            .args(["branch", "--list", &pattern, "--format=%(refname:short)"])
            .current_dir(repo_path)
            .output();
        let remote = Command::new("git")
            .args([
                "branch",
                "-r",
                "--list",
                &format!("origin/{}/*", prefix),
                "--format=%(refname:short)",
            ])
            .current_dir(repo_path)
            .output();
        let (Ok(local), Ok(remote)) = (local, remote) else {
            return;
        };
        let remote_names: std::collections::HashSet<String> =
            String::from_utf8_lossy(&remote.stdout)
                .lines()
                .filter_map(|l| l.trim().strip_prefix("origin/"))
                .map(|s| s.to_string())
                .collect();
        for line in String::from_utf8_lossy(&local.stdout).lines() {
            let branch = line.trim();
            if branch.is_empty() || remote_names.contains(branch) {
                continue;
            }
            // Only delete if fully merged to main (safe — no unmerged work lost)
            let merged = Command::new("git")
                .args(["branch", "--merged", "main", "--list", branch])
                .current_dir(repo_path)
                .output()
                .map(|o| !String::from_utf8_lossy(&o.stdout).trim().is_empty())
                .unwrap_or(false);
            if merged {
                let _ = Command::new("git")
                    .args(["branch", "-d", branch])
                    .current_dir(repo_path)
                    .output();
            }
        }
    }

    /// Get the main branch name (main or master)
    pub fn get_main_branch(repo_path: &Path) -> Result<String> {
        for branch in ["main", "master"] {
            let output = Command::new("git")
                .args(["rev-parse", "--verify", branch])
                .current_dir(repo_path)
                .output()?;

            if output.status.success() {
                return Ok(branch.to_string());
            }
        }

        let output = Command::new("git")
            .args(["rev-parse", "--abbrev-ref", "HEAD"])
            .current_dir(repo_path)
            .output()?;

        if output.status.success() {
            return Ok(String::from_utf8_lossy(&output.stdout).trim().to_string());
        }

        Ok("main".to_string())
    }

    /// Get the diff between worktree and main branch
    pub fn get_diff(worktree_path: &Path, main_branch: &str) -> Result<DiffInfo> {
        let diff_output = Command::new("git")
            .args(["diff", &format!("{}...HEAD", main_branch)])
            .current_dir(worktree_path)
            .output()
            .context("Failed to get git diff")?;

        let diff_text = String::from_utf8_lossy(&diff_output.stdout).to_string();

        let stats_output = Command::new("git")
            .args(["diff", "--stat", &format!("{}...HEAD", main_branch)])
            .current_dir(worktree_path)
            .output()
            .context("Failed to get git diff stats")?;

        let stats_text = String::from_utf8_lossy(&stats_output.stdout);

        let mut files_changed = Vec::new();
        let mut additions = 0;
        let mut deletions = 0;

        for line in stats_text.lines() {
            if line.contains('|') {
                if let Some(file) = line.split('|').next() {
                    files_changed.push(file.trim().to_string());
                }
            } else if line.contains("insertion") || line.contains("deletion") {
                for part in line.split(',') {
                    let part = part.trim();
                    if part.contains("insertion") {
                        if let Some(num) = part.split_whitespace().next() {
                            additions = num.parse().unwrap_or(0);
                        }
                    } else if part.contains("deletion") {
                        if let Some(num) = part.split_whitespace().next() {
                            deletions = num.parse().unwrap_or(0);
                        }
                    }
                }
            }
        }

        Ok(DiffInfo {
            session_id: String::new(),
            diff_text,
            files_changed,
            additions,
            deletions,
            timestamp: chrono::Utc::now(),
        })
    }

    /// Get the git directory for a worktree
    pub(crate) fn get_git_dir(worktree_path: &Path) -> Option<std::path::PathBuf> {
        let output = Command::new("git")
            .args(["rev-parse", "--git-dir"])
            .current_dir(worktree_path)
            .output()
            .ok()?;

        if output.status.success() {
            let path_str = String::from_utf8_lossy(&output.stdout).trim().to_string();
            let path = std::path::PathBuf::from(&path_str);
            if path.is_absolute() {
                Some(path)
            } else {
                Some(worktree_path.join(path))
            }
        } else {
            None
        }
    }

    /// Get short status
    pub fn status(worktree_path: &Path) -> Result<String> {
        let output = Command::new("git")
            .args(["status", "--short"])
            .current_dir(worktree_path)
            .output()
            .context("Failed to get git status")?;

        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }

    /// Get per-file diff stats against main branch.
    /// Returns Vec<(path, status_char, additions, deletions, staged)> by combining
    /// `git diff --name-status` (M/A/D/R) with `git diff --numstat` (+/-).
    /// The `staged` bool is true if the file has staged changes (in the index).
    pub fn get_diff_files(
        worktree_path: &Path,
        _main_branch: &str,
    ) -> Result<Vec<(String, char, usize, usize, bool)>> {
        // Show working tree changes (staged + unstaged) — this is what the user
        // is actively working on. Uses `git diff HEAD` to compare working tree
        // against last commit, capturing both staged and unstaged modifications.
        // Untracked files added separately via `git ls-files --others --exclude-standard`.

        // M\tpath — status of each changed file vs HEAD
        let status_out = Command::new("git")
            .args(["diff", "HEAD", "--name-status"])
            .current_dir(worktree_path)
            .output()
            .context("Failed to get diff name-status")?;
        let status_text = String::from_utf8_lossy(&status_out.stdout);

        // add\tdel\tpath — line-level stats for each changed file
        let numstat_out = Command::new("git")
            .args(["diff", "HEAD", "--numstat"])
            .current_dir(worktree_path)
            .output()
            .context("Failed to get diff numstat")?;
        let numstat_text = String::from_utf8_lossy(&numstat_out.stdout);

        // Build path → (additions, deletions) lookup from numstat
        let mut stats: std::collections::HashMap<String, (usize, usize)> =
            std::collections::HashMap::new();
        for line in numstat_text.lines() {
            let parts: Vec<&str> = line.split('\t').collect();
            if parts.len() >= 3 {
                let add = parts[0].parse().unwrap_or(0);
                let del = parts[1].parse().unwrap_or(0);
                stats.insert(parts[2].to_string(), (add, del));
            }
        }

        let mut result = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for line in status_text.lines() {
            let parts: Vec<&str> = line.split('\t').collect();
            if parts.len() >= 2 {
                let status = parts[0].chars().next().unwrap_or('M');
                let path = parts.last().unwrap().to_string();
                let (add, del) = stats.get(&path).copied().unwrap_or((0, 0));
                seen.insert(path.clone());
                // Default all files to staged=true; user unstages explicitly via UI
                result.push((path, status, add, del, true));
            }
        }

        // Also pick up untracked files (shown as '?' status, 0/0 stats, never staged)
        let untracked_out = Command::new("git")
            .args(["ls-files", "--others", "--exclude-standard"])
            .current_dir(worktree_path)
            .output()
            .context("Failed to list untracked files")?;
        for line in String::from_utf8_lossy(&untracked_out.stdout).lines() {
            let path = line.trim().to_string();
            if !path.is_empty() && !seen.contains(&path) {
                result.push((path, '?', 0, 0, true));
            }
        }

        // Filter out gitignored files — tracked files in .gitignore still appear
        // in `git diff HEAD` but are noise the user doesn't want to see
        if !result.is_empty() {
            let paths: Vec<&str> = result.iter().map(|(p, ..)| p.as_str()).collect();
            let mut child = Command::new("git")
                .args(["check-ignore", "--no-index", "--stdin"])
                .current_dir(worktree_path)
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::piped())
                .spawn()
                .context("Failed to spawn git check-ignore")?;
            if let Some(mut stdin) = child.stdin.take() {
                use std::io::Write;
                let _ = stdin.write_all(paths.join("\n").as_bytes());
            }
            let ignore_out = child
                .wait_with_output()
                .context("git check-ignore failed")?;
            let ignored: std::collections::HashSet<&str> = std::str::from_utf8(&ignore_out.stdout)
                .unwrap_or("")
                .lines()
                .map(|l| l.trim())
                .filter(|l| !l.is_empty())
                .collect();
            result.retain(|(path, ..)| !ignored.contains(path.as_str()));
        }

        Ok(result)
    }

    /// Get the diff for a single file (working tree vs HEAD, for viewer display)
    pub fn get_file_diff(
        worktree_path: &Path,
        _main_branch: &str,
        file_path: &str,
    ) -> Result<String> {
        let output = Command::new("git")
            .args(["diff", "HEAD", "--", file_path])
            .current_dir(worktree_path)
            .output()
            .context("Failed to get file diff")?;
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }

    /// Pull from remote (current branch's upstream, or origin/<branch> if no upstream set)
    pub fn pull(worktree_path: &Path) -> Result<String> {
        let has_upstream = Command::new("git")
            .args(["rev-parse", "--abbrev-ref", "--symbolic-full-name", "@{u}"])
            .current_dir(worktree_path)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);

        let output = if has_upstream {
            Command::new("git")
                .args(["pull"])
                .current_dir(worktree_path)
                .output()
                .context("Failed to pull")?
        } else {
            let branch = Command::new("git")
                .args(["rev-parse", "--abbrev-ref", "HEAD"])
                .current_dir(worktree_path)
                .output()
                .context("Failed to get branch name")?;
            let branch_name = String::from_utf8_lossy(&branch.stdout).trim().to_string();
            Command::new("git")
                .args(["pull", "origin", &branch_name])
                .current_dir(worktree_path)
                .output()
                .context("Failed to pull")?
        };

        let combined = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        if output.status.success() {
            Ok(combined.trim().to_string())
        } else {
            anyhow::bail!("{}", combined.trim())
        }
    }

    /// Push current branch to remote (auto-sets upstream on first push)
    pub fn push(worktree_path: &Path) -> Result<String> {
        let branch = Command::new("git")
            .args(["rev-parse", "--abbrev-ref", "HEAD"])
            .current_dir(worktree_path)
            .output()
            .context("Failed to get branch name")?;
        let branch_name = String::from_utf8_lossy(&branch.stdout).trim().to_string();

        // Check if local branch has diverged from remote (e.g. after rebase).
        // `rev-list --left-right --count` returns "<ahead>\t<behind>".
        // If behind > 0 AND ahead > 0, the histories have diverged — force-with-lease is needed.
        let diverged = Command::new("git")
            .args([
                "rev-list",
                "--left-right",
                "--count",
                &format!("HEAD...origin/{}", branch_name),
            ])
            .current_dir(worktree_path)
            .output()
            .ok()
            .and_then(|o| {
                if !o.status.success() {
                    return None;
                }
                let s = String::from_utf8_lossy(&o.stdout).trim().to_string();
                let parts: Vec<&str> = s.split('\t').collect();
                if parts.len() == 2 {
                    let ahead = parts[0].parse::<u64>().unwrap_or(0);
                    let behind = parts[1].parse::<u64>().unwrap_or(0);
                    Some(ahead > 0 && behind > 0)
                } else {
                    None
                }
            })
            .unwrap_or(false);

        if !diverged {
            // Normal case: pull --rebase first to integrate remote changes.
            // Non-fatal: if offline or no upstream yet, skip silently and let push create it.
            let pull = Command::new("git")
                .args(["pull", "--rebase", "origin", &branch_name])
                .current_dir(worktree_path)
                .output();
            if let Ok(ref o) = pull {
                if !o.status.success() {
                    let msg = String::from_utf8_lossy(&o.stderr);
                    if msg.contains("CONFLICT") || msg.contains("could not apply") {
                        anyhow::bail!(
                            "Pull rebase failed with conflicts — resolve manually then push"
                        );
                    }
                }
            }
        }

        // Use --force-with-lease when diverged (post-rebase), regular push otherwise
        let push_args = if diverged {
            vec!["push", "--force-with-lease", "-u", "origin", &branch_name]
        } else {
            vec!["push", "-u", "origin", &branch_name]
        };

        let output = Command::new("git")
            .args(&push_args)
            .current_dir(worktree_path)
            .output()
            .context("Failed to push")?;

        let combined = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        if output.status.success() {
            let suffix = if diverged { " (force-pushed)" } else { "" };
            Ok(format!("{}{}", combined.trim(), suffix))
        } else {
            anyhow::bail!("{}", combined.trim())
        }
    }

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

    /// Stage all changes (tracked + untracked) via `git add -A`, then
    /// untrack any files that match `.gitignore` patterns. `git add -A`
    /// stages modifications to already-tracked files even if they're in
    /// `.gitignore` — this guard removes them from the index so they
    /// never end up in commits.
    pub fn stage_all(worktree_path: &Path) -> Result<()> {
        let output = Command::new("git")
            .args(["add", "-A"])
            .current_dir(worktree_path)
            .output()
            .context("Failed to stage changes")?;
        if !output.status.success() {
            anyhow::bail!("{}", String::from_utf8_lossy(&output.stderr).trim());
        }
        Self::untrack_gitignored_files(worktree_path);
        Ok(())
    }

    /// Stage a single file via `git add <path>`
    pub fn stage_file(worktree_path: &Path, file_path: &str) -> Result<()> {
        let output = Command::new("git")
            .args(["add", "--", file_path])
            .current_dir(worktree_path)
            .output()
            .context("Failed to stage file")?;
        if !output.status.success() {
            anyhow::bail!("{}", String::from_utf8_lossy(&output.stderr).trim());
        }
        Ok(())
    }

    /// Discard working tree changes for a single file (revert to HEAD).
    /// For untracked files, uses `git clean -f <path>` instead.
    pub fn discard_file(worktree_path: &Path, file_path: &str, is_untracked: bool) -> Result<()> {
        if is_untracked {
            let output = Command::new("git")
                .args(["clean", "-f", "--", file_path])
                .current_dir(worktree_path)
                .output()
                .context("Failed to clean untracked file")?;
            if !output.status.success() {
                anyhow::bail!("{}", String::from_utf8_lossy(&output.stderr).trim());
            }
        } else {
            // Unstage first (if staged), then restore working tree
            let _ = Command::new("git")
                .args(["restore", "--staged", "--", file_path])
                .current_dir(worktree_path)
                .output();
            let output = Command::new("git")
                .args(["restore", "--", file_path])
                .current_dir(worktree_path)
                .output()
                .context("Failed to discard file changes")?;
            if !output.status.success() {
                anyhow::bail!("{}", String::from_utf8_lossy(&output.stderr).trim());
            }
        }
        Ok(())
    }

    /// Unstage all files via `git reset HEAD`
    pub fn unstage_all(worktree_path: &Path) -> Result<()> {
        let output = Command::new("git")
            .args(["reset", "HEAD"])
            .current_dir(worktree_path)
            .output()
            .context("Failed to unstage all")?;
        if !output.status.success() {
            anyhow::bail!("{}", String::from_utf8_lossy(&output.stderr).trim());
        }
        Ok(())
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

    /// Find tracked files that match `.gitignore` and remove them from the
    /// index (`git rm --cached`). Does NOT delete the working-tree copy.
    /// Silently no-ops if nothing matches or if any git command fails.
    pub fn untrack_gitignored_files(path: &Path) {
        let ls = Command::new("git")
            .args(["ls-files", "-i", "--exclude-standard"])
            .current_dir(path)
            .output();
        let files: Vec<String> = match ls {
            Ok(ref o) if o.status.success() => String::from_utf8_lossy(&o.stdout)
                .lines()
                .filter(|l| !l.trim().is_empty())
                .map(|l| l.to_string())
                .collect(),
            _ => return,
        };
        if files.is_empty() {
            return;
        }
        let mut cmd = Command::new("git");
        cmd.args(["rm", "--cached", "--quiet", "--"]);
        for f in &files {
            cmd.arg(f);
        }
        let _ = cmd.current_dir(path).output();
    }

    /// Ensure `worktrees/` is listed in the project's `.gitignore`.
    /// If missing, appends it, stages `.gitignore`, and commits.
    /// Silently no-ops if already present or if any step fails.
    /// Entries that must be in .gitignore for azureal to work correctly.
    /// Each tuple: (canonical form to write, all accepted variants).
    const REQUIRED_GITIGNORE: &[(&str, &[&str])] = &[(
        "worktrees/",
        &["worktrees", "worktrees/", "/worktrees", "/worktrees/"],
    )];

    pub fn ensure_worktrees_gitignored(repo_root: &Path) {
        let gitignore = repo_root.join(".gitignore");
        let content = std::fs::read_to_string(&gitignore).unwrap_or_default();

        // collect missing entries
        let mut missing: Vec<&str> = Vec::new();
        for (canonical, variants) in Self::REQUIRED_GITIGNORE {
            let covered = content.lines().any(|line| {
                let l = line.trim();
                variants.contains(&l)
            });
            if !covered {
                missing.push(canonical);
            }
        }
        if missing.is_empty() {
            return;
        }

        // append missing entries
        let mut new = content.clone();
        if !new.is_empty() && !new.ends_with('\n') {
            new.push('\n');
        }
        for entry in &missing {
            new.push_str(entry);
            new.push('\n');
        }
        if std::fs::write(&gitignore, &new).is_err() {
            return;
        }

        // stage + commit
        let staged = Command::new("git")
            .args(["add", ".gitignore"])
            .current_dir(repo_root)
            .output();
        if !staged.map(|o| o.status.success()).unwrap_or(false) {
            return;
        }

        let msg = format!("chore: gitignore {}", missing.join(", "));
        let _ = Command::new("git")
            .args(["commit", "-m", &msg])
            .current_dir(repo_root)
            .output();
    }

    /// Get the full diff of staged changes for commit message generation
    pub fn get_staged_diff(worktree_path: &Path) -> Result<String> {
        let output = Command::new("git")
            .args(["diff", "--staged"])
            .current_dir(worktree_path)
            .output()
            .context("Failed to get staged diff")?;
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }

    /// Get a short summary of staged changes (file count + stats)
    pub fn get_staged_stat(worktree_path: &Path) -> Result<String> {
        let output = Command::new("git")
            .args(["diff", "--staged", "--stat"])
            .current_dir(worktree_path)
            .output()
            .context("Failed to get staged stat")?;
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }

    /// Get recent commit log for the commits pane in the Git panel.
    /// Returns (short_hash, full_hash, subject, is_pushed) tuples.
    /// Unpushed commits (ahead of upstream) are marked `is_pushed=false`.
    pub fn get_commit_log(
        worktree_path: &Path,
        max_count: usize,
        main_branch: Option<&str>,
    ) -> Result<Vec<(String, String, String, bool)>> {
        // How many commits ahead of upstream? (0 if no upstream configured)
        let ahead = Command::new("git")
            .args(["rev-list", "--count", "@{u}..HEAD"])
            .current_dir(worktree_path)
            .output()
            .ok()
            .filter(|o| o.status.success())
            .and_then(|o| {
                String::from_utf8_lossy(&o.stdout)
                    .trim()
                    .parse::<usize>()
                    .ok()
            })
            .unwrap_or(0);

        // Feature branches: show only commits unique to this branch (main..HEAD)
        // Main branch or no main_branch provided: show full log from HEAD
        let max_arg = format!("--max-count={}", max_count);
        let range = main_branch.map(|m| format!("{}..HEAD", m));
        let mut args = vec!["log", &max_arg, "--format=%h\t%H\t%s"];
        if let Some(ref r) = range {
            args.push(r);
        }

        let output = Command::new("git")
            .args(&args)
            .current_dir(worktree_path)
            .output()
            .context("Failed to get commit log")?;

        let mut commits = Vec::new();
        for (i, line) in String::from_utf8_lossy(&output.stdout).lines().enumerate() {
            let parts: Vec<&str> = line.splitn(3, '\t').collect();
            if parts.len() >= 3 {
                commits.push((
                    parts[0].to_string(),
                    parts[1].to_string(),
                    parts[2].to_string(),
                    i >= ahead, // first `ahead` commits are unpushed
                ));
            }
        }
        Ok(commits)
    }

    /// Get divergence counts between two refs: (behind, ahead).
    /// `behind` = commits in `upstream` not in `local`, `ahead` = commits in `local` not in `upstream`.
    /// Uses `git rev-list --left-right --count upstream...local`.
    fn rev_list_divergence(worktree_path: &Path, upstream: &str, local: &str) -> (usize, usize) {
        Command::new("git")
            .args([
                "rev-list",
                "--left-right",
                "--count",
                &format!("{}...{}", upstream, local),
            ])
            .current_dir(worktree_path)
            .output()
            .ok()
            .filter(|o| o.status.success())
            .and_then(|o| {
                let s = String::from_utf8_lossy(&o.stdout);
                let mut parts = s.trim().split('\t');
                let behind = parts.next()?.parse::<usize>().ok()?;
                let ahead = parts.next()?.parse::<usize>().ok()?;
                Some((behind, ahead))
            })
            .unwrap_or((0, 0))
    }

    /// Get divergence from main: (behind_main, ahead_of_main).
    /// Returns (0, 0) on main branch or any error.
    pub fn get_main_divergence(worktree_path: &Path, main_branch: &str) -> (usize, usize) {
        Self::rev_list_divergence(worktree_path, main_branch, "HEAD")
    }

    /// Get divergence from remote tracking branch: (behind_remote, ahead_of_remote).
    /// Returns (0, 0) when no upstream is configured or any error.
    pub fn get_remote_divergence(worktree_path: &Path) -> (usize, usize) {
        // Resolve upstream tracking ref (e.g. "origin/feat-x")
        let upstream = Command::new("git")
            .args(["rev-parse", "--abbrev-ref", "--symbolic-full-name", "@{u}"])
            .current_dir(worktree_path)
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string());
        match upstream {
            Some(u) if !u.is_empty() => Self::rev_list_divergence(worktree_path, &u, "HEAD"),
            _ => (0, 0),
        }
    }

    /// Get full diff for a single commit (for the viewer pane in Git panel)
    pub fn get_commit_diff(worktree_path: &Path, hash: &str) -> Result<String> {
        let output = Command::new("git")
            .args(["show", hash, "--stat", "--patch"])
            .current_dir(worktree_path)
            .output()
            .context("Failed to get commit diff")?;
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }

    /// Commit staged changes with the given message
    pub fn commit(worktree_path: &Path, message: &str) -> Result<String> {
        let output = Command::new("git")
            .args(["commit", "-m", message])
            .current_dir(worktree_path)
            .output()
            .context("Failed to commit")?;
        let combined = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        if output.status.success() {
            Ok(combined.trim().to_string())
        } else {
            anyhow::bail!("{}", combined.trim())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    // ── SquashMergeResult: construction & pattern matching ──

    #[test]
    fn test_squash_merge_result_success_construction() {
        let result = SquashMergeResult::Success("Merged: abc123".to_string());
        assert!(matches!(result, SquashMergeResult::Success(_)));
    }

    #[test]
    fn test_squash_merge_result_success_extracts_message() {
        let result = SquashMergeResult::Success("Merged: abc123 feat".to_string());
        if let SquashMergeResult::Success(msg) = result {
            assert_eq!(msg, "Merged: abc123 feat");
        } else {
            panic!("expected Success variant");
        }
    }

    #[test]
    fn test_squash_merge_result_success_empty_message() {
        let result = SquashMergeResult::Success(String::new());
        if let SquashMergeResult::Success(msg) = result {
            assert!(msg.is_empty());
        } else {
            panic!("expected Success variant");
        }
    }

    #[test]
    fn test_squash_merge_result_conflict_construction() {
        let result = SquashMergeResult::Conflict {
            conflicted: vec!["src/main.rs".to_string()],
            auto_merged: vec!["Cargo.toml".to_string()],
            _raw_output: "CONFLICT (content): ...".to_string(),
        };
        assert!(matches!(result, SquashMergeResult::Conflict { .. }));
    }

    #[test]
    fn test_squash_merge_result_conflict_fields() {
        let result = SquashMergeResult::Conflict {
            conflicted: vec!["a.rs".to_string(), "b.rs".to_string()],
            auto_merged: vec!["c.rs".to_string()],
            _raw_output: "raw output here".to_string(),
        };
        if let SquashMergeResult::Conflict {
            conflicted,
            auto_merged,
            _raw_output,
        } = result
        {
            assert_eq!(conflicted.len(), 2);
            assert_eq!(conflicted[0], "a.rs");
            assert_eq!(conflicted[1], "b.rs");
            assert_eq!(auto_merged.len(), 1);
            assert_eq!(auto_merged[0], "c.rs");
            assert_eq!(_raw_output, "raw output here");
        } else {
            panic!("expected Conflict variant");
        }
    }

    #[test]
    fn test_squash_merge_result_conflict_empty_vecs() {
        let result = SquashMergeResult::Conflict {
            conflicted: vec![],
            auto_merged: vec![],
            _raw_output: String::new(),
        };
        if let SquashMergeResult::Conflict {
            conflicted,
            auto_merged,
            ..
        } = result
        {
            assert!(conflicted.is_empty());
            assert!(auto_merged.is_empty());
        } else {
            panic!("expected Conflict variant");
        }
    }

    #[test]
    fn test_squash_merge_result_conflict_many_files() {
        let conflicted: Vec<String> = (0..50).map(|i| format!("file_{}.rs", i)).collect();
        let auto_merged: Vec<String> = (0..30).map(|i| format!("auto_{}.rs", i)).collect();
        let result = SquashMergeResult::Conflict {
            conflicted: conflicted.clone(),
            auto_merged: auto_merged.clone(),
            _raw_output: "lots of output".to_string(),
        };
        if let SquashMergeResult::Conflict {
            conflicted: c,
            auto_merged: a,
            ..
        } = result
        {
            assert_eq!(c.len(), 50);
            assert_eq!(a.len(), 30);
            assert_eq!(c[49], "file_49.rs");
            assert_eq!(a[29], "auto_29.rs");
        } else {
            panic!("expected Conflict variant");
        }
    }

    #[test]
    fn test_squash_merge_result_success_not_conflict() {
        let result = SquashMergeResult::Success("ok".to_string());
        assert!(!matches!(result, SquashMergeResult::Conflict { .. }));
    }

    #[test]
    fn test_squash_merge_result_conflict_not_success() {
        let result = SquashMergeResult::Conflict {
            conflicted: vec!["x".to_string()],
            auto_merged: vec![],
            _raw_output: String::new(),
        };
        assert!(!matches!(result, SquashMergeResult::Success(_)));
    }

    #[test]
    fn test_squash_merge_result_success_with_pull_note() {
        let result = SquashMergeResult::Success("Merged: abc123 (pull skipped)".to_string());
        if let SquashMergeResult::Success(msg) = result {
            assert!(msg.contains("(pull skipped)"));
        } else {
            panic!("expected Success variant");
        }
    }

    #[test]
    fn test_squash_merge_result_success_already_up_to_date() {
        let result =
            SquashMergeResult::Success("Already up to date — nothing to merge".to_string());
        if let SquashMergeResult::Success(msg) = result {
            assert!(msg.contains("Already up to date"));
        } else {
            panic!("expected Success variant");
        }
    }

    // ── WorktreeInfo: construction & field access ──

    #[test]
    fn test_worktree_info_construction_with_branch() {
        let info = WorktreeInfo {
            path: PathBuf::from("/repo/worktrees/feature"),
            branch: Some("azureal/feature".to_string()),
            _commit: "abc1234".to_string(),
            is_main: false,
        };
        assert_eq!(info.path, PathBuf::from("/repo/worktrees/feature"));
        assert_eq!(info.branch.as_deref(), Some("azureal/feature"));
        assert_eq!(info._commit, "abc1234");
        assert!(!info.is_main);
    }

    #[test]
    fn test_worktree_info_construction_main() {
        let info = WorktreeInfo {
            path: PathBuf::from("/repo"),
            branch: Some("main".to_string()),
            _commit: "def5678".to_string(),
            is_main: true,
        };
        assert!(info.is_main);
        assert_eq!(info.branch.as_deref(), Some("main"));
    }

    #[test]
    fn test_worktree_info_construction_no_branch() {
        let info = WorktreeInfo {
            path: PathBuf::from("/repo/worktrees/detached"),
            branch: None,
            _commit: "deadbeef".to_string(),
            is_main: false,
        };
        assert!(info.branch.is_none());
    }

    #[test]
    fn test_worktree_info_clone() {
        let info = WorktreeInfo {
            path: PathBuf::from("/a/b/c"),
            branch: Some("feat".to_string()),
            _commit: "aaa".to_string(),
            is_main: false,
        };
        let cloned = info.clone();
        assert_eq!(info.path, cloned.path);
        assert_eq!(info.branch, cloned.branch);
        assert_eq!(info._commit, cloned._commit);
        assert_eq!(info.is_main, cloned.is_main);
    }

    #[test]
    fn test_worktree_info_debug() {
        let info = WorktreeInfo {
            path: PathBuf::from("/debug/test"),
            branch: Some("debug-branch".to_string()),
            _commit: "fff".to_string(),
            is_main: false,
        };
        let dbg = format!("{:?}", info);
        assert!(dbg.contains("WorktreeInfo"));
        assert!(dbg.contains("debug-branch"));
        assert!(dbg.contains("/debug/test"));
    }

    #[test]
    fn test_worktree_info_clone_with_none_branch() {
        let info = WorktreeInfo {
            path: PathBuf::from("/x"),
            branch: None,
            _commit: "000".to_string(),
            is_main: false,
        };
        let cloned = info.clone();
        assert!(cloned.branch.is_none());
    }

    #[test]
    fn test_worktree_info_path_components() {
        let info = WorktreeInfo {
            path: PathBuf::from("/home/user/project/worktrees/my-feature"),
            branch: Some("azureal/my-feature".to_string()),
            _commit: "123".to_string(),
            is_main: false,
        };
        assert_eq!(
            info.path.file_name().unwrap().to_str().unwrap(),
            "my-feature"
        );
        assert!(info.path.starts_with("/home/user/project"));
    }

    #[test]
    fn test_worktree_info_master_is_main() {
        let info = WorktreeInfo {
            path: PathBuf::from("/repo"),
            branch: Some("master".to_string()),
            _commit: "bbb".to_string(),
            is_main: true,
        };
        assert!(info.is_main);
        assert_eq!(info.branch.as_deref(), Some("master"));
    }

    #[test]
    fn test_worktree_info_empty_commit() {
        let info = WorktreeInfo {
            path: PathBuf::from("/empty"),
            branch: None,
            _commit: String::new(),
            is_main: false,
        };
        assert!(info._commit.is_empty());
    }

    #[test]
    fn test_worktree_info_long_commit_hash() {
        let info = WorktreeInfo {
            path: PathBuf::from("/repo"),
            branch: Some("main".to_string()),
            _commit: "abc1234567890abcdef1234567890abcdef12345".to_string(),
            is_main: true,
        };
        assert_eq!(info._commit.len(), 40);
    }

    // ── Git unit struct ──

    #[test]
    fn test_git_struct_exists() {
        let _git = Git;
    }

    #[test]
    fn test_git_struct_is_zero_sized() {
        assert_eq!(std::mem::size_of::<Git>(), 0);
    }

    // ── SquashMergeResult: variant discrimination via match ──

    #[test]
    fn test_squash_merge_result_match_success() {
        let result = SquashMergeResult::Success("done".to_string());
        let is_success = match result {
            SquashMergeResult::Success(_) => true,
            SquashMergeResult::Conflict { .. } => false,
        };
        assert!(is_success);
    }

    #[test]
    fn test_squash_merge_result_match_conflict() {
        let result = SquashMergeResult::Conflict {
            conflicted: vec!["f.rs".to_string()],
            auto_merged: vec![],
            _raw_output: "err".to_string(),
        };
        let is_conflict = match result {
            SquashMergeResult::Success(_) => false,
            SquashMergeResult::Conflict { .. } => true,
        };
        assert!(is_conflict);
    }

    // ── WorktreeInfo: variations on is_main ──

    #[test]
    fn test_worktree_info_is_main_false_for_feature() {
        let info = WorktreeInfo {
            path: PathBuf::from("/wt/feature"),
            branch: Some("azureal/feature".to_string()),
            _commit: "abc".to_string(),
            is_main: false,
        };
        assert!(!info.is_main);
    }

    #[test]
    fn test_worktree_info_is_main_true_even_without_main_branch_name() {
        // is_main can be true when path == repo_path, regardless of branch name
        let info = WorktreeInfo {
            path: PathBuf::from("/repo"),
            branch: Some("develop".to_string()),
            _commit: "ccc".to_string(),
            is_main: true,
        };
        assert!(info.is_main);
    }

    // ── SquashMergeResult: conflict file path patterns ──

    #[test]
    fn test_squash_merge_conflict_paths_with_subdirs() {
        let result = SquashMergeResult::Conflict {
            conflicted: vec![
                "src/git/core.rs".to_string(),
                "src/models.rs".to_string(),
                "tests/integration.rs".to_string(),
            ],
            auto_merged: vec!["Cargo.toml".to_string(), "Cargo.lock".to_string()],
            _raw_output: "multiple conflicts".to_string(),
        };
        if let SquashMergeResult::Conflict {
            conflicted,
            auto_merged,
            ..
        } = result
        {
            assert!(conflicted.iter().any(|f| f.contains("core.rs")));
            assert!(auto_merged.iter().any(|f| f == "Cargo.toml"));
        }
    }

    #[test]
    fn test_squash_merge_success_multiline_message() {
        let msg = "Merged: feat\n\n- commit 1\n- commit 2\n- commit 3".to_string();
        let result = SquashMergeResult::Success(msg.clone());
        if let SquashMergeResult::Success(m) = result {
            assert!(m.contains("commit 1"));
            assert!(m.contains("commit 3"));
            assert_eq!(m.lines().count(), 5);
        }
    }

    // ── WorktreeInfo: Debug format stability ──

    #[test]
    fn test_worktree_info_debug_contains_all_fields() {
        let info = WorktreeInfo {
            path: PathBuf::from("/check/debug"),
            branch: Some("test-branch".to_string()),
            _commit: "1a2b3c".to_string(),
            is_main: true,
        };
        let dbg = format!("{:?}", info);
        assert!(dbg.contains("check/debug"));
        assert!(dbg.contains("test-branch"));
        assert!(dbg.contains("1a2b3c"));
        assert!(dbg.contains("true"));
    }

    #[test]
    fn test_worktree_info_debug_none_branch() {
        let info = WorktreeInfo {
            path: PathBuf::from("/x"),
            branch: None,
            _commit: "z".to_string(),
            is_main: false,
        };
        let dbg = format!("{:?}", info);
        assert!(dbg.contains("None"));
    }

    // ── WorktreeInfo: size / memory ──

    #[test]
    fn test_worktree_info_is_not_zero_sized() {
        assert!(std::mem::size_of::<WorktreeInfo>() > 0);
    }

    #[test]
    fn test_squash_merge_result_is_not_zero_sized() {
        assert!(std::mem::size_of::<SquashMergeResult>() > 0);
    }

    // ── Commit log format parsing (tab-delimited) ──

    #[test]
    fn test_commit_log_parse_three_fields() {
        let line = "abc1234\tabcdef1234567890\tfeat: add feature";
        let parts: Vec<&str> = line.splitn(3, '\t').collect();
        assert_eq!(parts.len(), 3);
        assert_eq!(parts[0], "abc1234");
        assert_eq!(parts[1], "abcdef1234567890");
        assert_eq!(parts[2], "feat: add feature");
    }

    #[test]
    fn test_commit_log_parse_subject_with_tabs() {
        // splitn(3) keeps tabs in the third field (subject)
        let line = "abc1234\thash\tsubject\twith\textra\ttabs";
        let parts: Vec<&str> = line.splitn(3, '\t').collect();
        assert_eq!(parts.len(), 3);
        assert_eq!(parts[2], "subject\twith\textra\ttabs");
    }

    #[test]
    fn test_commit_log_parse_too_few_fields_ignored() {
        let line = "abc1234\thashonly";
        let parts: Vec<&str> = line.splitn(3, '\t').collect();
        assert!(parts.len() < 3);
        // Should not be pushed to commits vec
    }

    #[test]
    fn test_commit_log_parse_empty_subject() {
        let line = "abc1234\tfullhash\t";
        let parts: Vec<&str> = line.splitn(3, '\t').collect();
        assert_eq!(parts.len(), 3);
        assert_eq!(parts[2], "");
    }

    // ── Divergence count parsing ──

    #[test]
    fn test_divergence_parse_tab_split() {
        let s = "3\t7";
        let mut parts = s.split('\t');
        let behind: usize = parts.next().unwrap().parse().unwrap();
        let ahead: usize = parts.next().unwrap().parse().unwrap();
        assert_eq!(behind, 3);
        assert_eq!(ahead, 7);
    }

    #[test]
    fn test_divergence_parse_zeros() {
        let s = "0\t0";
        let mut parts = s.split('\t');
        let behind: usize = parts.next().unwrap().parse().unwrap();
        let ahead: usize = parts.next().unwrap().parse().unwrap();
        assert_eq!((behind, ahead), (0, 0));
    }

    #[test]
    fn test_divergence_parse_only_behind() {
        let s = "10\t0";
        let mut parts = s.split('\t');
        let behind: usize = parts.next().unwrap().parse().unwrap();
        let ahead: usize = parts.next().unwrap().parse().unwrap();
        assert_eq!(behind, 10);
        assert_eq!(ahead, 0);
    }

    #[test]
    fn test_divergence_parse_only_ahead() {
        let s = "0\t5";
        let mut parts = s.split('\t');
        let behind: usize = parts.next().unwrap().parse().unwrap();
        let ahead: usize = parts.next().unwrap().parse().unwrap();
        assert_eq!(behind, 0);
        assert_eq!(ahead, 5);
    }

    // ── Merge message construction ──

    #[test]
    fn test_merge_message_no_commit_log() {
        let display = "my-feature";
        let commit_log = "";
        let message = if commit_log.is_empty() {
            format!("feat: merge {} into main", display)
        } else {
            format!("feat: merge {} into main\n\n{}", display, commit_log)
        };
        assert_eq!(message, "feat: merge my-feature into main");
    }

    #[test]
    fn test_merge_message_with_commit_log() {
        let display = "my-feature";
        let commit_log = "- fix bug\n- add tests";
        let message = if commit_log.is_empty() {
            format!("feat: merge {} into main", display)
        } else {
            format!("feat: merge {} into main\n\n{}", display, commit_log)
        };
        assert!(message.starts_with("feat: merge my-feature into main"));
        assert!(message.contains("- fix bug"));
        assert!(message.contains("- add tests"));
    }

    #[test]
    fn test_merge_message_no_log_single_line() {
        let display = "branch-x";
        let commit_log = "";
        let message = if commit_log.is_empty() {
            format!("feat: merge {} into main", display)
        } else {
            format!("feat: merge {} into main\n\n{}", display, commit_log)
        };
        assert_eq!(message.lines().count(), 1);
    }

    #[test]
    fn test_merge_message_with_log_multiline() {
        let display = "branch-x";
        let commit_log = "- commit a\n- commit b\n- commit c";
        let message = if commit_log.is_empty() {
            format!("feat: merge {} into main", display)
        } else {
            format!("feat: merge {} into main\n\n{}", display, commit_log)
        };
        // summary + blank + 3 commits = 5 lines
        assert_eq!(message.lines().count(), 5);
    }

    // ── Status line parsing for unmerged detection ──

    #[test]
    fn test_status_line_unmerged_uu_detected() {
        let line = "UU src/main.rs";
        assert!(line.starts_with("U"));
    }

    #[test]
    fn test_status_line_unmerged_aa_detected() {
        let line = "AA src/conflict.rs";
        assert!(line.starts_with("AA"));
    }

    #[test]
    fn test_status_line_unmerged_dd_detected() {
        let line = "DD src/deleted.rs";
        assert!(line.starts_with("DD"));
    }

    #[test]
    fn test_status_line_clean_not_unmerged() {
        let line = "M  src/modified.rs";
        assert!(!line.starts_with("U") && !line.starts_with("AA") && !line.starts_with("DD"));
    }

    #[test]
    fn test_status_line_added_not_unmerged() {
        let line = "A  src/new.rs";
        assert!(!line.starts_with("U") && !line.starts_with("AA") && !line.starts_with("DD"));
    }

    // ── Conflict output parsing (from squash_merge_into_main) ──

    #[test]
    fn test_conflict_line_extract_path() {
        let line = "CONFLICT (content): Merge conflict in src/main.rs";
        let path = line.rsplit("Merge conflict in ").next().unwrap().trim();
        assert_eq!(path, "src/main.rs");
    }

    #[test]
    fn test_conflict_line_add_add_extract_path() {
        let line = "CONFLICT (add/add): Merge conflict in Cargo.toml";
        let path = line.rsplit("Merge conflict in ").next().unwrap().trim();
        assert_eq!(path, "Cargo.toml");
    }

    #[test]
    fn test_auto_merging_strip_prefix() {
        let line = "Auto-merging src/lib.rs";
        let path = line.strip_prefix("Auto-merging ").unwrap().trim();
        assert_eq!(path, "src/lib.rs");
    }

    #[test]
    fn test_non_conflict_line_not_stripped() {
        let line = "Squash commit -- not updating HEAD";
        assert!(line.strip_prefix("Auto-merging ").is_none());
        assert!(!line.starts_with("CONFLICT"));
    }

    // ── Git real-repo tests ──

    #[test]
    fn test_is_git_repo_cwd() {
        let cwd = std::env::current_dir().unwrap();
        assert!(Git::is_git_repo(&cwd));
    }

    #[test]
    fn test_is_git_repo_tmp_not_repo() {
        // /tmp is almost certainly not a git repo
        let result = Git::is_git_repo(std::path::Path::new("/tmp"));
        // Just verify it doesn't panic; result depends on environment
        let _ = result;
    }

    #[test]
    fn test_get_staged_diff_returns_ok_in_repo() {
        let cwd = std::env::current_dir().unwrap();
        if Git::is_git_repo(&cwd) {
            let result = Git::get_staged_diff(&cwd);
            assert!(result.is_ok());
        }
    }

    #[test]
    fn test_get_staged_stat_returns_ok_in_repo() {
        let cwd = std::env::current_dir().unwrap();
        if Git::is_git_repo(&cwd) {
            let result = Git::get_staged_stat(&cwd);
            assert!(result.is_ok());
        }
    }

    #[test]
    fn test_get_commit_log_returns_ok_in_repo() {
        let cwd = std::env::current_dir().unwrap();
        if Git::is_git_repo(&cwd) {
            let result = Git::get_commit_log(&cwd, 5, None);
            assert!(result.is_ok());
        }
    }

    #[test]
    fn test_get_commit_log_entries_have_three_string_fields() {
        let cwd = std::env::current_dir().unwrap();
        if Git::is_git_repo(&cwd) {
            let commits = Git::get_commit_log(&cwd, 5, None).unwrap();
            for (short_hash, full_hash, subject, _pushed) in &commits {
                assert!(!short_hash.is_empty());
                assert!(!full_hash.is_empty());
                let _ = subject; // may be empty for some commits
            }
        }
    }

    #[test]
    fn test_get_main_divergence_returns_pair() {
        let cwd = std::env::current_dir().unwrap();
        if Git::is_git_repo(&cwd) {
            let (behind, ahead) = Git::get_main_divergence(&cwd, "main");
            // Both are usize — just verify they're accessible
            let _ = (behind, ahead);
        }
    }

    #[test]
    fn test_get_remote_divergence_returns_pair() {
        let cwd = std::env::current_dir().unwrap();
        if Git::is_git_repo(&cwd) {
            let (behind, ahead) = Git::get_remote_divergence(&cwd);
            let _ = (behind, ahead);
        }
    }
}
