use anyhow::{bail, Context, Result};
use std::path::Path;
use std::process::Command;

use crate::models::{DiffInfo, RebaseResult, RebaseState, RebaseStatus};

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

    /// Rebase worktree onto main branch with full status tracking
    pub fn rebase_onto_main(worktree_path: &Path, main_branch: &str) -> Result<RebaseResult> {
        // Check if there's already a rebase in progress
        if Self::is_rebase_in_progress(worktree_path) {
            let status = Self::get_rebase_status(worktree_path)?;
            return Ok(RebaseResult::Conflicts(status));
        }

        // First fetch to ensure we have latest
        let _ = Command::new("git")
            .args(["fetch", "origin", main_branch])
            .current_dir(worktree_path)
            .output();

        // Check if we're already up to date
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

        if merge_base.stdout == main_rev.stdout {
            return Ok(RebaseResult::UpToDate);
        }

        // Perform the rebase
        let output = Command::new("git")
            .args(["rebase", main_branch])
            .current_dir(worktree_path)
            .output()
            .context("Failed to execute rebase")?;

        if output.status.success() {
            return Ok(RebaseResult::Success);
        }

        // Check if we have conflicts
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
            // Check for rebase-merge directory (interactive rebase)
            if git_dir.join("rebase-merge").exists() {
                return true;
            }
            // Check for rebase-apply directory (am-style rebase)
            if git_dir.join("rebase-apply").exists() {
                return true;
            }
        }
        false
    }

    /// Get the git directory for a worktree
    fn get_git_dir(worktree_path: &Path) -> Option<std::path::PathBuf> {
        let output = Command::new("git")
            .args(["rev-parse", "--git-dir"])
            .current_dir(worktree_path)
            .output()
            .ok()?;

        if output.status.success() {
            let path_str = String::from_utf8_lossy(&output.stdout).trim().to_string();
            let path = std::path::PathBuf::from(&path_str);
            // Handle relative paths
            if path.is_absolute() {
                Some(path)
            } else {
                Some(worktree_path.join(path))
            }
        } else {
            None
        }
    }

    /// Get detailed rebase status
    pub fn get_rebase_status(worktree_path: &Path) -> Result<RebaseStatus> {
        let git_dir = Self::get_git_dir(worktree_path)
            .context("Failed to get git directory")?;

        let mut status = RebaseStatus::default();

        // Determine which type of rebase is in progress
        let rebase_merge = git_dir.join("rebase-merge");
        let rebase_apply = git_dir.join("rebase-apply");

        if rebase_merge.exists() {
            status.state = RebaseState::InProgress;

            // Read onto branch
            if let Ok(onto) = std::fs::read_to_string(rebase_merge.join("onto")) {
                let onto_rev = onto.trim();
                // Try to get branch name for the commit
                if let Ok(name) = Self::rev_to_branch_name(worktree_path, onto_rev) {
                    status.onto_branch = Some(name);
                } else {
                    status.onto_branch = Some(onto_rev[..7.min(onto_rev.len())].to_string());
                }
            }

            // Read head name (original branch being rebased)
            if let Ok(head_name) = std::fs::read_to_string(rebase_merge.join("head-name")) {
                let name = head_name.trim().strip_prefix("refs/heads/").unwrap_or(head_name.trim());
                status.head_name = Some(name.to_string());
            }

            // Read current step
            if let Ok(msgnum) = std::fs::read_to_string(rebase_merge.join("msgnum")) {
                status.current_step = msgnum.trim().parse().ok();
            }

            // Read total steps
            if let Ok(end) = std::fs::read_to_string(rebase_merge.join("end")) {
                status.total_steps = end.trim().parse().ok();
            }

            // Read current commit being applied
            if let Ok(stopped_sha) = std::fs::read_to_string(rebase_merge.join("stopped-sha")) {
                let sha = stopped_sha.trim();
                status.current_commit = Some(sha[..7.min(sha.len())].to_string());

                // Get the commit message
                if let Ok(output) = Command::new("git")
                    .args(["log", "-1", "--format=%s", sha])
                    .current_dir(worktree_path)
                    .output()
                {
                    if output.status.success() {
                        status.current_commit_message = Some(
                            String::from_utf8_lossy(&output.stdout).trim().to_string()
                        );
                    }
                }
            }
        } else if rebase_apply.exists() {
            status.state = RebaseState::InProgress;

            // Read current step
            if let Ok(next) = std::fs::read_to_string(rebase_apply.join("next")) {
                status.current_step = next.trim().parse().ok();
            }

            // Read total steps
            if let Ok(last) = std::fs::read_to_string(rebase_apply.join("last")) {
                status.total_steps = last.trim().parse().ok();
            }

            // Read original branch
            if let Ok(head_name) = std::fs::read_to_string(rebase_apply.join("head-name")) {
                let name = head_name.trim().strip_prefix("refs/heads/").unwrap_or(head_name.trim());
                status.head_name = Some(name.to_string());
            }
        } else {
            status.state = RebaseState::None;
            return Ok(status);
        }

        // Get conflicted files
        status.conflicted_files = Self::get_conflicted_files(worktree_path)?;
        if !status.conflicted_files.is_empty() {
            status.state = RebaseState::Conflicts;
        }

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
            // Clean up the name (remove ~N suffixes, etc.)
            let clean_name = name.split('~').next().unwrap_or(&name);
            let clean_name = clean_name.split('^').next().unwrap_or(clean_name);
            Ok(clean_name.to_string())
        } else {
            bail!("Could not resolve revision to branch name")
        }
    }

    /// Continue a rebase after resolving conflicts
    pub fn rebase_continue(worktree_path: &Path) -> Result<RebaseResult> {
        if !Self::is_rebase_in_progress(worktree_path) {
            bail!("No rebase in progress");
        }

        // Check if there are still unresolved conflicts
        let conflicts = Self::get_conflicted_files(worktree_path)?;
        if !conflicts.is_empty() {
            let status = Self::get_rebase_status(worktree_path)?;
            return Ok(RebaseResult::Conflicts(status));
        }

        // Stage all changes before continuing
        let _ = Command::new("git")
            .args(["add", "-A"])
            .current_dir(worktree_path)
            .output();

        let output = Command::new("git")
            .args(["rebase", "--continue"])
            .current_dir(worktree_path)
            .output()
            .context("Failed to continue rebase")?;

        if output.status.success() {
            // Check if rebase is complete
            if Self::is_rebase_in_progress(worktree_path) {
                let status = Self::get_rebase_status(worktree_path)?;
                if status.state == RebaseState::Conflicts {
                    return Ok(RebaseResult::Conflicts(status));
                }
            }
            return Ok(RebaseResult::Success);
        }

        // Check if we hit more conflicts
        let stderr = String::from_utf8_lossy(&output.stderr);
        if stderr.contains("CONFLICT") || stderr.contains("could not apply") {
            let status = Self::get_rebase_status(worktree_path)?;
            return Ok(RebaseResult::Conflicts(status));
        }

        Ok(RebaseResult::Failed(stderr.to_string()))
    }

    /// Abort a rebase in progress
    pub fn rebase_abort(worktree_path: &Path) -> Result<RebaseResult> {
        if !Self::is_rebase_in_progress(worktree_path) {
            bail!("No rebase in progress");
        }

        let output = Command::new("git")
            .args(["rebase", "--abort"])
            .current_dir(worktree_path)
            .output()
            .context("Failed to abort rebase")?;

        if output.status.success() {
            return Ok(RebaseResult::Aborted);
        }

        let stderr = String::from_utf8_lossy(&output.stderr);
        Ok(RebaseResult::Failed(stderr.to_string()))
    }

    /// Skip the current commit during a rebase
    pub fn rebase_skip(worktree_path: &Path) -> Result<RebaseResult> {
        if !Self::is_rebase_in_progress(worktree_path) {
            bail!("No rebase in progress");
        }

        let output = Command::new("git")
            .args(["rebase", "--skip"])
            .current_dir(worktree_path)
            .output()
            .context("Failed to skip commit")?;

        if output.status.success() {
            // Check if rebase is complete
            if Self::is_rebase_in_progress(worktree_path) {
                let status = Self::get_rebase_status(worktree_path)?;
                if status.state == RebaseState::Conflicts {
                    return Ok(RebaseResult::Conflicts(status));
                }
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
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("Failed to mark file as resolved: {}", stderr);
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
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("Failed to use ours version: {}", stderr);
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
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("Failed to use theirs version: {}", stderr);
        }

        Self::mark_resolved(worktree_path, file_path)
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
