//! Periodic auto-rebase batching.
//!
//! Auto-rebase work is split into a lightweight UI-thread scheduler and a
//! bounded worker batch. Workers rebase independent feature worktrees against
//! one resolved main commit, while conflict presentation remains serialized
//! through the existing Git conflict overlay and RCR flow.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

use crate::app::types::{
    AutoRebaseBatch, AutoRebaseConflict, AutoRebaseOutcome, AutoRebaseProgress, GitConflictOverlay,
};
use crate::app::App;
use crate::backend::AgentProcess;
use crate::git::Git;
use crate::tui::input_git_actions::{exec_rebase_inner, is_unborn_head, RebaseOutcome};

/// Maximum number of auto-rebase workers allowed to run at once.
const AUTO_REBASE_WORKER_CAP: usize = 4;

/// Duration for the existing auto-rebase success toast.
const AUTO_REBASE_SUCCESS_TOAST: Duration = Duration::from_secs(3);

/// Captured worktree job for a single auto-rebase worker.
#[derive(Debug, Clone, PartialEq, Eq)]
struct AutoRebaseJob {
    /// Branch name for the target worktree.
    branch: String,
    /// User-facing branch label with the Azureal prefix stripped.
    display_name: String,
    /// Filesystem path to the worktree.
    worktree_path: PathBuf,
}

/// Summary data copied out of a finished batch before clearing its receiver.
#[derive(Debug, Clone, PartialEq, Eq)]
struct AutoRebaseSummary {
    /// Number of worktree jobs captured at batch start.
    total: usize,
    /// Number of worktree jobs reported before completion or disconnection.
    completed: usize,
    /// Number of worker threads used by this batch.
    max_workers: usize,
    /// Display names for successfully rebased worktrees.
    rebased: Vec<String>,
    /// Failure messages collected from workers.
    failures: Vec<String>,
    /// Number of worktrees skipped by safety checks.
    skipped: usize,
    /// Number of worktrees left in conflict state.
    conflicts: usize,
    /// True when the result channel disconnected before every job reported.
    stopped_early: bool,
}

/// Check enabled worktrees and start a bounded auto-rebase batch when eligible.
/// Returns true if UI-visible state changed.
pub fn check_auto_rebase(app: &mut App, _claude_process: &AgentProcess) -> bool {
    if !can_start_auto_rebase_batch(app) {
        return false;
    }

    let project = match &app.project {
        Some(project) => project.clone(),
        None => return false,
    };

    let git_panel_branch = app
        .git_actions_panel
        .as_ref()
        .map(|panel| panel.worktree_name.clone());

    let jobs = collect_auto_rebase_jobs(app, &project.main_branch, git_panel_branch.as_deref());
    if jobs.is_empty() {
        return false;
    }

    let Some(target_ref) = resolve_main_tip(&project.path, &project.main_branch) else {
        app.set_status(format!(
            "Auto-rebase skipped: could not resolve {}",
            project.main_branch
        ));
        return true;
    };

    let total = jobs.len();
    let max_workers = auto_rebase_worker_count(total);
    let receiver = spawn_auto_rebase_batch(jobs, target_ref, max_workers);
    app.auto_rebase_batch = Some(AutoRebaseBatch {
        receiver,
        total,
        completed: 0,
        max_workers,
        rebased: Vec::new(),
        failures: Vec::new(),
        skipped: 0,
        conflicts: 0,
    });
    app.set_status(format!(
        "Auto-rebase started: {} worktrees, {} workers",
        total, max_workers
    ));
    true
}

/// Poll the active auto-rebase batch and promote queued conflicts when the UI is free.
pub(super) fn poll_auto_rebase_batch(app: &mut App) -> bool {
    let mut changed = drain_auto_rebase_batch(app);
    if promote_next_auto_rebase_conflict(app) {
        changed = true;
    }
    changed
}

/// Return true when the scheduler may start a new auto-rebase batch.
fn can_start_auto_rebase_batch(app: &App) -> bool {
    if app.auto_rebase_batch.is_some() || !app.pending_auto_rebase_conflicts.is_empty() {
        return false;
    }
    if app.viewer_edit_mode || auto_rebase_conflict_ui_blocked(app) {
        return false;
    }
    true
}

