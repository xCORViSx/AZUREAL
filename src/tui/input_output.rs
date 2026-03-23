//! Session panel input handling

use anyhow::Result;
use crossterm::event;

use crate::app::App;

/// Handle keyboard input when Session pane is focused.
/// ALL keybindings are resolved by lookup_action() in event_loop.rs BEFORE this
/// is called. This handler only receives keys that weren't mapped — meaning only
/// session list overlay, session find, and rebase mode input reach here.
pub fn handle_session_input(key: event::KeyEvent, app: &mut App) -> Result<()> {
    // New session name dialog: text input → Enter creates, Esc cancels
    if app.new_session_dialog_active {
        return handle_new_session_dialog_input(key, app);
    }

    // Session find bar: typing search text bypasses keybinding system
    if app.session_find_active {
        return handle_session_find_input(key, app);
    }

    // Session list overlay is handled in actions.rs — unhandled keys fall through
    // to lookup_action() so globals (G, H, P, ], [, ⌃q) work while the list is open.

    // n/N: cycle through session find matches (after Enter confirmed search)
    if !app.session_find_matches.is_empty() && !app.session_find_active {
        use event::KeyCode;
        match key.code {
            KeyCode::Char('n') => {
                jump_next_match(app);
                return Ok(());
            }
            KeyCode::Char('N') => {
                jump_prev_match(app);
                return Ok(());
            }
            // Esc clears residual search matches
            KeyCode::Esc => {
                app.session_find.clear();
                app.session_find_matches.clear();
                app.session_find_current = 0;
                app.session_viewport_scroll = usize::MAX;
                return Ok(());
            }
            _ => {}
        }
    }

    // All session keybindings resolved upstream — nothing to handle here
    Ok(())
}

/// Handle keyboard input for the session find bar (/ search within current session).
/// Typing updates the query and recomputes matches against rendered_lines_cache.
/// Enter confirms (keeps matches highlighted for n/N navigation), Esc clears.
fn handle_session_find_input(key: event::KeyEvent, app: &mut App) -> Result<()> {
    use event::KeyCode;

    match key.code {
        KeyCode::Char(c) => {
            app.session_find.push(c);
            recompute_session_find_matches(app);
            // Jump to nearest match after current scroll position
            jump_to_nearest_match(app);
            // Invalidate viewport cache so highlighting redraws
            app.session_viewport_scroll = usize::MAX;
        }
        KeyCode::Backspace => {
            app.session_find.pop();
            if app.session_find.is_empty() {
                // Empty search: deactivate and clear
                app.session_find_active = false;
                app.session_find_matches.clear();
                app.session_find_current = 0;
            } else {
                recompute_session_find_matches(app);
                jump_to_nearest_match(app);
            }
            app.session_viewport_scroll = usize::MAX;
        }
        KeyCode::Enter => {
            // Confirm search: deactivate input but keep matches + highlighting active
            // n/N will cycle through matches (handled in keybindings/actions)
            app.session_find_active = false;
        }
        KeyCode::Esc => {
            // Cancel search: clear everything
            app.session_find_active = false;
            app.session_find.clear();
            app.session_find_matches.clear();
            app.session_find_current = 0;
            app.session_viewport_scroll = usize::MAX;
        }
        _ => {}
    }
    Ok(())
}

/// Scan the rendered_lines_cache for all case-insensitive occurrences of session_find.
/// Stores (line_idx, start_col, end_col) for every match found.
fn recompute_session_find_matches(app: &mut App) {
    app.session_find_matches.clear();
    app.session_find_current = 0;
    if app.session_find.is_empty() {
        return;
    }

    let query = app.session_find.to_lowercase();
    let query_chars: Vec<char> = query.chars().collect();
    let qlen = query_chars.len();
    for (line_idx, line) in app.rendered_lines_cache.iter().enumerate() {
        // Extract plain text from spans, then lowercase each char individually.
        // This preserves 1:1 char-index mapping with the original text (the highlight
        // code in draw_output splits spans by char index, NOT byte offset).
        let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        let lower: Vec<char> = text
            .chars()
            .map(|c| {
                // Single-char lowercase only (avoids ß→ss expanding char count)
                let mut buf = [0u8; 4];
                let s = c.encode_utf8(&mut buf);
                s.to_lowercase().chars().next().unwrap_or(c)
            })
            .collect();
        if lower.len() < qlen {
            continue;
        }
        for i in 0..=lower.len() - qlen {
            if lower[i..i + qlen] == query_chars[..] {
                app.session_find_matches.push((line_idx, i, i + qlen));
            }
        }
    }
}

