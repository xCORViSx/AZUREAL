//! Worktree discovery, project loading, and file tree initialization

use std::collections::HashSet;
use std::path::PathBuf;

use crate::app::types::WorktreeRefreshResult;
use crate::backend::Backend;
use crate::git::Git;
use crate::models::{Project, Worktree};

use super::super::helpers::build_file_tree;
use super::super::App;

/// Pure computation: all git + FS I/O for worktree discovery, no App state.
/// Safe to run on a background thread. Returns data to apply to App.
pub fn compute_worktree_refresh(
    project_path: PathBuf,
    main_branch: String,
    worktrees_dir: PathBuf,
    _backend: Backend,
    branch_prefix: String,
) -> anyhow::Result<WorktreeRefreshResult> {
    let worktrees = Git::list_worktrees_detailed(&project_path)?;

    // Repair detached HEADs (rebase state recovery, orphaned HEAD re-attach)
    let mut needs_refetch = false;
    let mut rebase_branches: Vec<(PathBuf, String)> = Vec::new();
    for wt in &worktrees {
        if wt.branch.is_some() {
            continue;
        }
        if !wt.is_main && !wt.path.starts_with(&worktrees_dir) {
            continue;
        }
        if Git::is_rebase_in_progress(&wt.path) {
            let git_dir = std::process::Command::new("git")
                .args(["rev-parse", "--git-dir"])
                .current_dir(&wt.path)
                .output()
                .ok()
                .filter(|o| o.status.success())
                .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string());
            if let Some(ref gd) = git_dir {
                let head_name = std::path::Path::new(gd).join("rebase-merge/head-name");
                if let Ok(content) = std::fs::read_to_string(&head_name) {
                    let branch = content
                        .trim()
                        .strip_prefix("refs/heads/")
                        .unwrap_or(content.trim());
                    if !branch.is_empty() {
                        rebase_branches.push((wt.path.clone(), branch.to_string()));
                        continue;
                    }
                }
                let head_name = std::path::Path::new(gd).join("rebase-apply/head-name");
                if let Ok(content) = std::fs::read_to_string(&head_name) {
                    let branch = content
                        .trim()
                        .strip_prefix("refs/heads/")
                        .unwrap_or(content.trim());
                    if !branch.is_empty() {
                        rebase_branches.push((wt.path.clone(), branch.to_string()));
                        continue;
                    }
                }
            }
            let _ = Git::rebase_abort(&wt.path);
            needs_refetch = true;
            continue;
        }
        let head_ok = std::process::Command::new("git")
            .args(["symbolic-ref", "--quiet", "HEAD"])
            .current_dir(&wt.path)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(true);
        if !head_ok {
            if let Ok(out) = std::process::Command::new("git")
                .args([
                    "for-each-ref",
                    "--points-at=HEAD",
                    "--format=%(refname:short)",
                    "refs/heads/",
                ])
                .current_dir(&wt.path)
                .output()
            {
                let branch = String::from_utf8_lossy(&out.stdout).trim().to_string();
                if let Some(target) = branch.lines().next().filter(|b| !b.is_empty()) {
                    let _ = std::process::Command::new("git")
                        .args(["checkout", target])
                        .current_dir(&wt.path)
                        .output();
                    needs_refetch = true;
                }
            }
        }
    }
    let mut worktrees = if needs_refetch {
        Git::list_worktrees_detailed(&project_path)?
    } else {
        worktrees
    };
    for (path, branch) in &rebase_branches {
        for wt in &mut worktrees {
            if wt.path == *path && wt.branch.is_none() {
                wt.branch = Some(branch.clone());
            }
        }
    }

    let azureal_branches = Git::list_azureal_branches(&project_path, &branch_prefix)?;

    let wt_paths: Vec<_> = worktrees.iter().map(|w| w.path.clone()).collect();
    crate::config::migrate_project_dirs(&wt_paths);

    let mut result_worktrees = Vec::new();
    let mut result_main: Option<Worktree> = None;
    let mut active_branches: HashSet<String> = HashSet::new();

    // Main worktree
    for wt in &worktrees {
        if wt.is_main {
            let branch_name = wt.branch.clone().unwrap_or_else(|| main_branch.clone());
            result_main = Some(Worktree {
                branch_name: branch_name.clone(),
                worktree_path: Some(wt.path.clone()),
                claude_session_id: None,
                archived: false,
            });
            active_branches.insert(branch_name);
        }
    }

    // Feature worktrees
    for wt in &worktrees {
        if !wt.is_main && wt.path.starts_with(&worktrees_dir) {
            let branch_name = wt.branch.clone().unwrap_or_default();
            result_worktrees.push(Worktree {
                branch_name: branch_name.clone(),
                worktree_path: Some(wt.path.clone()),
                claude_session_id: None,
                archived: false,
            });
            active_branches.insert(branch_name);
        }
    }

    // Archived branches
    for branch in azureal_branches {
        if !active_branches.contains(&branch) {
            result_worktrees.push(Worktree {
                branch_name: branch,
                worktree_path: None,
                claude_session_id: None,
                archived: true,
            });
        }
    }

    Ok(WorktreeRefreshResult {
        main_worktree: result_main,
        worktrees: result_worktrees,
    })
}

