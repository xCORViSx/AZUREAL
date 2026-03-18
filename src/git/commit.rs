//! Git commit operations
//!
//! Commit creation, commit log queries, and staged diff inspection.

use anyhow::{Context, Result};
use std::path::Path;
use std::process::Command;

use super::Git;

impl Git {
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
