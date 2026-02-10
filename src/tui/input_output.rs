//! Output panel input handling

use anyhow::Result;
use crossterm::event;

use crate::app::{App, Focus, ViewMode};
use crate::git::Git;
use super::input_rebase::handle_rebase_input;
use super::keybindings::{Action, lookup_action};

/// Handle keyboard input when Output pane is focused
pub fn handle_output_input(key: event::KeyEvent, app: &mut App) -> Result<()> {
    // Session list overlay: j/k navigate, Enter selects, s/Esc closes
    if app.show_session_list {
        return handle_session_list_input(key, app);
    }

    // Rebase mode has its own handler
    if app.view_mode == ViewMode::Rebase {
        return handle_rebase_input(key, app);
    }

    // Route through centralized keybinding lookup
    let action = lookup_action(Focus::Output, key.modifiers, key.code, false, false, false);

    match action {
        // j/k/↓/↑ line-by-line scroll (NavDown/NavUp only fires for j/k since ↓/↑ are mapped to JumpNext/PrevBubble)
        Some(Action::NavDown) => {
            match app.view_mode {
                ViewMode::Output => { app.scroll_output_down(1); }
                ViewMode::Diff => { app.scroll_diff_down(1); }
                _ => {}
            }
        }
        Some(Action::NavUp) => {
            match app.view_mode {
                ViewMode::Output => { app.scroll_output_up(1); }
                ViewMode::Diff => { app.scroll_diff_up(1); }
                _ => {}
            }
        }
        // ↓/↑ jump between user prompts (convo mode only)
        Some(Action::JumpNextBubble) => {
            if app.view_mode == ViewMode::Output { app.jump_to_next_bubble(false); }
        }
        Some(Action::JumpPrevBubble) => {
            if app.view_mode == ViewMode::Output { app.jump_to_prev_bubble(false); }
        }
        // ⇧↓/⇧↑ jump between all messages including assistant
        Some(Action::JumpNextMessage) => {
            if app.view_mode == ViewMode::Output { app.jump_to_next_bubble(true); }
        }
        Some(Action::JumpPrevMessage) => {
            if app.view_mode == ViewMode::Output { app.jump_to_prev_bubble(true); }
        }
        // J/K page scroll (full viewport minus 2 for overlap)
        Some(Action::PageDown) => {
            let page = app.output_viewport_height.saturating_sub(2);
            match app.view_mode {
                ViewMode::Output => { app.scroll_output_down(page); }
                ViewMode::Diff => { app.scroll_diff_down(page); }
                _ => {}
            }
        }
        Some(Action::PageUp) => {
            let page = app.output_viewport_height.saturating_sub(2);
            match app.view_mode {
                ViewMode::Output => { app.scroll_output_up(page); }
                ViewMode::Diff => { app.scroll_diff_up(page); }
                _ => {}
            }
        }
        // g/⌥↑ scroll to top
        Some(Action::GoToTop) => {
            match app.view_mode {
                ViewMode::Output => app.output_scroll = 0,
                ViewMode::Diff => app.diff_scroll = 0,
                _ => {}
            }
        }
        // G/⌥↓ scroll to bottom
        Some(Action::GoToBottom) => {
            match app.view_mode {
                ViewMode::Output => app.scroll_output_to_bottom(),
                ViewMode::Diff => app.scroll_diff_to_bottom(),
                _ => {}
            }
        }
        // o: switch to output view
        Some(Action::SwitchToOutput) => {
            app.view_mode = ViewMode::Output;
            app.output_scroll = usize::MAX; // follow bottom
        }
        // d: view diff
        Some(Action::ViewDiff) => {
            if let Err(e) = app.load_diff() {
                app.set_status(format!("Failed to get diff: {}", e));
            } else {
                app.diff_scroll = 0;
            }
        }
        // R: show rebase status
        Some(Action::RebaseStatus) => {
            if let Some(session) = app.current_session() {
                if let Some(ref wt_path) = session.worktree_path {
                    if Git::is_rebase_in_progress(wt_path) {
                        if let Ok(status) = Git::get_rebase_status(wt_path) {
                            app.set_rebase_status(status);
                        }
                    }
                }
            }
        }
        // Tab: switch focus to input (handled by global, but catch if it leaks through)
        Some(Action::CycleFocusForward) => app.focus = Focus::Input,
        // Esc: back to worktrees sidebar
        Some(Action::Escape) => app.focus = Focus::Worktrees,
        // 's': toggle session list overlay (only in Output view, not Diff/Rebase)
        _ if app.view_mode == ViewMode::Output
            && key.modifiers == event::KeyModifiers::NONE
            && key.code == event::KeyCode::Char('s') => {
            open_session_list(app);
        }
        _ => {}
    }
    Ok(())
}

