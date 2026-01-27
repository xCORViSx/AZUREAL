//! Git branch operations

use anyhow::{bail, Context, Result};
use std::path::Path;
use std::process::Command;

use super::Git;

impl Git {
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
        let branches: Vec<String> = stdout.lines()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();

        Ok(branches)
    }

    /// List remote branches (without remote prefix)
    pub fn list_remote_branches(repo_path: &Path) -> Result<Vec<String>> {
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
        let branches: Vec<String> = stdout.lines()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty() && !s.contains("HEAD") && s.contains('/'))
            .collect();

        Ok(branches)
    }

    /// Get branches that are not already checked out in a worktree
    pub fn list_available_branches(repo_path: &Path) -> Result<Vec<String>> {
        let worktrees = Self::list_worktrees(repo_path)?;

        let mut checked_out: Vec<String> = Vec::new();
        for wt_path in &worktrees {
            let path = Path::new(wt_path);
            if let Ok(branch) = Self::current_branch(path) { checked_out.push(branch); }
        }

        let local = Self::list_local_branches(repo_path)?;
        let remote = Self::list_remote_branches(repo_path)?;

        let mut available: Vec<String> = local.into_iter()
            .filter(|b| !checked_out.contains(b))
            .collect();

        for remote_branch in remote {
            let local_name = remote_branch.split('/').skip(1).collect::<Vec<_>>().join("/");
            if !checked_out.contains(&local_name) && !available.contains(&remote_branch) {
                available.push(remote_branch);
            }
        }

        Ok(available)
    }

    /// Get number of commits ahead/behind main branch
    pub fn get_ahead_behind(worktree_path: &Path, main_branch: &str) -> Result<(usize, usize)> {
        let output = Command::new("git")
            .args(["rev-list", "--left-right", "--count", &format!("{}...HEAD", main_branch)])
            .current_dir(worktree_path)
            .output()
            .context("Failed to get ahead/behind count")?;

        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let parts: Vec<&str> = stdout.trim().split_whitespace().collect();
            if parts.len() == 2 {
                let behind = parts[0].parse().unwrap_or(0);
                let ahead = parts[1].parse().unwrap_or(0);
                return Ok((ahead, behind));
            }
        }

        Ok((0, 0))
    }

    /// Delete a branch
    pub fn delete_branch(repo_path: &Path, branch_name: &str) -> Result<()> {
        let output = Command::new("git")
            .args(["branch", "-d", branch_name])
            .current_dir(repo_path)
            .output()
            .context("Failed to execute git branch -d")?;

        if output.status.success() { return Ok(()); }

        let output = Command::new("git")
            .args(["branch", "-D", branch_name])
            .current_dir(repo_path)
            .output()
            .context("Failed to execute git branch -D")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            if !stderr.contains("not found") {
                bail!("Failed to delete branch {}: {}", branch_name, stderr);
            }
        }

        Ok(())
    }
}