impl App {
    /// Load project and sessions from git (stateless discovery).
    /// If cwd is a git repo, auto-register it in ~/.azureal/projects.txt and load it.
    /// If NOT in a git repo, open the Projects panel so user can pick a project.
    pub fn load(&mut self) -> anyhow::Result<()> {
        let cwd = std::env::current_dir()?;

        if !Git::is_git_repo(&cwd) {
            // Not in a git repo — show Projects panel with a helpful message
            self.open_projects_panel();
            if let Some(ref mut panel) = self.projects_panel {
                panel.error = Some(
                    "Project not initialized. Press i to initialize or choose another project."
                        .to_string(),
                );
            }
            return Ok(());
        }

        let repo_root = Git::repo_root(&cwd)?;

        // Auto-register this repo in ~/.azureal/projects.txt (no-op if already there)
        crate::config::register_project(&repo_root);

        // Ensure worktrees/ is gitignored so new worktrees don't inherit the folder
        Git::ensure_worktrees_gitignored(&repo_root);

        let main_branch = Git::get_main_branch(&repo_root)?;
        self.project = Some(Project::from_path(repo_root.clone(), main_branch));

        // Session store is opened lazily on first use (ensure_session_store)
        // to avoid creating the .azs file for projects that haven't used sessions yet.

        // Load filetree hidden dirs from project azufig (persisted Options overlay state)
        let az = crate::azufig::load_project_azufig(&repo_root);
        self.file_tree_hidden_dirs = az.filetree.hidden.into_iter().collect();

        // Untrack any files that match .gitignore but are still in the index
        // (e.g. .DS_Store committed before gitignore was added).
        Git::untrack_gitignored_files(&repo_root);

        let prefix = self
            .project
            .as_ref()
            .map(|p| p.branch_prefix.clone())
            .unwrap_or_else(|| "project".to_string());

        // Prune stale remote-tracking refs so branches deleted on other machines
        // don't appear as archived worktrees. Best-effort (no-op if offline).
        Git::prune_remote_refs(&repo_root, &prefix);

        // Detached HEAD repair and orphaned rebase cleanup now handled
        // inside load_worktrees() so every refresh (not just startup) benefits.
        self.load_worktrees()?;

        // Load auto-rebase enabled branches from each worktree's azufig
        // (must be after load_worktrees() so self.worktrees is populated)
        self.auto_rebase_enabled = crate::azufig::load_auto_rebase_from_worktrees(&self.worktrees);

        Ok(())
    }

