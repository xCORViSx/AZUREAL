//! Periodic auto-rebase check
//!
//! Scans worktrees with auto-rebase enabled and rebases the first eligible one.

use std::time::{Duration, Instant};

use crate::app::App;
use crate::backend::AgentProcess;

/// Check all auto-rebase-enabled worktrees and rebase the first eligible one.
/// Returns true if any state changed (needs redraw).
pub fn check_auto_rebase(app: &mut App, _claude_process: &AgentProcess) -> bool {
    use crate::tui::input_git_actions::{exec_rebase_inner, RebaseOutcome};
    use crate::app::types::GitConflictOverlay;

    // Skip if RCR active or editing a file
    if app.rcr_session.is_some() {
        return false;
    }
    if app.viewer_edit_mode {
        return false;
    }

    let project = match &app.project {
        Some(p) => p.clone(),
        None => return false,
    };

    // Collect eligible worktrees (avoid borrowing app during iteration)
    let candidates: Vec<(String, std::path::PathBuf)> = app
        .worktrees
        .iter()
        .filter(|wt| {
            wt.branch_name != project.main_branch
                && !wt.archived
                && app.auto_rebase_enabled.contains(&wt.branch_name)
                && !app.is_session_running(&wt.branch_name)
                && wt.worktree_path.is_some()
        })
        .map(|wt| (wt.branch_name.clone(), wt.worktree_path.clone().unwrap()))
        .collect();

    // If the git panel is open, note which worktree it's viewing
    let git_panel_branch = app
        .git_actions_panel
        .as_ref()
        .map(|p| p.worktree_name.clone());

    for (branch, wt_path) in candidates {
        // Skip the worktree whose git panel is currently open
        if git_panel_branch.as_ref() == Some(&branch) {
            continue;
        }

        let display = crate::models::strip_branch_prefix(&branch).to_string();

        // Skip worktrees with uncommitted changes — git rebase would fail
        let dirty = std::process::Command::new("git")
            .args(["status", "--porcelain"])
            .current_dir(&wt_path)
            .output()
            .ok()
            .map(|o| !o.stdout.is_empty())
            .unwrap_or(false);
        if dirty {
            continue;
        }

        let ar_files = crate::azufig::load_auto_resolve_files(&project.path);
        match exec_rebase_inner(&wt_path, &project.main_branch, &ar_files) {
            RebaseOutcome::UpToDate => continue,
            RebaseOutcome::Rebased => {
                // Push the rebased branch to its remote
                let push_suffix = match crate::git::Git::push(&wt_path) {
                    Ok(_) => " → pushed",
                    Err(_) => "",
                };
                app.auto_rebase_success_until = Some((
                    format!("{}{}", display, push_suffix),
                    Instant::now() + Duration::from_secs(2),
                ));
                app.invalidate_sidebar();
                return true;
            }
            RebaseOutcome::Conflict {
                conflicted,
                auto_merged,
                ..
            } => {
                // Switch to the conflicted worktree and open Git panel with conflict overlay
                if let Some(idx) = app.worktrees.iter().position(|w| w.branch_name == branch) {
                    app.save_live_display_events();
                    app.selected_worktree = Some(idx);
                    app.load_session_output();
                }
                app.open_git_actions_panel();
                if let Some(ref mut panel) = app.git_actions_panel {
                    panel.conflict_overlay = Some(GitConflictOverlay {
                        conflicted_files: conflicted,
                        auto_merged_files: auto_merged,
                        scroll: 0,
                        selected: 0,
                        continue_with_merge: false,
                    });
                }
                app.invalidate_sidebar();
                return true;
            }
            RebaseOutcome::Failed(_) => continue,
        }
    }
    false
}
