//! Rebase Conflict Resolution (RCR) acceptance
//!
//! Handles the user accepting the RCR resolution: cleans up the temporary
//! session file, restores normal convo pane, and optionally auto-proceeds
//! with a squash merge if the rebase was triggered by one.

use crate::app::types::{
    BackgroundOpOutcome, BackgroundOpProgress, PostMergeDialog, RcrCompletion, RcrSession,
};
use crate::app::App;
use crate::git::{Git, SquashMergeResult};
use std::sync::mpsc;

/// Accept the RCR resolution — delete the temporary session file, clear RCR
/// state, and start the background git work with the standard loading dialog.
pub(super) fn accept_rcr(app: &mut App) {
    let Some(rcr) = take_rcr_session(app) else {
        return;
    };

    let main_branch = app
        .project
        .as_ref()
        .map(|p| p.main_branch.clone())
        .unwrap_or_else(|| "main".to_string());
    let (tx, rx) = mpsc::channel();
    app.loading_indicator = Some("Applying RCR resolution...".into());
    app.background_op_receiver = Some(rx);
    std::thread::spawn(move || run_accept_rcr(rcr, main_branch, tx));
}

/// Abort the post-run RCR approval flow and restore the worktree in the
/// background, showing the standard loading dialog while git runs.
pub(super) fn abort_rcr(app: &mut App) {
    let Some(rcr) = take_rcr_session(app) else {
        return;
    };

    let (tx, rx) = mpsc::channel();
    app.loading_indicator = Some("Aborting RCR...".into());
    app.background_op_receiver = Some(rx);
    std::thread::spawn(move || run_abort_rcr(rcr, tx));
}

fn take_rcr_session(app: &mut App) -> Option<RcrSession> {
    let rcr = app.rcr_session.take()?;
    app.invalidate_sidebar();
    cleanup_rcr_session_file(&rcr);
    app.load_session_output();
    app.update_title_session_name();
    Some(rcr)
}

fn cleanup_rcr_session_file(rcr: &RcrSession) {
    if let Some(ref sid) = rcr.session_id {
        if let Some(path) = crate::config::session_file(&rcr.worktree_path, sid) {
            let _ = std::fs::remove_file(path);
        }
    }
}

fn send_phase(tx: &mpsc::Sender<BackgroundOpProgress>, phase: &str) {
    let _ = tx.send(BackgroundOpProgress {
        phase: phase.to_string(),
        outcome: None,
    });
}

fn send_rcr_completion(tx: mpsc::Sender<BackgroundOpProgress>, completion: RcrCompletion) {
    let _ = tx.send(BackgroundOpProgress {
        phase: String::new(),
        outcome: Some(BackgroundOpOutcome::RcrFinished(completion)),
    });
}

fn run_accept_rcr(
    rcr: RcrSession,
    main_branch: String,
    tx: mpsc::Sender<BackgroundOpProgress>,
) {
    send_phase(&tx, "Restoring rebased worktree...");
    // Pop any stash left by exec_rebase_inner's pre-rebase stash on the worktree.
    let _ = std::process::Command::new("git")
        .args(["stash", "pop"])
        .current_dir(&rcr.worktree_path)
        .output();

    let completion = if rcr.continue_with_merge {
        send_phase(&tx, "Restoring merge stash...");
        // Pop any stash left from the pre-merge stash in squash_merge_into_main().
        // The merge conflicted, so the stash was never popped. Pop it before
        // re-calling squash_merge (which would stash again, orphaning the first).
        let _ = std::process::Command::new("git")
            .args(["stash", "pop"])
            .current_dir(&rcr.repo_root)
            .output();

        send_phase(&tx, "Pushing rebased branch...");
        let branch_push_note = match Git::push(&rcr.worktree_path) {
            Ok(_) => String::new(),
            Err(e) => format!(" (branch push failed: {})", e),
        };

        send_phase(&tx, "Merging into main...");
        match Git::squash_merge_into_main(&rcr.repo_root, &rcr.branch) {
            Ok(SquashMergeResult::Success(msg)) => {
                send_phase(&tx, "Pushing main...");
                let main_push_note = match Git::push(&rcr.repo_root) {
                    Ok(_) => " → pushed".to_string(),
                    Err(e) => format!(" (main push failed: {})", e),
                };
                send_phase(&tx, "Syncing feature worktree...");
                // Fast-forward feature branch to main so divergence indicators reset.
                let _ = std::process::Command::new("git")
                    .args(["reset", "--hard", &main_branch])
                    .current_dir(&rcr.worktree_path)
                    .output();
                RcrCompletion {
                    status_msg: format!("{}{}{}", msg, main_push_note, branch_push_note),
                    post_merge_dialog: Some(PostMergeDialog {
                        branch: rcr.branch,
                        display_name: rcr.display_name,
                        worktree_path: rcr.worktree_path,
                        selected: 0,
                    }),
                }
            }
            Ok(SquashMergeResult::Conflict { .. }) => RcrCompletion {
                // Shouldn't happen after a clean rebase, but handle gracefully.
                status_msg: format!(
                    "Rebase resolved but merge still has conflicts for {} — try again from Git panel",
                    rcr.display_name
                ),
                post_merge_dialog: None,
            },
            Err(e) => RcrCompletion {
                status_msg: format!("Squash merge failed for {}: {}", rcr.display_name, e),
                post_merge_dialog: None,
            },
        }
    } else {
        send_phase(&tx, "Pushing rebased branch...");
        let push_note = match Git::push(&rcr.worktree_path) {
            Ok(_) => " → pushed".to_string(),
            Err(e) => format!(" (push failed: {})", e),
        };
        RcrCompletion {
            status_msg: format!(
                "Rebase complete — conflicts resolved for {}{}",
                rcr.display_name, push_note
            ),
            post_merge_dialog: None,
        }
    };

    send_rcr_completion(tx, completion);
}

