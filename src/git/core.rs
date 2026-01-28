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

    /// List all azural/* branches (for archived session detection)
    pub fn list_azural_branches(repo_path: &Path) -> Result<Vec<String>> {
        let output = Command::new("git")
            .args(["branch", "--list", "azural/*", "--format=%(refname:short)"])
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
}
