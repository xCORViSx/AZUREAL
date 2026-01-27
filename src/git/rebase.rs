//! Git rebase operations

use anyhow::{bail, Context, Result};
use std::path::Path;
use std::process::Command;

use crate::models::{RebaseResult, RebaseState, RebaseStatus};
use super::Git;

impl Git {
    /// Rebase worktree onto main branch with full status tracking
    pub fn rebase_onto_main(worktree_path: &Path, main_branch: &str) -> Result<RebaseResult> {
        if Self::is_rebase_in_progress(worktree_path) {
            let status = Self::get_rebase_status(worktree_path)?;
            return Ok(RebaseResult::Conflicts(status));
        }

        let _ = Command::new("git")
            .args(["fetch", "origin", main_branch])
            .current_dir(worktree_path)
            .output();

        let merge_base = Command::new("git")
            .args(["merge-base", "HEAD", main_branch])
            .current_dir(worktree_path)
            .output()
            .context("Failed to find merge base")?;

        let main_rev = Command::new("git")
            .args(["rev-parse", main_branch])
            .current_dir(worktree_path)
            .output()
            .context("Failed to get main branch rev")?;

        if merge_base.stdout == main_rev.stdout { return Ok(RebaseResult::UpToDate); }

        let output = Command::new("git")
            .args(["rebase", main_branch])
            .current_dir(worktree_path)
            .output()
            .context("Failed to execute rebase")?;

        if output.status.success() { return Ok(RebaseResult::Success); }

        let stderr = String::from_utf8_lossy(&output.stderr);
        if stderr.contains("CONFLICT") || stderr.contains("could not apply") {
            let status = Self::get_rebase_status(worktree_path)?;
            return Ok(RebaseResult::Conflicts(status));
        }

        Ok(RebaseResult::Failed(stderr.to_string()))
    }

    /// Check if a rebase is currently in progress
    pub fn is_rebase_in_progress(worktree_path: &Path) -> bool {
        let git_dir = Self::get_git_dir(worktree_path);
        if let Some(git_dir) = git_dir {
            if git_dir.join("rebase-merge").exists() { return true; }
            if git_dir.join("rebase-apply").exists() { return true; }
        }
        false
    }