/// Collect auto-rebase jobs from the current app snapshot.
fn collect_auto_rebase_jobs(
    app: &App,
    main_branch: &str,
    git_panel_branch: Option<&str>,
) -> Vec<AutoRebaseJob> {
    app.worktrees
        .iter()
        .filter(|worktree| {
            worktree.branch_name != main_branch
                && !worktree.archived
                && app.auto_rebase_enabled.contains(&worktree.branch_name)
                && !app.is_session_running(&worktree.branch_name)
                && git_panel_branch != Some(worktree.branch_name.as_str())
        })
        .filter_map(|worktree| {
            let worktree_path = worktree.worktree_path.clone()?;
            Some(AutoRebaseJob {
                display_name: crate::models::strip_branch_prefix(&worktree.branch_name).to_string(),
                branch: worktree.branch_name.clone(),
                worktree_path,
            })
        })
        .collect()
}

/// Resolve the exact main commit all workers should rebase onto.
fn resolve_main_tip(repo_root: &Path, main_branch: &str) -> Option<String> {
    let verified_ref = format!("{main_branch}^{{commit}}");
    Command::new("git")
        .args(["rev-parse", "--verify", &verified_ref])
        .current_dir(repo_root)
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| {
            let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if text.is_empty() {
                None
            } else {
                Some(text)
            }
        })
}

/// Choose the bounded worker count for a batch.
fn auto_rebase_worker_count(total_jobs: usize) -> usize {
    if total_jobs == 0 {
        return 0;
    }
    let available = std::thread::available_parallelism()
        .map(usize::from)
        .unwrap_or(1);
    total_jobs.min(available).clamp(1, AUTO_REBASE_WORKER_CAP)
}

/// Spawn the coordinator that owns the bounded worker pool.
fn spawn_auto_rebase_batch(
    jobs: Vec<AutoRebaseJob>,
    target_ref: String,
    max_workers: usize,
) -> mpsc::Receiver<AutoRebaseProgress> {
    let (tx, rx) = mpsc::channel();
    let queue = Arc::new(Mutex::new(VecDeque::from(jobs)));

    std::thread::spawn(move || {
        let mut handles = Vec::new();
        for _ in 0..max_workers {
            let worker_queue = Arc::clone(&queue);
            let worker_tx = tx.clone();
            let worker_target = target_ref.clone();
            handles.push(std::thread::spawn(move || {
                while let Some(job) = pop_auto_rebase_job(&worker_queue) {
                    let progress = run_auto_rebase_job(job, &worker_target);
                    if worker_tx.send(progress).is_err() {
                        break;
                    }
                }
            }));
        }
        drop(tx);
        for handle in handles {
            let _ = handle.join();
        }
    });

    rx
}

/// Pop the next job from the shared worker queue.
fn pop_auto_rebase_job(queue: &Arc<Mutex<VecDeque<AutoRebaseJob>>>) -> Option<AutoRebaseJob> {
    queue.lock().ok()?.pop_front()
}

/// Run one auto-rebase job and map it into an event-loop friendly result.
fn run_auto_rebase_job(job: AutoRebaseJob, target_ref: &str) -> AutoRebaseProgress {
    if worktree_has_uncommitted_changes(&job.worktree_path) && !is_unborn_head(&job.worktree_path) {
        return AutoRebaseProgress {
            branch: job.branch,
            display_name: job.display_name,
            worktree_path: job.worktree_path,
            outcome: AutoRebaseOutcome::Skipped {
                reason: "worktree has uncommitted changes".to_string(),
            },
        };
    }

    let auto_resolve_files = crate::azufig::load_auto_resolve_files(&job.worktree_path);
    let outcome = match exec_rebase_inner(&job.worktree_path, target_ref, &auto_resolve_files) {
        RebaseOutcome::UpToDate => AutoRebaseOutcome::UpToDate,
        RebaseOutcome::Rebased => match Git::push(&job.worktree_path) {
            Ok(_) => AutoRebaseOutcome::Rebased {
                pushed: true,
                push_error: None,
            },
            Err(err) => AutoRebaseOutcome::Rebased {
                pushed: false,
                push_error: Some(err.to_string()),
            },
        },
        RebaseOutcome::Conflict {
            conflicted,
            auto_merged,
            ..
        } => AutoRebaseOutcome::Conflict {
            conflicted_files: conflicted,
            auto_merged_files: auto_merged,
        },
        RebaseOutcome::Failed(message) => AutoRebaseOutcome::Failed { message },
    };

    AutoRebaseProgress {
        branch: job.branch,
        display_name: job.display_name,
        worktree_path: job.worktree_path,
        outcome,
    }
}

