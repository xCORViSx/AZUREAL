//! Git worktree operations

use anyhow::{bail, Context, Result};
use std::path::Path;
use std::process::Command;

use super::{Git, WorktreeInfo};

impl Git {
    /// Create a new worktree
    pub fn create_worktree(repo_path: &Path, worktree_path: &Path, branch_name: &str) -> Result<()> {
        if let Some(parent) = worktree_path.parent() {
            std::fs::create_dir_all(parent).context("Failed to create worktrees directory")?;
        }

        let output = Command::new("git")
            .args(["worktree", "add", "-b", branch_name, &worktree_path.to_string_lossy()])
            .current_dir(repo_path)
            .output()
            .context("Failed to execute git worktree add")?;

        if !output.status.success() {
            bail!("Failed to create worktree: {}", String::from_utf8_lossy(&output.stderr));
        }

        Ok(())
    }

    /// Remove a worktree
    pub fn remove_worktree(repo_path: &Path, worktree_path: &Path) -> Result<()> {
        let output = Command::new("git")
            .args(["worktree", "remove", &worktree_path.to_string_lossy()])
            .current_dir(repo_path)
            .output()?;

        if output.status.success() { return Ok(()); }

        let output = Command::new("git")
            .args(["worktree", "remove", "--force", &worktree_path.to_string_lossy()])
            .current_dir(repo_path)
            .output()?;

        if !output.status.success() {
            if worktree_path.exists() {
                std::fs::remove_dir_all(worktree_path).context("Failed to remove worktree directory")?;
            }
            let _ = Command::new("git").args(["worktree", "prune"]).current_dir(repo_path).output();
        }

        Ok(())
    }

    /// List existing worktrees (paths only)
    pub fn list_worktrees(repo_path: &Path) -> Result<Vec<String>> {
        let output = Command::new("git")
            .args(["worktree", "list", "--porcelain"])
            .current_dir(repo_path)
            .output()
            .context("Failed to list worktrees")?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let worktrees: Vec<String> = stdout.lines()
            .filter(|line| line.starts_with("worktree "))
            .map(|line| line.strip_prefix("worktree ").unwrap_or(line).to_string())
            .collect();

        Ok(worktrees)
    }

    /// List worktrees with full details (path, branch, commit)
    pub fn list_worktrees_detailed(repo_path: &Path) -> Result<Vec<WorktreeInfo>> {
        let output = Command::new("git")
            .args(["worktree", "list", "--porcelain"])
            .current_dir(repo_path)
            .output()
            .context("Failed to list worktrees")?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut worktrees = Vec::new();
        let mut current_path: Option<std::path::PathBuf> = None;
        let mut current_commit: Option<String> = None;
        let mut current_branch: Option<String> = None;

        for line in stdout.lines() {
            if let Some(path) = line.strip_prefix("worktree ") {
                if let (Some(path), Some(commit)) = (current_path.take(), current_commit.take()) {
                    let is_main = current_branch.as_ref().map(|b| b == "main" || b == "master").unwrap_or(false)
                        || path == repo_path;
                    worktrees.push(WorktreeInfo { path, branch: current_branch.take(), _commit: commit, is_main });
                }
                current_path = Some(std::path::PathBuf::from(path));
            } else if let Some(commit) = line.strip_prefix("HEAD ") {
                current_commit = Some(commit.to_string());
            } else if let Some(branch) = line.strip_prefix("branch refs/heads/") {
                current_branch = Some(branch.to_string());
            }
        }

        if let (Some(path), Some(commit)) = (current_path, current_commit) {
            let is_main = current_branch.as_ref().map(|b| b == "main" || b == "master").unwrap_or(false)
                || path == repo_path;
            worktrees.push(WorktreeInfo { path, branch: current_branch, _commit: commit, is_main });
        }

        Ok(worktrees)
    }

    /// Create a worktree from an existing branch
    pub fn create_worktree_from_branch(repo_path: &Path, worktree_path: &Path, branch_name: &str) -> Result<()> {
        if let Some(parent) = worktree_path.parent() {
            std::fs::create_dir_all(parent).context("Failed to create worktrees directory")?;
        }

        let is_remote = branch_name.contains('/');

        let output = if is_remote {
            let local_branch = branch_name.split('/').skip(1).collect::<Vec<_>>().join("/");
            Command::new("git")
                .args(["worktree", "add", "--track", "-b", &local_branch, &worktree_path.to_string_lossy(), branch_name])
                .current_dir(repo_path)
                .output()
                .context("Failed to execute git worktree add")?
        } else {
            Command::new("git")
                .args(["worktree", "add", &worktree_path.to_string_lossy(), branch_name])
                .current_dir(repo_path)
                .output()
                .context("Failed to execute git worktree add")?
        };

        if !output.status.success() {
            bail!("Failed to create worktree: {}", String::from_utf8_lossy(&output.stderr));
        }

        Ok(())
    }
}
