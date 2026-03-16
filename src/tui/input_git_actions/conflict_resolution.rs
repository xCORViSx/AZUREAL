//! Conflict resolution overlay and RCR (Resolve Conflicts with Claude) spawning.
//!
//! Handles the conflict overlay UI (navigate, resolve with Claude, abort rebase)
//! and spawns a streaming Claude session to resolve rebase conflicts.

use anyhow::Result;
use crossterm::event;

use crate::app::types::RcrSession;
use crate::app::{App, Focus};
use crate::backend::AgentProcess;
use crate::git::Git;

/// Handle input while the conflict resolution overlay is open.
/// j/k or Up/Down navigate between "Resolve with Claude" and "Abort rebase".
/// Enter/y resolves, n/Esc aborts the rebase and closes the overlay.
pub(super) fn handle_conflict_overlay(
    key: event::KeyEvent,
    app: &mut App,
    claude_process: &AgentProcess,
) -> Result<()> {
    use crossterm::event::{KeyCode, KeyModifiers};

    let (sel, wt_path, repo_root, branch, conflicted, auto_merged, continue_merge) =
        match app.git_actions_panel.as_ref() {
            Some(p) => match p.conflict_overlay.as_ref() {
                Some(ov) => (
                    ov.selected,
                    p.worktree_path.clone(),
                    p.repo_root.clone(),
                    p.worktree_name.clone(),
                    ov.conflicted_files.clone(),
                    ov.auto_merged_files.clone(),
                    ov.continue_with_merge,
                ),
                None => return Ok(()),
            },
            None => return Ok(()),
        };

    match (key.modifiers, key.code) {
        (KeyModifiers::NONE, KeyCode::Char('j')) | (KeyModifiers::NONE, KeyCode::Down) => {
            if let Some(ref mut p) = app.git_actions_panel {
                if let Some(ref mut ov) = p.conflict_overlay {
                    if ov.selected < 1 {
                        ov.selected = 1;
                    }
                }
            }
        }
        (KeyModifiers::NONE, KeyCode::Char('k')) | (KeyModifiers::NONE, KeyCode::Up) => {
            if let Some(ref mut p) = app.git_actions_panel {
                if let Some(ref mut ov) = p.conflict_overlay {
                    if ov.selected > 0 {
                        ov.selected = 0;
                    }
                }
            }
        }

        (KeyModifiers::NONE, KeyCode::Enter) => {
            if sel == 0 {
                spawn_conflict_claude(
                    app,
                    claude_process,
                    &wt_path,
                    &repo_root,
                    &branch,
                    &conflicted,
                    &auto_merged,
                    continue_merge,
                );
            } else {
                abort_rebase(app, &wt_path);
            }
        }

        (KeyModifiers::NONE, KeyCode::Char('y')) => {
            spawn_conflict_claude(
                app,
                claude_process,
                &wt_path,
                &repo_root,
                &branch,
                &conflicted,
                &auto_merged,
                continue_merge,
            );
        }

        (KeyModifiers::NONE, KeyCode::Char('n')) | (KeyModifiers::NONE, KeyCode::Esc) => {
            abort_rebase(app, &wt_path);
        }

        _ => {}
    }
    Ok(())
}

/// Abort an in-progress rebase/merge on the feature branch, close the overlay.
/// Also cleans up squash merge state on main (repo root) if present.
fn abort_rebase(app: &mut App, wt_path: &std::path::Path) {
    let _ = Git::rebase_abort(wt_path);
    // Pop any stash left by exec_rebase_inner's pre-rebase stash
    let _ = std::process::Command::new("git")
        .args(["stash", "pop"])
        .current_dir(wt_path)
        .output();
    // Clean up squash merge state on main (no MERGE_HEAD to abort)
    if let Some(ref p) = app.git_actions_panel {
        Git::cleanup_squash_merge_state(&p.repo_root);
    }
    if let Some(ref mut p) = app.git_actions_panel {
        p.conflict_overlay = None;
        p.result_message = Some(("Aborted".into(), false));
        super::refresh_changed_files(p);
    }
}

/// Build the RCR prompt that Claude receives for conflict resolution.
/// Extracted so unit tests can verify prompt content without spawning a process.
fn build_conflict_prompt(display: &str, conflicted: &[String], auto_merged: &[String]) -> String {
    let mut prompt = format!(
        "Rebasing branch '{}' onto main produced merge conflicts.\n\
         Git left the working directory in a partially-rebased state.\n\n",
        display
    );
    prompt.push_str(&format!("Conflicted files ({}):\n", conflicted.len()));
    for f in conflicted {
        prompt.push_str(&format!("  - {}\n", f));
    }
    if !auto_merged.is_empty() {
        prompt.push_str(&format!("\nAuto-merged cleanly ({}):\n", auto_merged.len()));
        for f in auto_merged {
            prompt.push_str(&format!("  - {}\n", f));
        }
    }
    prompt.push_str(
        "\nResolve all conflicts:\n\
         1. Read each conflicted file — look for <<<<<<< / ======= / >>>>>>> markers\n\
         2. Edit each file to keep the correct combined content, removing all markers\n\
         3. Stage resolved files: git add <files>\n\
         4. Continue the rebase: git rebase --continue\n\
         5. If more conflicts appear, repeat steps 1-4 until the rebase completes\n\
         6. Verify with: git status\n\n\
         Ask me if any conflict is ambiguous.",
    );
    prompt
}

