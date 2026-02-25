//! Git rebase operations

use anyhow::{bail, Context, Result};
use std::path::Path;
use std::process::Command;

use crate::models::RebaseResult;
use super::Git;

impl Git {
    /// Check if a rebase is currently in progress
    pub fn is_rebase_in_progress(worktree_path: &Path) -> bool {
        let git_dir = Self::get_git_dir(worktree_path);
        if let Some(git_dir) = git_dir {
            if git_dir.join("rebase-merge").exists() { return true; }
            if git_dir.join("rebase-apply").exists() { return true; }
        }
        false
    }

    /// Get list of files with merge conflicts
    pub fn get_conflicted_files(worktree_path: &Path) -> Result<Vec<String>> {
        let output = Command::new("git")
            .args(["diff", "--name-only", "--diff-filter=U"])
            .current_dir(worktree_path)
            .output()
            .context("Failed to get conflicted files")?;

        let files: Vec<String> = String::from_utf8_lossy(&output.stdout)
            .lines()
            .map(|s| s.to_string())
            .filter(|s| !s.is_empty())
            .collect();

        Ok(files)
    }

    /// Abort a rebase in progress
    pub fn rebase_abort(worktree_path: &Path) -> Result<RebaseResult> {
        if !Self::is_rebase_in_progress(worktree_path) { bail!("No rebase in progress"); }

        let output = Command::new("git")
            .args(["rebase", "--abort"])
            .current_dir(worktree_path)
            .output()
            .context("Failed to abort rebase")?;

        if output.status.success() { return Ok(RebaseResult::Aborted); }

        Ok(RebaseResult::Failed(String::from_utf8_lossy(&output.stderr).to_string()))
    }
}
