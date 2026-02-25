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

    /// List remote branches from cache (no network fetch — instant, won't block UI)
    pub fn list_remote_branches_cached(repo_path: &Path) -> Result<Vec<String>> {
        let output = Command::new("git")
            .args(["branch", "-r", "--format=%(refname:short)"])
            .current_dir(repo_path)
            .output()
            .context("Failed to list remote branches")?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        Ok(stdout.lines()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty() && !s.contains("HEAD") && s.contains('/'))
            .collect())
    }

    /// Get all branches with their checked-out status.
    /// Returns (all_branches, checked_out_set) so the UI can show which are active.
    /// Uses cached remote refs to avoid blocking the UI with network calls.
    pub fn list_all_branches_with_status(repo_path: &Path) -> Result<(Vec<String>, Vec<String>)> {
        let worktrees = Self::list_worktrees(repo_path)?;

        let mut checked_out: Vec<String> = Vec::new();
        for wt_path in &worktrees {
            let path = Path::new(wt_path);
            if let Ok(branch) = Self::current_branch(path) { checked_out.push(branch); }
        }

        // Local branches first, excluding main/master (always the base repo root)
        let mut all: Vec<String> = Self::list_local_branches(repo_path)?
            .into_iter()
            .filter(|b| b != "main" && b != "master")
            .collect();

        // Append remote branches that don't have a local equivalent (skip main/master)
        let remote = Self::list_remote_branches_cached(repo_path)?;
        for remote_branch in remote {
            let local_name = remote_branch.split('/').skip(1).collect::<Vec<_>>().join("/");
            if local_name == "main" || local_name == "master" { continue; }
            if !all.contains(&local_name) && !all.contains(&remote_branch) {
                all.push(remote_branch);
            }
        }

        Ok((all, checked_out))
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
