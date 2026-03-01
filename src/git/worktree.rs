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

    /// Parse `git worktree list --porcelain` output into worktree paths.
    /// Extracted for testability — the public `list_worktrees` calls git and
    /// feeds the stdout here.
    pub(crate) fn parse_worktree_paths(stdout: &str) -> Vec<String> {
        stdout.lines()
            .filter(|line| line.starts_with("worktree "))
            .map(|line| line.strip_prefix("worktree ").unwrap_or(line).to_string())
            .collect()
    }

    /// Parse `git worktree list --porcelain` output into `WorktreeInfo` structs.
    /// Extracted for testability — the public `list_worktrees_detailed` calls
    /// git and feeds stdout + repo_path here.
    pub(crate) fn parse_worktree_info(stdout: &str, repo_path: &Path) -> Vec<WorktreeInfo> {
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

        worktrees
    }

    /// Create a worktree from an existing branch
    pub fn create_worktree_from_branch(repo_path: &Path, worktree_path: &Path, branch_name: &str) -> Result<()> {
        if let Some(parent) = worktree_path.parent() {
            std::fs::create_dir_all(parent).context("Failed to create worktrees directory")?;
        }

        // Check if branch exists locally (e.g. azureal/foo is local despite containing '/').
        // Only use the remote-tracking -b path for genuine remote refs (e.g. origin/main).
        let is_local = Command::new("git")
            .args(["rev-parse", "--verify", &format!("refs/heads/{}", branch_name)])
            .current_dir(repo_path)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);

        let output = if !is_local && branch_name.contains('/') {
            // Remote branch: create a local tracking branch
            let local_branch = branch_name.split('/').skip(1).collect::<Vec<_>>().join("/");
            Command::new("git")
                .args(["worktree", "add", "--track", "-b", &local_branch, &worktree_path.to_string_lossy(), branch_name])
                .current_dir(repo_path)
                .output()
                .context("Failed to execute git worktree add")?
        } else {
            // Local branch: just check it out directly
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    // ── Helper: typical porcelain output for a single worktree ──

    fn single_worktree_output() -> String {
        "worktree /repo\nHEAD abc1234\nbranch refs/heads/main\n\n".to_string()
    }

    fn two_worktree_output() -> String {
        "worktree /repo\nHEAD abc1234\nbranch refs/heads/main\n\n\
         worktree /repo/.worktrees/feature\nHEAD def5678\nbranch refs/heads/feature\n\n"
            .to_string()
    }

    // ── 1. parse_worktree_paths: empty input ──

    #[test]
    fn test_parse_paths_empty() {
        let paths = Git::parse_worktree_paths("");
        assert!(paths.is_empty());
    }

    // ── 2. parse_worktree_paths: single worktree ──

    #[test]
    fn test_parse_paths_single() {
        let paths = Git::parse_worktree_paths(&single_worktree_output());
        assert_eq!(paths, vec!["/repo"]);
    }

    // ── 3. parse_worktree_paths: two worktrees ──

    #[test]
    fn test_parse_paths_two() {
        let paths = Git::parse_worktree_paths(&two_worktree_output());
        assert_eq!(paths, vec!["/repo", "/repo/.worktrees/feature"]);
    }

    // ── 4. parse_worktree_paths: ignores non-worktree lines ──

    #[test]
    fn test_parse_paths_ignores_head_branch() {
        let output = "HEAD abc\nbranch refs/heads/main\nworktree /repo\n";
        let paths = Git::parse_worktree_paths(output);
        assert_eq!(paths, vec!["/repo"]);
    }

    // ── 5. parse_worktree_paths: only blank lines ──

    #[test]
    fn test_parse_paths_blank_lines() {
        let paths = Git::parse_worktree_paths("\n\n\n");
        assert!(paths.is_empty());
    }

    // ── 6. parse_worktree_info: empty input ──

    #[test]
    fn test_parse_info_empty() {
        let info = Git::parse_worktree_info("", Path::new("/repo"));
        assert!(info.is_empty());
    }

    // ── 7. parse_worktree_info: single main worktree ──

    #[test]
    fn test_parse_info_single_main() {
        let info = Git::parse_worktree_info(&single_worktree_output(), Path::new("/repo"));
        assert_eq!(info.len(), 1);
        assert_eq!(info[0].path, Path::new("/repo"));
        assert_eq!(info[0].branch.as_deref(), Some("main"));
        assert_eq!(info[0]._commit, "abc1234");
        assert!(info[0].is_main);
    }

    // ── 8. parse_worktree_info: two worktrees ──

    #[test]
    fn test_parse_info_two_worktrees() {
        let info = Git::parse_worktree_info(&two_worktree_output(), Path::new("/repo"));
        assert_eq!(info.len(), 2);
        assert!(info[0].is_main);
        assert!(!info[1].is_main);
        assert_eq!(info[1].branch.as_deref(), Some("feature"));
    }

    // ── 9. parse_worktree_info: master branch is_main ──

    #[test]
    fn test_parse_info_master_branch_is_main() {
        let output = "worktree /repo\nHEAD abc\nbranch refs/heads/master\n\n";
        let info = Git::parse_worktree_info(output, Path::new("/other"));
        assert_eq!(info.len(), 1);
        assert!(info[0].is_main);
    }

    // ── 10. parse_worktree_info: path == repo_path means is_main ──

    #[test]
    fn test_parse_info_path_equals_repo_is_main() {
        let output = "worktree /repo\nHEAD abc\nbranch refs/heads/feature\n\n";
        let info = Git::parse_worktree_info(output, Path::new("/repo"));
        assert!(info[0].is_main); // path matches repo_path
    }

    // ── 11. parse_worktree_info: detached HEAD (no branch line) ──

    #[test]
    fn test_parse_info_detached_head() {
        let output = "worktree /repo\nHEAD abc\n\n";
        let info = Git::parse_worktree_info(output, Path::new("/other"));
        assert_eq!(info.len(), 1);
        assert!(info[0].branch.is_none());
        assert!(!info[0].is_main); // no branch match, path doesn't match
    }

    // ── 12. parse_worktree_info: detached HEAD at repo_path is_main ──

    #[test]
    fn test_parse_info_detached_head_at_repo_path() {
        let output = "worktree /repo\nHEAD abc\n\n";
        let info = Git::parse_worktree_info(output, Path::new("/repo"));
        assert!(info[0].is_main); // path matches repo_path
    }

    // ── 13. parse_worktree_info: three worktrees ──

    #[test]
    fn test_parse_info_three_worktrees() {
        let output = "\
worktree /repo
HEAD abc
branch refs/heads/main

worktree /repo/.wt/feat1
HEAD def
branch refs/heads/feat1

worktree /repo/.wt/feat2
HEAD ghi
branch refs/heads/feat2

";
        let info = Git::parse_worktree_info(output, Path::new("/repo"));
        assert_eq!(info.len(), 3);
        assert!(info[0].is_main);
        assert!(!info[1].is_main);
        assert!(!info[2].is_main);
        assert_eq!(info[1].branch.as_deref(), Some("feat1"));
        assert_eq!(info[2].branch.as_deref(), Some("feat2"));
    }

    // ── 14. parse_worktree_info: commit hash preserved ──

    #[test]
    fn test_parse_info_commit_hash() {
        let output = "worktree /repo\nHEAD deadbeef1234567890\nbranch refs/heads/main\n\n";
        let info = Git::parse_worktree_info(output, Path::new("/repo"));
        assert_eq!(info[0]._commit, "deadbeef1234567890");
    }

    // ── 15. parse_worktree_paths: path with spaces ──

    #[test]
    fn test_parse_paths_with_spaces() {
        let output = "worktree /path with spaces/repo\nHEAD abc\n\n";
        let paths = Git::parse_worktree_paths(output);
        assert_eq!(paths, vec!["/path with spaces/repo"]);
    }

    // ── 16. parse_worktree_info: path with spaces ──

    #[test]
    fn test_parse_info_path_with_spaces() {
        let output = "worktree /path with spaces/repo\nHEAD abc\nbranch refs/heads/main\n\n";
        let info = Git::parse_worktree_info(output, Path::new("/path with spaces/repo"));
        assert_eq!(info[0].path, Path::new("/path with spaces/repo"));
        assert!(info[0].is_main);
    }

    // ── 17. parse_worktree_info: branch with slashes ──

    #[test]
    fn test_parse_info_branch_with_slashes() {
        let output = "worktree /wt\nHEAD abc\nbranch refs/heads/azureal/feature-name\n\n";
        let info = Git::parse_worktree_info(output, Path::new("/repo"));
        assert_eq!(info[0].branch.as_deref(), Some("azureal/feature-name"));
    }

    // ── 18. parse_worktree_paths: multiple paths ──

    #[test]
    fn test_parse_paths_many() {
        let output = (0..10)
            .map(|i| format!("worktree /repo/{}\nHEAD abc{}\n\n", i, i))
            .collect::<String>();
        let paths = Git::parse_worktree_paths(&output);
        assert_eq!(paths.len(), 10);
    }

    // ── 19. parse_worktree_info: no HEAD line (malformed) ──

    #[test]
    fn test_parse_info_missing_head_skips() {
        let output = "worktree /repo\nbranch refs/heads/main\n\nworktree /wt\nHEAD abc\nbranch refs/heads/feat\n\n";
        let info = Git::parse_worktree_info(output, Path::new("/repo"));
        // First entry has no HEAD so when second "worktree" line triggers flush,
        // current_commit is None so it's skipped
        assert_eq!(info.len(), 1);
        assert_eq!(info[0].branch.as_deref(), Some("feat"));
    }

    // ── 20. parse_worktree_info: only "worktree" line, no HEAD ──

    #[test]
    fn test_parse_info_worktree_only_no_head() {
        let output = "worktree /repo\n\n";
        let info = Git::parse_worktree_info(output, Path::new("/repo"));
        // No HEAD, so no entry created
        assert!(info.is_empty());
    }

    // ── 21. parse_worktree_info: branch line before worktree line ──

    #[test]
    fn test_parse_info_branch_before_worktree() {
        let output = "branch refs/heads/main\nworktree /repo\nHEAD abc\nbranch refs/heads/feat\n\n";
        let info = Git::parse_worktree_info(output, Path::new("/repo"));
        // First "branch" line is orphaned (no current_path), worktree line starts fresh
        assert_eq!(info.len(), 1);
        assert_eq!(info[0].branch.as_deref(), Some("feat"));
    }

    // ── 22. parse_worktree_info: main at non-repo path ──

    #[test]
    fn test_parse_info_main_branch_at_different_path() {
        let output = "worktree /wt1\nHEAD abc\nbranch refs/heads/main\n\n";
        let info = Git::parse_worktree_info(output, Path::new("/repo"));
        assert!(info[0].is_main); // "main" branch name makes it main
    }

    // ── 23. parse_worktree_info: feature at repo path ──

    #[test]
    fn test_parse_info_feature_at_repo_path_is_main() {
        let output = "worktree /repo\nHEAD abc\nbranch refs/heads/feature\n\n";
        let info = Git::parse_worktree_info(output, Path::new("/repo"));
        // Even though branch is "feature", path == repo_path makes it main
        assert!(info[0].is_main);
    }

    // ── 24. parse_worktree_paths: trailing newlines ──

    #[test]
    fn test_parse_paths_trailing_newlines() {
        let output = "worktree /repo\nHEAD abc\n\n\n\n";
        let paths = Git::parse_worktree_paths(output);
        assert_eq!(paths, vec!["/repo"]);
    }

    // ── 25. parse_worktree_info: extra blank lines between entries ──

    #[test]
    fn test_parse_info_extra_blank_lines() {
        let output = "worktree /repo\nHEAD abc\nbranch refs/heads/main\n\n\n\nworktree /wt\nHEAD def\nbranch refs/heads/feat\n\n";
        let info = Git::parse_worktree_info(output, Path::new("/repo"));
        assert_eq!(info.len(), 2);
    }

    // ── 26. parse_worktree_info: consecutive worktree lines ──

    #[test]
    fn test_parse_info_consecutive_worktree_lines() {
        // This is malformed but shouldn't panic
        let output = "worktree /a\nworktree /b\nHEAD abc\nbranch refs/heads/main\n\n";
        let info = Git::parse_worktree_info(output, Path::new("/repo"));
        // /a has no HEAD so gets skipped on flush. /b has HEAD + branch.
        assert_eq!(info.len(), 1);
        assert_eq!(info[0].path, Path::new("/b"));
    }

    // ── 27. parse_worktree_info: HEAD line with full 40-char hash ──

    #[test]
    fn test_parse_info_full_hash() {
        let hash = "a".repeat(40);
        let output = format!("worktree /repo\nHEAD {}\nbranch refs/heads/main\n\n", hash);
        let info = Git::parse_worktree_info(&output, Path::new("/repo"));
        assert_eq!(info[0]._commit, hash);
    }

    // ── 28. parse_worktree_paths: "worktree" without space is not matched ──

    #[test]
    fn test_parse_paths_worktree_no_space() {
        let output = "worktree_invalid\nworktree /valid\n";
        let paths = Git::parse_worktree_paths(output);
        assert_eq!(paths, vec!["/valid"]);
    }

    // ── 29. parse_worktree_info: branch with many slashes ──

    #[test]
    fn test_parse_info_deeply_nested_branch() {
        let output = "worktree /wt\nHEAD abc\nbranch refs/heads/user/feature/sub/task\n\n";
        let info = Git::parse_worktree_info(output, Path::new("/repo"));
        assert_eq!(info[0].branch.as_deref(), Some("user/feature/sub/task"));
    }

    // ── 30. parse_worktree_info: bare "branch" line (no refs/heads/) ──

    #[test]
    fn test_parse_info_bare_branch_line() {
        let output = "worktree /wt\nHEAD abc\nbranch main\n\n";
        let info = Git::parse_worktree_info(output, Path::new("/repo"));
        // "branch main" doesn't start with "branch refs/heads/" so branch is None
        assert!(info[0].branch.is_none());
    }

    // ── 31. parse_worktree_paths: windows-style paths ──

    #[test]
    fn test_parse_paths_windows_style() {
        let output = "worktree C:\\Users\\test\\repo\nHEAD abc\n\n";
        let paths = Git::parse_worktree_paths(output);
        assert_eq!(paths, vec!["C:\\Users\\test\\repo"]);
    }

    // ── 32. parse_worktree_info: single entry at end of input (no trailing newline) ──

    #[test]
    fn test_parse_info_no_trailing_newline() {
        let output = "worktree /repo\nHEAD abc\nbranch refs/heads/main";
        let info = Git::parse_worktree_info(output, Path::new("/repo"));
        assert_eq!(info.len(), 1);
        assert_eq!(info[0].branch.as_deref(), Some("main"));
    }

    // ── 33. parse_worktree_info: last entry without branch ──

    #[test]
    fn test_parse_info_last_entry_no_branch() {
        let output = "worktree /wt\nHEAD abc\n";
        let info = Git::parse_worktree_info(output, Path::new("/repo"));
        assert_eq!(info.len(), 1);
        assert!(info[0].branch.is_none());
        assert!(!info[0].is_main);
    }

    // ── 34. parse_worktree_info: "master" is also main ──

    #[test]
    fn test_parse_info_master_is_main() {
        let output = "worktree /wt\nHEAD abc\nbranch refs/heads/master\n\n";
        let info = Git::parse_worktree_info(output, Path::new("/other"));
        assert!(info[0].is_main);
    }

    // ── 35. parse_worktree_info: "develop" is not main ──

    #[test]
    fn test_parse_info_develop_not_main() {
        let output = "worktree /wt\nHEAD abc\nbranch refs/heads/develop\n\n";
        let info = Git::parse_worktree_info(output, Path::new("/other"));
        assert!(!info[0].is_main);
    }

    // ── 36. parse_worktree_paths: many worktrees ──

    #[test]
    fn test_parse_paths_50_worktrees() {
        let output: String = (0..50)
            .map(|i| format!("worktree /wt/{}\nHEAD abc{}\nbranch refs/heads/b{}\n\n", i, i, i))
            .collect();
        let paths = Git::parse_worktree_paths(&output);
        assert_eq!(paths.len(), 50);
        assert_eq!(paths[0], "/wt/0");
        assert_eq!(paths[49], "/wt/49");
    }

    // ── 37. parse_worktree_info: HEAD with spaces (unlikely but safe) ──

    #[test]
    fn test_parse_info_head_with_trailing_content() {
        // "HEAD abc extra" — everything after "HEAD " is the commit
        let output = "worktree /wt\nHEAD abc extra\nbranch refs/heads/main\n\n";
        let info = Git::parse_worktree_info(output, Path::new("/repo"));
        assert_eq!(info[0]._commit, "abc extra");
    }

    // ── 38. parse_worktree_info: empty branch after refs/heads/ ──

    #[test]
    fn test_parse_info_empty_branch_name() {
        let output = "worktree /wt\nHEAD abc\nbranch refs/heads/\n\n";
        let info = Git::parse_worktree_info(output, Path::new("/repo"));
        assert_eq!(info[0].branch.as_deref(), Some(""));
    }

    // ── 39. parse_worktree_paths: line "worktree " (empty path) ──

    #[test]
    fn test_parse_paths_empty_path() {
        let output = "worktree \n";
        let paths = Git::parse_worktree_paths(output);
        assert_eq!(paths, vec![""]);
    }

    // ── 40. parse_worktree_info: two mains (path match + branch match) ──

    #[test]
    fn test_parse_info_multiple_main_indicators() {
        let output = "worktree /repo\nHEAD abc\nbranch refs/heads/main\n\n";
        let info = Git::parse_worktree_info(output, Path::new("/repo"));
        // Both path match and branch match → still is_main true
        assert!(info[0].is_main);
    }

    // ── 41. parse_worktree_info: mixed main and feature worktrees ──

    #[test]
    fn test_parse_info_mixed_main_feature() {
        let output = "\
worktree /repo
HEAD abc
branch refs/heads/main

worktree /wt1
HEAD def
branch refs/heads/feat1

worktree /wt2
HEAD ghi
branch refs/heads/feat2

worktree /wt3
HEAD jkl
branch refs/heads/master

";
        let info = Git::parse_worktree_info(output, Path::new("/repo"));
        assert_eq!(info.len(), 4);
        assert!(info[0].is_main);   // main branch + path match
        assert!(!info[1].is_main);  // feat1
        assert!(!info[2].is_main);  // feat2
        assert!(info[3].is_main);   // master branch
    }

    // ── 42. parse_worktree_info: empty string produces empty vec ──

    #[test]
    fn test_parse_info_empty_string() {
        let info = Git::parse_worktree_info("", Path::new("/any"));
        assert!(info.is_empty());
    }

    // ── 43. parse_worktree_paths: "worktree" at end of line ──

    #[test]
    fn test_parse_paths_worktree_keyword_without_space() {
        let output = "worktree";
        let paths = Git::parse_worktree_paths(output);
        assert!(paths.is_empty()); // "worktree" doesn't start with "worktree "
    }

    // ── 44. parse_worktree_info: HEAD and branch swap order ──

    #[test]
    fn test_parse_info_branch_before_head() {
        let output = "worktree /wt\nbranch refs/heads/feat\nHEAD abc\n\n";
        let info = Git::parse_worktree_info(output, Path::new("/repo"));
        // Branch and HEAD can come in any order
        assert_eq!(info.len(), 1);
        assert_eq!(info[0].branch.as_deref(), Some("feat"));
        assert_eq!(info[0]._commit, "abc");
    }

    // ── 45. parse_worktree_info: only HEAD, no branch ──

    #[test]
    fn test_parse_info_only_head() {
        let output = "worktree /wt\nHEAD abc\n";
        let info = Git::parse_worktree_info(output, Path::new("/repo"));
        assert_eq!(info.len(), 1);
        assert!(info[0].branch.is_none());
    }

    // ── 46. parse_worktree_info: path ordering preserved ──

    #[test]
    fn test_parse_info_order_preserved() {
        let output = "\
worktree /z
HEAD z1
branch refs/heads/z

worktree /a
HEAD a1
branch refs/heads/a

worktree /m
HEAD m1
branch refs/heads/m

";
        let info = Git::parse_worktree_info(output, Path::new("/repo"));
        assert_eq!(info[0].path, Path::new("/z"));
        assert_eq!(info[1].path, Path::new("/a"));
        assert_eq!(info[2].path, Path::new("/m"));
    }

    // ── 47. parse_worktree_paths: line with "worktree " prefix in middle ──

    #[test]
    fn test_parse_paths_only_matches_line_start() {
        let output = "some worktree /repo\nworktree /valid\n";
        let paths = Git::parse_worktree_paths(output);
        // Only lines starting with "worktree " are matched
        assert_eq!(paths, vec!["/valid"]);
    }

    // ── 48. parse_worktree_info: long branch name ──

    #[test]
    fn test_parse_info_long_branch_name() {
        let branch = "a".repeat(200);
        let output = format!("worktree /wt\nHEAD abc\nbranch refs/heads/{}\n\n", branch);
        let info = Git::parse_worktree_info(&output, Path::new("/repo"));
        assert_eq!(info[0].branch.as_deref(), Some(branch.as_str()));
    }

    // ── 49. parse_worktree_info: unicode path ──

    #[test]
    fn test_parse_info_unicode_path() {
        let output = "worktree /日本語/パス\nHEAD abc\nbranch refs/heads/main\n\n";
        let info = Git::parse_worktree_info(output, Path::new("/日本語/パス"));
        assert_eq!(info[0].path, Path::new("/日本語/パス"));
        assert!(info[0].is_main);
    }

    // ── 50. parse_worktree_info: "bare" line is ignored ──

    #[test]
    fn test_parse_info_bare_line_ignored() {
        let output = "worktree /repo\nHEAD abc\nbranch refs/heads/main\nbare\n\n";
        let info = Git::parse_worktree_info(output, Path::new("/repo"));
        assert_eq!(info.len(), 1);
    }

    // ── 51. parse_worktree_info: "prunable" line is ignored ──

    #[test]
    fn test_parse_info_prunable_line_ignored() {
        let output = "worktree /repo\nHEAD abc\nbranch refs/heads/main\nprunable gitdir file points to non-existent location\n\n";
        let info = Git::parse_worktree_info(output, Path::new("/repo"));
        assert_eq!(info.len(), 1);
    }
}
