//! Output panel input handling

use anyhow::Result;
use crossterm::event;

use crate::app::{App, ViewMode};
use super::input_rebase::handle_rebase_input;

/// Handle keyboard input when Output pane is focused.
/// ALL keybindings are resolved by lookup_action() in event_loop.rs BEFORE this
/// is called. This handler only receives keys that weren't mapped — meaning only
/// session list overlay, convo search, and rebase mode input reach here.
pub fn handle_output_input(key: event::KeyEvent, app: &mut App) -> Result<()> {
    // Convo search bar: typing search text bypasses keybinding system
    if app.convo_search_active {
        return handle_convo_search_input(key, app);
    }

    // Session list overlay: j/k navigate, Enter selects, s/Esc closes, / filters
    if app.show_session_list {
        return handle_session_list_input(key, app);
    }

    // n/N: cycle through convo search matches (after Enter confirmed search)
    if !app.convo_search_matches.is_empty() && !app.convo_search_active {
        use event::KeyCode;
        match key.code {
            KeyCode::Char('n') => { jump_next_match(app); return Ok(()); }
            KeyCode::Char('N') => { jump_prev_match(app); return Ok(()); }
            // Esc clears residual search matches
            KeyCode::Esc => {
                app.convo_search.clear();
                app.convo_search_matches.clear();
                app.convo_search_current = 0;
                app.output_viewport_scroll = usize::MAX;
                return Ok(());
            }
            _ => {}
        }
    }

    // Rebase mode has its own handler
    if app.view_mode == ViewMode::Rebase {
        return handle_rebase_input(key, app);
    }

    // All output keybindings resolved upstream — nothing to handle here
    Ok(())
}

/// Handle keyboard input for the convo search bar (/ search within current session).
/// Typing updates the query and recomputes matches against rendered_lines_cache.
/// Enter confirms (keeps matches highlighted for n/N navigation), Esc clears.
fn handle_convo_search_input(key: event::KeyEvent, app: &mut App) -> Result<()> {
    use event::KeyCode;

    match key.code {
        KeyCode::Char(c) => {
            app.convo_search.push(c);
            recompute_convo_matches(app);
            // Jump to nearest match after current scroll position
            jump_to_nearest_match(app);
            // Invalidate viewport cache so highlighting redraws
            app.output_viewport_scroll = usize::MAX;
        }
        KeyCode::Backspace => {
            app.convo_search.pop();
            if app.convo_search.is_empty() {
                // Empty search: deactivate and clear
                app.convo_search_active = false;
                app.convo_search_matches.clear();
                app.convo_search_current = 0;
            } else {
                recompute_convo_matches(app);
                jump_to_nearest_match(app);
            }
            app.output_viewport_scroll = usize::MAX;
        }
        KeyCode::Enter => {
            // Confirm search: deactivate input but keep matches + highlighting active
            // n/N will cycle through matches (handled in keybindings/actions)
            app.convo_search_active = false;
        }
        KeyCode::Esc => {
            // Cancel search: clear everything
            app.convo_search_active = false;
            app.convo_search.clear();
            app.convo_search_matches.clear();
            app.convo_search_current = 0;
            app.output_viewport_scroll = usize::MAX;
        }
        _ => {}
    }
    Ok(())
}

/// Scan the rendered_lines_cache for all case-insensitive occurrences of convo_search.
/// Stores (line_idx, start_col, end_col) for every match found.
fn recompute_convo_matches(app: &mut App) {
    app.convo_search_matches.clear();
    app.convo_search_current = 0;
    if app.convo_search.is_empty() { return; }

    let query = app.convo_search.to_lowercase();
    for (line_idx, line) in app.rendered_lines_cache.iter().enumerate() {
        // Extract plain text from spans (same technique as extract_text_from_cache)
        let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        let lower = text.to_lowercase();
        // Find all occurrences in this line
        let mut start = 0;
        while let Some(pos) = lower[start..].find(&query) {
            let abs_pos = start + pos;
            app.convo_search_matches.push((line_idx, abs_pos, abs_pos + query.len()));
            start = abs_pos + 1;
        }
    }
}

