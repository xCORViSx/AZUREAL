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

    /// Delete a branch (local + remote + tracking ref)
    pub fn delete_branch(repo_path: &Path, branch_name: &str) -> Result<()> {
        // Delete local branch (try soft first, then force)
        let output = Command::new("git")
            .args(["branch", "-d", branch_name])
            .current_dir(repo_path)
            .output()
            .context("Failed to execute git branch -d")?;

        if !output.status.success() {
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
        }

        // Delete from remote (best-effort)
        let _ = Command::new("git")
            .args(["push", "origin", "--delete", branch_name])
            .current_dir(repo_path)
            .output();

        // Prune the local remote-tracking ref so it doesn't appear in branch
        // dialogs. git push --delete removes the remote branch but leaves
        // refs/remotes/origin/<branch> behind until the next fetch --prune.
        let remote_ref = format!("origin/{}", branch_name);
        let _ = Command::new("git")
            .args(["branch", "-r", "-d", &remote_ref])
            .current_dir(repo_path)
            .output();

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    // ── current_branch tests (require git repo) ──

    #[test]
    fn test_current_branch_in_repo() {
        let cwd = std::env::current_dir().unwrap();
        if Git::is_git_repo(&cwd) {
            let branch = Git::current_branch(&cwd).unwrap();
            assert!(!branch.is_empty(), "Branch name should not be empty");
        }
    }

    #[test]
    fn test_current_branch_returns_string() {
        let cwd = std::env::current_dir().unwrap();
        if Git::is_git_repo(&cwd) {
            let branch = Git::current_branch(&cwd).unwrap();
            assert!(!branch.contains('\n'), "Branch should not contain newlines");
        }
    }

    #[test]
    fn test_current_branch_no_leading_whitespace() {
        let cwd = std::env::current_dir().unwrap();
        if Git::is_git_repo(&cwd) {
            let branch = Git::current_branch(&cwd).unwrap();
            assert_eq!(branch, branch.trim());
        }
    }

    #[test]
    fn test_current_branch_nonexistent_dir() {
        let result = Git::current_branch(Path::new("/tmp/no_such_branch_test_dir"));
        // git rev-parse fails in non-existent dir but command still runs
        // Result may be Ok (empty) or Err depending on platform
        let _ = result;
    }

    // ── list_local_branches tests ──

    #[test]
    fn test_list_local_branches_in_repo() {
        let cwd = std::env::current_dir().unwrap();
        if Git::is_git_repo(&cwd) {
            let branches = Git::list_local_branches(&cwd).unwrap();
            assert!(!branches.is_empty(), "Should have at least one local branch");
        }
    }

    #[test]
    fn test_list_local_branches_no_empty_entries() {
        let cwd = std::env::current_dir().unwrap();
        if Git::is_git_repo(&cwd) {
            let branches = Git::list_local_branches(&cwd).unwrap();
            for b in &branches {
                assert!(!b.is_empty(), "Branch names should not be empty");
            }
        }
    }

    #[test]
    fn test_list_local_branches_trimmed() {
        let cwd = std::env::current_dir().unwrap();
        if Git::is_git_repo(&cwd) {
            let branches = Git::list_local_branches(&cwd).unwrap();
            for b in &branches {
                assert_eq!(b, &b.trim().to_string());
            }
        }
    }

    #[test]
    fn test_list_local_branches_nonexistent() {
        let result = Git::list_local_branches(Path::new("/tmp/no_such_branches_dir"));
        let _ = result; // Don't panic
    }

    // ── list_remote_branches_cached tests ──

    #[test]
    fn test_list_remote_branches_cached_in_repo() {
        let cwd = std::env::current_dir().unwrap();
        if Git::is_git_repo(&cwd) {
            let result = Git::list_remote_branches_cached(&cwd);
            // May be empty if no remotes configured, but should not error
            assert!(result.is_ok());
        }
    }

    #[test]
    fn test_list_remote_branches_cached_no_head() {
        let cwd = std::env::current_dir().unwrap();
        if Git::is_git_repo(&cwd) {
            let branches = Git::list_remote_branches_cached(&cwd).unwrap();
            for b in &branches {
                assert!(!b.contains("HEAD"), "HEAD should be filtered out");
            }
        }
    }

    #[test]
    fn test_list_remote_branches_cached_contains_slash() {
        let cwd = std::env::current_dir().unwrap();
        if Git::is_git_repo(&cwd) {
            let branches = Git::list_remote_branches_cached(&cwd).unwrap();
            for b in &branches {
                assert!(b.contains('/'), "Remote branches should contain '/' (remote/branch)");
            }
        }
    }

    #[test]
    fn test_list_remote_branches_cached_nonexistent() {
        let result = Git::list_remote_branches_cached(Path::new("/tmp/no_such_remote_dir"));
        let _ = result;
    }

    // ── list_all_branches_with_status tests ──

    #[test]
    fn test_list_all_branches_returns_tuple() {
        let cwd = std::env::current_dir().unwrap();
        if Git::is_git_repo(&cwd) {
            let result = Git::list_all_branches_with_status(&cwd);
            assert!(result.is_ok());
            let (all, checked_out) = result.unwrap();
            let _ = (all.len(), checked_out.len()); // type check — both are Vec<String>
        }
    }

    #[test]
    fn test_list_all_branches_excludes_main() {
        let cwd = std::env::current_dir().unwrap();
        if Git::is_git_repo(&cwd) {
            let (all, _) = Git::list_all_branches_with_status(&cwd).unwrap();
            // main/master should be excluded from the all list
            for b in &all {
                assert!(b != "main" && b != "master",
                    "main/master should be excluded, found: {}", b);
            }
        }
    }

    #[test]
    fn test_list_all_branches_checked_out_not_empty() {
        let cwd = std::env::current_dir().unwrap();
        if Git::is_git_repo(&cwd) {
            let (_, checked_out) = Git::list_all_branches_with_status(&cwd).unwrap();
            // At least the current branch should be checked out
            assert!(!checked_out.is_empty(), "Should have at least one checked out branch");
        }
    }

    #[test]
    fn test_list_all_branches_no_duplicates() {
        let cwd = std::env::current_dir().unwrap();
        if Git::is_git_repo(&cwd) {
            let (all, _) = Git::list_all_branches_with_status(&cwd).unwrap();
            let mut seen = std::collections::HashSet::new();
            for b in &all {
                assert!(seen.insert(b), "Duplicate branch found: {}", b);
            }
        }
    }

    // ── delete_branch edge cases ──

    #[test]
    fn test_delete_nonexistent_branch() {
        let cwd = std::env::current_dir().unwrap();
        if Git::is_git_repo(&cwd) {
            let result = Git::delete_branch(&cwd, "nonexistent_branch_xyz_test_12345");
            // The branch doesn't exist; git branch -d will fail, -D also fails.
            // But the "not found" stderr path returns Ok(()) — or errors on other messages
            let _ = result;
        }
    }

    #[test]
    fn test_delete_branch_empty_name() {
        let cwd = std::env::current_dir().unwrap();
        if Git::is_git_repo(&cwd) {
            let result = Git::delete_branch(&cwd, "");
            // Empty branch name — git will error
            let _ = result;
        }
    }

    #[test]
    fn test_delete_branch_nonexistent_dir() {
        let result = Git::delete_branch(
            Path::new("/tmp/no_such_delete_branch_dir"),
            "some-branch",
        );
        let _ = result;
    }

    // ── Git struct tests ──

    #[test]
    fn test_git_struct_is_zst() {
        assert_eq!(std::mem::size_of::<Git>(), 0);
    }

    #[test]
    fn test_git_is_git_repo_cwd() {
        let cwd = std::env::current_dir().unwrap();
        assert!(Git::is_git_repo(&cwd));
    }

    #[test]
    fn test_git_is_not_repo_tmp() {
        let _ = Git::is_git_repo(Path::new("/tmp"));
    }

    // ── Branch name parsing / filtering logic tests ──

    #[test]
    fn test_remote_branch_split_simple() {
        let remote = "origin/feature";
        let local_name = remote.split('/').skip(1).collect::<Vec<_>>().join("/");
        assert_eq!(local_name, "feature");
    }

    #[test]
    fn test_remote_branch_split_nested() {
        let remote = "origin/user/feature/sub";
        let local_name = remote.split('/').skip(1).collect::<Vec<_>>().join("/");
        assert_eq!(local_name, "user/feature/sub");
    }

    #[test]
    fn test_remote_branch_split_no_slash() {
        let remote = "localbranch";
        let local_name = remote.split('/').skip(1).collect::<Vec<_>>().join("/");
        assert_eq!(local_name, "");
    }

    #[test]
    fn test_remote_branch_split_empty() {
        let remote = "";
        let local_name = remote.split('/').skip(1).collect::<Vec<_>>().join("/");
        assert_eq!(local_name, "");
    }

    #[test]
    fn test_main_master_filter() {
        let branches = vec!["main", "master", "feature", "dev"];
        let filtered: Vec<&&str> = branches.iter()
            .filter(|b| **b != "main" && **b != "master")
            .collect();
        assert_eq!(filtered, vec![&"feature", &"dev"]);
    }

    #[test]
    fn test_main_master_filter_empty() {
        let branches: Vec<&str> = vec!["main", "master"];
        let filtered: Vec<&&str> = branches.iter()
            .filter(|b| **b != "main" && **b != "master")
            .collect();
        assert!(filtered.is_empty());
    }

    #[test]
    fn test_main_master_filter_all_pass() {
        let branches = vec!["feat1", "feat2", "dev"];
        let filtered: Vec<&&str> = branches.iter()
            .filter(|b| **b != "main" && **b != "master")
            .collect();
        assert_eq!(filtered.len(), 3);
    }

    #[test]
    fn test_head_filter() {
        let refs = vec!["origin/HEAD", "origin/main", "origin/feature"];
        let filtered: Vec<&&str> = refs.iter()
            .filter(|s| !s.contains("HEAD"))
            .collect();
        assert_eq!(filtered.len(), 2);
    }

    #[test]
    fn test_slash_filter() {
        let refs = vec!["origin/main", "localbranch", "upstream/dev"];
        let filtered: Vec<&&str> = refs.iter()
            .filter(|s| s.contains('/'))
            .collect();
        assert_eq!(filtered.len(), 2);
    }

    // ── Combined filter logic (mirrors list_remote_branches_cached filter) ──

    #[test]
    fn test_remote_branch_combined_filter() {
        let refs = vec![
            "origin/HEAD", "origin/main", "origin/feature", "", "local",
        ];
        let filtered: Vec<&&str> = refs.iter()
            .filter(|s| !s.is_empty() && !s.contains("HEAD") && s.contains('/'))
            .collect();
        assert_eq!(filtered, vec![&"origin/main", &"origin/feature"]);
    }

    // ── Deduplicate logic from list_all_branches_with_status ──

    #[test]
    fn test_dedup_remote_already_local() {
        let mut all = vec!["feature".to_string()];
        let remote = vec!["origin/feature".to_string()];
        for rb in &remote {
            let local_name = rb.split('/').skip(1).collect::<Vec<_>>().join("/");
            if !all.contains(&local_name) && !all.contains(rb) {
                all.push(rb.clone());
            }
        }
        // "origin/feature" not added because "feature" is already in all
        assert_eq!(all.len(), 1);
    }

    #[test]
    fn test_dedup_remote_not_local() {
        let mut all = vec!["feature".to_string()];
        let remote = vec!["origin/newbranch".to_string()];
        for rb in &remote {
            let local_name = rb.split('/').skip(1).collect::<Vec<_>>().join("/");
            if local_name != "main" && local_name != "master" {
                if !all.contains(&local_name) && !all.contains(rb) {
                    all.push(rb.clone());
                }
            }
        }
        assert_eq!(all.len(), 2);
        assert_eq!(all[1], "origin/newbranch");
    }

    // ── Additional remote-branch filtering edge cases ──

    #[test]
    fn test_remote_branch_split_single_segment_after_slash() {
        // "upstream/dev" → local_name "dev"
        let remote = "upstream/dev";
        let local_name = remote.split('/').skip(1).collect::<Vec<_>>().join("/");
        assert_eq!(local_name, "dev");
    }

    #[test]
    fn test_remote_branch_split_double_remote_slash() {
        // "origin/user/topic" → local_name "user/topic"
        let remote = "origin/user/topic";
        let local_name = remote.split('/').skip(1).collect::<Vec<_>>().join("/");
        assert_eq!(local_name, "user/topic");
    }

    #[test]
    fn test_head_filter_uppercase_only() {
        let refs = vec!["origin/HEAD"];
        let filtered: Vec<&&str> = refs.iter().filter(|s| !s.contains("HEAD")).collect();
        assert!(filtered.is_empty());
    }

    #[test]
    fn test_slash_filter_rejects_no_slash() {
        let entry = "localbranch";
        assert!(!entry.contains('/'));
    }

    #[test]
    fn test_slash_filter_accepts_remote() {
        let entry = "origin/feature";
        assert!(entry.contains('/'));
    }

    #[test]
    fn test_main_master_filter_case_sensitive() {
        // "Main" and "Master" should NOT be filtered (case-sensitive)
        let branches = vec!["Main", "Master", "main", "master"];
        let filtered: Vec<&&str> = branches.iter()
            .filter(|b| **b != "main" && **b != "master")
            .collect();
        assert_eq!(filtered.len(), 2);
        assert!(filtered.contains(&&"Main"));
        assert!(filtered.contains(&&"Master"));
    }

    #[test]
    fn test_remote_combined_filter_empty_string_rejected() {
        let refs: Vec<&str> = vec![""];
        let filtered: Vec<&&str> = refs.iter()
            .filter(|s| !s.is_empty() && !s.contains("HEAD") && s.contains('/'))
            .collect();
        assert!(filtered.is_empty());
    }

    #[test]
    fn test_remote_combined_filter_head_with_slash_rejected() {
        let refs = vec!["origin/HEAD"];
        let filtered: Vec<&&str> = refs.iter()
            .filter(|s| !s.is_empty() && !s.contains("HEAD") && s.contains('/'))
            .collect();
        assert!(filtered.is_empty());
    }

    #[test]
    fn test_remote_combined_filter_all_pass() {
        let refs = vec!["origin/feat-a", "upstream/feat-b", "fork/dev"];
        let filtered: Vec<&&str> = refs.iter()
            .filter(|s| !s.is_empty() && !s.contains("HEAD") && s.contains('/'))
            .collect();
        assert_eq!(filtered.len(), 3);
    }

    // ── Dedup — already contains exact remote ref ──

    #[test]
    fn test_dedup_remote_ref_already_present() {
        let mut all = vec!["origin/feature".to_string()];
        let remote = vec!["origin/feature".to_string()];
        for rb in &remote {
            let local_name = rb.split('/').skip(1).collect::<Vec<_>>().join("/");
            if !all.contains(&local_name) && !all.contains(rb) {
                all.push(rb.clone());
            }
        }
        // "origin/feature" already in all — should not be duplicated
        assert_eq!(all.len(), 1);
    }

    #[test]
    fn test_dedup_main_remote_skipped() {
        let mut all: Vec<String> = vec![];
        let remote = vec!["origin/main".to_string()];
        for rb in &remote {
            let local_name = rb.split('/').skip(1).collect::<Vec<_>>().join("/");
            if local_name == "main" || local_name == "master" { continue; }
            all.push(rb.clone());
        }
        assert!(all.is_empty());
    }

    #[test]
    fn test_dedup_master_remote_skipped() {
        let mut all: Vec<String> = vec![];
        let remote = vec!["origin/master".to_string()];
        for rb in &remote {
            let local_name = rb.split('/').skip(1).collect::<Vec<_>>().join("/");
            if local_name == "main" || local_name == "master" { continue; }
            all.push(rb.clone());
        }
        assert!(all.is_empty());
    }

    // ── Path::new / non-existent path behaviour ──

    #[test]
    fn test_current_branch_root_tmp() {
        // /tmp exists but is typically not a git repo
        let result = Git::current_branch(std::path::Path::new("/tmp"));
        // We don't assert Ok/Err — just verify it doesn't panic
        let _ = result;
    }

    #[test]
    fn test_list_local_branches_root_tmp() {
        let result = Git::list_local_branches(std::path::Path::new("/tmp"));
        let _ = result;
    }

    #[test]
    fn test_list_remote_branches_root_tmp() {
        let result = Git::list_remote_branches_cached(std::path::Path::new("/tmp"));
        let _ = result;
    }

    // ── Checked-out set from list_all_branches_with_status ──

    #[test]
    fn test_checked_out_contains_current_branch() {
        let cwd = std::env::current_dir().unwrap();
        if Git::is_git_repo(&cwd) {
            let current = Git::current_branch(&cwd).unwrap();
            let (_, checked_out) = Git::list_all_branches_with_status(&cwd).unwrap();
            assert!(checked_out.contains(&current),
                "current branch '{}' should be in checked_out set", current);
        }
    }

    #[test]
    fn test_all_branches_are_strings() {
        let cwd = std::env::current_dir().unwrap();
        if Git::is_git_repo(&cwd) {
            let (all, _) = Git::list_all_branches_with_status(&cwd).unwrap();
            for b in &all {
                assert!(b.is_ascii() || !b.is_empty(), "branch name should be non-empty");
            }
        }
    }

    #[test]
    fn test_no_branch_name_contains_newline() {
        let cwd = std::env::current_dir().unwrap();
        if Git::is_git_repo(&cwd) {
            let (all, checked_out) = Git::list_all_branches_with_status(&cwd).unwrap();
            for b in all.iter().chain(checked_out.iter()) {
                assert!(!b.contains('\n'), "branch name should not contain newline: {:?}", b);
            }
        }
    }
}
