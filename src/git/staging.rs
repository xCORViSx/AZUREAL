//! Git staging operations
//!
//! Stage, unstage, discard changes, and gitignore-aware index cleanup.

use anyhow::{Context, Result};
use std::path::Path;
use std::process::Command;

use super::Git;

impl Git {
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
            .args(["ls-files", "-i", "--exclude-standard"])
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

    /// Ensure `worktrees/` is listed in the project's `.gitignore`.
    /// If missing, appends it, stages `.gitignore`, and commits.
    /// Silently no-ops if already present or if any step fails.
    /// Entries that must be in .gitignore for azureal to work correctly.
    /// Each tuple: (canonical form to write, all accepted variants).
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

    pub fn ensure_worktrees_gitignored(repo_root: &Path) {
        let gitignore = repo_root.join(".gitignore");
        let content = std::fs::read_to_string(&gitignore).unwrap_or_default();

        // collect missing entries
        let mut missing: Vec<&str> = Vec::new();
        for (canonical, variants) in Self::REQUIRED_GITIGNORE {
            let covered = content.lines().any(|line| {
                let l = line.trim();
                variants.contains(&l)
            });
            if !covered {
                missing.push(canonical);
            }
        }
        if missing.is_empty() {
            return;
        }

        // append missing entries
        let mut new = content.clone();
        if !new.is_empty() && !new.ends_with('\n') {
            new.push('\n');
        }
        for entry in &missing {
            new.push_str(entry);
            new.push('\n');
        }
        if std::fs::write(&gitignore, &new).is_err() {
            return;
        }

        // stage + commit
        let staged = Command::new("git")
            .args(["add", ".gitignore"])
            .current_dir(repo_root)
            .output();
        if !staged.map(|o| o.status.success()).unwrap_or(false) {
            return;
        }

        let msg = format!("chore: gitignore {}", missing.join(", "));
        let _ = Command::new("git")
            .args(["commit", "-m", &msg])
            .current_dir(repo_root)
            .output();
    }
}
