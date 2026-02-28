//! Rebase Conflict Resolution (RCR) acceptance
//!
//! Handles the user accepting the RCR resolution: cleans up the temporary
//! session file, restores normal convo pane, and optionally auto-proceeds
//! with a squash merge if the rebase was triggered by one.

use crate::app::App;

/// Accept the RCR resolution — delete the temporary session file, clear RCR
/// state, and restore normal convo pane. If the rebase was triggered by a
/// squash merge, auto-proceed with the merge (the user's original intent).
pub(super) fn accept_rcr(app: &mut App) {
    if let Some(rcr) = app.rcr_session.take() {
        app.invalidate_sidebar();
        // Delete the RCR session file so it doesn't pollute the session list
        if let Some(ref sid) = rcr.session_id {
            if let Some(path) = crate::config::claude_session_file(&rcr.worktree_path, sid) {
                let _ = std::fs::remove_file(path);
            }
        }
        // Restore convo pane to the feature branch's normal session
        app.load_session_output();
        app.update_title_session_name();

        if rcr.continue_with_merge {
            // Pop any stash left from the pre-merge stash in squash_merge_into_main().
            // The merge conflicted, so the stash was never popped. Pop it before
            // re-calling squash_merge (which would stash again, orphaning the first).
            let _ = std::process::Command::new("git")
                .args(["stash", "pop"])
                .current_dir(&rcr.repo_root)
                .output();
            // Rebase was part of squash merge — auto-proceed with the merge
            match crate::git::Git::squash_merge_into_main(&rcr.repo_root, &rcr.branch) {
                Ok(crate::git::SquashMergeResult::Success(msg)) => {
                    app.post_merge_dialog = Some(crate::app::types::PostMergeDialog {
                        branch: rcr.branch.clone(),
                        display_name: rcr.display_name.clone(),
                        worktree_path: rcr.worktree_path,
                        selected: 0,
                    });
                    app.set_status(msg);
                }
                Ok(crate::git::SquashMergeResult::Conflict { .. }) => {
                    // Shouldn't happen after a clean rebase, but handle gracefully
                    app.set_status(format!(
                        "Rebase resolved but merge still has conflicts for {} — try again from Git panel",
                        rcr.display_name
                    ));
                }
                Err(e) => {
                    app.set_status(format!("Squash merge failed for {}: {}", rcr.display_name, e));
                }
            }
        } else {
            app.set_status(format!("Rebase complete — conflicts resolved for {}", rcr.display_name));
        }
    }
}
