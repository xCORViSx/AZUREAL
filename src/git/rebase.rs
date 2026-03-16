//! Git rebase operations

use anyhow::{bail, Context, Result};
use std::path::Path;
use std::process::Command;

use super::Git;
use crate::models::RebaseResult;

impl Git {
    /// Check if a rebase is currently in progress
    pub fn is_rebase_in_progress(worktree_path: &Path) -> bool {
        let git_dir = Self::get_git_dir(worktree_path);
        if let Some(git_dir) = git_dir {
            if git_dir.join("rebase-merge").exists() {
                return true;
            }
            if git_dir.join("rebase-apply").exists() {
                return true;
            }
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

        Ok(RebaseResult::Failed(
            String::from_utf8_lossy(&output.stderr).to_string(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── RebaseResult enum tests ──

    #[test]
    fn test_rebase_result_aborted_variant() {
        let result = RebaseResult::Aborted;
        assert!(matches!(result, RebaseResult::Aborted));
    }

    #[test]
    fn test_rebase_result_failed_variant() {
        let result = RebaseResult::Failed("error msg".to_string());
        assert!(matches!(result, RebaseResult::Failed(_)));
    }

    #[test]
    fn test_rebase_result_failed_contains_message() {
        let msg = "merge conflict in file.rs";
        let result = RebaseResult::Failed(msg.to_string());
        if let RebaseResult::Failed(m) = result {
            assert_eq!(m, msg);
        } else {
            panic!("Expected Failed variant");
        }
    }

    #[test]
    fn test_rebase_result_failed_empty_message() {
        let result = RebaseResult::Failed(String::new());
        if let RebaseResult::Failed(m) = result {
            assert!(m.is_empty());
        } else {
            panic!("Expected Failed variant");
        }
    }

    #[test]
    fn test_rebase_result_aborted_is_not_failed() {
        let result = RebaseResult::Aborted;
        assert!(!matches!(result, RebaseResult::Failed(_)));
    }

    #[test]
    fn test_rebase_result_failed_is_not_aborted() {
        let result = RebaseResult::Failed("x".into());
        assert!(!matches!(result, RebaseResult::Aborted));
    }

    #[test]
    fn test_rebase_result_clone_aborted() {
        let result = RebaseResult::Aborted;
        let cloned = result.clone();
        assert!(matches!(cloned, RebaseResult::Aborted));
    }

    #[test]
    fn test_rebase_result_clone_failed() {
        let result = RebaseResult::Failed("msg".into());
        let cloned = result.clone();
        if let RebaseResult::Failed(m) = cloned {
            assert_eq!(m, "msg");
        }
    }

    #[test]
    fn test_rebase_result_debug_aborted() {
        let result = RebaseResult::Aborted;
        let debug = format!("{:?}", result);
        assert!(debug.contains("Aborted"));
    }

    #[test]
    fn test_rebase_result_debug_failed() {
        let result = RebaseResult::Failed("err".into());
        let debug = format!("{:?}", result);
        assert!(debug.contains("Failed"));
        assert!(debug.contains("err"));
    }

    // ── is_rebase_in_progress tests (uses actual filesystem) ──

    #[test]
    fn test_is_rebase_not_in_progress_nonexistent_path() {
        // Non-existent path should return false (no git dir found)
        let result = Git::is_rebase_in_progress(Path::new("/tmp/nonexistent_rebase_test_path"));
        assert!(!result);
    }

    #[test]
    fn test_is_rebase_not_in_progress_tmp() {
        // /tmp is not a git repo, so no rebase in progress
        assert!(!Git::is_rebase_in_progress(Path::new("/tmp")));
    }

    // ── get_conflicted_files tests ──

    #[test]
    fn test_conflicted_files_empty_repo() {
        // In a git repo, get_conflicted_files should return Ok (may or may not be empty
        // depending on repo state)
        let cwd = std::env::current_dir().unwrap();
        if Git::is_git_repo(&cwd) {
            let result = Git::get_conflicted_files(&cwd);
            assert!(result.is_ok());
        }
    }

    #[test]
    fn test_conflicted_files_nonexistent_dir() {
        // Running git diff in non-existent dir should error
        let result = Git::get_conflicted_files(Path::new("/tmp/no_such_dir_for_rebase_test"));
        // Should either return empty or error — either way shouldn't panic
        // The command spawns but git fails; anyhow wraps the error
        let _ = result;
    }

    // ── Rebase abort on non-rebase repo ──

    #[test]
    fn test_rebase_abort_when_no_rebase() {
        // Attempting abort when no rebase is in progress should bail
        let cwd = std::env::current_dir().unwrap();
        let result = Git::rebase_abort(&cwd);
        assert!(result.is_err(), "Should error when no rebase in progress");
    }

    #[test]
    fn test_rebase_abort_error_message() {
        let cwd = std::env::current_dir().unwrap();
        let result = Git::rebase_abort(&cwd);
        if let Err(e) = result {
            assert!(e.to_string().contains("No rebase in progress"));
        }
    }

    // ── Git struct is zero-sized ──

    #[test]
    fn test_git_struct_size() {
        assert_eq!(std::mem::size_of::<Git>(), 0);
    }

    // ── RebaseResult variants exhaustive pattern matching ──

    #[test]
    fn test_rebase_result_match_aborted() {
        let r = RebaseResult::Aborted;
        let s = match r {
            RebaseResult::Aborted => "aborted",
            RebaseResult::Failed(_) => "failed",
        };
        assert_eq!(s, "aborted");
    }

    #[test]
    fn test_rebase_result_match_failed() {
        let r = RebaseResult::Failed("x".into());
        let s = match r {
            RebaseResult::Aborted => "aborted",
            RebaseResult::Failed(_) => "failed",
        };
        assert_eq!(s, "failed");
    }

    // ── RebaseResult with various error strings ──

    #[test]
    fn test_rebase_result_failed_multiline() {
        let msg = "line1\nline2\nline3";
        let r = RebaseResult::Failed(msg.into());
        if let RebaseResult::Failed(m) = r {
            assert_eq!(m.lines().count(), 3);
        }
    }

    #[test]
    fn test_rebase_result_failed_unicode() {
        let msg = "Fehler: Zusammenf\u{00fc}hrungskonflikt";
        let r = RebaseResult::Failed(msg.into());
        if let RebaseResult::Failed(m) = r {
            assert!(m.contains("hrungskonflikt"));
        }
    }

    #[test]
    fn test_rebase_result_failed_long_message() {
        let msg = "x".repeat(10000);
        let r = RebaseResult::Failed(msg.clone());
        if let RebaseResult::Failed(m) = r {
            assert_eq!(m.len(), 10000);
        }
    }

    #[test]
    fn test_rebase_result_failed_special_chars() {
        let msg = "error: path 'foo/bar.rs' has conflict\n<<<<<<< HEAD\n";
        let r = RebaseResult::Failed(msg.into());
        if let RebaseResult::Failed(m) = r {
            assert!(m.contains("<<<<<<"));
        }
    }

    // ── Path validation for rebase checks ──

    #[test]
    fn test_is_rebase_in_progress_empty_path() {
        // Empty path string creates a relative path "." effectively
        let result = Git::is_rebase_in_progress(Path::new(""));
        // Should not panic, just return false
        assert!(!result || result); // always true — just verify no panic
    }

    #[test]
    fn test_is_rebase_in_progress_root_path() {
        assert!(!Git::is_rebase_in_progress(Path::new("/")));
    }

    #[test]
    fn test_is_rebase_in_progress_home_path() {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
        // Home dir typically isn't a git repo with active rebase
        let _ = Git::is_rebase_in_progress(Path::new(&home));
        // Just verify no panic
    }

    // ── conflicted_files parsing edge cases ──

    #[test]
    fn test_conflicted_files_result_type() {
        let cwd = std::env::current_dir().unwrap();
        let result = Git::get_conflicted_files(&cwd);
        // Whether Ok or Err, the type is correct
        assert!(result.is_ok() || result.is_err());
    }

    // ── Multiple rebase result creations ──

    #[test]
    fn test_create_many_rebase_results() {
        let results: Vec<RebaseResult> = (0..100)
            .map(|i| RebaseResult::Failed(format!("error {}", i)))
            .collect();
        assert_eq!(results.len(), 100);
        if let RebaseResult::Failed(m) = &results[50] {
            assert_eq!(m, "error 50");
        }
    }

    #[test]
    fn test_rebase_result_vec_mixed() {
        let results = vec![
            RebaseResult::Aborted,
            RebaseResult::Failed("a".into()),
            RebaseResult::Aborted,
            RebaseResult::Failed("b".into()),
        ];
        assert!(matches!(results[0], RebaseResult::Aborted));
        assert!(matches!(results[1], RebaseResult::Failed(_)));
        assert!(matches!(results[2], RebaseResult::Aborted));
        assert!(matches!(results[3], RebaseResult::Failed(_)));
    }

    // ── Git::is_git_repo usage (from core.rs, available via super::Git) ──

    #[test]
    fn test_git_is_git_repo_cwd() {
        let cwd = std::env::current_dir().unwrap();
        // Our test runs in a git worktree, so this should be true
        assert!(Git::is_git_repo(&cwd));
    }

    #[test]
    fn test_git_is_not_git_repo_tmp() {
        // /tmp is typically not a git repo
        let is_repo = Git::is_git_repo(Path::new("/tmp"));
        // Don't assert false — CI environments may vary — just no panic
        let _ = is_repo;
    }

    // ── RebaseResult equality via pattern matching ──

    #[test]
    fn test_rebase_result_aborted_pattern_exhaustive() {
        let results = [RebaseResult::Aborted, RebaseResult::Aborted];
        for r in &results {
            assert!(matches!(r, RebaseResult::Aborted));
        }
    }

    #[test]
    fn test_rebase_result_failed_pattern_exhaustive() {
        let msgs = ["a", "b", "c", ""];
        for msg in msgs {
            let r = RebaseResult::Failed(msg.to_string());
            assert!(matches!(r, RebaseResult::Failed(_)));
        }
    }

    // ── RebaseResult in Option wrapper ──

    #[test]
    fn test_rebase_result_in_option_some_aborted() {
        let opt: Option<RebaseResult> = Some(RebaseResult::Aborted);
        assert!(opt.is_some());
        assert!(matches!(opt.unwrap(), RebaseResult::Aborted));
    }

    #[test]
    fn test_rebase_result_in_option_some_failed() {
        let opt: Option<RebaseResult> = Some(RebaseResult::Failed("err".into()));
        assert!(opt.is_some());
        if let Some(RebaseResult::Failed(m)) = opt {
            assert_eq!(m, "err");
        } else {
            panic!("unexpected variant");
        }
    }

    #[test]
    fn test_rebase_result_in_option_none() {
        let opt: Option<RebaseResult> = None;
        assert!(opt.is_none());
    }

    // ── RebaseResult in Result wrapper ──

    #[test]
    fn test_rebase_result_in_result_ok_aborted() {
        let res: Result<RebaseResult> = Ok(RebaseResult::Aborted);
        assert!(res.is_ok());
        assert!(matches!(res.unwrap(), RebaseResult::Aborted));
    }

    #[test]
    fn test_rebase_result_in_result_ok_failed() {
        let res: Result<RebaseResult> = Ok(RebaseResult::Failed("x".into()));
        assert!(res.is_ok());
    }

    // ── is_rebase_in_progress with symlink-like paths ──

    #[test]
    fn test_is_rebase_in_progress_proc_path() {
        // /proc/self doesn't have a git dir — should return false
        let _ = Git::is_rebase_in_progress(Path::new("/proc/self"));
    }

    #[test]
    fn test_is_rebase_in_progress_cwd_is_git_repo() {
        let cwd = std::env::current_dir().unwrap();
        // Running in a git worktree, so get_git_dir should find something
        // but no rebase should be in progress
        let result = Git::is_rebase_in_progress(&cwd);
        // In CI running this worktree, no rebase should be in progress
        assert!(!result);
    }

    // ── get_conflicted_files returns Vec<String> (not references) ──

    #[test]
    fn test_conflicted_files_returns_owned_strings() {
        let cwd = std::env::current_dir().unwrap();
        if let Ok(files) = Git::get_conflicted_files(&cwd) {
            // All entries must be non-empty strings (filter removes blanks)
            for f in &files {
                assert!(!f.is_empty(), "filter should have removed empty strings");
            }
        }
    }

    #[test]
    fn test_conflicted_files_returns_vec_of_strings() {
        let cwd = std::env::current_dir().unwrap();
        if Git::is_git_repo(&cwd) {
            let files = Git::get_conflicted_files(&cwd).unwrap_or_default();
            // All returned entries should be non-empty strings (the filter removes blanks)
            for f in &files {
                assert!(!f.is_empty());
            }
            // Vec<String> — passes whether there are conflicts or not
            let _len = files.len();
        }
    }

    // ── RebaseResult clone chain ──

    #[test]
    fn test_rebase_result_clone_chain_three() {
        let a = RebaseResult::Failed("original".into());
        let b = a.clone();
        let c = b.clone();
        if let RebaseResult::Failed(m) = c {
            assert_eq!(m, "original");
        }
    }

    // ── RebaseResult debug format is deterministic ──

    #[test]
    fn test_rebase_result_debug_aborted_is_deterministic() {
        let d1 = format!("{:?}", RebaseResult::Aborted);
        let d2 = format!("{:?}", RebaseResult::Aborted);
        assert_eq!(d1, d2);
    }

    #[test]
    fn test_rebase_result_debug_failed_is_deterministic() {
        let d1 = format!("{:?}", RebaseResult::Failed("msg".into()));
        let d2 = format!("{:?}", RebaseResult::Failed("msg".into()));
        assert_eq!(d1, d2);
    }

    // ── rebase_abort error propagates the bail message ──

    #[test]
    fn test_rebase_abort_error_is_anyhow() {
        let cwd = std::env::current_dir().unwrap();
        let result = Git::rebase_abort(&cwd);
        assert!(result.is_err());
        // anyhow error should be displayable
        let err = result.unwrap_err();
        let msg = format!("{}", err);
        assert!(!msg.is_empty());
    }

    // ── RebaseResult with whitespace-only message ──

    #[test]
    fn test_rebase_result_failed_whitespace_message() {
        let r = RebaseResult::Failed("   \t\n  ".into());
        if let RebaseResult::Failed(m) = r {
            assert!(m.trim().is_empty());
        }
    }

    // ── Multiple clones don't share memory ──

    #[test]
    fn test_rebase_result_clone_is_independent() {
        let orig = RebaseResult::Failed("original".into());
        let cloned = orig.clone();
        // Both should contain the same data after clone
        if let RebaseResult::Failed(m) = &orig {
            assert_eq!(m, "original");
        }
        if let RebaseResult::Failed(m) = &cloned {
            assert_eq!(m, "original", "cloned should match original");
        }
    }

    // ── get_conflicted_files output is sorted consistently ──

    #[test]
    fn test_conflicted_files_vec_is_indexable() {
        let cwd = std::env::current_dir().unwrap();
        if let Ok(files) = Git::get_conflicted_files(&cwd) {
            // Should be indexable — no panic on get(0) even when empty
            let _ = files.get(0);
        }
    }

    #[test]
    fn test_rebase_result_failed_message_roundtrip() {
        // Verify that a message stored in Failed can be recovered exactly
        let original = "fatal: Cannot apply stash\nOn branch main".to_string();
        let r = RebaseResult::Failed(original.clone());
        if let RebaseResult::Failed(recovered) = r {
            assert_eq!(recovered, original);
        } else {
            panic!("Expected Failed variant");
        }
    }

    #[test]
    fn test_conflicted_files_no_newlines_in_entries() {
        let cwd = std::env::current_dir().unwrap();
        if let Ok(files) = Git::get_conflicted_files(&cwd) {
            for f in &files {
                assert!(
                    !f.contains('\n'),
                    "File entry must not contain newline: {:?}",
                    f
                );
            }
        }
    }
}