    /// Get detailed rebase status
    pub fn get_rebase_status(worktree_path: &Path) -> Result<RebaseStatus> {
        let git_dir = Self::get_git_dir(worktree_path).context("Failed to get git directory")?;

        let mut status = RebaseStatus::default();

        let rebase_merge = git_dir.join("rebase-merge");
        let rebase_apply = git_dir.join("rebase-apply");

        if rebase_merge.exists() {
            status.state = RebaseState::InProgress;

            if let Ok(onto) = std::fs::read_to_string(rebase_merge.join("onto")) {
                let onto_rev = onto.trim();
                if let Ok(name) = Self::rev_to_branch_name(worktree_path, onto_rev) {
                    status.onto_branch = Some(name);
                } else {
                    status.onto_branch = Some(onto_rev[..7.min(onto_rev.len())].to_string());
                }
            }

            if let Ok(head_name) = std::fs::read_to_string(rebase_merge.join("head-name")) {
                let name = head_name.trim().strip_prefix("refs/heads/").unwrap_or(head_name.trim());
                status.head_name = Some(name.to_string());
            }

            if let Ok(msgnum) = std::fs::read_to_string(rebase_merge.join("msgnum")) {
                status.current_step = msgnum.trim().parse().ok();
            }

            if let Ok(end) = std::fs::read_to_string(rebase_merge.join("end")) {
                status.total_steps = end.trim().parse().ok();
            }

            if let Ok(stopped_sha) = std::fs::read_to_string(rebase_merge.join("stopped-sha")) {
                let sha = stopped_sha.trim();
                status.current_commit = Some(sha[..7.min(sha.len())].to_string());

                if let Ok(output) = Command::new("git")
                    .args(["log", "-1", "--format=%s", sha])
                    .current_dir(worktree_path)
                    .output()
                {
                    if output.status.success() {
                        status.current_commit_message = Some(String::from_utf8_lossy(&output.stdout).trim().to_string());
                    }
                }
            }
        } else if rebase_apply.exists() {
            status.state = RebaseState::InProgress;

            if let Ok(next) = std::fs::read_to_string(rebase_apply.join("next")) {
                status.current_step = next.trim().parse().ok();
            }

            if let Ok(last) = std::fs::read_to_string(rebase_apply.join("last")) {
                status.total_steps = last.trim().parse().ok();
            }

            if let Ok(head_name) = std::fs::read_to_string(rebase_apply.join("head-name")) {
                let name = head_name.trim().strip_prefix("refs/heads/").unwrap_or(head_name.trim());
                status.head_name = Some(name.to_string());
            }
        } else {
            status.state = RebaseState::None;
            return Ok(status);
        }

        status.conflicted_files = Self::get_conflicted_files(worktree_path)?;
        if !status.conflicted_files.is_empty() { status.state = RebaseState::Conflicts; }

        Ok(status)
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

    /// Try to convert a revision to a branch name
    fn rev_to_branch_name(worktree_path: &Path, rev: &str) -> Result<String> {
        let output = Command::new("git")
            .args(["name-rev", "--name-only", "--no-undefined", rev])
            .current_dir(worktree_path)
            .output()
            .context("Failed to resolve revision")?;

        if output.status.success() {
            let name = String::from_utf8_lossy(&output.stdout).trim().to_string();
            let clean_name = name.split('~').next().unwrap_or(&name);
            let clean_name = clean_name.split('^').next().unwrap_or(clean_name);
            Ok(clean_name.to_string())
        } else {
            bail!("Could not resolve revision to branch name")
        }
    }

    /// Continue a rebase after resolving conflicts
    pub fn rebase_continue(worktree_path: &Path) -> Result<RebaseResult> {
        if !Self::is_rebase_in_progress(worktree_path) { bail!("No rebase in progress"); }

        let conflicts = Self::get_conflicted_files(worktree_path)?;
        if !conflicts.is_empty() {
            let status = Self::get_rebase_status(worktree_path)?;
            return Ok(RebaseResult::Conflicts(status));
        }

        let _ = Command::new("git").args(["add", "-A"]).current_dir(worktree_path).output();

        let output = Command::new("git")
            .args(["rebase", "--continue"])
            .current_dir(worktree_path)
            .output()
            .context("Failed to continue rebase")?;

        if output.status.success() {
            if Self::is_rebase_in_progress(worktree_path) {
                let status = Self::get_rebase_status(worktree_path)?;
                if status.state == RebaseState::Conflicts { return Ok(RebaseResult::Conflicts(status)); }
            }
            return Ok(RebaseResult::Success);
        }

        let stderr = String::from_utf8_lossy(&output.stderr);
        if stderr.contains("CONFLICT") || stderr.contains("could not apply") {
            let status = Self::get_rebase_status(worktree_path)?;
            return Ok(RebaseResult::Conflicts(status));
        }

        Ok(RebaseResult::Failed(stderr.to_string()))
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

    /// Skip the current commit during a rebase
    pub fn rebase_skip(worktree_path: &Path) -> Result<RebaseResult> {
        if !Self::is_rebase_in_progress(worktree_path) { bail!("No rebase in progress"); }

        let output = Command::new("git")
            .args(["rebase", "--skip"])
            .current_dir(worktree_path)
            .output()
            .context("Failed to skip commit")?;

        if output.status.success() {
            if Self::is_rebase_in_progress(worktree_path) {
                let status = Self::get_rebase_status(worktree_path)?;
                if status.state == RebaseState::Conflicts { return Ok(RebaseResult::Conflicts(status)); }
            }
            return Ok(RebaseResult::Success);
        }

        let stderr = String::from_utf8_lossy(&output.stderr);
        if stderr.contains("CONFLICT") {
            let status = Self::get_rebase_status(worktree_path)?;
            return Ok(RebaseResult::Conflicts(status));
        }

        Ok(RebaseResult::Failed(stderr.to_string()))
    }

    /// Mark a file as resolved (stage it)
    pub fn mark_resolved(worktree_path: &Path, file_path: &str) -> Result<()> {
        let output = Command::new("git")
            .args(["add", file_path])
            .current_dir(worktree_path)
            .output()
            .context("Failed to stage file")?;

        if !output.status.success() {
            bail!("Failed to mark file as resolved: {}", String::from_utf8_lossy(&output.stderr));
        }

        Ok(())
    }

    /// Get the content of a file in conflict (shows conflict markers)
    pub fn get_conflict_diff(worktree_path: &Path, file_path: &str) -> Result<String> {
        let output = Command::new("git")
            .args(["diff", file_path])
            .current_dir(worktree_path)
            .output()
            .context("Failed to get conflict diff")?;

        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }

    /// Choose "ours" version for a conflicted file
    pub fn resolve_using_ours(worktree_path: &Path, file_path: &str) -> Result<()> {
        let output = Command::new("git")
            .args(["checkout", "--ours", file_path])
            .current_dir(worktree_path)
            .output()
            .context("Failed to checkout ours version")?;

        if !output.status.success() {
            bail!("Failed to use ours version: {}", String::from_utf8_lossy(&output.stderr));
        }

        Self::mark_resolved(worktree_path, file_path)
    }

    /// Choose "theirs" version for a conflicted file
    pub fn resolve_using_theirs(worktree_path: &Path, file_path: &str) -> Result<()> {
        let output = Command::new("git")
            .args(["checkout", "--theirs", file_path])
            .current_dir(worktree_path)
            .output()
            .context("Failed to checkout theirs version")?;

        if !output.status.success() {
            bail!("Failed to use theirs version: {}", String::from_utf8_lossy(&output.stderr));
        }

        Self::mark_resolved(worktree_path, file_path)
    }
}