/// Return true when `git status --porcelain` reports local changes.
fn worktree_has_uncommitted_changes(worktree_path: &Path) -> bool {
    Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(worktree_path)
        .output()
        .ok()
        .map(|output| !output.stdout.is_empty())
        .unwrap_or(false)
}

/// Drain completed worker results from the active batch.
fn drain_auto_rebase_batch(app: &mut App) -> bool {
    let mut changed = false;
    let mut queued_conflicts = Vec::new();
    let mut disconnected = false;

    if let Some(batch) = app.auto_rebase_batch.as_mut() {
        loop {
            match batch.receiver.try_recv() {
                Ok(progress) => {
                    batch.completed += 1;
                    record_auto_rebase_progress(batch, progress, &mut queued_conflicts);
                    changed = true;
                }
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    disconnected = true;
                    break;
                }
            }
        }
    }

    for conflict in queued_conflicts {
        app.pending_auto_rebase_conflicts.push_back(conflict);
    }

    let summary = app.auto_rebase_batch.as_ref().and_then(|batch| {
        let stopped_early = disconnected && batch.completed < batch.total;
        if stopped_early || batch.completed >= batch.total {
            Some(AutoRebaseSummary {
                total: batch.total,
                completed: batch.completed,
                max_workers: batch.max_workers,
                rebased: batch.rebased.clone(),
                failures: batch.failures.clone(),
                skipped: batch.skipped,
                conflicts: batch.conflicts,
                stopped_early,
            })
        } else {
            None
        }
    });

    if let Some(summary) = summary {
        app.auto_rebase_batch = None;
        complete_auto_rebase_batch(app, summary);
        changed = true;
    }

    changed
}

/// Record one worker result into batch aggregates and the conflict queue.
fn record_auto_rebase_progress(
    batch: &mut AutoRebaseBatch,
    progress: AutoRebaseProgress,
    queued_conflicts: &mut Vec<AutoRebaseConflict>,
) {
    match progress.outcome {
        AutoRebaseOutcome::UpToDate => {}
        AutoRebaseOutcome::Rebased { pushed, push_error } => {
            if pushed {
                batch
                    .rebased
                    .push(format!("{} → pushed", progress.display_name));
            } else {
                batch.rebased.push(progress.display_name.clone());
                if let Some(error) = push_error {
                    batch
                        .failures
                        .push(format!("{} push failed: {}", progress.display_name, error));
                }
            }
        }
        AutoRebaseOutcome::Conflict {
            conflicted_files,
            auto_merged_files,
        } => {
            batch.conflicts += 1;
            queued_conflicts.push(AutoRebaseConflict {
                branch: progress.branch,
                display_name: progress.display_name,
                worktree_path: progress.worktree_path,
                conflicted_files,
                auto_merged_files,
            });
        }
        AutoRebaseOutcome::Skipped { reason } => {
            batch.skipped += 1;
            batch
                .failures
                .push(format!("{} skipped: {}", progress.display_name, reason));
        }
        AutoRebaseOutcome::Failed { message } => {
            batch
                .failures
                .push(format!("{} failed: {}", progress.display_name, message));
        }
    }
}

/// Apply completion status, toast state, and redraw invalidation for a batch.
fn complete_auto_rebase_batch(app: &mut App, summary: AutoRebaseSummary) {
    if !summary.rebased.is_empty() {
        app.auto_rebase_success_until = Some((
            summary.rebased.clone(),
            Instant::now() + AUTO_REBASE_SUCCESS_TOAST,
        ));
        app.invalidate_sidebar();
    }

    let status = auto_rebase_summary_status(&summary);
    app.set_status(status);
}

