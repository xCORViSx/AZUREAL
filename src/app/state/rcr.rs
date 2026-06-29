//! Rebase conflict resolution recovery helpers.
//!
//! RCR state is app-global because only one assisted rebase can be active at a
//! time, but the session pane can show any worktree/session. These helpers keep
//! that global state recoverable without letting it leak into unrelated panes.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::app::session_store::SessionStore;
use crate::app::App;
use crate::git::Git;

/// Branch/worktree/session state for the active RCR session.
#[derive(Clone)]
struct RcrTarget {
    /// Branch name that owns the RCR workflow.
    branch: String,
    /// Human-readable worktree name used in status messages.
    display_name: String,
    /// Worktree path where the rebase was being resolved.
    worktree_path: PathBuf,
    /// Process slot that produced the RCR transcript.
    slot_id: String,
}

impl App {
    /// Return true when the current worktree owns the active RCR flow even if
    /// the session pane is currently viewing a different store session.
    pub fn rcr_session_matches_current_worktree(&self) -> bool {
        let Some(rcr) = self.rcr_session.as_ref() else {
            return false;
        };
        self.current_worktree()
            .map(|worktree| {
                worktree.branch_name == rcr.branch
                    && worktree.worktree_path.as_deref() == Some(rcr.worktree_path.as_path())
            })
            .unwrap_or(false)
    }

    /// Select the active RCR store session when the currently selected
    /// worktree owns it. Returns true when the session selection was recovered.
    pub fn select_active_rcr_session_for_current_worktree(&mut self) -> bool {
        let Some(rcr) = self.rcr_session.as_ref() else {
            return false;
        };
        if !self.rcr_session_matches_current_worktree() {
            return false;
        }
        let Some((store_session_id, wt_path, _, _)) =
            self.pid_session_target.get(&rcr.slot_id).cloned()
        else {
            return false;
        };
        let branch = rcr.branch.clone();
        self.select_store_session_for_branch(&branch, store_session_id, &wt_path)
    }

    /// Clear a pending RCR flow when Git shows that the branch was already
    /// reconciled externally, such as after a manual squash merge plus rebase.
    /// Returns true when visible state changed.
    pub fn reconcile_rcr_after_external_git_resolution(&mut self) -> bool {
        let Some(target) = self.rcr_target_snapshot() else {
            return false;
        };
        if self.running_sessions.contains(&target.slot_id) {
            return false;
        }
        if rcr_still_needs_attention(&target.worktree_path) {
            return false;
        }
        if target.worktree_path.exists() && !self.rcr_target_matches_main(&target) {
            return false;
        }

        self.clear_rcr_target_tracking(&target);
        self.set_status(format!(
            "[RCR] Cleared stale approval for {} — already reconciled with main",
            target.display_name
        ));
        true
    }

    /// Snapshot the active RCR state without holding an immutable borrow over
    /// later mutable App updates.
    fn rcr_target_snapshot(&self) -> Option<RcrTarget> {
        self.rcr_session.as_ref().map(|rcr| RcrTarget {
            branch: rcr.branch.clone(),
            display_name: rcr.display_name.clone(),
            worktree_path: rcr.worktree_path.clone(),
            slot_id: rcr.slot_id.clone(),
        })
    }

    /// Select a store session id in the branch session cache, refreshing the
    /// cache from the target worktree store when possible.
    fn select_store_session_for_branch(
        &mut self,
        branch: &str,
        store_session_id: i64,
        wt_path: &Path,
    ) -> bool {
        let id_key = store_session_id.to_string();
        let mut selected = false;

        if let Ok(store) = SessionStore::open(wt_path) {
            if let Ok(sessions) = store.list_sessions(Some(branch)) {
                let mut files = Vec::new();
                let mut selected_idx = 0usize;
                for (idx, session) in sessions.iter().enumerate() {
                    let key = session.id.to_string();
                    if key == id_key {
                        selected = true;
                        selected_idx = idx;
                    }
                    files.push((key.clone(), PathBuf::new(), session.created.clone()));
                    self.session_msg_counts
                        .insert(key, (session.message_count, 0));
                }
                if selected {
                    self.session_files.insert(branch.to_string(), files);
                    self.session_selected_file_idx
                        .insert(branch.to_string(), selected_idx);
                }
            }
            self.session_store = Some(store);
            self.session_store_path = Some(wt_path.to_path_buf());
        }

        if !selected {
            selected = self
                .session_files
                .get(branch)
                .and_then(|files| files.iter().position(|(id, _, _)| id == &id_key))
                .map(|idx| {
                    self.session_selected_file_idx
                        .insert(branch.to_string(), idx);
                })
                .is_some();
        }

        if selected {
            self.current_session_id = Some(store_session_id);
        }
        selected
    }

    /// Return true when the target worktree is at the same commit as the
    /// project main branch.
    fn rcr_target_matches_main(&self, target: &RcrTarget) -> bool {
        let Some(project) = self.project.as_ref() else {
            return false;
        };
        git_divergence(&target.worktree_path, &project.main_branch, "HEAD") == Some((0, 0))
    }

    /// Remove stale RCR state and process-target bookkeeping for an externally
    /// completed workflow.
    fn clear_rcr_target_tracking(&mut self, target: &RcrTarget) {
        self.rcr_session = None;
        self.pid_session_target.remove(&target.slot_id);
        self.pending_session_names
            .retain(|(slot, _)| slot != &target.slot_id);
        self.invalidate_sidebar();
        if self.current_worktree().is_some_and(|worktree| {
            worktree.branch_name == target.branch
                && worktree.worktree_path.as_deref() == Some(target.worktree_path.as_path())
        }) {
            self.update_title_session_name();
        }
    }
}

