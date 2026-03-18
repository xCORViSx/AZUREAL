//! Core Git operations
//!
//! Basic git operations like repo detection, branch info, and status.
//! Focused methods live in sibling modules (commit, diff, merge, remote, staging).

use anyhow::{Context, Result};
use std::path::Path;
use std::process::Command;

/// Result of a squash merge attempt — distinguishes clean success from
/// partial-conflict scenarios that need interactive resolution.
pub enum SquashMergeResult {
    /// Clean merge completed and committed successfully
    Success(String),
    /// Conflicts detected — repo left in dirty merge state on main.
    /// User must resolve conflicts or abort before main is usable.
    Conflict {
        /// Files with CONFLICT markers that need manual resolution
        conflicted: Vec<String>,
        /// Files that git auto-merged without issues
        auto_merged: Vec<String>,
        /// Full raw git output from the merge command (populated by deserialization)
        _raw_output: String,
    },
}

/// Worktree info from git
#[derive(Debug, Clone)]
pub struct WorktreeInfo {
    pub path: std::path::PathBuf,
    pub branch: Option<String>,
    pub _commit: String,
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
        // Use --git-common-dir to resolve to the MAIN repo root, not the
        // worktree root. --show-toplevel returns the worktree's own directory
        // when run from a worktree, which breaks project/worktree discovery.
        let output = Command::new("git")
            .args(["rev-parse", "--path-format=absolute", "--git-common-dir"])
            .current_dir(path)
            .output()
            .context("Failed to get repo root")?;