/// Open the session list overlay, computing message counts for all session files
fn open_session_list(app: &mut App) {
    app.show_session_list = true;
    app.session_list_selected = 0;
    app.session_list_scroll = 0;

    // Ensure session files are loaded for all worktrees
    for session in &app.sessions {
        if !app.session_files.contains_key(&session.branch_name) {
            if let Some(ref wt_path) = session.worktree_path {
                let files = crate::config::list_claude_sessions(wt_path);
                app.session_files.insert(session.branch_name.clone(), files);
            }
        }
    }

    // Compute message counts (lightweight JSONL line scan) for all session files not yet cached
    for files in app.session_files.values() {
        for (session_id, path, _) in files.iter() {
            if !app.session_msg_counts.contains_key(session_id) {
                let count = count_messages_in_jsonl(path);
                app.session_msg_counts.insert(session_id.clone(), count);
            }
        }
    }
}

/// Count human+assistant messages in a JSONL session file by scanning for "type" fields.
/// Lightweight: reads file as text and counts lines containing message type markers.
fn count_messages_in_jsonl(path: &std::path::Path) -> usize {
    let Ok(content) = std::fs::read_to_string(path) else { return 0; };
    content.lines().filter(|line| {
        line.contains("\"type\":\"human\"") || line.contains("\"type\":\"assistant\"")
            || line.contains("\"type\": \"human\"") || line.contains("\"type\": \"assistant\"")
    }).count()
}

/// Handle keyboard input for the session list overlay
fn handle_session_list_input(key: event::KeyEvent, app: &mut App) -> Result<()> {
    use event::{KeyCode, KeyModifiers};

    // Build flat count of total rows (same structure as draw_session_list)
    let total_rows: usize = app.sessions.iter().map(|s| {
        app.session_files.get(&s.branch_name).map(|f| f.len().max(1)).unwrap_or(1)
    }).sum();

    match (key.modifiers, key.code) {
        // j/↓: next row
        (KeyModifiers::NONE, KeyCode::Char('j')) | (KeyModifiers::NONE, KeyCode::Down) => {
            if app.session_list_selected + 1 < total_rows {
                app.session_list_selected += 1;
            }
        }
        // k/↑: prev row
        (KeyModifiers::NONE, KeyCode::Char('k')) | (KeyModifiers::NONE, KeyCode::Up) => {
            app.session_list_selected = app.session_list_selected.saturating_sub(1);
        }
        // J: page down
        (KeyModifiers::NONE, KeyCode::Char('J')) => {
            let page = app.output_viewport_height.saturating_sub(2);
            app.session_list_selected = (app.session_list_selected + page).min(total_rows.saturating_sub(1));
        }
        // K: page up
        (KeyModifiers::NONE, KeyCode::Char('K')) => {
            let page = app.output_viewport_height.saturating_sub(2);
            app.session_list_selected = app.session_list_selected.saturating_sub(page);
        }
        // Enter: load the selected session file
        (KeyModifiers::NONE, KeyCode::Enter) => {
            // Walk the flat list to find which (session_idx, file_idx) corresponds to selection
            let mut row = 0;
            for (sess_idx, session) in app.sessions.iter().enumerate() {
                let files = app.session_files.get(&session.branch_name);
                let file_count = files.map(|f| f.len()).unwrap_or(0).max(1);
                if app.session_list_selected < row + file_count {
                    let file_idx = app.session_list_selected - row;
                    if files.map(|f| f.len()).unwrap_or(0) > 0 {
                        // Select the session and file
                        let branch = session.branch_name.clone();
                        app.save_current_terminal();
                        app.selected_session = Some(sess_idx);
                        app.select_session_file(&branch, file_idx);
                        app.show_session_list = false;
                        app.invalidate_sidebar();
                    }
                    break;
                }
                row += file_count;
            }
        }
        // s or Esc: close overlay
        (KeyModifiers::NONE, KeyCode::Char('s')) | (_, KeyCode::Esc) => {
            app.show_session_list = false;
        }
        _ => {}
    }
    Ok(())
}
