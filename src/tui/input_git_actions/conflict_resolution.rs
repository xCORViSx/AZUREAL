//! Conflict resolution overlay and RCR (Resolve Conflicts with Claude) spawning.
//!
//! Handles the conflict overlay UI (navigate, resolve with Claude, abort rebase)
//! and spawns a streaming Claude session to resolve rebase conflicts.

use anyhow::Result;
use crossterm::event;

use crate::app::{App, Focus};
use crate::app::types::RcrSession;
use crate::claude::ClaudeProcess;
use crate::git::Git;

/// Handle input while the conflict resolution overlay is open.
/// j/k or Up/Down navigate between "Resolve with Claude" and "Abort rebase".
/// Enter/y resolves, n/Esc aborts the rebase and closes the overlay.
pub(super) fn handle_conflict_overlay(key: event::KeyEvent, app: &mut App, claude_process: &ClaudeProcess) -> Result<()> {
    use crossterm::event::{KeyCode, KeyModifiers};

    let (sel, wt_path, repo_root, branch, conflicted, auto_merged, continue_merge) = match app.git_actions_panel.as_ref() {
        Some(p) => match p.conflict_overlay.as_ref() {
            Some(ov) => (
                ov.selected, p.worktree_path.clone(), p.repo_root.clone(),
                p.worktree_name.clone(), ov.conflicted_files.clone(), ov.auto_merged_files.clone(),
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
                    if ov.selected < 1 { ov.selected = 1; }
                }
            }
        }
        (KeyModifiers::NONE, KeyCode::Char('k')) | (KeyModifiers::NONE, KeyCode::Up) => {
            if let Some(ref mut p) = app.git_actions_panel {
                if let Some(ref mut ov) = p.conflict_overlay {
                    if ov.selected > 0 { ov.selected = 0; }
                }
            }
        }

        (KeyModifiers::NONE, KeyCode::Enter) => {
            if sel == 0 {
                spawn_conflict_claude(app, claude_process, &wt_path, &repo_root, &branch, &conflicted, &auto_merged, continue_merge);
            } else {
                abort_rebase(app, &wt_path);
            }
        }

        (KeyModifiers::NONE, KeyCode::Char('y')) => {
            spawn_conflict_claude(app, claude_process, &wt_path, &repo_root, &branch, &conflicted, &auto_merged, continue_merge);
        }

        (KeyModifiers::NONE, KeyCode::Char('n')) | (KeyModifiers::NONE, KeyCode::Esc) => {
            abort_rebase(app, &wt_path);
        }

        _ => {}
    }
    Ok(())
}

/// Abort an in-progress rebase on the feature branch, close the overlay
fn abort_rebase(app: &mut App, wt_path: &std::path::Path) {
    let _ = Git::rebase_abort(wt_path);
    if let Some(ref mut p) = app.git_actions_panel {
        p.conflict_overlay = None;
        p.result_message = Some(("Rebase aborted".into(), false));
        super::refresh_changed_files(p);
    }
}

/// Spawn a streaming Claude session to resolve rebase conflicts on the
/// feature branch worktree. Claude runs in the worktree directory and uses
/// `git add` + `git rebase --continue` to complete the rebase.
fn spawn_conflict_claude(
    app: &mut App,
    claude_process: &ClaudeProcess,
    wt_path: &std::path::Path,
    repo_root: &std::path::Path,
    branch: &str,
    conflicted: &[String],
    auto_merged: &[String],
    continue_with_merge: bool,
) {
    let display = crate::models::strip_branch_prefix(branch);
    let mut prompt = format!(
        "Rebasing branch '{}' onto main produced merge conflicts.\n\
         Git left the working directory in a partially-rebased state.\n\n",
        display
    );
    prompt.push_str(&format!("Conflicted files ({}):\n", conflicted.len()));
    for f in conflicted { prompt.push_str(&format!("  - {}\n", f)); }
    if !auto_merged.is_empty() {
        prompt.push_str(&format!("\nAuto-merged cleanly ({}):\n", auto_merged.len()));
        for f in auto_merged { prompt.push_str(&format!("  - {}\n", f)); }
    }
    prompt.push_str(
        "\nResolve all conflicts:\n\
         1. Read each conflicted file — look for <<<<<<< / ======= / >>>>>>> markers\n\
         2. Edit each file to keep the correct combined content, removing all markers\n\
         3. Stage resolved files: git add <files>\n\
         4. Continue the rebase: git rebase --continue\n\
         5. If more conflicts appear, repeat steps 1-4 until the rebase completes\n\
         6. Verify with: git status\n\n\
         Ask me if any conflict is ambiguous."
    );

    match claude_process.spawn(wt_path, &prompt, None) {
        Ok((rx, pid)) => {
            let slot = pid.to_string();
            app.pending_session_names.push((slot.clone(), format!("[RCR] {}", display)));
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
            app.output_lines.clear();
            app.output_buffer.clear();
            app.output_scroll = usize::MAX;
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
            app.focus = Focus::Output;
        }
        Err(e) => {
            if let Some(ref mut p) = app.git_actions_panel {
                p.conflict_overlay = None;
                p.result_message = Some((format!("Failed to spawn Claude: {}", e), true));
            }
        }
    }
}
