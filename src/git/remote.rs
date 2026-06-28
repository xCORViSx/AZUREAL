//! Git remote operations
//!
//! Pull, push, and remote/main divergence queries.

use anyhow::{Context, Result};
use std::path::{Component, Path, PathBuf};
use std::process::Command;

use super::Git;

/// Remote-operation methods for pulling, pushing, and branch divergence checks.
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

    /// Push current branch to remote (auto-sets upstream on first push).
    ///
    /// Detached Azureal worktrees are pushed to their inferred worktree branch
    /// when a matching local or remote branch already exists. The push uses a
    /// fully qualified `HEAD:refs/heads/<branch>` refspec so Git never guesses
    /// an ambiguous destination such as `HEAD`.
    pub fn push(worktree_path: &Path) -> Result<String> {
        let branch_name = resolve_push_branch(worktree_path)?;
        if current_symbolic_branch(worktree_path).is_none() {
            reattach_detached_worktree_branch(worktree_path, &branch_name)?;
        }
        Self::push_branch(worktree_path, &branch_name)
    }

    /// Push `HEAD` to a specific local branch name on origin.
    ///
    /// This is used by callers that already know the intended branch even when
    /// the worktree itself is temporarily detached. The destination ref is
    /// always fully qualified as `refs/heads/<branch_name>`.
    pub fn push_branch(worktree_path: &Path, branch_name: &str) -> Result<String> {
        let branch_name = normalize_push_branch(branch_name)?;

        // Check if local branch has diverged from remote (e.g. after rebase).
        // `rev-list --left-right --count` returns "<ahead>\t<behind>".
        // If behind > 0 AND ahead > 0, the histories have diverged — force-with-lease is needed.
        let diverged = Command::new("git")
            .args([
                "rev-list",
                "--left-right",
                "--count",
                &format!("HEAD...refs/remotes/origin/{}", branch_name),
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
        let refspec = format!("HEAD:refs/heads/{}", branch_name);
        let mut push_args = vec!["push"];
        if diverged {
            push_args.push("--force-with-lease");
        }
        push_args.extend(["-u", "origin", &refspec]);

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

/// Resolve the branch that `Git::push` should update on origin.
///
/// Symbolic HEADs use their current local branch. Detached Azureal worktrees
/// fall back to `{repo-prefix}/{worktree-relative-path}` only when that branch
/// exists locally or as `origin/<branch>`.
fn resolve_push_branch(worktree_path: &Path) -> Result<String> {
    if let Some(branch) = current_symbolic_branch(worktree_path) {
        return normalize_push_branch(&branch);
    }

    if let Some(branch) = infer_azureal_worktree_branch(worktree_path) {
        return normalize_push_branch(&branch);
    }

    anyhow::bail!(
        "Cannot push detached HEAD without a branch target; checkout a branch or refresh the Azureal worktree"
    );
}

/// Return the current branch name when HEAD is a symbolic branch ref.
fn current_symbolic_branch(worktree_path: &Path) -> Option<String> {
    let output = Command::new("git")
        .args(["symbolic-ref", "--quiet", "--short", "HEAD"])
        .current_dir(worktree_path)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let branch = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if branch.is_empty() {
        None
    } else {
        Some(branch)
    }
}

/// Normalize a caller-provided branch name into a local short branch ref.
fn normalize_push_branch(branch_name: &str) -> Result<String> {
    let branch = branch_name
        .trim()
        .strip_prefix("refs/heads/")
        .unwrap_or_else(|| branch_name.trim())
        .to_string();
    if branch.is_empty() || branch == "HEAD" {
        anyhow::bail!("Cannot push without a concrete branch name");
    }
    Ok(branch)
}

/// Infer the intended Azureal branch for a detached worktree path.
///
/// Azureal creates worktrees at `<repo>/worktrees/<name>` and branches at
/// `<repo-prefix>/<name>`. The inferred branch is accepted only if Git already
/// knows the local branch or the matching remote-tracking branch.
fn infer_azureal_worktree_branch(worktree_path: &Path) -> Option<String> {
    let repo_root = Git::repo_root(worktree_path).ok()?;
    let worktrees_dir = repo_root.join("worktrees");
    let absolute_worktree = canonical_or_original(worktree_path);
    let absolute_worktrees_dir = canonical_or_original(&worktrees_dir);
    let relative = absolute_worktree
        .strip_prefix(absolute_worktrees_dir)
        .ok()?;
    let suffix = branch_suffix_from_path(relative)?;
    let prefix = crate::models::branch_prefix_for_path(&repo_root);
    let branch = format!("{}/{}", prefix, suffix);
    if branch_ref_exists(worktree_path, &format!("refs/heads/{}", branch))
        || branch_ref_exists(worktree_path, &format!("refs/remotes/origin/{}", branch))
    {
        Some(branch)
    } else {
        None
    }
}

/// Return a canonicalized path when possible, otherwise preserve the input path.
fn canonical_or_original(path: &Path) -> PathBuf {
    dunce::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

/// Convert a worktree-relative path into the branch suffix Azureal uses.
fn branch_suffix_from_path(path: &Path) -> Option<String> {
    let mut suffix = String::new();
    for component in path.components() {
        let Component::Normal(part) = component else {
            return None;
        };
        let part = part.to_str()?;
        if part.is_empty() {
            return None;
        }
        if !suffix.is_empty() {
            suffix.push('/');
        }
        suffix.push_str(part);
    }
    if suffix.is_empty() {
        None
    } else {
        Some(suffix)
    }
}

/// Return true when the given fully qualified ref exists in the worktree repo.
fn branch_ref_exists(worktree_path: &Path, ref_name: &str) -> bool {
    Command::new("git")
        .args(["show-ref", "--verify", "--quiet", ref_name])
        .current_dir(worktree_path)
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

/// Move the inferred local branch to detached `HEAD` and check it out.
///
/// This keeps Azureal's later branch-based actions, such as squash-merge, from
/// operating on a stale local branch after commit-and-push ran in detached HEAD.
fn reattach_detached_worktree_branch(worktree_path: &Path, branch_name: &str) -> Result<()> {
    let update = Command::new("git")
        .args(["branch", "-f", branch_name, "HEAD"])
        .current_dir(worktree_path)
        .output()
        .context("Failed to update detached worktree branch")?;
    if !update.status.success() {
        let combined = format!(
            "{}{}",
            String::from_utf8_lossy(&update.stdout),
            String::from_utf8_lossy(&update.stderr)
        );
        anyhow::bail!("Failed to reattach detached branch: {}", combined.trim());
    }

    let checkout = Command::new("git")
        .args(["checkout", branch_name])
        .current_dir(worktree_path)
        .output()
        .context("Failed to checkout detached worktree branch")?;
    if !checkout.status.success() {
        let combined = format!(
            "{}{}",
            String::from_utf8_lossy(&checkout.stdout),
            String::from_utf8_lossy(&checkout.stderr)
        );
        anyhow::bail!("Failed to checkout reattached branch: {}", combined.trim());
    }

    Ok(())
}

/// Regression tests for remote push behavior around detached worktrees.
#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use tempfile::TempDir;

    /// Run a git command in the current process directory and return stdout.
    fn git(args: &[&str]) -> String {
        let output = Command::new("git")
            .args(args)
            .output()
            .unwrap_or_else(|err| panic!("failed to run git {:?}: {}", args, err));
        assert!(
            output.status.success(),
            "git {:?} failed\nstdout:\n{}\nstderr:\n{}",
            args,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    }

    /// Run a git command inside `dir` and return stdout.
    fn git_in(dir: &Path, args: &[&str]) -> String {
        let output = Command::new("git")
            .args(args)
            .current_dir(dir)
            .output()
            .unwrap_or_else(|err| panic!("failed to run git {:?}: {}", args, err));
        assert!(
            output.status.success(),
            "git {:?} in {} failed\nstdout:\n{}\nstderr:\n{}",
            args,
            dir.display(),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    }

    /// Configure a temporary repository with a deterministic test identity.
    fn configure_identity(repo: &Path) {
        git_in(repo, &["config", "user.email", "azureal@example.test"]);
        git_in(repo, &["config", "user.name", "Azureal Test"]);
    }

    /// Write a file in a repository and commit it with the provided message.
    fn commit_file(repo: &Path, name: &str, content: &str, message: &str) {
        std::fs::write(repo.join(name), content).unwrap();
        git_in(repo, &["add", name]);
        git_in(repo, &["commit", "-m", message]);
    }

    /// Create a clone backed by a bare origin whose repo name yields the idiosonix prefix.
    fn clone_idiosonix_repo(temp: &TempDir) -> (PathBuf, PathBuf) {
        let remote = temp.path().join("idiosonix.git");
        let repo = temp.path().join("repo");
        let remote_s = remote.to_string_lossy().into_owned();
        let repo_s = repo.to_string_lossy().into_owned();
        git(&["init", "--bare", &remote_s]);
        git(&["clone", &remote_s, &repo_s]);
        configure_identity(&repo);
        (repo, remote)
    }

    /// Detached Azureal worktrees push to the inferred branch with a full refspec.
    #[test]
    fn push_infers_diverged_detached_azureal_worktree_branch() {
        let temp = TempDir::new().unwrap();
        let (repo, _remote) = clone_idiosonix_repo(&temp);
        git_in(&repo, &["checkout", "-b", "main"]);
        commit_file(&repo, "main.txt", "main", "main commit");
        git_in(&repo, &["push", "-u", "origin", "HEAD:refs/heads/main"]);

        let worktrees_dir = repo.join("worktrees");
        std::fs::create_dir_all(&worktrees_dir).unwrap();
        let memory = worktrees_dir.join("memory");
        let memory_s = memory.to_string_lossy().into_owned();
        git_in(
            &repo,
            &[
                "worktree",
                "add",
                "-b",
                "idiosonix/memory",
                &memory_s,
                "main",
            ],
        );
        configure_identity(&memory);
        commit_file(&memory, "branch.txt", "branch", "branch commit");
        git_in(
            &memory,
            &["push", "-u", "origin", "HEAD:refs/heads/idiosonix/memory"],
        );

        git_in(&memory, &["checkout", "--detach", "main"]);
        commit_file(&memory, "detached.txt", "detached", "detached commit");
        let detached_head = git_in(&memory, &["rev-parse", "HEAD"]);

        let result = Git::push(&memory).unwrap();
        let remote_head = git_in(
            &memory,
            &["ls-remote", "origin", "refs/heads/idiosonix/memory"],
        );
        let current_branch = git_in(&memory, &["symbolic-ref", "--short", "HEAD"]);
        let local_branch_head = git_in(&memory, &["rev-parse", "idiosonix/memory"]);

        assert!(result.contains("force-pushed"));
        assert!(remote_head.starts_with(&detached_head));
        assert_eq!(current_branch, "idiosonix/memory");
        assert_eq!(local_branch_head, detached_head);
    }

    /// Detached non-Azureal paths get a clear error instead of `git push origin HEAD`.
    #[test]
    fn push_detached_non_azureal_path_fails_before_ambiguous_git_refspec() {
        let temp = TempDir::new().unwrap();
        let (repo, _remote) = clone_idiosonix_repo(&temp);
        git_in(&repo, &["checkout", "-b", "main"]);
        commit_file(&repo, "main.txt", "main", "main commit");
        git_in(&repo, &["push", "-u", "origin", "HEAD:refs/heads/main"]);
        git_in(&repo, &["checkout", "--detach", "main"]);
        commit_file(&repo, "detached.txt", "detached", "detached commit");

        let error = Git::push(&repo).unwrap_err().to_string();

        assert!(error.contains("Cannot push detached HEAD without a branch target"));
    }
}