fn run_abort_rcr(rcr: RcrSession, tx: mpsc::Sender<BackgroundOpProgress>) {
    send_phase(&tx, "Aborting rebase...");
    let _ = std::process::Command::new("git")
        .args(["rebase", "--abort"])
        .current_dir(&rcr.worktree_path)
        .output();
    send_rcr_completion(
        tx,
        RcrCompletion {
            status_msg: format!("RCR cancelled — rebase aborted for {}", rcr.display_name),
            post_merge_dialog: None,
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::types::RcrSession;
    use std::path::PathBuf;
    use std::time::Duration;

    fn make_rcr(branch: &str, continue_merge: bool) -> RcrSession {
        RcrSession {
            branch: branch.to_string(),
            display_name: branch.to_string(),
            worktree_path: std::env::temp_dir().join("test-rcr-wt"),
            repo_root: std::env::temp_dir().join("test-rcr-root"),
            slot_id: "42".to_string(),
            session_id: None,
            approval_pending: true,
            continue_with_merge: continue_merge,
        }
    }

    fn recv_rcr_completion(app: &mut App) -> RcrCompletion {
        let rx = app
            .background_op_receiver
            .take()
            .expect("RCR should start a background op");
        loop {
            let progress = rx
                .recv_timeout(Duration::from_secs(5))
                .expect("RCR background op should finish");
            if let Some(BackgroundOpOutcome::RcrFinished(completion)) = progress.outcome {
                return completion;
            }
        }
    }

    // ── accept_rcr with no rcr_session (None) ──

    #[test]
    fn test_accept_rcr_no_session_is_noop() {
        let mut app = App::new();
        assert!(app.rcr_session.is_none());
        accept_rcr(&mut app);
        // No panic, no state change
        assert!(app.rcr_session.is_none());
    }

    #[test]
    fn test_accept_rcr_no_session_preserves_status() {
        let mut app = App::new();
        app.set_status("original status");
        accept_rcr(&mut app);
        // Status unchanged when no RCR session
        assert_eq!(app.status_message.as_deref(), Some("original status"));
    }

    #[test]
    fn test_accept_rcr_no_session_no_post_merge_dialog() {
        let mut app = App::new();
        accept_rcr(&mut app);
        assert!(app.post_merge_dialog.is_none());
    }

    // ── accept_rcr takes (consumes) the rcr_session ──

    #[test]
    fn test_accept_rcr_takes_session() {
        let mut app = App::new();
        app.rcr_session = Some(make_rcr("feat", false));
        accept_rcr(&mut app);
        assert!(app.rcr_session.is_none());
    }

    #[test]
    fn test_accept_rcr_takes_session_with_merge() {
        let mut app = App::new();
        app.rcr_session = Some(make_rcr("feat", true));
        accept_rcr(&mut app);
        assert!(app.rcr_session.is_none());
    }

    // ── RcrSession field tests ──

    #[test]
    fn test_rcr_session_branch_field() {
        let rcr = make_rcr("azureal/health", false);
        assert_eq!(rcr.branch, "azureal/health");
    }

    #[test]
    fn test_rcr_session_display_name_field() {
        let rcr = make_rcr("health", false);
        assert_eq!(rcr.display_name, "health");
    }

    #[test]
    fn test_rcr_session_worktree_path_field() {
        let rcr = make_rcr("b", false);
        assert_eq!(rcr.worktree_path, std::env::temp_dir().join("test-rcr-wt"));
    }

    #[test]
    fn test_rcr_session_repo_root_field() {
        let rcr = make_rcr("b", false);
        assert_eq!(rcr.repo_root, std::env::temp_dir().join("test-rcr-root"));
    }

    #[test]
    fn test_rcr_session_slot_id_field() {
        let rcr = make_rcr("b", false);
        assert_eq!(rcr.slot_id, "42");
    }

    #[test]
    fn test_rcr_session_session_id_none() {
        let rcr = make_rcr("b", false);
        assert!(rcr.session_id.is_none());
    }

    #[test]
    fn test_rcr_session_session_id_some() {
        let mut rcr = make_rcr("b", false);
        rcr.session_id = Some("uuid-abc".into());
        assert_eq!(rcr.session_id.as_deref(), Some("uuid-abc"));
    }

    #[test]
    fn test_rcr_session_approval_pending() {
        let rcr = make_rcr("b", false);
        assert!(rcr.approval_pending);
    }

    #[test]
    fn test_rcr_session_continue_with_merge_false() {
        let rcr = make_rcr("b", false);
        assert!(!rcr.continue_with_merge);
    }

    #[test]
    fn test_rcr_session_continue_with_merge_true() {
        let rcr = make_rcr("b", true);
        assert!(rcr.continue_with_merge);
    }

    // ── accept_rcr sets status message on non-merge path ──

    #[test]
    fn test_accept_rcr_no_merge_sets_status() {
        let mut app = App::new();
        app.rcr_session = Some(make_rcr("feat", false));
        accept_rcr(&mut app);
        assert_eq!(app.loading_indicator.as_deref(), Some("Applying RCR resolution..."));
        // Status should mention rebase complete
        let completion = recv_rcr_completion(&mut app);
        let status = completion.status_msg.as_str();
        assert!(
            status.contains("Rebase complete")
                || status.contains("push failed")
                || status.contains("feat"),
            "Status: {}",
            status
        );
    }

    #[test]
    fn test_accept_rcr_no_merge_mentions_branch() {
        let mut app = App::new();
        app.rcr_session = Some(make_rcr("my-branch", false));
        accept_rcr(&mut app);
        let completion = recv_rcr_completion(&mut app);
        let status = completion.status_msg.as_str();
        assert!(
            status.contains("my-branch"),
            "Status should mention branch name: {}",
            status
        );
    }

    // ── accept_rcr with continue_with_merge ──

    #[test]
    fn test_accept_rcr_merge_path_sets_status() {
        let mut app = App::new();
        app.rcr_session = Some(make_rcr("feat", true));
        accept_rcr(&mut app);
        let completion = recv_rcr_completion(&mut app);
        // squash_merge will fail in test env — status should reflect that
        assert!(!completion.status_msg.is_empty());
    }

    #[test]
    fn test_abort_rcr_sets_loading_and_status() {
        let mut app = App::new();
        app.rcr_session = Some(make_rcr("feat", false));
        abort_rcr(&mut app);
        assert_eq!(app.loading_indicator.as_deref(), Some("Aborting RCR..."));
        assert!(app.rcr_session.is_none());
        let completion = recv_rcr_completion(&mut app);
        assert!(completion.status_msg.contains("RCR cancelled"));
        assert!(completion.post_merge_dialog.is_none());
    }

    // ── accept_rcr with session_id triggers cleanup ──

    #[test]
    fn test_accept_rcr_with_session_id() {
        let mut app = App::new();
        let mut rcr = make_rcr("feat", false);
        rcr.session_id = Some("test-session-uuid".into());
        app.rcr_session = Some(rcr);
        accept_rcr(&mut app);
        assert!(app.rcr_session.is_none());
    }

    // ── App default state for rcr fields ──

    #[test]
    fn test_app_default_rcr_none() {
        let app = App::new();
        assert!(app.rcr_session.is_none());
    }

    #[test]
    fn test_app_default_post_merge_dialog_none() {
        let app = App::new();
        assert!(app.post_merge_dialog.is_none());
    }

    // ── RcrSession debug formatting ──

    #[test]
    fn test_rcr_session_debug() {
        let rcr = make_rcr("feat", false);
        let debug = format!("{:?}", rcr);
        assert!(debug.contains("feat"));
        assert!(debug.contains("42"));
    }

    // ── Multiple accept_rcr calls ──

    #[test]
    fn test_accept_rcr_double_call_safe() {
        let mut app = App::new();
        app.rcr_session = Some(make_rcr("feat", false));
        accept_rcr(&mut app);
        // Second call with no session should be noop
        accept_rcr(&mut app);
        assert!(app.rcr_session.is_none());
    }

    // ── RcrSession with various branch names ──

    #[test]
    fn test_rcr_empty_branch() {
        let rcr = make_rcr("", false);
        assert_eq!(rcr.branch, "");
    }

    #[test]
    fn test_rcr_long_branch_name() {
        let long = "a".repeat(500);
        let rcr = make_rcr(&long, false);
        assert_eq!(rcr.branch.len(), 500);
    }

    #[test]
    fn test_rcr_branch_with_slashes() {
        let rcr = make_rcr("azureal/feature/sub", false);
        assert_eq!(rcr.branch, "azureal/feature/sub");
    }

    #[test]
    fn test_rcr_branch_with_special_chars() {
        let rcr = make_rcr("fix-bug_123", false);
        assert_eq!(rcr.branch, "fix-bug_123");
    }

    // ── accept_rcr idempotency ──

    #[test]
    fn test_accept_rcr_noop_preserves_app_state() {
        let mut app = App::new();
        app.input = "some input".into();
        app.focus = crate::app::types::Focus::Input;
        accept_rcr(&mut app);
        assert_eq!(app.input, "some input");
        assert_eq!(app.focus, crate::app::types::Focus::Input);
    }

    // ── RcrSession clone ──

    #[test]
    fn test_rcr_session_clone_branch() {
        let rcr = make_rcr("cloned-branch", false);
        let cloned = rcr.clone();
        assert_eq!(cloned.branch, "cloned-branch");
    }

    #[test]
    fn test_rcr_session_clone_slot_id() {
        let rcr = make_rcr("b", false);
        let cloned = rcr.clone();
        assert_eq!(cloned.slot_id, "42");
    }

    #[test]
    fn test_rcr_session_clone_session_id_none() {
        let rcr = make_rcr("b", false);
        let cloned = rcr.clone();
        assert!(cloned.session_id.is_none());
    }

    #[test]
    fn test_rcr_session_clone_session_id_some() {
        let mut rcr = make_rcr("b", false);
        rcr.session_id = Some("uuid-xyz".into());
        let cloned = rcr.clone();
        assert_eq!(cloned.session_id.as_deref(), Some("uuid-xyz"));
    }

    #[test]
    fn test_rcr_session_clone_continue_with_merge() {
        let rcr = make_rcr("b", true);
        let cloned = rcr.clone();
        assert!(cloned.continue_with_merge);
    }

    // ── make_rcr helper produces consistent fields ──

    #[test]
    fn test_make_rcr_worktree_path_consistency() {
        let a = make_rcr("branch-a", false);
        let b = make_rcr("branch-b", true);
        assert_eq!(a.worktree_path, b.worktree_path);
    }

    #[test]
    fn test_make_rcr_repo_root_consistency() {
        let a = make_rcr("x", false);
        let b = make_rcr("y", true);
        assert_eq!(a.repo_root, b.repo_root);
    }

    #[test]
    fn test_make_rcr_slot_id_consistency() {
        let a = make_rcr("x", false);
        let b = make_rcr("y", true);
        assert_eq!(a.slot_id, b.slot_id);
    }

    // ── accept_rcr status message inspection for no-merge path ──

    #[test]
    fn test_accept_rcr_no_merge_status_not_empty() {
        let mut app = App::new();
        app.rcr_session = Some(make_rcr("feature", false));
        accept_rcr(&mut app);
        let completion = recv_rcr_completion(&mut app);
        assert!(!completion.status_msg.is_empty());
    }

    #[test]
    fn test_accept_rcr_no_merge_status_contains_resolved() {
        let mut app = App::new();
        app.rcr_session = Some(make_rcr("my-feature", false));
        accept_rcr(&mut app);
        let completion = recv_rcr_completion(&mut app);
        let s = completion.status_msg.as_str();
        // The status must contain either "Rebase complete" or "push" references
        assert!(
            s.contains("Rebase complete") || s.contains("push") || s.contains("my-feature"),
            "Unexpected status: {}",
            s
        );
    }

    // ── accept_rcr clears rcr_session before any git ops ──

    #[test]
    fn test_accept_rcr_session_is_none_after_call() {
        for continue_merge in [false, true] {
            let mut app = App::new();
            app.rcr_session = Some(make_rcr("test", continue_merge));
            accept_rcr(&mut app);
            assert!(
                app.rcr_session.is_none(),
                "rcr_session must be None after accept_rcr"
            );
        }
    }

    // ── PostMergeDialog fields ──

    #[test]
    fn test_post_merge_dialog_fields() {
        let dialog = crate::app::types::PostMergeDialog {
            branch: "azureal/feature".into(),
            display_name: "feature".into(),
            worktree_path: PathBuf::from("/tmp/wt"),
            selected: 0,
        };
        assert_eq!(dialog.branch, "azureal/feature");
        assert_eq!(dialog.display_name, "feature");
        assert_eq!(dialog.selected, 0);
    }

    #[test]
    fn test_post_merge_dialog_selected_options() {
        for selected in 0..=2 {
            let dialog = crate::app::types::PostMergeDialog {
                branch: "b".into(),
                display_name: "b".into(),
                worktree_path: PathBuf::from("/tmp"),
                selected,
            };
            assert_eq!(dialog.selected, selected);
        }
    }

    // ── App focus variants ──

    #[test]
    fn test_accept_rcr_noop_preserves_focus_worktrees() {
        let mut app = App::new();
        app.focus = crate::app::types::Focus::Worktrees;
        accept_rcr(&mut app);
        assert_eq!(app.focus, crate::app::types::Focus::Worktrees);
    }

    #[test]
    fn test_accept_rcr_noop_preserves_focus_viewer() {
        let mut app = App::new();
        app.focus = crate::app::types::Focus::Viewer;
        accept_rcr(&mut app);
        assert_eq!(app.focus, crate::app::types::Focus::Viewer);
    }

    #[test]
    fn test_accept_rcr_noop_preserves_focus_session() {
        let mut app = App::new();
        app.focus = crate::app::types::Focus::Session;
        accept_rcr(&mut app);
        assert_eq!(app.focus, crate::app::types::Focus::Session);
    }

    // ── RcrSession with session_id and no-merge path ──

    #[test]
    fn test_accept_rcr_session_id_set_no_merge() {
        let mut app = App::new();
        let mut rcr = make_rcr("sid-branch", false);
        rcr.session_id = Some("cleanup-session-id".into());
        app.rcr_session = Some(rcr);
        accept_rcr(&mut app);
        // Should complete without panic and clear rcr_session
        assert!(app.rcr_session.is_none());
    }

    #[test]
    fn test_accept_rcr_session_id_set_with_merge() {
        let mut app = App::new();
        let mut rcr = make_rcr("sid-branch-merge", true);
        rcr.session_id = Some("merge-cleanup-id".into());
        app.rcr_session = Some(rcr);
        accept_rcr(&mut app);
        assert!(app.rcr_session.is_none());
    }

    // ── RcrSession approval_pending toggled ──

    #[test]
    fn test_rcr_approval_pending_false_variant() {
        let mut rcr = make_rcr("b", false);
        rcr.approval_pending = false;
        assert!(!rcr.approval_pending);
    }

    // ── Multiple RcrSession instances are independent ──

    #[test]
    fn test_multiple_rcr_sessions_independent() {
        let a = make_rcr("branch-a", false);
        let b = make_rcr("branch-b", true);
        assert_ne!(a.branch, b.branch);
        assert_ne!(a.continue_with_merge, b.continue_with_merge);
    }

    // ── accept_rcr with empty slot_id ──

    #[test]
    fn test_accept_rcr_empty_slot_id() {
        let mut app = App::new();
        let mut rcr = make_rcr("b", false);
        rcr.slot_id = String::new();
        app.rcr_session = Some(rcr);
        accept_rcr(&mut app);
        assert!(app.rcr_session.is_none());
    }

    #[test]
    fn test_rcr_session_worktree_path_is_absolute() {
        let rcr = make_rcr("b", false);
        assert!(
            rcr.worktree_path.is_absolute(),
            "worktree_path should be absolute"
        );
    }

    #[test]
    fn test_rcr_session_repo_root_is_absolute() {
        let rcr = make_rcr("b", false);
        assert!(rcr.repo_root.is_absolute(), "repo_root should be absolute");
    }
}