/// Return true when the target worktree still has rebase or unmerged index
/// state that needs RCR review or a user decision.
fn rcr_still_needs_attention(worktree_path: &Path) -> bool {
    Git::is_rebase_in_progress(worktree_path) || Git::has_unmerged_files(worktree_path)
}

/// Return divergence counts for two refs, preserving Git failures as `None`
/// instead of treating invalid refs as a clean zero-divergence branch.
fn git_divergence(worktree_path: &Path, upstream: &str, local: &str) -> Option<(usize, usize)> {
    let output = Command::new("git")
        .args([
            "rev-list",
            "--left-right",
            "--count",
            &format!("{}...{}", upstream, local),
        ])
        .current_dir(worktree_path)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut parts = stdout.split_whitespace();
    let behind = parts.next()?.parse::<usize>().ok()?;
    let ahead = parts.next()?.parse::<usize>().ok()?;
    Some((behind, ahead))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::types::RcrSession;
    use crate::models::{Project, Worktree};
    use tempfile::TempDir;

    /// Run a git command in a test repository and require success.
    fn run_git(path: &Path, args: &[&str]) {
        let output = Command::new("git")
            .args(args)
            .current_dir(path)
            .output()
            .expect("git command should spawn");
        assert!(
            output.status.success(),
            "git {:?} failed: {}{}",
            args,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    /// Build a tiny repository with one feature worktree already equal to main.
    fn resolved_rcr_repo() -> (TempDir, PathBuf, PathBuf, String) {
        let temp = TempDir::new().expect("tempdir");
        let repo = temp.path().join("repo");
        std::fs::create_dir(&repo).expect("repo dir");
        run_git(&repo, &["init", "-b", "main"]);
        run_git(&repo, &["config", "user.email", "test@example.com"]);
        run_git(&repo, &["config", "user.name", "Test User"]);
        std::fs::write(repo.join("README.md"), "base\n").expect("write readme");
        run_git(&repo, &["add", "README.md"]);
        run_git(&repo, &["commit", "-m", "base"]);

        let branch = "azureal/manual-rcr".to_string();
        run_git(&repo, &["branch", &branch]);
        let worktree = repo.join("worktrees").join("manual-rcr");
        run_git(
            &repo,
            &["worktree", "add", worktree.to_str().unwrap(), &branch],
        );
        (temp, repo, worktree, branch)
    }

    /// Build an App with one active RCR target for a branch worktree.
    fn app_with_rcr(repo: &Path, worktree: &Path, branch: &str) -> App {
        let mut app = App::new();
        app.project = Some(Project::from_path(repo.to_path_buf(), "main".to_string()));
        app.worktrees.push(Worktree {
            branch_name: branch.to_string(),
            worktree_path: Some(worktree.to_path_buf()),
            claude_session_id: None,
            archived: false,
        });
        app.selected_worktree = Some(0);
        app.rcr_session = Some(RcrSession {
            branch: branch.to_string(),
            display_name: "manual-rcr".to_string(),
            worktree_path: worktree.to_path_buf(),
            repo_root: repo.to_path_buf(),
            slot_id: "42".to_string(),
            session_id: None,
            approval_pending: true,
            continue_with_merge: true,
        });
        app.pid_session_target
            .insert("42".to_string(), (7, worktree.to_path_buf(), 0, 0));
        app
    }

    /// Current-worktree matching ignores unrelated visible store sessions.
    #[test]
    fn rcr_session_matches_current_worktree_even_when_hidden() {
        let (_temp, repo, worktree, branch) = resolved_rcr_repo();
        let mut app = app_with_rcr(&repo, &worktree, &branch);
        app.current_session_id = Some(99);

        assert!(app.rcr_session_matches_current_worktree());
        assert!(!app.rcr_session_is_visible());
    }

    /// RCR store-session recovery selects the active RCR transcript.
    #[test]
    fn select_active_rcr_session_recovers_hidden_store_session() {
        let (_temp, repo, worktree, branch) = resolved_rcr_repo();
        let store = SessionStore::open(&worktree).expect("session store");
        let store_id = store.create_session(&branch).expect("create session");
        let mut app = app_with_rcr(&repo, &worktree, &branch);
        app.pid_session_target
            .insert("42".to_string(), (store_id, worktree.clone(), 0, 0));
        app.current_session_id = Some(99);

        assert!(app.select_active_rcr_session_for_current_worktree());
        assert_eq!(app.current_session_id, Some(store_id));
        assert_eq!(app.session_selected_file_idx.get(&branch), Some(&0));
    }

    /// External manual completion clears a stale approval-only RCR session.
    #[test]
    fn reconcile_rcr_clears_approval_when_branch_matches_main() {
        let (_temp, repo, worktree, branch) = resolved_rcr_repo();
        let mut app = app_with_rcr(&repo, &worktree, &branch);

        assert!(app.reconcile_rcr_after_external_git_resolution());
        assert!(app.rcr_session.is_none());
        assert!(!app.pid_session_target.contains_key("42"));
    }

    /// A still-diverged branch keeps the RCR approval pending for user review.
    #[test]
    fn reconcile_rcr_keeps_pending_when_branch_still_ahead() {
        let (_temp, repo, worktree, branch) = resolved_rcr_repo();
        std::fs::write(worktree.join("feature.txt"), "feature\n").expect("write feature");
        run_git(&worktree, &["add", "feature.txt"]);
        run_git(&worktree, &["commit", "-m", "feature"]);
        let mut app = app_with_rcr(&repo, &worktree, &branch);

        assert!(!app.reconcile_rcr_after_external_git_resolution());
        assert!(app.rcr_session.is_some());
    }
}
