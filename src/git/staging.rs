//! Git staging operations
//!
//! Stage, unstage, discard changes, and gitignore-aware index cleanup.

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::process::Command;

use super::Git;

/// Staging, stash, and ignore-maintenance operations for Git repositories.
impl Git {
    /// Stash marker used before Azureal starts an automatic rebase.
    pub const PRE_REBASE_STASH_MESSAGE: &'static str = "azureal-pre-rebase";
    /// Stash marker used before Azureal starts a squash merge.
    pub const PRE_SQUASH_MERGE_STASH_MESSAGE: &'static str = "azureal-pre-squash-merge";

    /// Stage all changes (tracked + untracked) via `git add -A`, then
    /// untrack any files that match `.gitignore` patterns. `git add -A`
    /// stages modifications to already-tracked files even if they're in
    /// `.gitignore` — this guard removes them from the index so they
    /// never end up in commits.
    pub fn stage_all(worktree_path: &Path) -> Result<()> {
        let output = Command::new("git")
            .args(["add", "-A"])
            .current_dir(worktree_path)
            .output()
            .context("Failed to stage changes")?;
        if !output.status.success() {
            anyhow::bail!("{}", String::from_utf8_lossy(&output.stderr).trim());
        }
        Self::untrack_gitignored_files(worktree_path);
        Ok(())
    }

    /// Stage a single file via `git add <path>`
    pub fn stage_file(worktree_path: &Path, file_path: &str) -> Result<()> {
        let output = Command::new("git")
            .args(["add", "--", file_path])
            .current_dir(worktree_path)
            .output()
            .context("Failed to stage file")?;
        if !output.status.success() {
            anyhow::bail!("{}", String::from_utf8_lossy(&output.stderr).trim());
        }
        Ok(())
    }

    /// Discard working tree changes for a single file (revert to HEAD).
    /// For untracked files, uses `git clean -f <path>` instead.
    pub fn discard_file(worktree_path: &Path, file_path: &str, is_untracked: bool) -> Result<()> {
        if is_untracked {
            let output = Command::new("git")
                .args(["clean", "-f", "--", file_path])
                .current_dir(worktree_path)
                .output()
                .context("Failed to clean untracked file")?;
            if !output.status.success() {
                anyhow::bail!("{}", String::from_utf8_lossy(&output.stderr).trim());
            }
        } else {
            // Unstage first (if staged), then restore working tree
            let _ = Command::new("git")
                .args(["restore", "--staged", "--", file_path])
                .current_dir(worktree_path)
                .output();
            let output = Command::new("git")
                .args(["restore", "--", file_path])
                .current_dir(worktree_path)
                .output()
                .context("Failed to discard file changes")?;
            if !output.status.success() {
                anyhow::bail!("{}", String::from_utf8_lossy(&output.stderr).trim());
            }
        }
        Ok(())
    }

    /// Unstage all files via `git reset HEAD`
    pub fn unstage_all(worktree_path: &Path) -> Result<()> {
        let output = Command::new("git")
            .args(["reset", "HEAD"])
            .current_dir(worktree_path)
            .output()
            .context("Failed to unstage all")?;
        if !output.status.success() {
            anyhow::bail!("{}", String::from_utf8_lossy(&output.stderr).trim());
        }
        Ok(())
    }

    /// Find tracked files that match `.gitignore` and remove them from the
    /// index (`git rm --cached`). Does NOT delete the working-tree copy.
    /// Silently no-ops if nothing matches or if any git command fails.
    pub fn untrack_gitignored_files(path: &Path) {
        let ls = Command::new("git")
            .args(["ls-files", "-ic", "--exclude-standard"])
            .current_dir(path)
            .output();
        let files: Vec<String> = match ls {
            Ok(ref o) if o.status.success() => String::from_utf8_lossy(&o.stdout)
                .lines()
                .filter(|l| !l.trim().is_empty())
                .map(|l| l.to_string())
                .collect(),
            _ => return,
        };
        if files.is_empty() {
            return;
        }
        let mut cmd = Command::new("git");
        cmd.args(["rm", "--cached", "--quiet", "--"]);
        for f in &files {
            cmd.arg(f);
        }
        let _ = cmd.current_dir(path).output();
    }

    /// Ignore entries that keep Azureal runtime files out of repository status.
    ///
    /// Each tuple contains the canonical form to write and accepted variants
    /// already found in `.gitignore` or Git's local exclude file.
    const REQUIRED_GITIGNORE: &[(&str, &[&str])] = &[
        (
            "worktrees/",
            &["worktrees", "worktrees/", "/worktrees", "/worktrees/"],
        ),
        (
            ".azureal/",
            &[".azureal", ".azureal/", "/.azureal", "/.azureal/"],
        ),
    ];

    /// Stash all changes (tracked + untracked) via `git stash push -u`
    pub fn stash(worktree_path: &Path) -> Result<String> {
        let output = Command::new("git")
            .args(["stash", "push", "-u"])
            .current_dir(worktree_path)
            .output()
            .context("Failed to run git stash")?;
        if !output.status.success() {
            anyhow::bail!("{}", String::from_utf8_lossy(&output.stderr).trim());
        }
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }

    /// Stash tracked and untracked changes with a message. Returns true when
    /// Git actually created a stash entry.
    pub fn stash_push_named_include_untracked(worktree_path: &Path, message: &str) -> Result<bool> {
        let output = Command::new("git")
            .args(["stash", "push", "--include-untracked", "-m", message])
            .current_dir(worktree_path)
            .output()
            .context("Failed to run git stash")?;
        if !output.status.success() {
            anyhow::bail!("{}", String::from_utf8_lossy(&output.stderr).trim());
        }
        let stdout = String::from_utf8_lossy(&output.stdout);
        Ok(!stdout.contains("No local changes"))
    }

    /// Pop the newest stash whose subject contains `message`.
    /// Returns false when no matching stash entry exists.
    pub fn stash_pop_by_message(worktree_path: &Path, message: &str) -> Result<bool> {
        let list = Command::new("git")
            .args(["stash", "list", "--format=%gd%x00%gs"])
            .current_dir(worktree_path)
            .output()
            .context("Failed to list git stashes")?;
        if !list.status.success() {
            anyhow::bail!("{}", String::from_utf8_lossy(&list.stderr).trim());
        }

        for line in String::from_utf8_lossy(&list.stdout).lines() {
            let mut parts = line.splitn(2, '\0');
            let Some(stash_ref) = parts.next() else {
                continue;
            };
            let subject = parts.next().unwrap_or("");
            if subject.contains(message) {
                let output = Command::new("git")
                    .args(["stash", "pop", stash_ref])
                    .current_dir(worktree_path)
                    .output()
                    .context("Failed to run git stash pop")?;
                if !output.status.success() {
                    anyhow::bail!("{}", String::from_utf8_lossy(&output.stderr).trim());
                }
                return Ok(true);
            }
        }

        Ok(false)
    }

    /// Pop the most recent stash entry via `git stash pop`
    pub fn stash_pop(worktree_path: &Path) -> Result<String> {
        let output = Command::new("git")
            .args(["stash", "pop"])
            .current_dir(worktree_path)
            .output()
            .context("Failed to run git stash pop")?;
        if !output.status.success() {
            anyhow::bail!("{}", String::from_utf8_lossy(&output.stderr).trim());
        }
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }

    /// Ensure Azureal runtime directories are ignored without modifying tracked files.
    ///
    /// Project `.gitignore` entries are respected when present, but missing entries
    /// are appended to Git's local `info/exclude` file so startup never stages,
    /// commits, or otherwise dirties the user's repository.
    pub fn ensure_worktrees_gitignored(repo_root: &Path) {
        let gitignore_content =
            std::fs::read_to_string(repo_root.join(".gitignore")).unwrap_or_default();
        let Some(exclude_path) = Self::git_info_exclude_path(repo_root) else {
            return;
        };
        let exclude_content = std::fs::read_to_string(&exclude_path).unwrap_or_default();

        let mut missing: Vec<&str> = Vec::new();
        for (canonical, variants) in Self::REQUIRED_GITIGNORE {
            let covered = Self::ignore_content_covers(&gitignore_content, variants)
                || Self::ignore_content_covers(&exclude_content, variants);
            if !covered {
                missing.push(canonical);
            }
        }
        if missing.is_empty() {
            return;
        }

        if let Some(parent) = exclude_path.parent() {
            if std::fs::create_dir_all(parent).is_err() {
                return;
            }
        }

        let mut new = exclude_content;
        if !new.is_empty() && !new.ends_with('\n') {
            new.push('\n');
        }
        if !new
            .lines()
            .any(|line| line.trim() == "# Azureal local ignores")
        {
            new.push_str("# Azureal local ignores\n");
        }
        for entry in &missing {
            new.push_str(entry);
            new.push('\n');
        }

        let _ = std::fs::write(exclude_path, new);
    }

    /// Resolve the repository-local Git exclude path through Git's own path rules.
    fn git_info_exclude_path(repo_root: &Path) -> Option<PathBuf> {
        let output = Command::new("git")
            .args(["rev-parse", "--git-path", "info/exclude"])
            .current_dir(repo_root)
            .output()
            .ok()?;
        if !output.status.success() {
            return None;
        }
        let raw_path = String::from_utf8_lossy(&output.stdout);
        let trimmed = raw_path.trim();
        if trimmed.is_empty() {
            return None;
        }
        let path = PathBuf::from(trimmed);
        Some(if path.is_absolute() {
            path
        } else {
            repo_root.join(path)
        })
    }

    /// Check whether existing ignore content already covers one required entry.
    fn ignore_content_covers(content: &str, variants: &[&str]) -> bool {
        content.lines().any(|line| variants.contains(&line.trim()))
    }
}

/// Unit tests for staging helpers that can run against temporary Git repositories.
#[cfg(test)]
mod tests {
    use super::Git;
    use std::fs;
    use std::path::Path;
    use std::process::Command;