/// Spawn a streaming Claude session to resolve rebase conflicts on the
/// feature branch worktree. Claude runs in the worktree directory and uses
/// `git add` + `git rebase --continue` to complete the rebase.
#[allow(clippy::too_many_arguments)]
fn spawn_conflict_claude(
    app: &mut App,
    claude_process: &AgentProcess,
    wt_path: &std::path::Path,
    repo_root: &std::path::Path,
    branch: &str,
    conflicted: &[String],
    auto_merged: &[String],
    continue_with_merge: bool,
) {
    let display = crate::models::strip_branch_prefix(branch);
    let prompt = build_conflict_prompt(display, conflicted, auto_merged);

    match claude_process.spawn(wt_path, &prompt, None, None) {
        Ok((rx, pid)) => {
            let slot = pid.to_string();
            app.pending_session_names
                .push((slot.clone(), format!("[RCR] {}", display)));
            app.register_claude(branch.to_string(), pid, rx);
            app.rcr_session = Some(RcrSession {
                branch: branch.to_string(),
                display_name: display.to_string(),
                worktree_path: wt_path.to_path_buf(),
                repo_root: repo_root.to_path_buf(),
                slot_id: slot,
                session_id: None,
                approval_pending: false,
                continue_with_merge,
            });
            app.title_session_name = format!("[RCR] {}", display);
            app.display_events.clear();
            app.session_lines.clear();
            app.session_buffer.clear();
            app.session_scroll = usize::MAX;
            app.session_file_parse_offset = 0;
            app.rendered_events_count = 0;
            app.rendered_content_line_count = 0;
            app.rendered_events_start = 0;
            app.event_parser = crate::events::EventParser::new();
            app.selected_event = None;
            app.pending_tool_calls.clear();
            app.failed_tool_calls.clear();
            app.session_tokens = None;
            app.model_context_window = None;
            app.token_badge_cache = None;
            app.current_todos.clear();
            app.subagent_todos.clear();
            app.active_task_tool_ids.clear();
            app.subagent_parent_idx = None;
            app.awaiting_ask_user_question = false;
            app.ask_user_questions_cache = None;
            app.invalidate_render_cache();
            app.git_actions_panel = None;
            app.focus = Focus::Session;
        }
        Err(e) => {
            if let Some(ref mut p) = app.git_actions_panel {
                p.conflict_overlay = None;
                p.result_message = Some((format!("Failed to spawn Claude: {}", e), true));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::types::{GitActionsPanel, GitConflictOverlay};
    use crate::app::App;
    use crate::config::Config;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    /// Shorthand for building a KeyEvent
    fn key(code: KeyCode, mods: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, mods)
    }

    /// Build an App with a GitActionsPanel containing a conflict overlay.
    /// `selected` sets the initial overlay selection (0 = resolve, 1 = abort).
    fn app_with_conflict(
        conflicted: Vec<String>,
        auto_merged: Vec<String>,
        selected: usize,
    ) -> App {
        let mut app = App::new();
        app.git_actions_panel = Some(GitActionsPanel {
            worktree_name: "feature-test".into(),
            worktree_path: std::path::PathBuf::from("/tmp/test-wt"),
            repo_root: std::path::PathBuf::from("/tmp/test-repo"),
            main_branch: "main".into(),
            is_on_main: false,
            changed_files: Vec::new(),
            selected_file: 0,
            file_scroll: 0,
            focused_pane: 0,
            selected_action: 0,
            result_message: None,
            commit_overlay: None,
            conflict_overlay: Some(GitConflictOverlay {
                conflicted_files: conflicted,
                auto_merged_files: auto_merged,
                scroll: 0,
                selected,
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
        });
        app
    }

    /// Build an AgentProcess for tests (spawn will fail since no real executable)
    fn test_claude() -> AgentProcess {
        AgentProcess::new(Config::default())
    }

    /// Helper: get the conflict overlay from the app (panics if missing)
    fn overlay(app: &App) -> &GitConflictOverlay {
        app.git_actions_panel
            .as_ref()
            .unwrap()
            .conflict_overlay
            .as_ref()
            .unwrap()
    }

    // ══════════════════════════════════════════════════════════════════
    // 1. Early returns — no panel, no overlay
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn test_no_panel_returns_ok() {
        let mut app = App::new();
        assert!(app.git_actions_panel.is_none());
        let cp = test_claude();
        let result =
            handle_conflict_overlay(key(KeyCode::Char('j'), KeyModifiers::NONE), &mut app, &cp);
        assert!(result.is_ok());
    }

    #[test]
    fn test_panel_without_overlay_returns_ok() {
        let mut app = app_with_conflict(vec![], vec![], 0);
        // Remove the overlay
        app.git_actions_panel.as_mut().unwrap().conflict_overlay = None;
        let cp = test_claude();
        let result =
            handle_conflict_overlay(key(KeyCode::Char('j'), KeyModifiers::NONE), &mut app, &cp);
        assert!(result.is_ok());
    }

    #[test]
    fn test_no_panel_enter_returns_ok() {
        let mut app = App::new();
        let cp = test_claude();
        let result =
            handle_conflict_overlay(key(KeyCode::Enter, KeyModifiers::NONE), &mut app, &cp);
        assert!(result.is_ok());
    }

    #[test]
    fn test_no_panel_esc_returns_ok() {
        let mut app = App::new();
        let cp = test_claude();
        let result = handle_conflict_overlay(key(KeyCode::Esc, KeyModifiers::NONE), &mut app, &cp);
        assert!(result.is_ok());
    }

    #[test]
    fn test_no_panel_y_returns_ok() {
        let mut app = App::new();
        let cp = test_claude();
        let result =
            handle_conflict_overlay(key(KeyCode::Char('y'), KeyModifiers::NONE), &mut app, &cp);
        assert!(result.is_ok());
    }

    #[test]
    fn test_no_panel_n_returns_ok() {
        let mut app = App::new();
        let cp = test_claude();
        let result =
            handle_conflict_overlay(key(KeyCode::Char('n'), KeyModifiers::NONE), &mut app, &cp);
        assert!(result.is_ok());
    }

    // ══════════════════════════════════════════════════════════════════
    // 2. Navigation — j / Down moves selection from 0 to 1
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn test_j_moves_selection_down_from_0() {
        let mut app = app_with_conflict(vec!["a.rs".into()], vec![], 0);
        let cp = test_claude();
        handle_conflict_overlay(key(KeyCode::Char('j'), KeyModifiers::NONE), &mut app, &cp)
            .unwrap();
        assert_eq!(overlay(&app).selected, 1);
    }

    #[test]
    fn test_down_moves_selection_down_from_0() {
        let mut app = app_with_conflict(vec!["a.rs".into()], vec![], 0);
        let cp = test_claude();
        handle_conflict_overlay(key(KeyCode::Down, KeyModifiers::NONE), &mut app, &cp).unwrap();
        assert_eq!(overlay(&app).selected, 1);
    }

    #[test]
    fn test_j_stays_at_1_when_already_at_1() {
        let mut app = app_with_conflict(vec!["a.rs".into()], vec![], 1);
        let cp = test_claude();
        handle_conflict_overlay(key(KeyCode::Char('j'), KeyModifiers::NONE), &mut app, &cp)
            .unwrap();
        assert_eq!(overlay(&app).selected, 1);
    }

    #[test]
    fn test_down_stays_at_1_when_already_at_1() {
        let mut app = app_with_conflict(vec!["a.rs".into()], vec![], 1);
        let cp = test_claude();
        handle_conflict_overlay(key(KeyCode::Down, KeyModifiers::NONE), &mut app, &cp).unwrap();
        assert_eq!(overlay(&app).selected, 1);
    }

    // ══════════════════════════════════════════════════════════════════
    // 3. Navigation — k / Up moves selection from 1 to 0
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn test_k_moves_selection_up_from_1() {
        let mut app = app_with_conflict(vec!["a.rs".into()], vec![], 1);
        let cp = test_claude();
        handle_conflict_overlay(key(KeyCode::Char('k'), KeyModifiers::NONE), &mut app, &cp)
            .unwrap();
        assert_eq!(overlay(&app).selected, 0);
    }

    #[test]
    fn test_up_moves_selection_up_from_1() {
        let mut app = app_with_conflict(vec!["a.rs".into()], vec![], 1);
        let cp = test_claude();
        handle_conflict_overlay(key(KeyCode::Up, KeyModifiers::NONE), &mut app, &cp).unwrap();
        assert_eq!(overlay(&app).selected, 0);
    }

    #[test]
    fn test_k_stays_at_0_when_already_at_0() {
        let mut app = app_with_conflict(vec!["a.rs".into()], vec![], 0);
        let cp = test_claude();
        handle_conflict_overlay(key(KeyCode::Char('k'), KeyModifiers::NONE), &mut app, &cp)
            .unwrap();
        assert_eq!(overlay(&app).selected, 0);
    }

    #[test]
    fn test_up_stays_at_0_when_already_at_0() {
        let mut app = app_with_conflict(vec!["a.rs".into()], vec![], 0);
        let cp = test_claude();
        handle_conflict_overlay(key(KeyCode::Up, KeyModifiers::NONE), &mut app, &cp).unwrap();
        assert_eq!(overlay(&app).selected, 0);
    }

    // ══════════════════════════════════════════════════════════════════
    // 4. Navigation round-trips
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn test_j_then_k_round_trip() {
        let mut app = app_with_conflict(vec!["a.rs".into()], vec![], 0);
        let cp = test_claude();
        handle_conflict_overlay(key(KeyCode::Char('j'), KeyModifiers::NONE), &mut app, &cp)
            .unwrap();
        assert_eq!(overlay(&app).selected, 1);
        handle_conflict_overlay(key(KeyCode::Char('k'), KeyModifiers::NONE), &mut app, &cp)
            .unwrap();
        assert_eq!(overlay(&app).selected, 0);
    }

    #[test]
    fn test_down_then_up_round_trip() {
        let mut app = app_with_conflict(vec!["a.rs".into()], vec![], 0);
        let cp = test_claude();
        handle_conflict_overlay(key(KeyCode::Down, KeyModifiers::NONE), &mut app, &cp).unwrap();
        assert_eq!(overlay(&app).selected, 1);
        handle_conflict_overlay(key(KeyCode::Up, KeyModifiers::NONE), &mut app, &cp).unwrap();
        assert_eq!(overlay(&app).selected, 0);
    }

    #[test]
    fn test_multiple_j_presses_clamp_at_1() {
        let mut app = app_with_conflict(vec!["a.rs".into()], vec![], 0);
        let cp = test_claude();
        for _ in 0..5 {
            handle_conflict_overlay(key(KeyCode::Char('j'), KeyModifiers::NONE), &mut app, &cp)
                .unwrap();
        }
        assert_eq!(overlay(&app).selected, 1);
    }

    #[test]
    fn test_multiple_k_presses_clamp_at_0() {
        let mut app = app_with_conflict(vec!["a.rs".into()], vec![], 1);
        let cp = test_claude();
        for _ in 0..5 {
            handle_conflict_overlay(key(KeyCode::Char('k'), KeyModifiers::NONE), &mut app, &cp)
                .unwrap();
        }
        assert_eq!(overlay(&app).selected, 0);
    }

    // ══════════════════════════════════════════════════════════════════
    // 5. Abort path — 'n' key
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn test_n_key_aborts_rebase_clears_overlay() {
        let mut app = app_with_conflict(vec!["a.rs".into()], vec![], 0);
        let cp = test_claude();
        handle_conflict_overlay(key(KeyCode::Char('n'), KeyModifiers::NONE), &mut app, &cp)
            .unwrap();
        let panel = app.git_actions_panel.as_ref().unwrap();
        assert!(panel.conflict_overlay.is_none());
    }

    #[test]
    fn test_n_key_sets_abort_message() {
        let mut app = app_with_conflict(vec!["a.rs".into()], vec![], 0);
        let cp = test_claude();
        handle_conflict_overlay(key(KeyCode::Char('n'), KeyModifiers::NONE), &mut app, &cp)
            .unwrap();
        let panel = app.git_actions_panel.as_ref().unwrap();
        let (msg, is_err) = panel.result_message.as_ref().unwrap();
        assert_eq!(msg, "Aborted");
        assert!(!is_err);
    }

    #[test]
    fn test_n_key_from_selection_1_also_aborts() {
        let mut app = app_with_conflict(vec!["a.rs".into()], vec![], 1);
        let cp = test_claude();
        handle_conflict_overlay(key(KeyCode::Char('n'), KeyModifiers::NONE), &mut app, &cp)
            .unwrap();
        let panel = app.git_actions_panel.as_ref().unwrap();
        assert!(panel.conflict_overlay.is_none());
        assert_eq!(panel.result_message.as_ref().unwrap().0, "Aborted");
    }

    // ══════════════════════════════════════════════════════════════════
    // 6. Abort path — Esc key
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn test_esc_aborts_rebase_clears_overlay() {
        let mut app = app_with_conflict(vec!["a.rs".into()], vec![], 0);
        let cp = test_claude();
        handle_conflict_overlay(key(KeyCode::Esc, KeyModifiers::NONE), &mut app, &cp).unwrap();
        let panel = app.git_actions_panel.as_ref().unwrap();
        assert!(panel.conflict_overlay.is_none());
    }

    #[test]
    fn test_esc_sets_abort_message() {
        let mut app = app_with_conflict(vec!["a.rs".into()], vec![], 0);
        let cp = test_claude();
        handle_conflict_overlay(key(KeyCode::Esc, KeyModifiers::NONE), &mut app, &cp).unwrap();
        let panel = app.git_actions_panel.as_ref().unwrap();
        let (msg, _) = panel.result_message.as_ref().unwrap();
        assert_eq!(msg, "Aborted");
    }

    #[test]
    fn test_esc_from_selection_1_aborts() {
        let mut app = app_with_conflict(vec!["a.rs".into()], vec![], 1);
        let cp = test_claude();
        handle_conflict_overlay(key(KeyCode::Esc, KeyModifiers::NONE), &mut app, &cp).unwrap();
        assert!(app
            .git_actions_panel
            .as_ref()
            .unwrap()
            .conflict_overlay
            .is_none());
    }

    // ══════════════════════════════════════════════════════════════════
    // 7. Abort path — Enter with sel=1
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn test_enter_sel1_aborts_rebase() {
        let mut app = app_with_conflict(vec!["a.rs".into()], vec![], 1);
        let cp = test_claude();
        handle_conflict_overlay(key(KeyCode::Enter, KeyModifiers::NONE), &mut app, &cp).unwrap();
        let panel = app.git_actions_panel.as_ref().unwrap();
        assert!(panel.conflict_overlay.is_none());
        assert_eq!(panel.result_message.as_ref().unwrap().0, "Aborted");
    }

    #[test]
    fn test_enter_sel1_abort_not_error() {
        let mut app = app_with_conflict(vec!["a.rs".into()], vec![], 1);
        let cp = test_claude();
        handle_conflict_overlay(key(KeyCode::Enter, KeyModifiers::NONE), &mut app, &cp).unwrap();
        let (_, is_err) = app
            .git_actions_panel
            .as_ref()
            .unwrap()
            .result_message
            .as_ref()
            .unwrap();
        assert!(!is_err);
    }

    // ══════════════════════════════════════════════════════════════════
    // 8. Spawn path — Enter with sel=0 (fails in test env, hits Err branch)
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn test_enter_sel0_spawns_claude_fails_clears_overlay() {
        let mut app = app_with_conflict(vec!["a.rs".into()], vec![], 0);
        let cp = test_claude();
        handle_conflict_overlay(key(KeyCode::Enter, KeyModifiers::NONE), &mut app, &cp).unwrap();
        // Spawn fails in test env -> Err branch clears overlay
        let panel = app.git_actions_panel.as_ref().unwrap();
        assert!(panel.conflict_overlay.is_none());
    }

    #[test]
    fn test_enter_sel0_spawn_fail_sets_error_message() {
        let mut app = app_with_conflict(vec!["a.rs".into()], vec![], 0);
        let cp = test_claude();
        handle_conflict_overlay(key(KeyCode::Enter, KeyModifiers::NONE), &mut app, &cp).unwrap();
        let panel = app.git_actions_panel.as_ref().unwrap();
        let (msg, is_err) = panel.result_message.as_ref().unwrap();
        assert!(msg.starts_with("Failed to spawn Claude:"));
        assert!(is_err);
    }

    #[test]
    fn test_y_key_spawns_claude_fails_clears_overlay() {
        let mut app = app_with_conflict(vec!["a.rs".into()], vec![], 0);
        let cp = test_claude();
        handle_conflict_overlay(key(KeyCode::Char('y'), KeyModifiers::NONE), &mut app, &cp)
            .unwrap();
        let panel = app.git_actions_panel.as_ref().unwrap();
        assert!(panel.conflict_overlay.is_none());
    }

    #[test]
    fn test_y_key_spawn_fail_is_error() {
        let mut app = app_with_conflict(vec!["a.rs".into()], vec![], 0);
        let cp = test_claude();
        handle_conflict_overlay(key(KeyCode::Char('y'), KeyModifiers::NONE), &mut app, &cp)
            .unwrap();
        let (_, is_err) = app
            .git_actions_panel
            .as_ref()
            .unwrap()
            .result_message
            .as_ref()
            .unwrap();
        assert!(is_err);
    }

    #[test]
    fn test_y_key_from_selection_1_still_resolves() {
        // 'y' always resolves regardless of selection
        let mut app = app_with_conflict(vec!["a.rs".into()], vec![], 1);
        let cp = test_claude();
        handle_conflict_overlay(key(KeyCode::Char('y'), KeyModifiers::NONE), &mut app, &cp)
            .unwrap();
        let panel = app.git_actions_panel.as_ref().unwrap();
        // It called spawn_conflict_claude (which failed), not abort_rebase
        let (msg, is_err) = panel.result_message.as_ref().unwrap();
        assert!(msg.starts_with("Failed to spawn Claude:"));
        assert!(is_err);
    }

    // ══════════════════════════════════════════════════════════════════
    // 9. Unmatched keys — no-op
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn test_unmatched_key_a_is_noop() {
        let mut app = app_with_conflict(vec!["a.rs".into()], vec![], 0);
        let cp = test_claude();
        handle_conflict_overlay(key(KeyCode::Char('a'), KeyModifiers::NONE), &mut app, &cp)
            .unwrap();
        assert_eq!(overlay(&app).selected, 0);
        assert!(app
            .git_actions_panel
            .as_ref()
            .unwrap()
            .result_message
            .is_none());
    }

    #[test]
    fn test_unmatched_key_space_is_noop() {
        let mut app = app_with_conflict(vec!["a.rs".into()], vec![], 0);
        let cp = test_claude();
        handle_conflict_overlay(key(KeyCode::Char(' '), KeyModifiers::NONE), &mut app, &cp)
            .unwrap();
        assert_eq!(overlay(&app).selected, 0);
    }

    #[test]
    fn test_unmatched_key_tab_is_noop() {
        let mut app = app_with_conflict(vec!["a.rs".into()], vec![], 0);
        let cp = test_claude();
        handle_conflict_overlay(key(KeyCode::Tab, KeyModifiers::NONE), &mut app, &cp).unwrap();
        assert_eq!(overlay(&app).selected, 0);
    }

    #[test]
    fn test_unmatched_key_backspace_is_noop() {
        let mut app = app_with_conflict(vec!["a.rs".into()], vec![], 1);
        let cp = test_claude();
        handle_conflict_overlay(key(KeyCode::Backspace, KeyModifiers::NONE), &mut app, &cp)
            .unwrap();
        assert_eq!(overlay(&app).selected, 1);
    }

    #[test]
    fn test_unmatched_key_left_is_noop() {
        let mut app = app_with_conflict(vec!["a.rs".into()], vec![], 0);
        let cp = test_claude();
        handle_conflict_overlay(key(KeyCode::Left, KeyModifiers::NONE), &mut app, &cp).unwrap();
        assert_eq!(overlay(&app).selected, 0);
    }

    #[test]
    fn test_unmatched_key_right_is_noop() {
        let mut app = app_with_conflict(vec!["a.rs".into()], vec![], 1);
        let cp = test_claude();
        handle_conflict_overlay(key(KeyCode::Right, KeyModifiers::NONE), &mut app, &cp).unwrap();
        assert_eq!(overlay(&app).selected, 1);
    }

    // ══════════════════════════════════════════════════════════════════
    // 10. Modifier keys — Shift/Ctrl+j/k should be no-ops
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn test_shift_j_is_noop() {
        let mut app = app_with_conflict(vec!["a.rs".into()], vec![], 0);
        let cp = test_claude();
        handle_conflict_overlay(key(KeyCode::Char('J'), KeyModifiers::SHIFT), &mut app, &cp)
            .unwrap();
        assert_eq!(overlay(&app).selected, 0);
    }

    #[test]
    fn test_ctrl_j_is_noop() {
        let mut app = app_with_conflict(vec!["a.rs".into()], vec![], 0);
        let cp = test_claude();
        handle_conflict_overlay(
            key(KeyCode::Char('j'), KeyModifiers::CONTROL),
            &mut app,
            &cp,
        )
        .unwrap();
        assert_eq!(overlay(&app).selected, 0);
    }

    #[test]
    fn test_alt_k_is_noop() {
        let mut app = app_with_conflict(vec!["a.rs".into()], vec![], 1);
        let cp = test_claude();
        handle_conflict_overlay(key(KeyCode::Char('k'), KeyModifiers::ALT), &mut app, &cp).unwrap();
        assert_eq!(overlay(&app).selected, 1);
    }

    #[test]
    fn test_shift_enter_is_noop() {
        let mut app = app_with_conflict(vec!["a.rs".into()], vec![], 0);
        let cp = test_claude();
        handle_conflict_overlay(key(KeyCode::Enter, KeyModifiers::SHIFT), &mut app, &cp).unwrap();
        // No action taken — overlay still present
        assert!(app
            .git_actions_panel
            .as_ref()
            .unwrap()
            .conflict_overlay
            .is_some());
    }

    #[test]
    fn test_ctrl_n_is_noop() {
        let mut app = app_with_conflict(vec!["a.rs".into()], vec![], 0);
        let cp = test_claude();
        handle_conflict_overlay(
            key(KeyCode::Char('n'), KeyModifiers::CONTROL),
            &mut app,
            &cp,
        )
        .unwrap();
        assert!(app
            .git_actions_panel
            .as_ref()
            .unwrap()
            .conflict_overlay
            .is_some());
    }

    #[test]
    fn test_ctrl_y_is_noop() {
        let mut app = app_with_conflict(vec!["a.rs".into()], vec![], 0);
        let cp = test_claude();
        handle_conflict_overlay(
            key(KeyCode::Char('y'), KeyModifiers::CONTROL),
            &mut app,
            &cp,
        )
        .unwrap();
        assert!(app
            .git_actions_panel
            .as_ref()
            .unwrap()
            .conflict_overlay
            .is_some());
    }

    // ══════════════════════════════════════════════════════════════════
    // 11. build_conflict_prompt — single conflicted file
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn test_prompt_contains_branch_name() {
        let p = build_conflict_prompt("my-feature", &["a.rs".into()], &[]);
        assert!(p.contains("'my-feature'"));
    }

    #[test]
    fn test_prompt_contains_conflicted_count() {
        let p = build_conflict_prompt("feat", &["a.rs".into(), "b.rs".into()], &[]);
        assert!(p.contains("Conflicted files (2):"));
    }

    #[test]
    fn test_prompt_lists_conflicted_files() {
        let p = build_conflict_prompt("feat", &["src/main.rs".into()], &[]);
        assert!(p.contains("  - src/main.rs"));
    }

    #[test]
    fn test_prompt_no_auto_merged_section_when_empty() {
        let p = build_conflict_prompt("feat", &["a.rs".into()], &[]);
        assert!(!p.contains("Auto-merged"));
    }

    #[test]
    fn test_prompt_has_auto_merged_section_when_present() {
        let p = build_conflict_prompt("feat", &["a.rs".into()], &["b.rs".into()]);
        assert!(p.contains("Auto-merged cleanly (1):"));
        assert!(p.contains("  - b.rs"));
    }

    #[test]
    fn test_prompt_lists_multiple_auto_merged() {
        let p = build_conflict_prompt("feat", &["a.rs".into()], &["b.rs".into(), "c.rs".into()]);
        assert!(p.contains("Auto-merged cleanly (2):"));
        assert!(p.contains("  - b.rs"));
        assert!(p.contains("  - c.rs"));
    }

    #[test]
    fn test_prompt_contains_resolve_instructions() {
        let p = build_conflict_prompt("feat", &["a.rs".into()], &[]);
        assert!(p.contains("Resolve all conflicts:"));
        assert!(p.contains("git add <files>"));
        assert!(p.contains("git rebase --continue"));
    }

    #[test]
    fn test_prompt_mentions_markers() {
        let p = build_conflict_prompt("feat", &["a.rs".into()], &[]);
        assert!(p.contains("<<<<<<<"));
        assert!(p.contains("======="));
        assert!(p.contains(">>>>>>>"));
    }

    #[test]
    fn test_prompt_mentions_git_status() {
        let p = build_conflict_prompt("feat", &["a.rs".into()], &[]);
        assert!(p.contains("git status"));
    }

    #[test]
    fn test_prompt_asks_about_ambiguity() {
        let p = build_conflict_prompt("feat", &["a.rs".into()], &[]);
        assert!(p.contains("Ask me if any conflict is ambiguous"));
    }

    #[test]
    fn test_prompt_zero_conflicted_shows_zero() {
        let p = build_conflict_prompt("feat", &[], &[]);
        assert!(p.contains("Conflicted files (0):"));
    }

    #[test]
    fn test_prompt_many_conflicted_files() {
        let files: Vec<String> = (0..10).map(|i| format!("file_{}.rs", i)).collect();
        let p = build_conflict_prompt("feat", &files, &[]);
        assert!(p.contains("Conflicted files (10):"));
        for i in 0..10 {
            assert!(p.contains(&format!("  - file_{}.rs", i)));
        }
    }

    #[test]
    fn test_prompt_special_chars_in_branch() {
        let p = build_conflict_prompt("feat/my-branch_v2.0", &["a.rs".into()], &[]);
        assert!(p.contains("'feat/my-branch_v2.0'"));
    }

    #[test]
    fn test_prompt_special_chars_in_filenames() {
        let p = build_conflict_prompt("feat", &["src/my file.rs".into()], &[]);
        assert!(p.contains("  - src/my file.rs"));
    }

    // ══════════════════════════════════════════════════════════════════
    // 12. Overlay state preserved across navigation
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn test_navigation_preserves_conflicted_files() {
        let mut app = app_with_conflict(vec!["a.rs".into(), "b.rs".into()], vec![], 0);
        let cp = test_claude();
        handle_conflict_overlay(key(KeyCode::Char('j'), KeyModifiers::NONE), &mut app, &cp)
            .unwrap();
        let ov = overlay(&app);
        assert_eq!(ov.conflicted_files.len(), 2);
        assert_eq!(ov.conflicted_files[0], "a.rs");
        assert_eq!(ov.conflicted_files[1], "b.rs");
    }

    #[test]
    fn test_navigation_preserves_auto_merged_files() {
        let mut app = app_with_conflict(vec!["a.rs".into()], vec!["x.rs".into()], 0);
        let cp = test_claude();
        handle_conflict_overlay(key(KeyCode::Char('j'), KeyModifiers::NONE), &mut app, &cp)
            .unwrap();
        let ov = overlay(&app);
        assert_eq!(ov.auto_merged_files.len(), 1);
        assert_eq!(ov.auto_merged_files[0], "x.rs");
    }

    #[test]
    fn test_navigation_preserves_scroll() {
        let mut app = app_with_conflict(vec!["a.rs".into()], vec![], 0);
        // Set scroll to non-zero
        app.git_actions_panel
            .as_mut()
            .unwrap()
            .conflict_overlay
            .as_mut()
            .unwrap()
            .scroll = 5;
        let cp = test_claude();
        handle_conflict_overlay(key(KeyCode::Char('j'), KeyModifiers::NONE), &mut app, &cp)
            .unwrap();
        assert_eq!(overlay(&app).scroll, 5);
    }

    #[test]
    fn test_navigation_preserves_continue_with_merge() {
        let mut app = app_with_conflict(vec!["a.rs".into()], vec![], 0);
        app.git_actions_panel
            .as_mut()
            .unwrap()
            .conflict_overlay
            .as_mut()
            .unwrap()
            .continue_with_merge = true;
        let cp = test_claude();
        handle_conflict_overlay(key(KeyCode::Char('j'), KeyModifiers::NONE), &mut app, &cp)
            .unwrap();
        assert!(overlay(&app).continue_with_merge);
    }

    // ══════════════════════════════════════════════════════════════════
    // 13. Abort preserves panel but clears overlay
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn test_abort_preserves_panel_worktree_name() {
        let mut app = app_with_conflict(vec!["a.rs".into()], vec![], 0);
        let cp = test_claude();
        handle_conflict_overlay(key(KeyCode::Char('n'), KeyModifiers::NONE), &mut app, &cp)
            .unwrap();
        let panel = app.git_actions_panel.as_ref().unwrap();
        assert_eq!(panel.worktree_name, "feature-test");
    }

    #[test]
    fn test_abort_preserves_panel_worktree_path() {
        let mut app = app_with_conflict(vec!["a.rs".into()], vec![], 0);
        let cp = test_claude();
        handle_conflict_overlay(key(KeyCode::Esc, KeyModifiers::NONE), &mut app, &cp).unwrap();
        let panel = app.git_actions_panel.as_ref().unwrap();
        assert_eq!(
            panel.worktree_path,
            std::path::PathBuf::from("/tmp/test-wt")
        );
    }

    #[test]
    fn test_abort_preserves_panel_repo_root() {
        let mut app = app_with_conflict(vec!["a.rs".into()], vec![], 0);
        let cp = test_claude();
        handle_conflict_overlay(key(KeyCode::Char('n'), KeyModifiers::NONE), &mut app, &cp)
            .unwrap();
        let panel = app.git_actions_panel.as_ref().unwrap();
        assert_eq!(panel.repo_root, std::path::PathBuf::from("/tmp/test-repo"));
    }

    // ══════════════════════════════════════════════════════════════════
    // 14. Spawn fail preserves panel but clears overlay
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn test_spawn_fail_preserves_panel_worktree_name() {
        let mut app = app_with_conflict(vec!["a.rs".into()], vec![], 0);
        let cp = test_claude();
        handle_conflict_overlay(key(KeyCode::Char('y'), KeyModifiers::NONE), &mut app, &cp)
            .unwrap();
        let panel = app.git_actions_panel.as_ref().unwrap();
        assert_eq!(panel.worktree_name, "feature-test");
    }

    #[test]
    fn test_spawn_fail_preserves_panel_main_branch() {
        let mut app = app_with_conflict(vec!["a.rs".into()], vec![], 0);
        let cp = test_claude();
        handle_conflict_overlay(key(KeyCode::Enter, KeyModifiers::NONE), &mut app, &cp).unwrap();
        let panel = app.git_actions_panel.as_ref().unwrap();
        assert_eq!(panel.main_branch, "main");
    }

    #[test]
    fn test_spawn_fail_error_message_is_flagged_as_error() {
        let mut app = app_with_conflict(vec!["a.rs".into()], vec![], 0);
        let cp = test_claude();
        handle_conflict_overlay(key(KeyCode::Char('y'), KeyModifiers::NONE), &mut app, &cp)
            .unwrap();
        let (_, is_err) = app
            .git_actions_panel
            .as_ref()
            .unwrap()
            .result_message
            .as_ref()
            .unwrap();
        assert!(is_err);
    }

    // ══════════════════════════════════════════════════════════════════
    // 15. Various overlay configurations
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn test_empty_conflicted_list_enter_sel0_still_spawns() {
        let mut app = app_with_conflict(vec![], vec![], 0);
        let cp = test_claude();
        handle_conflict_overlay(key(KeyCode::Enter, KeyModifiers::NONE), &mut app, &cp).unwrap();
        // Spawn attempted (and failed) — overlay cleared
        assert!(app
            .git_actions_panel
            .as_ref()
            .unwrap()
            .conflict_overlay
            .is_none());
    }

    #[test]
    fn test_empty_conflicted_list_n_still_aborts() {
        let mut app = app_with_conflict(vec![], vec![], 0);
        let cp = test_claude();
        handle_conflict_overlay(key(KeyCode::Char('n'), KeyModifiers::NONE), &mut app, &cp)
            .unwrap();
        let panel = app.git_actions_panel.as_ref().unwrap();
        assert!(panel.conflict_overlay.is_none());
        assert_eq!(panel.result_message.as_ref().unwrap().0, "Aborted");
    }

    #[test]
    fn test_only_auto_merged_no_conflicts() {
        let mut app = app_with_conflict(vec![], vec!["auto.rs".into()], 0);
        let cp = test_claude();
        handle_conflict_overlay(key(KeyCode::Char('j'), KeyModifiers::NONE), &mut app, &cp)
            .unwrap();
        assert_eq!(overlay(&app).selected, 1);
    }

    #[test]
    fn test_continue_with_merge_flag_preserved_through_nav() {
        let mut app = app_with_conflict(vec!["a.rs".into()], vec![], 0);
        app.git_actions_panel
            .as_mut()
            .unwrap()
            .conflict_overlay
            .as_mut()
            .unwrap()
            .continue_with_merge = true;
        let cp = test_claude();
        handle_conflict_overlay(key(KeyCode::Down, KeyModifiers::NONE), &mut app, &cp).unwrap();
        assert!(overlay(&app).continue_with_merge);
    }

    // ══════════════════════════════════════════════════════════════════
    // 16. Return value is always Ok
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn test_all_nav_keys_return_ok() {
        let cp = test_claude();
        for code in [
            KeyCode::Char('j'),
            KeyCode::Char('k'),
            KeyCode::Down,
            KeyCode::Up,
        ] {
            let mut app = app_with_conflict(vec!["a.rs".into()], vec![], 0);
            assert!(handle_conflict_overlay(key(code, KeyModifiers::NONE), &mut app, &cp).is_ok());
        }
    }

    #[test]
    fn test_all_abort_keys_return_ok() {
        let cp = test_claude();
        for code in [KeyCode::Char('n'), KeyCode::Esc] {
            let mut app = app_with_conflict(vec!["a.rs".into()], vec![], 0);
            assert!(handle_conflict_overlay(key(code, KeyModifiers::NONE), &mut app, &cp).is_ok());
        }
    }

    #[test]
    fn test_enter_sel0_returns_ok() {
        let mut app = app_with_conflict(vec!["a.rs".into()], vec![], 0);
        let cp = test_claude();
        assert!(
            handle_conflict_overlay(key(KeyCode::Enter, KeyModifiers::NONE), &mut app, &cp).is_ok()
        );
    }

    #[test]
    fn test_enter_sel1_returns_ok() {
        let mut app = app_with_conflict(vec!["a.rs".into()], vec![], 1);
        let cp = test_claude();
        assert!(
            handle_conflict_overlay(key(KeyCode::Enter, KeyModifiers::NONE), &mut app, &cp).is_ok()
        );
    }

    #[test]
    fn test_y_returns_ok() {
        let mut app = app_with_conflict(vec!["a.rs".into()], vec![], 0);
        let cp = test_claude();
        assert!(handle_conflict_overlay(
            key(KeyCode::Char('y'), KeyModifiers::NONE),
            &mut app,
            &cp
        )
        .is_ok());
    }

    #[test]
    fn test_unmatched_returns_ok() {
        let mut app = app_with_conflict(vec!["a.rs".into()], vec![], 0);
        let cp = test_claude();
        assert!(handle_conflict_overlay(
            key(KeyCode::Char('z'), KeyModifiers::NONE),
            &mut app,
            &cp
        )
        .is_ok());
    }

    // ══════════════════════════════════════════════════════════════════
    // 17. Prompt edge cases
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn test_prompt_empty_branch_name() {
        let p = build_conflict_prompt("", &["a.rs".into()], &[]);
        assert!(p.contains("Rebasing branch '' onto main"));
    }

    #[test]
    fn test_prompt_unicode_branch_name() {
        let p = build_conflict_prompt("feat/unicode-\u{1f600}", &["a.rs".into()], &[]);
        assert!(p.contains("feat/unicode-\u{1f600}"));
    }

    #[test]
    fn test_prompt_unicode_filenames() {
        let p = build_conflict_prompt("feat", &["\u{00e9}tude.rs".into()], &[]);
        assert!(p.contains("  - \u{00e9}tude.rs"));
    }

    #[test]
    fn test_prompt_both_sections_have_correct_counts() {
        let conflicted: Vec<String> = (0..3).map(|i| format!("c{}.rs", i)).collect();
        let auto_merged: Vec<String> = (0..5).map(|i| format!("a{}.rs", i)).collect();
        let p = build_conflict_prompt("feat", &conflicted, &auto_merged);
        assert!(p.contains("Conflicted files (3):"));
        assert!(p.contains("Auto-merged cleanly (5):"));
    }

    #[test]
    fn test_prompt_numbered_steps_present() {
        let p = build_conflict_prompt("feat", &["a.rs".into()], &[]);
        for i in 1..=6 {
            assert!(p.contains(&format!("{}.", i)));
        }
    }
}
