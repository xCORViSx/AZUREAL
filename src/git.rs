use anyhow::{bail, Context, Result};
use std::path::Path;
use std::process::Command;

use crate::models::DiffInfo;

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

    /// Get the main branch name (main or master)
    pub fn get_main_branch(repo_path: &Path) -> Result<String> {
        // Try to detect main branch
        for branch in ["main", "master"] {
            let output = Command::new("git")
                .args(["rev-parse", "--verify", branch])
                .current_dir(repo_path)
                .output()?;

            if output.status.success() {
                return Ok(branch.to_string());
            }
        }

        // Fall back to getting the current branch
        let output = Command::new("git")
            .args(["rev-parse", "--abbrev-ref", "HEAD"])
            .current_dir(repo_path)
            .output()?;

        if output.status.success() {
            let branch = String::from_utf8_lossy(&output.stdout).trim().to_string();
            return Ok(branch);
        }

        Ok("main".to_string())
    }

    /// Create a new worktree
    pub fn create_worktree(
        repo_path: &Path,
        worktree_path: &Path,
        branch_name: &str,
    ) -> Result<()> {
        // Ensure worktrees directory exists
        if let Some(parent) = worktree_path.parent() {
            std::fs::create_dir_all(parent)
                .context("Failed to create worktrees directory")?;
        }

        // Create the worktree with a new branch
        let output = Command::new("git")
            .args([
                "worktree",
                "add",
                "-b",
                branch_name,
                &worktree_path.to_string_lossy(),
            ])
            .current_dir(repo_path)
            .output()
            .context("Failed to execute git worktree add")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("Failed to create worktree: {}", stderr);
        }

        Ok(())
    }

    /// Remove a worktree
    pub fn remove_worktree(repo_path: &Path, worktree_path: &Path) -> Result<()> {
        // First try normal removal
        let output = Command::new("git")
            .args([
                "worktree",
                "remove",
                &worktree_path.to_string_lossy(),
            ])
            .current_dir(repo_path)
            .output()?;

        if output.status.success() {
            return Ok(());
        }

        // Try force removal
        let output = Command::new("git")
            .args([
                "worktree",
                "remove",
                "--force",
                &worktree_path.to_string_lossy(),
            ])
            .current_dir(repo_path)
            .output()?;

        if !output.status.success() {
            // Last resort: manual cleanup
            if worktree_path.exists() {
                std::fs::remove_dir_all(worktree_path)
                    .context("Failed to remove worktree directory")?;
            }

            // Prune worktrees
            let _ = Command::new("git")
                .args(["worktree", "prune"])
                .current_dir(repo_path)
                .output();
        }

        Ok(())
    }

    /// List existing worktrees
    pub fn list_worktrees(repo_path: &Path) -> Result<Vec<String>> {
        let output = Command::new("git")
            .args(["worktree", "list", "--porcelain"])
            .current_dir(repo_path)
            .output()
            .context("Failed to list worktrees")?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let worktrees: Vec<String> = stdout
            .lines()
            .filter(|line| line.starts_with("worktree "))
            .map(|line| line.strip_prefix("worktree ").unwrap_or(line).to_string())
            .collect();

        Ok(worktrees)
    }

    /// Get the diff between worktree and main branch
    pub fn get_diff(worktree_path: &Path, main_branch: &str) -> Result<DiffInfo> {
        // Get the diff text
        let diff_output = Command::new("git")
            .args(["diff", &format!("{}...HEAD", main_branch)])
            .current_dir(worktree_path)
            .output()
            .context("Failed to get git diff")?;

        let diff_text = String::from_utf8_lossy(&diff_output.stdout).to_string();

        // Get the diff stats
        let stats_output = Command::new("git")
            .args(["diff", "--stat", &format!("{}...HEAD", main_branch)])
            .current_dir(worktree_path)
            .output()
            .context("Failed to get git diff stats")?;

        let stats_text = String::from_utf8_lossy(&stats_output.stdout);

        // Parse stats
        let mut files_changed = Vec::new();
        let mut additions = 0;
        let mut deletions = 0;

        for line in stats_text.lines() {
            if line.contains('|') {
                // File line: " src/main.rs | 10 +++++-----"
                if let Some(file) = line.split('|').next() {
                    files_changed.push(file.trim().to_string());
                }
            } else if line.contains("insertion") || line.contains("deletion") {
                // Summary line: " 3 files changed, 50 insertions(+), 20 deletions(-)"
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
            session_id: String::new(), // Will be filled in by caller
            diff_text,
            files_changed,
            additions,
            deletions,
            timestamp: chrono::Utc::now(),
        })
    }

    /// Rebase worktree onto main branch
    pub fn rebase_onto_main(worktree_path: &Path, main_branch: &str) -> Result<()> {
        // First fetch
        let _ = Command::new("git")
            .args(["fetch", "origin", main_branch])
            .current_dir(worktree_path)
            .output();

        // Then rebase
        let output = Command::new("git")
            .args(["rebase", main_branch])
            .current_dir(worktree_path)
            .output()
            .context("Failed to rebase")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("Rebase failed: {}", stderr);
        }

        Ok(())
    }

    /// Get current branch name
    pub fn current_branch(worktree_path: &Path) -> Result<String> {
        let output = Command::new("git")
            .args(["rev-parse", "--abbrev-ref", "HEAD"])
            .current_dir(worktree_path)
            .output()
            .context("Failed to get current branch")?;

        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }

    /// List all local branches
    pub fn list_local_branches(repo_path: &Path) -> Result<Vec<String>> {
        let output = Command::new("git")
            .args(["branch", "--format=%(refname:short)"])
            .current_dir(repo_path)
            .output()
            .context("Failed to list branches")?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let branches: Vec<String> = stdout
            .lines()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();

        Ok(branches)
    }

    /// List remote branches (without remote prefix)
    pub fn list_remote_branches(repo_path: &Path) -> Result<Vec<String>> {
        // Fetch to get latest remote branches
        let _ = Command::new("git")
            .args(["fetch", "--all", "--prune"])
            .current_dir(repo_path)
            .output();

        let output = Command::new("git")
            .args(["branch", "-r", "--format=%(refname:short)"])
            .current_dir(repo_path)
            .output()
            .context("Failed to list remote branches")?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let branches: Vec<String> = stdout
            .lines()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty() && !s.contains("HEAD"))
            .collect();

        Ok(branches)
    }

    /// Create a worktree from an existing branch
    pub fn create_worktree_from_branch(
        repo_path: &Path,
        worktree_path: &Path,
        branch_name: &str,
    ) -> Result<()> {
        // Ensure worktrees directory exists
        if let Some(parent) = worktree_path.parent() {
            std::fs::create_dir_all(parent)
                .context("Failed to create worktrees directory")?;
        }

        // Check if branch is remote
        let is_remote = branch_name.contains('/');

        let output = if is_remote {
            // For remote branches, create a local tracking branch
            let local_branch = branch_name
                .split('/')
                .skip(1)
                .collect::<Vec<_>>()
                .join("/");

            Command::new("git")
                .args([
                    "worktree",
                    "add",
                    "--track",
                    "-b",
                    &local_branch,
                    &worktree_path.to_string_lossy(),
                    branch_name,
                ])
                .current_dir(repo_path)
                .output()
                .context("Failed to execute git worktree add")?
        } else {
            // For local branches, just add the worktree
            Command::new("git")
                .args([
                    "worktree",
                    "add",
                    &worktree_path.to_string_lossy(),
                    branch_name,
                ])
                .current_dir(repo_path)
                .output()
                .context("Failed to execute git worktree add")?
        };

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("Failed to create worktree: {}", stderr);
        }

        Ok(())
    }

    /// Get branches that are not already checked out in a worktree
    pub fn list_available_branches(repo_path: &Path) -> Result<Vec<String>> {
        let worktrees = Self::list_worktrees(repo_path)?;

        // Get branches checked out in worktrees
        let mut checked_out: Vec<String> = Vec::new();
        for wt_path in &worktrees {
            let path = Path::new(wt_path);
            if let Ok(branch) = Self::current_branch(path) {
                checked_out.push(branch);
            }
        }

        // Get all local branches
        let local = Self::list_local_branches(repo_path)?;

        // Get remote branches
        let remote = Self::list_remote_branches(repo_path)?;

        // Combine and filter out already checked out branches
        let mut available: Vec<String> = local
            .into_iter()
            .filter(|b| !checked_out.contains(b))
            .collect();

        // Add remote branches that don't have a local equivalent checked out
        for remote_branch in remote {
            let local_name = remote_branch
                .split('/')
                .skip(1)
                .collect::<Vec<_>>()
                .join("/");

            if !checked_out.contains(&local_name) && !available.contains(&remote_branch) {
                available.push(remote_branch);
            }
        }

        Ok(available)
    }

    /// Check if there are uncommitted changes
    pub fn has_uncommitted_changes(worktree_path: &Path) -> Result<bool> {
        let output = Command::new("git")
            .args(["status", "--porcelain"])
            .current_dir(worktree_path)
            .output()
            .context("Failed to check git status")?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        Ok(!stdout.trim().is_empty())
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

    /// Delete a branch
    pub fn delete_branch(repo_path: &Path, branch_name: &str) -> Result<()> {
        // Try normal deletion first
        let output = Command::new("git")
            .args(["branch", "-d", branch_name])
            .current_dir(repo_path)
            .output()
            .context("Failed to execute git branch -d")?;

        if output.status.success() {
            return Ok(());
        }

        // Try force deletion if normal deletion fails
        let output = Command::new("git")
            .args(["branch", "-D", branch_name])
            .current_dir(repo_path)
            .output()
            .context("Failed to execute git branch -D")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            // Don't fail if branch doesn't exist
            if !stderr.contains("not found") {
                bail!("Failed to delete branch {}: {}", branch_name, stderr);
            }
        }

        Ok(())
    }
}