    /// Load sessions from git worktrees and branches.
    /// Synchronous — used at startup and for user-triggered refreshes.
    /// The event loop uses compute_worktree_refresh() + apply_worktree_result()
    /// on a background thread instead.
    pub fn load_worktrees(&mut self) -> anyhow::Result<()> {
        let Some(project) = &self.project else {
            return Ok(());
        };
        // Discard any in-flight background refresh — this synchronous call takes priority
        self.worktree_refresh_receiver = None;
        let result = compute_worktree_refresh(
            project.path.clone(),
            project.main_branch.clone(),
            project.worktrees_dir(),
            self.backend,
            project.branch_prefix.clone(),
        )?;
        self.apply_worktree_result(result);
        Ok(())
    }

    /// Apply pre-computed worktree data to App state.
    /// Handles selection preservation.
    pub fn apply_worktree_result(&mut self, result: WorktreeRefreshResult) {
        // Apply main worktree
        self.main_worktree = result.main_worktree;

        // Preserve current selection by branch name
        let prev_branch = self
            .selected_worktree
            .and_then(|i| self.worktrees.get(i))
            .map(|w| w.branch_name.clone());

        self.worktrees = result.worktrees;

        self.selected_worktree = if self.worktrees.is_empty() {
            None
        } else if let Some(ref branch) = prev_branch {
            self.worktrees
                .iter()
                .position(|w| w.branch_name == *branch)
                .or(Some(0))
        } else {
            let cwd = std::env::current_dir().ok();
            cwd.and_then(|c| {
                self.worktrees
                    .iter()
                    .position(|w| w.worktree_path.as_ref() == Some(&c))
            })
            .or(Some(0))
        };

        self.invalidate_sidebar();
    }

    /// Load file tree entries for the current session's worktree
    pub fn load_file_tree(&mut self) {
        // Discard any in-flight background scan — this synchronous call takes priority
        self.file_tree_receiver = None;
        self.file_tree_entries.clear();
        self.file_tree_selected = None;
        self.file_tree_scroll = 0;

        let Some(session) = self.current_worktree() else {
            self.invalidate_file_tree();
            return;
        };
        let Some(ref worktree_path) = session.worktree_path else {
            self.invalidate_file_tree();
            return;
        };

        self.file_tree_entries = build_file_tree(
            worktree_path,
            &self.file_tree_expanded,
            &self.file_tree_hidden_dirs,
        );
        if !self.file_tree_entries.is_empty() {
            self.file_tree_selected = Some(0);
        }
        self.invalidate_file_tree();
    }

    pub fn refresh_worktrees(&mut self) -> anyhow::Result<()> {
        self.load_worktrees()
    }
}

#[cfg(test)]
mod tests {
    use super::super::super::App;
    use std::path::PathBuf;

    // ── load_file_tree state reset ──

    #[test]
    fn load_file_tree_clears_when_no_worktree() {
        let mut app = App::new();
        app.file_tree_entries
            .push(crate::app::types::FileTreeEntry {
                path: PathBuf::from("/old"),
                name: "old".to_string(),
                is_dir: false,
                depth: 0,
                is_hidden: false,
            });
        app.file_tree_selected = Some(0);
        app.file_tree_scroll = 5;
        app.load_file_tree();
        assert!(app.file_tree_entries.is_empty());
        assert!(app.file_tree_selected.is_none());
        assert_eq!(app.file_tree_scroll, 0);
    }

    // ── refresh_worktrees ──

    #[test]
    fn refresh_worktrees_no_project_ok() {
        let mut app = App::new();
        assert!(app.refresh_worktrees().is_ok());
    }

    // ── load_file_tree: with worktree but nonexistent path ──

    #[test]
    fn load_file_tree_nonexistent_worktree_path() {
        let mut app = App::new();
        app.worktrees.push(crate::models::Worktree {
            branch_name: "azureal/test".to_string(),
            worktree_path: Some(PathBuf::from("/nonexistent/path/asdf")),
            claude_session_id: None,
            archived: false,
        });
        app.selected_worktree = Some(0);
        app.load_file_tree();
        // build_file_tree on nonexistent path should produce empty entries
        assert!(app.file_tree_entries.is_empty());
        assert!(app.file_tree_selected.is_none());
    }
}