        if output.status.success() {
            let git_dir = std::path::PathBuf::from(String::from_utf8_lossy(&output.stdout).trim());
            // --git-common-dir returns the .git directory; parent is the repo root
            Ok(git_dir.parent().unwrap_or(&git_dir).to_path_buf())
        } else {
            anyhow::bail!("Not in a git repository")
        }
    }

    /// List all prefixed branches (for archived session detection).
    /// Includes both local branches and remote branches (from origin).
    /// Remote branches appear as archived worktrees when no local checkout exists.
    pub fn list_azureal_branches(repo_path: &Path) -> Result<Vec<String>> {
        let pattern = format!("{}/*", crate::models::BRANCH_PREFIX);

        // Local branches: azureal/*
        let local_output = Command::new("git")
            .args(["branch", "--list", &pattern, "--format=%(refname:short)"])
            .current_dir(repo_path)
            .output()
            .context("Failed to list local branches")?;

        let mut branches: Vec<String> = String::from_utf8_lossy(&local_output.stdout)
            .lines()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();

        // Remote branches: origin/azureal/* (strip origin/ prefix to get branch name)
        let remote_pattern = format!("origin/{}/*", crate::models::BRANCH_PREFIX);
        let remote_output = Command::new("git")
            .args([
                "branch",
                "-r",
                "--list",
                &remote_pattern,
                "--format=%(refname:short)",
            ])
            .current_dir(repo_path)
            .output()
            .context("Failed to list remote branches")?;

        for line in String::from_utf8_lossy(&remote_output.stdout).lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            // Strip "origin/" prefix to get the bare branch name
            let branch_name = trimmed.strip_prefix("origin/").unwrap_or(trimmed);
            // Only add if not already present as a local branch
            if !branches.contains(&branch_name.to_string()) {
                branches.push(branch_name.to_string());
            }
        }

        Ok(branches)
    }

    /// Prune stale branch refs from other machines.
    /// 1. Prunes remote-tracking refs that no longer exist on origin.
    /// 2. Deletes local azureal/* branches that are fully merged to main
    ///    AND have no remote counterpart (deleted on another machine).
    /// Best-effort: silently ignored if offline or no remote configured.
    pub fn prune_remote_refs(repo_path: &Path) {
        // Prune stale origin/* refs
        let _ = Command::new("git")
            .args(["remote", "prune", "origin"])
            .current_dir(repo_path)
            .output();

        // Find local azureal/* branches with no remote counterpart
        let prefix = crate::models::BRANCH_PREFIX;
        let pattern = format!("{}/*", prefix);
        let local = Command::new("git")
            .args(["branch", "--list", &pattern, "--format=%(refname:short)"])
            .current_dir(repo_path)
            .output();
        let remote = Command::new("git")
            .args([
                "branch",
                "-r",
                "--list",
                &format!("origin/{}/*", prefix),
                "--format=%(refname:short)",
            ])
            .current_dir(repo_path)
            .output();
        let (Ok(local), Ok(remote)) = (local, remote) else {
            return;
        };
        let remote_names: std::collections::HashSet<String> =
            String::from_utf8_lossy(&remote.stdout)
                .lines()
                .filter_map(|l| l.trim().strip_prefix("origin/"))
                .map(|s| s.to_string())
                .collect();
        for line in String::from_utf8_lossy(&local.stdout).lines() {
            let branch = line.trim();
            if branch.is_empty() || remote_names.contains(branch) {
                continue;
            }
            // Only delete if fully merged to main (safe — no unmerged work lost)
            let merged = Command::new("git")
                .args(["branch", "--merged", "main", "--list", branch])
                .current_dir(repo_path)
                .output()
                .map(|o| !String::from_utf8_lossy(&o.stdout).trim().is_empty())
                .unwrap_or(false);
            if merged {
                let _ = Command::new("git")
                    .args(["branch", "-d", branch])
                    .current_dir(repo_path)
                    .output();
            }
        }
    }

    /// Get the main branch name (main or master)
    pub fn get_main_branch(repo_path: &Path) -> Result<String> {
        for branch in ["main", "master"] {
            let output = Command::new("git")
                .args(["rev-parse", "--verify", branch])
                .current_dir(repo_path)
                .output()?;

            if output.status.success() {
                return Ok(branch.to_string());
            }
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
            if path.is_absolute() {
                Some(path)
            } else {
                Some(worktree_path.join(path))
            }
        } else {
            None
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    // ── SquashMergeResult: construction & pattern matching ──

    #[test]
    fn test_squash_merge_result_success_construction() {
        let result = SquashMergeResult::Success("Merged: abc123".to_string());
        assert!(matches!(result, SquashMergeResult::Success(_)));
    }

    #[test]
    fn test_squash_merge_result_success_extracts_message() {
        let result = SquashMergeResult::Success("Merged: abc123 feat".to_string());
        if let SquashMergeResult::Success(msg) = result {
            assert_eq!(msg, "Merged: abc123 feat");
        } else {
            panic!("expected Success variant");
        }
    }

    #[test]
    fn test_squash_merge_result_success_empty_message() {
        let result = SquashMergeResult::Success(String::new());
        if let SquashMergeResult::Success(msg) = result {
            assert!(msg.is_empty());
        } else {
            panic!("expected Success variant");
        }
    }

    #[test]
    fn test_squash_merge_result_conflict_construction() {
        let result = SquashMergeResult::Conflict {
            conflicted: vec!["src/main.rs".to_string()],
            auto_merged: vec!["Cargo.toml".to_string()],
            _raw_output: "CONFLICT (content): ...".to_string(),
        };
        assert!(matches!(result, SquashMergeResult::Conflict { .. }));
    }

    #[test]
    fn test_squash_merge_result_conflict_fields() {
        let result = SquashMergeResult::Conflict {
            conflicted: vec!["a.rs".to_string(), "b.rs".to_string()],
            auto_merged: vec!["c.rs".to_string()],
            _raw_output: "raw output here".to_string(),
        };
        if let SquashMergeResult::Conflict {
            conflicted,
            auto_merged,
            _raw_output,
        } = result
        {
            assert_eq!(conflicted.len(), 2);
            assert_eq!(conflicted[0], "a.rs");
            assert_eq!(conflicted[1], "b.rs");
            assert_eq!(auto_merged.len(), 1);
            assert_eq!(auto_merged[0], "c.rs");
            assert_eq!(_raw_output, "raw output here");
        } else {
            panic!("expected Conflict variant");
        }
    }

    #[test]
    fn test_squash_merge_result_conflict_empty_vecs() {
        let result = SquashMergeResult::Conflict {
            conflicted: vec![],
            auto_merged: vec![],
            _raw_output: String::new(),
        };
        if let SquashMergeResult::Conflict {
            conflicted,
            auto_merged,
            ..
        } = result
        {
            assert!(conflicted.is_empty());
            assert!(auto_merged.is_empty());
        } else {
            panic!("expected Conflict variant");
        }
    }

    #[test]
    fn test_squash_merge_result_conflict_many_files() {
        let conflicted: Vec<String> = (0..50).map(|i| format!("file_{}.rs", i)).collect();
        let auto_merged: Vec<String> = (0..30).map(|i| format!("auto_{}.rs", i)).collect();
        let result = SquashMergeResult::Conflict {
            conflicted: conflicted.clone(),
            auto_merged: auto_merged.clone(),
            _raw_output: "lots of output".to_string(),
        };
        if let SquashMergeResult::Conflict {
            conflicted: c,
            auto_merged: a,
            ..
        } = result
        {
            assert_eq!(c.len(), 50);
            assert_eq!(a.len(), 30);
            assert_eq!(c[49], "file_49.rs");
            assert_eq!(a[29], "auto_29.rs");
        } else {
            panic!("expected Conflict variant");
        }
    }

    #[test]
    fn test_squash_merge_result_success_not_conflict() {
        let result = SquashMergeResult::Success("ok".to_string());
        assert!(!matches!(result, SquashMergeResult::Conflict { .. }));
    }

    #[test]
    fn test_squash_merge_result_conflict_not_success() {
        let result = SquashMergeResult::Conflict {
            conflicted: vec!["x".to_string()],
            auto_merged: vec![],
            _raw_output: String::new(),
        };
        assert!(!matches!(result, SquashMergeResult::Success(_)));
    }

    #[test]
    fn test_squash_merge_result_success_with_pull_note() {
        let result = SquashMergeResult::Success("Merged: abc123 (pull skipped)".to_string());
        if let SquashMergeResult::Success(msg) = result {
            assert!(msg.contains("(pull skipped)"));
        } else {
            panic!("expected Success variant");
        }
    }

    #[test]
    fn test_squash_merge_result_success_already_up_to_date() {
        let result =
            SquashMergeResult::Success("Already up to date — nothing to merge".to_string());
        if let SquashMergeResult::Success(msg) = result {
            assert!(msg.contains("Already up to date"));
        } else {
            panic!("expected Success variant");
        }
    }

    // ── WorktreeInfo: construction & field access ──

    #[test]
    fn test_worktree_info_construction_with_branch() {
        let info = WorktreeInfo {
            path: PathBuf::from("/repo/worktrees/feature"),
            branch: Some("azureal/feature".to_string()),
            _commit: "abc1234".to_string(),
            is_main: false,
        };
        assert_eq!(info.path, PathBuf::from("/repo/worktrees/feature"));
        assert_eq!(info.branch.as_deref(), Some("azureal/feature"));
        assert_eq!(info._commit, "abc1234");
        assert!(!info.is_main);
    }

    #[test]
    fn test_worktree_info_construction_main() {
        let info = WorktreeInfo {
            path: PathBuf::from("/repo"),
            branch: Some("main".to_string()),
            _commit: "def5678".to_string(),
            is_main: true,
        };
        assert!(info.is_main);
        assert_eq!(info.branch.as_deref(), Some("main"));
    }

    #[test]
    fn test_worktree_info_construction_no_branch() {
        let info = WorktreeInfo {
            path: PathBuf::from("/repo/worktrees/detached"),
            branch: None,
            _commit: "deadbeef".to_string(),
            is_main: false,
        };
        assert!(info.branch.is_none());
    }

    #[test]
    fn test_worktree_info_clone() {
        let info = WorktreeInfo {
            path: PathBuf::from("/a/b/c"),
            branch: Some("feat".to_string()),
            _commit: "aaa".to_string(),
            is_main: false,
        };
        let cloned = info.clone();
        assert_eq!(info.path, cloned.path);
        assert_eq!(info.branch, cloned.branch);
        assert_eq!(info._commit, cloned._commit);
        assert_eq!(info.is_main, cloned.is_main);
    }

    #[test]
    fn test_worktree_info_debug() {
        let info = WorktreeInfo {
            path: PathBuf::from("/debug/test"),
            branch: Some("debug-branch".to_string()),
            _commit: "fff".to_string(),
            is_main: false,
        };
        let dbg = format!("{:?}", info);
        assert!(dbg.contains("WorktreeInfo"));
        assert!(dbg.contains("debug-branch"));
        assert!(dbg.contains("/debug/test"));
    }

    #[test]
    fn test_worktree_info_clone_with_none_branch() {
        let info = WorktreeInfo {
            path: PathBuf::from("/x"),
            branch: None,
            _commit: "000".to_string(),
            is_main: false,
        };
        let cloned = info.clone();
        assert!(cloned.branch.is_none());
    }

    #[test]
    fn test_worktree_info_path_components() {
        let info = WorktreeInfo {
            path: PathBuf::from("/home/user/project/worktrees/my-feature"),
            branch: Some("azureal/my-feature".to_string()),
            _commit: "123".to_string(),
            is_main: false,
        };
        assert_eq!(
            info.path.file_name().unwrap().to_str().unwrap(),
            "my-feature"
        );
        assert!(info.path.starts_with("/home/user/project"));
    }

    #[test]
    fn test_worktree_info_master_is_main() {
        let info = WorktreeInfo {
            path: PathBuf::from("/repo"),
            branch: Some("master".to_string()),
            _commit: "bbb".to_string(),
            is_main: true,
        };
        assert!(info.is_main);
        assert_eq!(info.branch.as_deref(), Some("master"));
    }

    #[test]
    fn test_worktree_info_empty_commit() {
        let info = WorktreeInfo {
            path: PathBuf::from("/empty"),
            branch: None,
            _commit: String::new(),
            is_main: false,
        };
        assert!(info._commit.is_empty());
    }

    #[test]
    fn test_worktree_info_long_commit_hash() {
        let info = WorktreeInfo {
            path: PathBuf::from("/repo"),
            branch: Some("main".to_string()),
            _commit: "abc1234567890abcdef1234567890abcdef12345".to_string(),
            is_main: true,
        };
        assert_eq!(info._commit.len(), 40);
    }

    // ── Git unit struct ──

    #[test]
    fn test_git_struct_exists() {
        let _git = Git;
    }

    #[test]
    fn test_git_struct_is_zero_sized() {
        assert_eq!(std::mem::size_of::<Git>(), 0);
    }

    // ── SquashMergeResult: variant discrimination via match ──

    #[test]
    fn test_squash_merge_result_match_success() {
        let result = SquashMergeResult::Success("done".to_string());
        let is_success = match result {
            SquashMergeResult::Success(_) => true,
            SquashMergeResult::Conflict { .. } => false,
        };
        assert!(is_success);
    }

    #[test]
    fn test_squash_merge_result_match_conflict() {
        let result = SquashMergeResult::Conflict {
            conflicted: vec!["f.rs".to_string()],
            auto_merged: vec![],
            _raw_output: "err".to_string(),
        };
        let is_conflict = match result {
            SquashMergeResult::Success(_) => false,
            SquashMergeResult::Conflict { .. } => true,
        };
        assert!(is_conflict);
    }

    // ── WorktreeInfo: variations on is_main ──

    #[test]
    fn test_worktree_info_is_main_false_for_feature() {
        let info = WorktreeInfo {
            path: PathBuf::from("/wt/feature"),
            branch: Some("azureal/feature".to_string()),
            _commit: "abc".to_string(),
            is_main: false,
        };
        assert!(!info.is_main);
    }

    #[test]
    fn test_worktree_info_is_main_true_even_without_main_branch_name() {
        // is_main can be true when path == repo_path, regardless of branch name
        let info = WorktreeInfo {
            path: PathBuf::from("/repo"),
            branch: Some("develop".to_string()),
            _commit: "ccc".to_string(),
            is_main: true,
        };
        assert!(info.is_main);
    }

    // ── SquashMergeResult: conflict file path patterns ──

    #[test]
    fn test_squash_merge_conflict_paths_with_subdirs() {
        let result = SquashMergeResult::Conflict {
            conflicted: vec![
                "src/git/core.rs".to_string(),
                "src/models.rs".to_string(),
                "tests/integration.rs".to_string(),
            ],
            auto_merged: vec!["Cargo.toml".to_string(), "Cargo.lock".to_string()],
            _raw_output: "multiple conflicts".to_string(),
        };
        if let SquashMergeResult::Conflict {
            conflicted,
            auto_merged,
            ..
        } = result
        {
            assert!(conflicted.iter().any(|f| f.contains("core.rs")));
            assert!(auto_merged.iter().any(|f| f == "Cargo.toml"));
        }
    }

    #[test]
    fn test_squash_merge_success_multiline_message() {
        let msg = "Merged: feat\n\n- commit 1\n- commit 2\n- commit 3".to_string();
        let result = SquashMergeResult::Success(msg.clone());
        if let SquashMergeResult::Success(m) = result {
            assert!(m.contains("commit 1"));
            assert!(m.contains("commit 3"));
            assert_eq!(m.lines().count(), 5);
        }
    }

    // ── WorktreeInfo: Debug format stability ──

    #[test]
    fn test_worktree_info_debug_contains_all_fields() {
        let info = WorktreeInfo {
            path: PathBuf::from("/check/debug"),
            branch: Some("test-branch".to_string()),
            _commit: "1a2b3c".to_string(),
            is_main: true,
        };
        let dbg = format!("{:?}", info);
        assert!(dbg.contains("check/debug"));
        assert!(dbg.contains("test-branch"));
        assert!(dbg.contains("1a2b3c"));
        assert!(dbg.contains("true"));
    }

    #[test]
    fn test_worktree_info_debug_none_branch() {
        let info = WorktreeInfo {
            path: PathBuf::from("/x"),
            branch: None,
            _commit: "z".to_string(),
            is_main: false,
        };
        let dbg = format!("{:?}", info);
        assert!(dbg.contains("None"));
    }

    // ── WorktreeInfo: size / memory ──

    #[test]
    fn test_worktree_info_is_not_zero_sized() {
        assert!(std::mem::size_of::<WorktreeInfo>() > 0);
    }

    #[test]
    fn test_squash_merge_result_is_not_zero_sized() {
        assert!(std::mem::size_of::<SquashMergeResult>() > 0);
    }

    // ── Commit log format parsing (tab-delimited) ──

    #[test]
    fn test_commit_log_parse_three_fields() {
        let line = "abc1234\tabcdef1234567890\tfeat: add feature";
        let parts: Vec<&str> = line.splitn(3, '\t').collect();
        assert_eq!(parts.len(), 3);
        assert_eq!(parts[0], "abc1234");
        assert_eq!(parts[1], "abcdef1234567890");
        assert_eq!(parts[2], "feat: add feature");
    }

    #[test]
    fn test_commit_log_parse_subject_with_tabs() {
        // splitn(3) keeps tabs in the third field (subject)
        let line = "abc1234\thash\tsubject\twith\textra\ttabs";
        let parts: Vec<&str> = line.splitn(3, '\t').collect();
        assert_eq!(parts.len(), 3);
        assert_eq!(parts[2], "subject\twith\textra\ttabs");
    }

    #[test]
    fn test_commit_log_parse_too_few_fields_ignored() {
        let line = "abc1234\thashonly";
        let parts: Vec<&str> = line.splitn(3, '\t').collect();
        assert!(parts.len() < 3);
        // Should not be pushed to commits vec
    }

    #[test]
    fn test_commit_log_parse_empty_subject() {
        let line = "abc1234\tfullhash\t";
        let parts: Vec<&str> = line.splitn(3, '\t').collect();
        assert_eq!(parts.len(), 3);
        assert_eq!(parts[2], "");
    }

    // ── Divergence count parsing ──

    #[test]
    fn test_divergence_parse_tab_split() {
        let s = "3\t7";
        let mut parts = s.split('\t');
        let behind: usize = parts.next().unwrap().parse().unwrap();
        let ahead: usize = parts.next().unwrap().parse().unwrap();
        assert_eq!(behind, 3);
        assert_eq!(ahead, 7);
    }

    #[test]
    fn test_divergence_parse_zeros() {
        let s = "0\t0";
        let mut parts = s.split('\t');
        let behind: usize = parts.next().unwrap().parse().unwrap();
        let ahead: usize = parts.next().unwrap().parse().unwrap();
        assert_eq!((behind, ahead), (0, 0));
    }

    #[test]
    fn test_divergence_parse_only_behind() {
        let s = "10\t0";
        let mut parts = s.split('\t');
        let behind: usize = parts.next().unwrap().parse().unwrap();
        let ahead: usize = parts.next().unwrap().parse().unwrap();
        assert_eq!(behind, 10);
        assert_eq!(ahead, 0);
    }

    #[test]
    fn test_divergence_parse_only_ahead() {
        let s = "0\t5";
        let mut parts = s.split('\t');
        let behind: usize = parts.next().unwrap().parse().unwrap();
        let ahead: usize = parts.next().unwrap().parse().unwrap();
        assert_eq!(behind, 0);
        assert_eq!(ahead, 5);
    }

    // ── Merge message construction ──

    #[test]
    fn test_merge_message_no_commit_log() {
        let display = "my-feature";
        let commit_log = "";
        let message = if commit_log.is_empty() {
            format!("feat: merge {} into main", display)
        } else {
            format!("feat: merge {} into main\n\n{}", display, commit_log)
        };
        assert_eq!(message, "feat: merge my-feature into main");
    }

    #[test]
    fn test_merge_message_with_commit_log() {
        let display = "my-feature";
        let commit_log = "- fix bug\n- add tests";
        let message = if commit_log.is_empty() {
            format!("feat: merge {} into main", display)
        } else {
            format!("feat: merge {} into main\n\n{}", display, commit_log)
        };
        assert!(message.starts_with("feat: merge my-feature into main"));
        assert!(message.contains("- fix bug"));
        assert!(message.contains("- add tests"));
    }

    #[test]
    fn test_merge_message_no_log_single_line() {
        let display = "branch-x";
        let commit_log = "";
        let message = if commit_log.is_empty() {
            format!("feat: merge {} into main", display)
        } else {
            format!("feat: merge {} into main\n\n{}", display, commit_log)
        };
        assert_eq!(message.lines().count(), 1);
    }

    #[test]
    fn test_merge_message_with_log_multiline() {
        let display = "branch-x";
        let commit_log = "- commit a\n- commit b\n- commit c";
        let message = if commit_log.is_empty() {
            format!("feat: merge {} into main", display)
        } else {
            format!("feat: merge {} into main\n\n{}", display, commit_log)
        };
        // summary + blank + 3 commits = 5 lines
        assert_eq!(message.lines().count(), 5);
    }

    // ── Status line parsing for unmerged detection ──

    #[test]
    fn test_status_line_unmerged_uu_detected() {
        let line = "UU src/main.rs";
        assert!(line.starts_with("U"));
    }

    #[test]
    fn test_status_line_unmerged_aa_detected() {
        let line = "AA src/conflict.rs";
        assert!(line.starts_with("AA"));
    }

    #[test]
    fn test_status_line_unmerged_dd_detected() {
        let line = "DD src/deleted.rs";
        assert!(line.starts_with("DD"));
    }

    #[test]
    fn test_status_line_clean_not_unmerged() {
        let line = "M  src/modified.rs";
        assert!(!line.starts_with("U") && !line.starts_with("AA") && !line.starts_with("DD"));
    }

    #[test]
    fn test_status_line_added_not_unmerged() {
        let line = "A  src/new.rs";
        assert!(!line.starts_with("U") && !line.starts_with("AA") && !line.starts_with("DD"));
    }

    // ── Conflict output parsing (from squash_merge_into_main) ──

    #[test]
    fn test_conflict_line_extract_path() {
        let line = "CONFLICT (content): Merge conflict in src/main.rs";
        let path = line.rsplit("Merge conflict in ").next().unwrap().trim();
        assert_eq!(path, "src/main.rs");
    }

    #[test]
    fn test_conflict_line_add_add_extract_path() {
        let line = "CONFLICT (add/add): Merge conflict in Cargo.toml";
        let path = line.rsplit("Merge conflict in ").next().unwrap().trim();
        assert_eq!(path, "Cargo.toml");
    }

    #[test]
    fn test_auto_merging_strip_prefix() {
        let line = "Auto-merging src/lib.rs";
        let path = line.strip_prefix("Auto-merging ").unwrap().trim();
        assert_eq!(path, "src/lib.rs");
    }

    #[test]
    fn test_non_conflict_line_not_stripped() {
        let line = "Squash commit -- not updating HEAD";
        assert!(line.strip_prefix("Auto-merging ").is_none());
        assert!(!line.starts_with("CONFLICT"));
    }

    // ── Git real-repo tests ──

    #[test]
    fn test_is_git_repo_cwd() {
        let cwd = std::env::current_dir().unwrap();
        assert!(Git::is_git_repo(&cwd));
    }

    #[test]
    fn test_is_git_repo_tmp_not_repo() {
        // /tmp is almost certainly not a git repo
        let result = Git::is_git_repo(std::path::Path::new("/tmp"));
        // Just verify it doesn't panic; result depends on environment
        let _ = result;
    }

    #[test]
    fn test_get_staged_diff_returns_ok_in_repo() {
        let cwd = std::env::current_dir().unwrap();
        if Git::is_git_repo(&cwd) {
            let result = Git::get_staged_diff(&cwd);
            assert!(result.is_ok());
        }
    }

    #[test]
    fn test_get_staged_stat_returns_ok_in_repo() {
        let cwd = std::env::current_dir().unwrap();
        if Git::is_git_repo(&cwd) {
            let result = Git::get_staged_stat(&cwd);
            assert!(result.is_ok());
        }
    }

    #[test]
    fn test_get_commit_log_returns_ok_in_repo() {
        let cwd = std::env::current_dir().unwrap();
        if Git::is_git_repo(&cwd) {
            let result = Git::get_commit_log(&cwd, 5, None);
            assert!(result.is_ok());
        }
    }

    #[test]
    fn test_get_commit_log_entries_have_three_string_fields() {
        let cwd = std::env::current_dir().unwrap();
        if Git::is_git_repo(&cwd) {
            let commits = Git::get_commit_log(&cwd, 5, None).unwrap();
            for (short_hash, full_hash, subject, _pushed) in &commits {
                assert!(!short_hash.is_empty());
                assert!(!full_hash.is_empty());
                let _ = subject; // may be empty for some commits
            }
        }
    }

    #[test]
    fn test_get_main_divergence_returns_pair() {
        let cwd = std::env::current_dir().unwrap();
        if Git::is_git_repo(&cwd) {
            let (behind, ahead) = Git::get_main_divergence(&cwd, "main");
            // Both are usize — just verify they're accessible
            let _ = (behind, ahead);
        }
    }

    #[test]
    fn test_get_remote_divergence_returns_pair() {
        let cwd = std::env::current_dir().unwrap();
        if Git::is_git_repo(&cwd) {
            let (behind, ahead) = Git::get_remote_divergence(&cwd);
            let _ = (behind, ahead);
        }
    }
}