/// Jump to the nearest match at or after the current scroll position.
/// Sets output_scroll so the matched line sits ~3 lines from the top for context.
fn jump_to_nearest_match(app: &mut App) {
    if app.convo_search_matches.is_empty() { return; }

    // Find the match nearest to current viewport position
    let current_scroll = if app.output_scroll == usize::MAX {
        app.output_natural_bottom()
    } else {
        app.output_scroll
    };

    // Prefer match at/after current scroll, otherwise wrap to first
    let idx = app.convo_search_matches.iter()
        .position(|(line, _, _)| *line >= current_scroll)
        .unwrap_or(0);

    app.convo_search_current = idx;
    let (match_line, _, _) = app.convo_search_matches[idx];
    // Position match ~3 lines from top for context
    app.output_scroll = match_line.saturating_sub(3);
}

/// Jump to the next convo search match (n key after Enter)
pub fn jump_next_match(app: &mut App) {
    if app.convo_search_matches.is_empty() { return; }
    app.convo_search_current = (app.convo_search_current + 1) % app.convo_search_matches.len();
    let (match_line, _, _) = app.convo_search_matches[app.convo_search_current];
    app.output_scroll = match_line.saturating_sub(3);
    app.output_viewport_scroll = usize::MAX;
}

/// Jump to the previous convo search match (N key after Enter)
pub fn jump_prev_match(app: &mut App) {
    if app.convo_search_matches.is_empty() { return; }
    if app.convo_search_current == 0 {
        app.convo_search_current = app.convo_search_matches.len() - 1;
    } else {
        app.convo_search_current -= 1;
    }
    let (match_line, _, _) = app.convo_search_matches[app.convo_search_current];
    app.output_scroll = match_line.saturating_sub(3);
    app.output_viewport_scroll = usize::MAX;
}

/// Handle keyboard input for the session list overlay
fn handle_session_list_input(key: event::KeyEvent, app: &mut App) -> Result<()> {
    use event::{KeyCode, KeyModifiers};

    // Session filter bar is active: route text input to the filter
    if app.session_filter_active {
        return handle_session_filter_input(key, app);
    }

    // Build flat count of total rows (same structure as draw_session_list)
    let total_rows: usize = app.sessions.iter().map(|s| {
        app.session_files.get(&s.branch_name).map(|f| f.len().max(1)).unwrap_or(1)
    }).sum();

    match (key.modifiers, key.code) {
        // /: activate session filter (name search); // activates content search
        (KeyModifiers::NONE, KeyCode::Char('/')) => {
            app.session_filter_active = true;
            app.session_filter.clear();
            app.session_content_search = false;
            app.session_search_results.clear();
        }
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
            select_session_at_row(app);
        }
        // s or Esc: close overlay
        (KeyModifiers::NONE, KeyCode::Char('s')) | (_, KeyCode::Esc) => {
            app.show_session_list = false;
            app.session_filter.clear();
            app.session_filter_active = false;
            app.session_content_search = false;
            app.session_search_results.clear();
        }
        _ => {}
    }
    Ok(())
}

/// Handle text input for the session filter bar (both / name filter and // content search)
fn handle_session_filter_input(key: event::KeyEvent, app: &mut App) -> Result<()> {
    use event::KeyCode;

    match key.code {
        KeyCode::Char('/') if app.session_filter.is_empty() && !app.session_content_search => {
            // Second '/' while filter is empty → switch to content search mode
            app.session_content_search = true;
        }
        KeyCode::Char(c) => {
            app.session_filter.push(c);
            if app.session_content_search {
                // Re-run cross-session content search (only when ≥3 chars for performance)
                if app.session_filter.len() >= 3 {
                    run_cross_session_search(app);
                }
            }
            app.session_list_selected = 0;
        }
        KeyCode::Backspace => {
            app.session_filter.pop();
            if app.session_filter.is_empty() {
                // If in content search mode and filter emptied, drop back to name filter
                if app.session_content_search {
                    app.session_content_search = false;
                    app.session_search_results.clear();
                } else {
                    app.session_filter_active = false;
                }
            } else if app.session_content_search && app.session_filter.len() >= 3 {
                run_cross_session_search(app);
            }
            app.session_list_selected = 0;
        }
        KeyCode::Enter => {
            if app.session_content_search && !app.session_search_results.is_empty() {
                // In content search mode: Enter on a result loads that session
                select_content_search_result(app);
            } else {
                // Name filter mode: confirm filter, keep filtered list shown
                app.session_filter_active = false;
            }
        }
        KeyCode::Esc => {
            app.session_filter.clear();
            app.session_filter_active = false;
            app.session_content_search = false;
            app.session_search_results.clear();
        }
        // ↓: navigate results even while filter is active (j goes to Char(c) above)
        KeyCode::Down => {
            let max = if app.session_content_search {
                app.session_search_results.len()
            } else {
                usize::MAX
            };
            if app.session_list_selected + 1 < max {
                app.session_list_selected += 1;
            }
        }
        KeyCode::Up => {
            app.session_list_selected = app.session_list_selected.saturating_sub(1);
        }
        _ => {}
    }
    Ok(())
}