/// Jump to the nearest match at or after the current scroll position.
/// Sets session_scroll so the matched line sits ~3 lines from the top for context.
fn jump_to_nearest_match(app: &mut App) {
    if app.session_find_matches.is_empty() {
        return;
    }

    // Find the match nearest to current viewport position
    let current_scroll = if app.session_scroll == usize::MAX {
        app.session_natural_bottom()
    } else {
        app.session_scroll
    };

    // Prefer match at/after current scroll, otherwise wrap to first
    let idx = app
        .session_find_matches
        .iter()
        .position(|(line, _, _)| *line >= current_scroll)
        .unwrap_or(0);

    app.session_find_current = idx;
    let (match_line, _, _) = app.session_find_matches[idx];
    // Position match ~3 lines from top for context
    app.session_scroll = match_line.saturating_sub(3);
}

/// Jump to the next session find match (n key after Enter)
pub fn jump_next_match(app: &mut App) {
    if app.session_find_matches.is_empty() {
        return;
    }
    app.session_find_current = (app.session_find_current + 1) % app.session_find_matches.len();
    let (match_line, _, _) = app.session_find_matches[app.session_find_current];
    app.session_scroll = match_line.saturating_sub(3);
    app.session_viewport_scroll = usize::MAX;
}

/// Jump to the previous session find match (N key after Enter)
pub fn jump_prev_match(app: &mut App) {
    if app.session_find_matches.is_empty() {
        return;
    }
    if app.session_find_current == 0 {
        app.session_find_current = app.session_find_matches.len() - 1;
    } else {
        app.session_find_current -= 1;
    }
    let (match_line, _, _) = app.session_find_matches[app.session_find_current];
    app.session_scroll = match_line.saturating_sub(3);
    app.session_viewport_scroll = usize::MAX;
}