/// Build the user-visible status line for a finished batch.
fn auto_rebase_summary_status(summary: &AutoRebaseSummary) -> String {
    let mut parts = Vec::new();
    if summary.stopped_early {
        parts.push(format!(
            "stopped early after {}/{}",
            summary.completed, summary.total
        ));
    } else {
        parts.push(format!(
            "{}/{} checked by {} workers",
            summary.completed, summary.total, summary.max_workers
        ));
    }
    if !summary.rebased.is_empty() {
        parts.push(format!("{} rebased", summary.rebased.len()));
    }
    if summary.conflicts > 0 {
        parts.push(format!("{} need conflict resolution", summary.conflicts));
    }
    if summary.skipped > 0 {
        parts.push(format!("{} skipped", summary.skipped));
    }
    if !summary.failures.is_empty() {
        parts.push(format!("{} reported issues", summary.failures.len()));
    }
    if parts.len() == 1 && summary.rebased.is_empty() && summary.conflicts == 0 {
        parts.push("no updates".to_string());
    }
    format!("Auto-rebase complete: {}", parts.join(", "))
}

/// Promote the next queued auto-rebase conflict into the existing Git overlay.
fn promote_next_auto_rebase_conflict(app: &mut App) -> bool {
    if auto_rebase_conflict_ui_blocked(app) {
        return false;
    }

    let mut changed = false;
    while let Some(conflict) = app.pending_auto_rebase_conflicts.pop_front() {
        let Some(idx) = app
            .worktrees
            .iter()
            .position(|worktree| worktree.branch_name == conflict.branch)
        else {
            app.set_status(format!(
                "Auto-rebase conflict skipped: {} no longer exists",
                conflict.display_name
            ));
            changed = true;
            continue;
        };

        app.save_live_display_events();
        app.save_current_terminal();
        app.browsing_main = false;
        app.selected_worktree = Some(idx);
        app.load_session_output();
        app.open_git_actions_panel();

        if let Some(ref mut panel) = app.git_actions_panel {
            panel.conflict_overlay = Some(GitConflictOverlay {
                conflicted_files: conflict.conflicted_files,
                auto_merged_files: conflict.auto_merged_files,
                scroll: 0,
                selected: 0,
                continue_with_merge: false,
            });
            panel.result_message = Some((
                format!(
                    "Auto-rebase conflict on {}. Resolve or abort before the next queued conflict opens.",
                    conflict.display_name
                ),
                true,
            ));
            crate::tui::input_git_actions::refresh_changed_files(panel);
            crate::tui::input_git_actions::refresh_commit_log(panel);
        }

        let remaining = app.pending_auto_rebase_conflicts.len();
        if remaining > 0 {
            app.set_status(format!(
                "Auto-rebase conflict opened; {} more queued",
                remaining
            ));
        } else {
            app.set_status("Auto-rebase conflict opened");
        }
        app.invalidate_sidebar();
        return true;
    }

    changed
}

/// Return true when conflict/RCR UI is already occupied by another operation.
fn auto_rebase_conflict_ui_blocked(app: &App) -> bool {
    if app.rcr_session.is_some()
        || app.background_op_receiver.is_some()
        || app.rebase_op_receiver.is_some()
        || app.post_merge_dialog.is_some()
    {
        return true;
    }
    app.git_actions_panel
        .as_ref()
        .map(|panel| {
            panel.conflict_overlay.is_some()
                || panel.squash_merge_receiver.is_some()
                || panel
                    .commit_overlay
                    .as_ref()
                    .is_some_and(|overlay| overlay.generating)
        })
        .unwrap_or(false)
}