/// Walk the flat session list to find and load the session at session_list_selected
fn select_session_at_row(app: &mut App) {
    let mut row = 0;
    for (sess_idx, session) in app.sessions.iter().enumerate() {
        let files = app.session_files.get(&session.branch_name);
        let file_count = files.map(|f| f.len()).unwrap_or(0).max(1);
        if app.session_list_selected < row + file_count {
            let file_idx = app.session_list_selected - row;
            if files.map(|f| f.len()).unwrap_or(0) > 0 {
                let branch = session.branch_name.clone();
                app.save_current_terminal();
                app.selected_worktree = Some(sess_idx);
                app.select_session_file(&branch, file_idx);
                app.show_session_list = false;
                app.session_filter.clear();
                app.session_filter_active = false;
                app.session_content_search = false;
                app.session_search_results.clear();
                app.invalidate_sidebar();
            }
            break;
        }
        row += file_count;
    }
}

/// Load the session from the selected content search result
fn select_content_search_result(app: &mut App) {
    let sel = app.session_list_selected;
    if sel >= app.session_search_results.len() { return; }
    let (_row_idx, ref session_id, _) = app.session_search_results[sel];

    // Find which worktree + file index matches this session_id
    for (sess_idx, session) in app.sessions.iter().enumerate() {
        if let Some(files) = app.session_files.get(&session.branch_name) {
            for (file_idx, (sid, _, _)) in files.iter().enumerate() {
                if sid == session_id {
                    let branch = session.branch_name.clone();
                    app.save_current_terminal();
                    app.selected_worktree = Some(sess_idx);
                    app.select_session_file(&branch, file_idx);
                    app.show_session_list = false;
                    app.session_filter.clear();
                    app.session_filter_active = false;
                    app.session_content_search = false;
                    app.session_search_results.clear();
                    app.invalidate_sidebar();
                    return;
                }
            }
        }
    }
}

/// Search across all session JSONL files for the query text.
/// Inline (not background thread) — JSONL files are small. Caps at 100 results.
fn run_cross_session_search(app: &mut App) {
    app.session_search_results.clear();
    let query = app.session_filter.to_lowercase();
    if query.len() < 3 { return; }

    let mut result_idx = 0usize;
    for session in app.sessions.iter() {
        if let Some(files) = app.session_files.get(&session.branch_name) {
            for (session_id, path, _) in files.iter() {
                // Skip files > 5MB for safety
                if let Ok(meta) = std::fs::metadata(path) {
                    if meta.len() > 5_000_000 { continue; }
                }
                // Read file and search line-by-line
                if let Ok(contents) = std::fs::read_to_string(path) {
                    for line in contents.lines() {
                        let lower = line.to_lowercase();
                        if lower.contains(&query) {
                            // Extract a short preview from the matching line (strip JSON wrapper)
                            let preview = extract_search_preview(line, &query);
                            app.session_search_results.push((result_idx, session_id.clone(), preview));
                            result_idx += 1;
                            if app.session_search_results.len() >= 100 { return; }
                            // One match per session file is enough for listing
                            break;
                        }
                    }
                }
            }
        }
    }
}

/// Extract a short preview snippet from a JSONL line around the matched query.
/// Strips JSON structure to show just the text content around the match.
fn extract_search_preview(line: &str, query: &str) -> String {
    // Find the match position in the raw line (case-insensitive)
    let lower = line.to_lowercase();
    let pos = lower.find(query).unwrap_or(0);
    // Show ~40 chars before and after the match
    let start = pos.saturating_sub(40);
    let end = (pos + query.len() + 40).min(line.len());
    // Clamp to char boundaries
    let s = &line[line.ceil_char_boundary(start)..line.floor_char_boundary(end)];
    let trimmed = s.trim();
    if trimmed.len() < line.len() {
        format!("…{}…", trimmed)
    } else {
        trimmed.to_string()
    }
}
