//! Git remote operations
//!
//! Pull, push, and remote/main divergence queries.

use anyhow::{Context, Result};
use std::path::Path;
use std::process::Command;

use super::Git;

impl Git {
    /// Pull from remote (current branch's upstream, or origin/<branch> if no upstream set)
    pub fn pull(worktree_path: &Path) -> Result<String> {
        let has_upstream = Command::new("git")
            .args(["rev-parse", "--abbrev-ref", "--symbolic-full-name", "@{u}"])
            .current_dir(worktree_path)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);

        let output = if has_upstream {
            Command::new("git")
                .args(["pull"])
                .current_dir(worktree_path)
                .output()
                .context("Failed to pull")?
        } else {
            let branch = Command::new("git")
                .args(["rev-parse", "--abbrev-ref", "HEAD"])
                .current_dir(worktree_path)
                .output()
                .context("Failed to get branch name")?;
            let branch_name = String::from_utf8_lossy(&branch.stdout).trim().to_string();
            Command::new("git")
                .args(["pull", "origin", &branch_name])
                .current_dir(worktree_path)
                .output()
                .context("Failed to pull")?
        };

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

    /// Push current branch to remote (auto-sets upstream on first push)
    pub fn push(worktree_path: &Path) -> Result<String> {
        let branch = Command::new("git")
            .args(["rev-parse", "--abbrev-ref", "HEAD"])
            .current_dir(worktree_path)
            .output()
            .context("Failed to get branch name")?;
        let branch_name = String::from_utf8_lossy(&branch.stdout).trim().to_string();

        // Check if local branch has diverged from remote (e.g. after rebase).
        // `rev-list --left-right --count` returns "<ahead>\t<behind>".
        // If behind > 0 AND ahead > 0, the histories have diverged — force-with-lease is needed.
        let diverged = Command::new("git")
            .args([
                "rev-list",
                "--left-right",
                "--count",
                &format!("HEAD...origin/{}", branch_name),
            ])
            .current_dir(worktree_path)
            .output()
            .ok()
            .and_then(|o| {
                if !o.status.success() {
                    return None;
                }
                let s = String::from_utf8_lossy(&o.stdout).trim().to_string();
                let parts: Vec<&str> = s.split('\t').collect();
                if parts.len() == 2 {
                    let ahead = parts[0].parse::<u64>().unwrap_or(0);
                    let behind = parts[1].parse::<u64>().unwrap_or(0);
                    Some(ahead > 0 && behind > 0)
                } else {
                    None
                }
            })
            .unwrap_or(false);

        // Only pull --rebase before push on main/master. Feature branches are
        // kept up to date via the auto-rebase system, and pulling on a feature
        // branch whose remote was already squash-merged corrupts HEAD state
        // (replays already-merged commits onto the squashed main).
        let is_main = branch_name == "main" || branch_name == "master";
        if !diverged && is_main {
            let pull = Command::new("git")
                .args(["pull", "--rebase", "origin", &branch_name])
                .current_dir(worktree_path)
                .output();
            if let Ok(ref o) = pull {
                if !o.status.success() {
                    let msg = String::from_utf8_lossy(&o.stderr);
                    if msg.contains("CONFLICT") || msg.contains("could not apply") {
                        anyhow::bail!(
                            "Pull rebase failed with conflicts — resolve manually then push"
                        );
                    }
                }
            }
        }

        // Use --force-with-lease when diverged (post-rebase), regular push otherwise
        let push_args = if diverged {
            vec!["push", "--force-with-lease", "-u", "origin", &branch_name]
        } else {
            vec!["push", "-u", "origin", &branch_name]
        };

        let output = Command::new("git")
            .args(&push_args)
            .current_dir(worktree_path)
            .output()
            .context("Failed to push")?;

        let combined = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        if output.status.success() {
            let suffix = if diverged { " (force-pushed)" } else { "" };
            Ok(format!("{}{}", combined.trim(), suffix))
        } else {
            anyhow::bail!("{}", combined.trim())
        }
    }

    /// Get divergence counts between two refs: (behind, ahead).
    /// `behind` = commits in `upstream` not in `local`, `ahead` = commits in `local` not in `upstream`.
    /// Uses `git rev-list --left-right --count upstream...local`.
    fn rev_list_divergence(worktree_path: &Path, upstream: &str, local: &str) -> (usize, usize) {
        Command::new("git")
            .args([
                "rev-list",
                "--left-right",
                "--count",
                &format!("{}...{}", upstream, local),
            ])
            .current_dir(worktree_path)
            .output()
            .ok()
            .filter(|o| o.status.success())
            .and_then(|o| {
                let s = String::from_utf8_lossy(&o.stdout);
                let mut parts = s.trim().split('\t');
                let behind = parts.next()?.parse::<usize>().ok()?;
                let ahead = parts.next()?.parse::<usize>().ok()?;
                Some((behind, ahead))
            })
            .unwrap_or((0, 0))
    }

    /// Get divergence from main: (behind_main, ahead_of_main).
    /// Returns (0, 0) on main branch or any error.
    pub fn get_main_divergence(worktree_path: &Path, main_branch: &str) -> (usize, usize) {
        Self::rev_list_divergence(worktree_path, main_branch, "HEAD")
    }

    /// Get divergence from remote tracking branch: (behind_remote, ahead_of_remote).
    /// Returns (0, 0) when no upstream is configured or any error.
    pub fn get_remote_divergence(worktree_path: &Path) -> (usize, usize) {
        // Resolve upstream tracking ref (e.g. "origin/feat-x")
        let upstream = Command::new("git")
            .args(["rev-parse", "--abbrev-ref", "--symbolic-full-name", "@{u}"])
            .current_dir(worktree_path)
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string());
        match upstream {
            Some(u) if !u.is_empty() => Self::rev_list_divergence(worktree_path, &u, "HEAD"),
            _ => (0, 0),
        }
    }
}
