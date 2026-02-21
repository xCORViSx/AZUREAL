//! Core Git operations
//!
//! Basic git operations like repo detection, branch info, and diffs.

use anyhow::{Context, Result};
use std::path::Path;
use std::process::Command;

use crate::models::DiffInfo;

/// Worktree info from git
#[derive(Debug, Clone)]
pub struct WorktreeInfo {
    pub path: std::path::PathBuf,
    pub branch: Option<String>,
    pub commit: String,
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
        let output = Command::new("git")
            .args(["rev-parse", "--show-toplevel"])
            .current_dir(path)
            .output()
            .context("Failed to get repo root")?;

        if output.status.success() {
            Ok(std::path::PathBuf::from(String::from_utf8_lossy(&output.stdout).trim()))
        } else {
            anyhow::bail!("Not in a git repository")
        }
    }

    /// List all azureal/* branches (for archived session detection)
    pub fn list_azureal_branches(repo_path: &Path) -> Result<Vec<String>> {
        let output = Command::new("git")
            .args(["branch", "--list", "azureal/*", "--format=%(refname:short)"])
            .current_dir(repo_path)
            .output()
            .context("Failed to list branches")?;

        let branches: Vec<String> = String::from_utf8_lossy(&output.stdout)
            .lines()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();

        Ok(branches)
    }

    /// Get the main branch name (main or master)
    pub fn get_main_branch(repo_path: &Path) -> Result<String> {
        for branch in ["main", "master"] {
            let output = Command::new("git")
                .args(["rev-parse", "--verify", branch])
                .current_dir(repo_path)
                .output()?;

            if output.status.success() { return Ok(branch.to_string()); }
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
            if path.is_absolute() { Some(path) } else { Some(worktree_path.join(path)) }
        } else {
            None
        }
    }

    /// Check if there are uncommitted changes
    pub fn has_uncommitted_changes(worktree_path: &Path) -> Result<bool> {
        let output = Command::new("git")
            .args(["status", "--porcelain"])
            .current_dir(worktree_path)
            .output()
            .context("Failed to check git status")?;

        Ok(!String::from_utf8_lossy(&output.stdout).trim().is_empty())
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
    /// Returns Vec<(path, status_char, additions, deletions)> by combining
    /// `git diff --name-status` (M/A/D/R) with `git diff --numstat` (+/-).
    pub fn get_diff_files(worktree_path: &Path, _main_branch: &str) -> Result<Vec<(String, char, usize, usize)>> {
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
        let mut stats: std::collections::HashMap<String, (usize, usize)> = std::collections::HashMap::new();
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
                result.push((path, status, add, del));
            }
        }

        // Also pick up untracked files (shown as '?' status, 0/0 stats)
        let untracked_out = Command::new("git")
            .args(["ls-files", "--others", "--exclude-standard"])
            .current_dir(worktree_path)
            .output()
            .context("Failed to list untracked files")?;
        for line in String::from_utf8_lossy(&untracked_out.stdout).lines() {
            let path = line.trim().to_string();
            if !path.is_empty() && !seen.contains(&path) {
                result.push((path, '?', 0, 0));
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
            let ignore_out = child.wait_with_output().context("git check-ignore failed")?;
            let ignored: std::collections::HashSet<&str> =
                std::str::from_utf8(&ignore_out.stdout).unwrap_or("")
                    .lines().map(|l| l.trim()).filter(|l| !l.is_empty()).collect();
            result.retain(|(path, ..)| !ignored.contains(path.as_str()));
        }

        Ok(result)
    }

    /// Get the diff for a single file (working tree vs HEAD, for viewer display)
    pub fn get_file_diff(worktree_path: &Path, _main_branch: &str, file_path: &str) -> Result<String> {
        let output = Command::new("git")
            .args(["diff", "HEAD", "--", file_path])
            .current_dir(worktree_path)
            .output()
            .context("Failed to get file diff")?;
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }

    /// Fetch from all remotes, pruning stale tracking branches
    pub fn fetch(worktree_path: &Path) -> Result<String> {
        let output = Command::new("git")
            .args(["fetch", "--all", "--prune"])
            .current_dir(worktree_path)
            .output()
            .context("Failed to fetch")?;
        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stderr).trim().to_string())
        } else {
            anyhow::bail!("{}", String::from_utf8_lossy(&output.stderr).trim())
        }
    }

    /// Pull from remote (current branch's upstream)
    pub fn pull(worktree_path: &Path) -> Result<String> {
        let output = Command::new("git")
            .args(["pull"])
            .current_dir(worktree_path)
            .output()
            .context("Failed to pull")?;
        let combined = format!("{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
        if output.status.success() { Ok(combined.trim().to_string()) }
        else { anyhow::bail!("{}", combined.trim()) }
    }

    /// Push current branch to remote
    pub fn push(worktree_path: &Path) -> Result<String> {
        let output = Command::new("git")
            .args(["push"])
            .current_dir(worktree_path)
            .output()
            .context("Failed to push")?;
        let combined = format!("{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
        if output.status.success() { Ok(combined.trim().to_string()) }
        else { anyhow::bail!("{}", combined.trim()) }
    }

    /// Squash-merge a feature branch into main:
    /// 1. Pull main from remote (so we merge onto the latest upstream)
    /// 2. Squash-merge the branch (collapses all commits into one staged changeset)
    /// 3. Commit with a clean message
    /// Push is separate — user triggers it manually via the Push action.
    /// Runs from the repo root (main worktree, already on main branch).
    pub fn squash_merge_into_main(repo_root: &Path, branch_name: &str) -> Result<String> {
        // Step 1: pull main so we're merging onto the latest upstream.
        // --ff-only prevents accidental merge commits on main itself.
        // Failure here is non-fatal — we might be offline or have no remote.
        let pull_out = Command::new("git")
            .args(["pull", "--ff-only"])
            .current_dir(repo_root)
            .output();
        let pull_note = match pull_out {
            Ok(ref o) if !o.status.success() => {
                let err = String::from_utf8_lossy(&o.stderr);
                // Diverged main is a real problem — abort before squash merge
                if err.contains("fatal") || err.contains("divergent") {
                    anyhow::bail!("Cannot update main before merge: {}", err.trim());
                }
                " (pull skipped)"
            }
            Err(_) => " (pull skipped)",
            _ => "",
        };

        // Step 2: squash-merge stages all changes without committing
        let merge_out = Command::new("git")
            .args(["merge", "--squash", branch_name])
            .current_dir(repo_root)
            .output()
            .context("Failed to squash merge")?;
        let combined = format!("{}{}", String::from_utf8_lossy(&merge_out.stdout), String::from_utf8_lossy(&merge_out.stderr));
        let text = combined.trim();

        if !merge_out.status.success() {
            let conflicts: Vec<&str> = text.lines()
                .filter(|l| l.starts_with("CONFLICT"))
                .collect();
            if !conflicts.is_empty() {
                let merged = text.lines().filter(|l| l.starts_with("Auto-merging")).count();
                anyhow::bail!(
                    "Squash merge has {} conflict{} ({} file{} auto-merged). Resolve on main:\n{}",
                    conflicts.len(),
                    if conflicts.len() == 1 { "" } else { "s" },
                    merged,
                    if merged == 1 { "" } else { "s" },
                    conflicts.join("\n"),
                );
            }
            anyhow::bail!("{}", text);
        }

        // Step 3: commit the squashed changes with a clean message.
        // Strip "azureal/" prefix from branch name for a readable commit.
        let display = branch_name.strip_prefix("azureal/").unwrap_or(branch_name);
        let commit_out = Command::new("git")
            .args(["commit", "-m", &format!("feat: merge {} into main", display)])
            .current_dir(repo_root)
            .output()
            .context("Failed to commit squash merge")?;
        if !commit_out.status.success() {
            let err = String::from_utf8_lossy(&commit_out.stderr).trim().to_string();
            if err.contains("nothing to commit") {
                return Ok("Already up to date — nothing to merge".into());
            }
            anyhow::bail!("Squash merge staged but commit failed: {}", err);
        }

        let out = String::from_utf8_lossy(&commit_out.stdout).trim().to_string();
        let first = out.lines().next().unwrap_or(&out);
        Ok(format!("Merged: {}{}", first, pull_note))
    }

    /// Stage all changes (tracked + untracked) via `git add -A`
    pub fn stage_all(worktree_path: &Path) -> Result<()> {
        let output = Command::new("git")
            .args(["add", "-A"])
            .current_dir(worktree_path)
            .output()
            .context("Failed to stage changes")?;
        if output.status.success() { Ok(()) }
        else { anyhow::bail!("{}", String::from_utf8_lossy(&output.stderr).trim()) }
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

    /// Commit staged changes with the given message
    pub fn commit(worktree_path: &Path, message: &str) -> Result<String> {
        let output = Command::new("git")
            .args(["commit", "-m", message])
            .current_dir(worktree_path)
            .output()
            .context("Failed to commit")?;
        let combined = format!("{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
        if output.status.success() { Ok(combined.trim().to_string()) }
        else { anyhow::bail!("{}", combined.trim()) }
    }

}