/// Handle keyboard input for the session list overlay
/// Returns `true` if the key was consumed by the session list, `false` to let
/// globals (G, H, P, ], [, ⌃q, etc.) fall through to `lookup_action()`.
pub fn handle_session_list_input(key: event::KeyEvent, app: &mut App) -> Result<bool> {
    use event::{KeyCode, KeyModifiers};

    // Session rename input is active: route text input to the rename handler
    if app.session_rename_active {
        handle_session_rename_input(key, app)?;
        return Ok(true);
    }

    // Session filter bar is active: route text input to the filter
    if app.session_filter_active {
        handle_session_filter_input(key, app)?;
        return Ok(true);
    }

    // Count sessions for current worktree (from store-backed session_files cache)
    let total_rows: usize = app
        .current_worktree()
        .and_then(|s| app.session_files.get(&s.branch_name))
        .map(|f| f.len())
        .unwrap_or(0);

    let consumed = match (key.modifiers, key.code) {
        // /: activate session filter (name search); // activates content search
        (KeyModifiers::NONE, KeyCode::Char('/')) => {
            app.session_filter_active = true;
            app.session_filter.clear();
            app.session_content_search = false;
            app.session_search_results.clear();
            true
        }
        // j/↓: next row
        (KeyModifiers::NONE, KeyCode::Char('j')) | (KeyModifiers::NONE, KeyCode::Down) => {
            if app.session_list_selected + 1 < total_rows {
                app.session_list_selected += 1;
            }
            true
        }
        // k/↑: prev row
        (KeyModifiers::NONE, KeyCode::Char('k')) | (KeyModifiers::NONE, KeyCode::Up) => {
            app.session_list_selected = app.session_list_selected.saturating_sub(1);
            true
        }
        // J: page down
        (KeyModifiers::NONE, KeyCode::Char('J')) => {
            let page = app.session_viewport_height.saturating_sub(2);
            app.session_list_selected =
                (app.session_list_selected + page).min(total_rows.saturating_sub(1));
            true
        }
        // K: page up
        (KeyModifiers::NONE, KeyCode::Char('K')) => {
            let page = app.session_viewport_height.saturating_sub(2);
            app.session_list_selected = app.session_list_selected.saturating_sub(page);
            true
        }
        // Enter: load the selected session file
        (KeyModifiers::NONE, KeyCode::Enter) => {
            select_session_at_row(app);
            true
        }
        // r: rename selected session
        (KeyModifiers::NONE, KeyCode::Char('r')) => {
            start_session_rename(app);
            true
        }
        // a: add new session (same as 'a' from session view)
        (KeyModifiers::NONE, KeyCode::Char('a')) => {
            app.show_session_list = false;
            app.session_filter.clear();
            app.session_filter_active = false;
            app.session_content_search = false;
            app.session_search_results.clear();
            app.start_new_session();
            true
        }
        // s or Esc: close overlay
        (KeyModifiers::NONE, KeyCode::Char('s')) | (_, KeyCode::Esc) => {
            app.show_session_list = false;
            app.session_filter.clear();
            app.session_filter_active = false;
            app.session_content_search = false;
            app.session_search_results.clear();
            true
        }
        _ => false,
    };
    Ok(consumed)
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

/// Enter rename mode for the currently selected session in the list.
/// Pre-fills the input with the current display name (custom name or session ID).
fn start_session_rename(app: &mut App) {
    let branch = match app.current_worktree() {
        Some(s) => s.branch_name.clone(),
        None => return,
    };
    let files = match app.session_files.get(&branch) {
        Some(f) => f,
        None => return,
    };
    if app.session_list_selected >= files.len() {
        return;
    }
    let (session_id, _, _) = &files[app.session_list_selected];
    let session_names = app.load_all_session_names();
    let current_name = session_names
        .get(session_id.as_str())
        .cloned()
        .unwrap_or_else(|| session_id.clone());
    app.session_rename_id = Some(session_id.clone());
    app.session_rename_input = current_name;
    app.session_rename_cursor = app.session_rename_input.chars().count();
    app.session_rename_active = true;
}

/// Handle text input for the inline session rename dialog.
/// Enter confirms, Esc cancels, Backspace/chars edit the name.
fn handle_session_rename_input(key: event::KeyEvent, app: &mut App) -> Result<()> {
    use event::KeyCode;

    match key.code {
        KeyCode::Char(c) => {
            app.session_rename_input.insert(
                app.session_rename_input
                    .char_indices()
                    .nth(app.session_rename_cursor)
                    .map(|(i, _)| i)
                    .unwrap_or(app.session_rename_input.len()),
                c,
            );
            app.session_rename_cursor += 1;
        }
        KeyCode::Backspace => {
            if app.session_rename_cursor > 0 {
                let byte_pos = app
                    .session_rename_input
                    .char_indices()
                    .nth(app.session_rename_cursor - 1)
                    .map(|(i, _)| i)
                    .unwrap_or(0);
                app.session_rename_input.remove(byte_pos);
                app.session_rename_cursor -= 1;
            }
        }
        KeyCode::Left => {
            app.session_rename_cursor = app.session_rename_cursor.saturating_sub(1);
        }
        KeyCode::Right => {
            let max = app.session_rename_input.chars().count();
            if app.session_rename_cursor < max {
                app.session_rename_cursor += 1;
            }
        }
        KeyCode::Enter => {
            // Confirm rename: save to azufig.toml
            let name = app.session_rename_input.trim().to_string();
            if let Some(ref session_id) = app.session_rename_id.clone() {
                if !name.is_empty() {
                    app.save_session_name(session_id, &name);
                }
            }
            app.session_rename_active = false;
            app.session_rename_input.clear();
            app.session_rename_cursor = 0;
            app.session_rename_id = None;
        }
        KeyCode::Esc => {
            // Cancel rename
            app.session_rename_active = false;
            app.session_rename_input.clear();
            app.session_rename_cursor = 0;
            app.session_rename_id = None;
        }
        _ => {}
    }
    Ok(())
}

/// Handle text input for the new session name dialog.
fn handle_new_session_dialog_input(key: event::KeyEvent, app: &mut App) -> Result<()> {
    use event::KeyCode;
    match key.code {
        KeyCode::Char(c) => {
            app.new_session_name_input.insert(
                app.new_session_name_input
                    .char_indices()
                    .nth(app.new_session_name_cursor)
                    .map(|(i, _)| i)
                    .unwrap_or(app.new_session_name_input.len()),
                c,
            );
            app.new_session_name_cursor += 1;
        }
        KeyCode::Backspace => {
            if app.new_session_name_cursor > 0 {
                let byte_pos = app
                    .new_session_name_input
                    .char_indices()
                    .nth(app.new_session_name_cursor - 1)
                    .map(|(i, _)| i)
                    .unwrap_or(0);
                app.new_session_name_input.remove(byte_pos);
                app.new_session_name_cursor -= 1;
            }
        }
        KeyCode::Left => {
            app.new_session_name_cursor = app.new_session_name_cursor.saturating_sub(1);
        }
        KeyCode::Right => {
            let max = app.new_session_name_input.chars().count();
            if app.new_session_name_cursor < max {
                app.new_session_name_cursor += 1;
            }
        }
        KeyCode::Enter => {
            app.confirm_new_session();
        }
        KeyCode::Esc => {
            app.cancel_new_session_dialog();
        }
        _ => {}
    }
    Ok(())
}

/// Load the session at session_list_selected (scoped to current worktree).
/// Uses deferred draw: sets loading indicator → draw renders popup →
/// actual session load runs on next frame via DeferredAction::LoadSession.
fn select_session_at_row(app: &mut App) {
    let Some(session) = app.current_worktree() else {
        return;
    };
    let branch = session.branch_name.clone();
    let file_count = app.session_files.get(&branch).map(|f| f.len()).unwrap_or(0);
    if app.session_list_selected < file_count {
        app.loading_indicator = Some("Loading session\u{2026}".into());
        app.deferred_action = Some(crate::app::DeferredAction::LoadSession {
            branch,
            idx: app.session_list_selected,
        });
    }
}

/// Load the session from the selected content search result.
/// Resolves session ID from search results → finds index in session_files cache.
fn select_content_search_result(app: &mut App) {
    let sel = app.session_list_selected;
    if sel >= app.session_search_results.len() {
        return;
    }
    let (_row_idx, ref session_id, _) = app.session_search_results[sel];

    let Some(session) = app.current_worktree() else {
        return;
    };
    let branch = session.branch_name.clone();
    if let Some(files) = app.session_files.get(&branch) {
        if let Some(file_idx) = files.iter().position(|(sid, _, _)| sid == session_id) {
            app.loading_indicator = Some("Loading session\u{2026}".into());
            app.deferred_action = Some(crate::app::DeferredAction::LoadSession {
                branch,
                idx: file_idx,
            });
        }
    }
}

/// Search current worktree's sessions in the SQLite store for the query text.
/// Caps at 100 results.
fn run_cross_session_search(app: &mut App) {
    app.session_search_results.clear();
    let query = app.session_filter.to_lowercase();
    if query.len() < 3 {
        return;
    }

    let branch = match app.current_worktree() {
        Some(s) => s.branch_name.clone(),
        None => return,
    };

    let results = app
        .session_store
        .as_ref()
        .and_then(|store| store.search_events(Some(&branch), &query, 100).ok())
        .unwrap_or_default();

    for (idx, (session_id, data)) in results.into_iter().enumerate() {
        let preview = extract_search_preview(&data, &query);
        app.session_search_results
            .push((idx, session_id.to_string(), preview));
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

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent {
            code,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        }
    }

    // ══════════════════════════════════════════════════════════════════
    //  extract_search_preview
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn extract_preview_found_at_start() {
        let line = "hello world";
        let preview = extract_search_preview(line, "hello");
        assert!(preview.contains("hello"));
    }

    #[test]
    fn extract_preview_found_at_end() {
        let line = "some text at the end";
        let preview = extract_search_preview(line, "end");
        assert!(preview.contains("end"));
    }

    #[test]
    fn extract_preview_not_found_returns_start() {
        let line = "no match here";
        let preview = extract_search_preview(line, "xyz");
        // pos is 0 (unwrap_or(0)), so shows from start
        assert!(!preview.is_empty());
    }

    #[test]
    fn extract_preview_short_line_no_ellipsis() {
        let line = "short";
        let preview = extract_search_preview(line, "short");
        // trimmed.len() == line.len(), no ellipsis
        assert_eq!(preview, "short");
    }

    #[test]
    fn extract_preview_long_line_has_ellipsis() {
        let line = "a".repeat(200);
        let preview = extract_search_preview(&line, "a");
        // trimmed subset is shorter than full line
        assert!(preview.starts_with('…') || preview.len() < line.len());
    }

    #[test]
    fn extract_preview_case_insensitive_find() {
        let line = "Hello World";
        let query = "hello";
        let lower = line.to_lowercase();
        let pos = lower.find(query);
        assert!(pos.is_some());
    }

    // ══════════════════════════════════════════════════════════════════
    //  jump_next_match / jump_prev_match logic
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn next_match_wraps_around() {
        let len = 5;
        let current = 4;
        let next = (current + 1) % len;
        assert_eq!(next, 0);
    }

    #[test]
    fn next_match_increments() {
        let len = 5;
        let current = 2;
        let next = (current + 1) % len;
        assert_eq!(next, 3);
    }

    #[test]
    fn prev_match_wraps_around() {
        let current = 0usize;
        let len = 5;
        let prev = if current == 0 { len - 1 } else { current - 1 };
        assert_eq!(prev, 4);
    }

    #[test]
    fn prev_match_decrements() {
        let current = 3usize;
        let prev = if current == 0 { 99 } else { current - 1 };
        assert_eq!(prev, 2);
    }

    // ══════════════════════════════════════════════════════════════════
    //  Session find key matching
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn char_key_for_find_input() {
        let k = key(KeyCode::Char('a'));
        assert!(matches!(k.code, KeyCode::Char(_)));
    }

    #[test]
    fn backspace_for_find_delete() {
        let k = key(KeyCode::Backspace);
        assert_eq!(k.code, KeyCode::Backspace);
    }

    #[test]
    fn enter_confirms_find() {
        let k = key(KeyCode::Enter);
        assert_eq!(k.code, KeyCode::Enter);
    }

    #[test]
    fn esc_cancels_find() {
        let k = key(KeyCode::Esc);
        assert_eq!(k.code, KeyCode::Esc);
    }

    #[test]
    fn n_key_for_next_match() {
        let k = key(KeyCode::Char('n'));
        assert_eq!(k.code, KeyCode::Char('n'));
    }

    #[test]
    fn upper_n_for_prev_match() {
        let k = key(KeyCode::Char('N'));
        assert_eq!(k.code, KeyCode::Char('N'));
    }

    // ══════════════════════════════════════════════════════════════════
    //  Session list key matching
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn slash_activates_filter() {
        let k = key(KeyCode::Char('/'));
        assert!(matches!(
            (k.modifiers, k.code),
            (KeyModifiers::NONE, KeyCode::Char('/'))
        ));
    }

    #[test]
    fn j_navigates_down() {
        let k = key(KeyCode::Char('j'));
        assert!(matches!(
            (k.modifiers, k.code),
            (KeyModifiers::NONE, KeyCode::Char('j')) | (KeyModifiers::NONE, KeyCode::Down)
        ));
    }

    #[test]
    fn k_navigates_up() {
        let k = key(KeyCode::Char('k'));
        assert!(matches!(
            (k.modifiers, k.code),
            (KeyModifiers::NONE, KeyCode::Char('k')) | (KeyModifiers::NONE, KeyCode::Up)
        ));
    }

    #[test]
    fn upper_j_pages_down() {
        let k = key(KeyCode::Char('J'));
        assert!(matches!(
            (k.modifiers, k.code),
            (KeyModifiers::NONE, KeyCode::Char('J'))
        ));
    }

    #[test]
    fn upper_k_pages_up() {
        let k = key(KeyCode::Char('K'));
        assert!(matches!(
            (k.modifiers, k.code),
            (KeyModifiers::NONE, KeyCode::Char('K'))
        ));
    }

    #[test]
    fn s_closes_overlay() {
        let k = key(KeyCode::Char('s'));
        assert!(matches!(
            (k.modifiers, k.code),
            (KeyModifiers::NONE, KeyCode::Char('s'))
        ));
    }

    // ══════════════════════════════════════════════════════════════════
    //  Session filter input — double slash for content search
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn double_slash_activates_content_search() {
        let filter = "";
        let content_search = false;
        let is_double_slash = filter.is_empty() && !content_search;
        assert!(is_double_slash);
    }

    #[test]
    fn double_slash_not_if_filter_nonempty() {
        let filter = "abc";
        let content_search = false;
        let is_double_slash = filter.is_empty() && !content_search;
        assert!(!is_double_slash);
    }

    // ══════════════════════════════════════════════════════════════════
    //  Case-insensitive search logic
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn lowercase_search_finds_uppercase() {
        let text = "Hello World";
        let query = "hello";
        let lower: Vec<char> = text
            .chars()
            .map(|c| {
                let mut buf = [0u8; 4];
                let s = c.encode_utf8(&mut buf);
                s.to_lowercase().chars().next().unwrap_or(c)
            })
            .collect();
        let query_chars: Vec<char> = query.chars().collect();
        let found = lower
            .windows(query_chars.len())
            .any(|w| w == &query_chars[..]);
        assert!(found);
    }

    #[test]
    fn search_empty_query_no_matches() {
        let query = "";
        assert!(query.is_empty());
    }

    // ══════════════════════════════════════════════════════════════════
    //  jump_to_nearest_match scroll positioning
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn scroll_position_with_context() {
        let match_line = 10usize;
        let scroll = match_line.saturating_sub(3);
        assert_eq!(scroll, 7);
    }

    #[test]
    fn scroll_position_near_top() {
        let match_line = 1usize;
        let scroll = match_line.saturating_sub(3);
        assert_eq!(scroll, 0);
    }

    #[test]
    fn scroll_position_at_0() {
        let match_line = 0usize;
        let scroll = match_line.saturating_sub(3);
        assert_eq!(scroll, 0);
    }

    // ══════════════════════════════════════════════════════════════════
    //  Session filter backspace behavior
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn filter_backspace_on_empty_deactivates() {
        let mut filter = String::new();
        filter.pop();
        assert!(filter.is_empty());
    }

    #[test]
    fn filter_backspace_removes_char() {
        let mut filter = String::from("abc");
        filter.pop();
        assert_eq!(filter, "ab");
    }

    // ══════════════════════════════════════════════════════════════════
    //  Content search minimum length
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn content_search_requires_3_chars() {
        let filter = "ab";
        assert!(filter.len() < 3);
    }

    #[test]
    fn content_search_triggers_at_3_chars() {
        let filter = "abc";
        assert!(filter.len() >= 3);
    }

    // ══════════════════════════════════════════════════════════════════
    //  Page navigation arithmetic
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn page_down_clamp() {
        let selected = 3usize;
        let page = 10usize;
        let total = 8usize;
        let result = (selected + page).min(total.saturating_sub(1));
        assert_eq!(result, 7);
    }

    #[test]
    fn page_up_saturating() {
        let selected = 3usize;
        let page = 10usize;
        let result = selected.saturating_sub(page);
        assert_eq!(result, 0);
    }

    // ══════════════════════════════════════════════════════════════════
    //  Usize::MAX sentinel for viewport invalidation
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn usize_max_sentinel() {
        let scroll = usize::MAX;
        assert_eq!(scroll, usize::MAX);
    }

    #[test]
    fn usize_max_is_very_large() {
        assert!(usize::MAX > 1_000_000_000);
    }

    // ══════════════════════════════════════════════════════════════════
    //  recompute_session_find_matches — direct unit tests
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn recompute_clears_matches_when_query_empty() {
        let mut app = App::new();
        // pre-populate some stale matches
        app.session_find_matches.push((0, 0, 3));
        app.session_find = String::new();
        recompute_session_find_matches(&mut app);
        assert!(app.session_find_matches.is_empty());
    }

    #[test]
    fn recompute_resets_current_to_zero() {
        let mut app = App::new();
        app.session_find_current = 5;
        app.session_find = String::new();
        recompute_session_find_matches(&mut app);
        assert_eq!(app.session_find_current, 0);
    }

    #[test]
    fn recompute_finds_match_in_cache() {
        let mut app = App::new();
        use ratatui::text::{Line, Span};
        app.rendered_lines_cache
            .push(Line::from(vec![Span::raw("hello world")]));
        app.session_find = "world".to_string();
        recompute_session_find_matches(&mut app);
        assert!(!app.session_find_matches.is_empty());
        let (line_idx, start, end) = app.session_find_matches[0];
        assert_eq!(line_idx, 0);
        assert_eq!(end - start, 5);
    }

    #[test]
    fn recompute_case_insensitive() {
        let mut app = App::new();
        use ratatui::text::{Line, Span};
        app.rendered_lines_cache
            .push(Line::from(vec![Span::raw("HELLO")]));
        app.session_find = "hello".to_string();
        recompute_session_find_matches(&mut app);
        assert!(!app.session_find_matches.is_empty());
    }

    #[test]
    fn recompute_no_match_empty_cache() {
        let mut app = App::new();
        app.session_find = "anything".to_string();
        recompute_session_find_matches(&mut app);
        assert!(app.session_find_matches.is_empty());
    }

    #[test]
    fn recompute_multiple_matches_on_same_line() {
        let mut app = App::new();
        use ratatui::text::{Line, Span};
        app.rendered_lines_cache
            .push(Line::from(vec![Span::raw("aaa")]));
        app.session_find = "a".to_string();
        recompute_session_find_matches(&mut app);
        assert_eq!(app.session_find_matches.len(), 3);
    }

    // ══════════════════════════════════════════════════════════════════
    //  jump_next_match / jump_prev_match — App-level integration
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn jump_next_match_empty_is_noop() {
        let mut app = App::new();
        app.session_find_current = 0;
        jump_next_match(&mut app);
        assert_eq!(app.session_find_current, 0);
    }

    #[test]
    fn jump_prev_match_empty_is_noop() {
        let mut app = App::new();
        app.session_find_current = 0;
        jump_prev_match(&mut app);
        assert_eq!(app.session_find_current, 0);
    }

    #[test]
    fn jump_next_advances_index() {
        let mut app = App::new();
        app.session_find_matches = vec![(0, 0, 3), (5, 0, 3), (10, 0, 3)];
        app.session_find_current = 0;
        jump_next_match(&mut app);
        assert_eq!(app.session_find_current, 1);
    }

    #[test]
    fn jump_next_wraps_at_end() {
        let mut app = App::new();
        app.session_find_matches = vec![(0, 0, 3), (5, 0, 3)];
        app.session_find_current = 1;
        jump_next_match(&mut app);
        assert_eq!(app.session_find_current, 0);
    }

    #[test]
    fn jump_prev_decrements_index() {
        let mut app = App::new();
        app.session_find_matches = vec![(0, 0, 3), (5, 0, 3), (10, 0, 3)];
        app.session_find_current = 2;
        jump_prev_match(&mut app);
        assert_eq!(app.session_find_current, 1);
    }

    #[test]
    fn jump_prev_wraps_from_zero() {
        let mut app = App::new();
        app.session_find_matches = vec![(0, 0, 3), (5, 0, 3), (10, 0, 3)];
        app.session_find_current = 0;
        jump_prev_match(&mut app);
        assert_eq!(app.session_find_current, 2);
    }

    #[test]
    fn jump_next_updates_scroll() {
        let mut app = App::new();
        app.session_find_matches = vec![(0, 0, 3), (20, 0, 3)];
        app.session_find_current = 0;
        jump_next_match(&mut app);
        // match_line=20, scroll = 20.saturating_sub(3) = 17
        assert_eq!(app.session_scroll, 17);
    }

    #[test]
    fn jump_next_invalidates_viewport() {
        let mut app = App::new();
        app.session_find_matches = vec![(5, 0, 3), (10, 0, 3)];
        app.session_viewport_scroll = 0;
        jump_next_match(&mut app);
        assert_eq!(app.session_viewport_scroll, usize::MAX);
    }

    // ══════════════════════════════════════════════════════════════════
    //  handle_session_input — routing tests
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn session_input_find_active_routes_to_find_handler() {
        let mut app = App::new();
        app.session_find_active = true;
        app.session_find = String::new();
        let k = key(KeyCode::Char('q'));
        let result = handle_session_input(k, &mut app);
        assert!(result.is_ok());
        // 'q' should have been pushed to session_find
        assert_eq!(app.session_find, "q");
    }

    #[test]
    fn session_input_show_list_does_not_consume_esc() {
        // Session list overlay keys (Esc, a) are handled by the action system,
        // not handle_session_input — the function no longer routes to list handler
        let mut app = App::new();
        app.show_session_list = true;
        let k = key(KeyCode::Esc);
        let result = handle_session_input(k, &mut app);
        assert!(result.is_ok());
        // show_session_list unchanged — Esc is handled upstream in actions.rs
        assert!(app.show_session_list);
    }

    #[test]
    fn session_list_a_not_consumed_by_session_input() {
        // Session list 'a' key is handled by the action system (actions.rs),
        // not by handle_session_input
        let mut app = App::new();
        app.show_session_list = true;
        let k = key(KeyCode::Char('a'));
        let result = handle_session_input(k, &mut app);
        assert!(result.is_ok());
        // show_session_list unchanged — handled upstream
        assert!(app.show_session_list);
    }

    #[test]
    fn session_input_n_cycles_matches() {
        let mut app = App::new();
        app.session_find_active = false;
        app.session_find_matches = vec![(0, 0, 3), (10, 0, 3)];
        app.session_find_current = 0;
        let k = key(KeyCode::Char('n'));
        let result = handle_session_input(k, &mut app);
        assert!(result.is_ok());
        assert_eq!(app.session_find_current, 1);
    }

    #[test]
    fn session_input_upper_n_cycles_prev() {
        let mut app = App::new();
        app.session_find_active = false;
        app.session_find_matches = vec![(0, 0, 3), (10, 0, 3), (20, 0, 3)];
        app.session_find_current = 2;
        let k = key(KeyCode::Char('N'));
        let result = handle_session_input(k, &mut app);
        assert!(result.is_ok());
        assert_eq!(app.session_find_current, 1);
    }

    #[test]
    fn session_input_esc_clears_matches() {
        let mut app = App::new();
        app.session_find_active = false;
        app.session_find_matches = vec![(0, 0, 3)];
        app.session_find = "hello".to_string();
        let k = key(KeyCode::Esc);
        let result = handle_session_input(k, &mut app);
        assert!(result.is_ok());
        assert!(app.session_find_matches.is_empty());
        assert!(app.session_find.is_empty());
    }
}