    /// Run a Git command in a test repository and return its raw output.
    fn run_git(dir: &Path, args: &[&str]) -> std::process::Output {
        Command::new("git")
            .args(args)
            .current_dir(dir)
            .output()
            .unwrap_or_else(|e| panic!("git {:?} failed to spawn: {}", args, e))
    }

    /// Run a Git command in a test repository and assert it exits successfully.
    fn run_git_ok(dir: &Path, args: &[&str]) {
        let output = run_git(dir, args);
        assert!(
            output.status.success(),
            "git {:?} failed\nstdout: {}\nstderr: {}",
            args,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    /// Create a committed temporary repository with user identity configured.
    fn committed_repo() -> tempfile::TempDir {
        let repo = tempfile::tempdir().unwrap();
        let repo_path = repo.path();
        run_git_ok(repo_path, &["init", "-q", "-b", "main"]);
        run_git_ok(repo_path, &["config", "user.email", "test@example.com"]);
        run_git_ok(repo_path, &["config", "user.name", "Test"]);
        fs::write(repo_path.join("README.md"), "base\n").unwrap();
        run_git_ok(repo_path, &["add", "README.md"]);
        run_git_ok(repo_path, &["commit", "-qm", "base"]);
        repo
    }

    /// Stash lookup by Azureal marker leaves unrelated user stashes untouched.
    #[test]
    fn test_stash_pop_by_message_preserves_unmatched_user_stash() {
        let repo = committed_repo();
        let repo_path = repo.path();

        fs::write(repo_path.join("tracked.txt"), "base\n").unwrap();
        run_git_ok(repo_path, &["add", "tracked.txt"]);
        run_git_ok(repo_path, &["commit", "-qm", "base"]);

        fs::write(repo_path.join("scratch.txt"), "user stash\n").unwrap();
        run_git_ok(
            repo_path,
            &["stash", "push", "--include-untracked", "-m", "user-scratch"],
        );

        let popped = Git::stash_pop_by_message(repo_path, Git::PRE_REBASE_STASH_MESSAGE).unwrap();
        assert!(!popped, "non-Azureal stash should not be popped");
        assert!(
            !repo_path.join("scratch.txt").exists(),
            "user stash should still hide scratch.txt"
        );

        let stash_list = run_git(repo_path, &["stash", "list"]);
        let stash_text = String::from_utf8_lossy(&stash_list.stdout);
        assert!(
            stash_text.contains("user-scratch"),
            "user stash should remain in stash list: {}",
            stash_text
        );
    }

    /// Startup ignore maintenance writes local excludes and does not create commits.
    #[test]
    fn test_ensure_worktrees_gitignored_uses_local_exclude_without_committing() {
        let repo = committed_repo();
        let repo_path = repo.path();
        let before = run_git(repo_path, &["rev-list", "--count", "HEAD"]);

        Git::ensure_worktrees_gitignored(repo_path);

        assert!(
            !repo_path.join(".gitignore").exists(),
            "startup ignore maintenance should not create a tracked .gitignore"
        );
        let exclude_path = Git::git_info_exclude_path(repo_path).unwrap();
        let exclude = fs::read_to_string(exclude_path).unwrap();
        assert!(exclude.contains("worktrees/"));
        assert!(exclude.contains(".azureal/"));

        let after = run_git(repo_path, &["rev-list", "--count", "HEAD"]);
        assert_eq!(
            before.stdout, after.stdout,
            "ignore maintenance must not commit"
        );
        let status = run_git(repo_path, &["status", "--porcelain"]);
        assert!(
            String::from_utf8_lossy(&status.stdout).is_empty(),
            "local exclude updates should leave worktree status clean"
        );
    }

    /// Existing project `.gitignore` coverage prevents duplicate local exclude entries.
    #[test]
    fn test_ensure_worktrees_gitignored_respects_existing_gitignore_entries() {
        let repo = committed_repo();
        let repo_path = repo.path();
        fs::write(repo_path.join(".gitignore"), "worktrees/\n").unwrap();

        Git::ensure_worktrees_gitignored(repo_path);

        let exclude_path = Git::git_info_exclude_path(repo_path).unwrap();
        let exclude = fs::read_to_string(exclude_path).unwrap();
        assert!(!exclude.lines().any(|line| line.trim() == "worktrees/"));
        assert!(exclude.lines().any(|line| line.trim() == ".azureal/"));
    }

    /// Re-running ignore maintenance is idempotent for Git's local exclude file.
    #[test]
    fn test_ensure_worktrees_gitignored_does_not_duplicate_local_entries() {
        let repo = committed_repo();
        let repo_path = repo.path();

        Git::ensure_worktrees_gitignored(repo_path);
        Git::ensure_worktrees_gitignored(repo_path);

        let exclude_path = Git::git_info_exclude_path(repo_path).unwrap();
        let exclude = fs::read_to_string(exclude_path).unwrap();
        assert_eq!(
            exclude
                .lines()
                .filter(|line| line.trim() == "worktrees/")
                .count(),
            1
        );
        assert_eq!(
            exclude
                .lines()
                .filter(|line| line.trim() == ".azureal/")
                .count(),
            1
        );
    }
}