/// Unit tests for auto-rebase batching and conflict queue behavior.
#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::types::{GitActionsPanel, RcrSession};

    /// Build an empty batch fixture with a receiver that will never produce data.
    fn empty_batch(total: usize) -> AutoRebaseBatch {
        let (_tx, rx) = mpsc::channel();
        AutoRebaseBatch {
            receiver: rx,
            total,
            completed: 0,
            max_workers: 1,
            rebased: Vec::new(),
            failures: Vec::new(),
            skipped: 0,
            conflicts: 0,
        }
    }

    /// Build an RCR fixture that blocks auto-rebase conflict promotion.
    fn dummy_rcr_session() -> RcrSession {
        RcrSession {
            branch: "azureal/conflict".to_string(),
            display_name: "conflict".to_string(),
            worktree_path: PathBuf::from("/tmp/conflict"),
            repo_root: PathBuf::from("/tmp/repo"),
            slot_id: "slot-1".to_string(),
            session_id: None,
            approval_pending: false,
            continue_with_merge: false,
        }
    }

    /// Build a minimal Git panel fixture for scheduler blocking tests.
    fn git_panel_with_conflict_overlay() -> GitActionsPanel {
        GitActionsPanel {
            worktree_name: "azureal/conflict".to_string(),
            worktree_path: PathBuf::from("/tmp/conflict"),
            repo_root: PathBuf::from("/tmp/repo"),
            main_branch: "main".to_string(),
            is_on_main: false,
            changed_files: Vec::new(),
            selected_file: 0,
            file_scroll: 0,
            focused_pane: 0,
            selected_action: 0,
            result_message: None,
            commit_overlay: None,
            conflict_overlay: Some(GitConflictOverlay {
                conflicted_files: vec!["src/lib.rs".to_string()],
                auto_merged_files: Vec::new(),
                scroll: 0,
                selected: 0,
                continue_with_merge: false,
            }),
            commits: Vec::new(),
            selected_commit: 0,
            commit_scroll: 0,
            viewer_diff: None,
            viewer_diff_title: None,
            commits_behind_main: 0,
            commits_ahead_main: 0,
            commits_behind_remote: 0,
            commits_ahead_remote: 0,
            auto_resolve_files: Vec::new(),
            auto_resolve_overlay: None,
            squash_merge_receiver: None,
            discard_confirm: None,
            cached_staged_count: 0,
            cached_total_add: 0,
            cached_total_del: 0,
        }
    }

    /// Worker count stays positive for non-empty batches and never exceeds the cap.
    #[test]
    fn worker_count_is_bounded_by_jobs_parallelism_and_cap() {
        assert_eq!(auto_rebase_worker_count(0), 0);
        let one = auto_rebase_worker_count(1);
        assert_eq!(one, 1);

        let many = auto_rebase_worker_count(usize::MAX);
        assert!(many >= 1);
        assert!(many <= AUTO_REBASE_WORKER_CAP);
    }

    /// An existing conflict overlay is active conflict UI and blocks new batches.
    #[test]
    fn conflict_overlay_blocks_starting_new_batch() {
        let mut app = App::new();
        app.git_actions_panel = Some(git_panel_with_conflict_overlay());

        assert!(!can_start_auto_rebase_batch(&app));
    }

    /// Recording a conflict stores it for later serialized UI promotion.
    #[test]
    fn record_progress_queues_conflicts_without_successes() {
        let mut batch = empty_batch(1);
        let mut queued = Vec::new();
        record_auto_rebase_progress(
            &mut batch,
            AutoRebaseProgress {
                branch: "azureal/conflict".to_string(),
                display_name: "conflict".to_string(),
                worktree_path: PathBuf::from("/tmp/conflict"),
                outcome: AutoRebaseOutcome::Conflict {
                    conflicted_files: vec!["src/lib.rs".to_string()],
                    auto_merged_files: vec!["README.md".to_string()],
                },
            },
            &mut queued,
        );

        assert_eq!(batch.conflicts, 1);
        assert!(batch.rebased.is_empty());
        assert_eq!(queued.len(), 1);
        assert_eq!(queued[0].conflicted_files, vec!["src/lib.rs"]);
    }

    /// RCR state blocks queued auto-rebase conflicts from opening a second overlay.
    #[test]
    fn poll_queues_conflict_without_promoting_while_rcr_active() {
        let (tx, rx) = mpsc::channel();
        tx.send(AutoRebaseProgress {
            branch: "azureal/conflict".to_string(),
            display_name: "conflict".to_string(),
            worktree_path: PathBuf::from("/tmp/conflict"),
            outcome: AutoRebaseOutcome::Conflict {
                conflicted_files: vec!["src/lib.rs".to_string()],
                auto_merged_files: Vec::new(),
            },
        })
        .unwrap();

        let mut app = App::new();
        app.rcr_session = Some(dummy_rcr_session());
        app.auto_rebase_batch = Some(AutoRebaseBatch {
            receiver: rx,
            total: 1,
            completed: 0,
            max_workers: 1,
            rebased: Vec::new(),
            failures: Vec::new(),
            skipped: 0,
            conflicts: 0,
        });

        assert!(poll_auto_rebase_batch(&mut app));
        assert!(app.auto_rebase_batch.is_none());
        assert_eq!(app.pending_auto_rebase_conflicts.len(), 1);
        assert!(app.git_actions_panel.is_none());
        assert!(app
            .status_message
            .as_deref()
            .unwrap_or_default()
            .contains("need conflict resolution"));
    }
}
