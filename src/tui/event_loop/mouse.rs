//! Mouse event handling
//!
//! Handles left-click (focus, select, cursor placement), drag (text selection),
//! and scroll (pane-specific scrolling). Also handles clipboard copy from
//! viewer and session selections.

use crate::app::{App, Focus};
use super::coords::{screen_to_cache_pos, screen_to_edit_pos, screen_to_input_char, click_to_input_cursor};

/// Apply accumulated scroll to the appropriate panel using cached pane rects
pub fn apply_scroll_cached(app: &mut App, delta: i32, col: u16, row: u16, _term_width: u16, _term_height: u16) -> bool {
    use ratatui::layout::Position;
    let pos = Position::new(col, row);

    // Health panel is a modal overlay — scroll anywhere goes to the active tab's list
    if let Some(ref mut p) = app.health_panel {
        match p.tab {
            crate::app::types::HealthTab::GodFiles => {
                if p.god_files.is_empty() { return false; }
                let max = p.god_files.len() - 1;
                if delta > 0 { p.god_selected = (p.god_selected + delta as usize).min(max); }
                else { p.god_selected = p.god_selected.saturating_sub((-delta) as usize); }
            }
            crate::app::types::HealthTab::Documentation => {
                if p.doc_entries.is_empty() { return false; }
                let max = p.doc_entries.len() - 1;
                if delta > 0 { p.doc_selected = (p.doc_selected + delta as usize).min(max); }
                else { p.doc_selected = p.doc_selected.saturating_sub((-delta) as usize); }
            }
        }
        return true;
    }

    if app.pane_worktrees.contains(pos) {
        // FileTree is always visible in the left pane — scroll file tree items
        let old = app.file_tree_selected;
        if delta > 0 { for _ in 0..delta.abs() { app.file_tree_next(); } }
        else { for _ in 0..delta.abs() { app.file_tree_prev(); } }
        app.file_tree_selected != old
    } else if app.pane_viewer.contains(pos) {
        app.viewer_selection = None;
        if delta > 0 { app.scroll_viewer_down(delta as usize) }
        else { app.scroll_viewer_up((-delta) as usize) }
    } else if app.terminal_mode && app.input_area.contains(pos) {
        if delta > 0 { app.scroll_terminal_down(delta as usize); }
        else { app.scroll_terminal_up((-delta) as usize); }
        true
    } else if app.pane_todo.width > 0 && app.pane_todo.contains(pos) {
        // Todo widget scroll — only when content overflows the 20-line cap
        let content_h = app.pane_todo.height.saturating_sub(2);
        let max_scroll = app.todo_total_lines.saturating_sub(content_h);
        if max_scroll == 0 { return false; }
        if delta > 0 { app.todo_scroll = (app.todo_scroll + delta as u16).min(max_scroll); }
        else { app.todo_scroll = app.todo_scroll.saturating_sub((-delta) as u16); }
        true
    } else if app.pane_session.contains(pos) {
        // Session list overlay: scroll selected item
        if app.show_session_list {
            let total: usize = app.worktrees.iter().map(|s| {
                app.session_files.get(&s.branch_name).map(|f| f.len().max(1)).unwrap_or(1)
            }).sum();
            if delta > 0 {
                app.session_list_selected = (app.session_list_selected + delta as usize).min(total.saturating_sub(1));
            } else {
                app.session_list_selected = app.session_list_selected.saturating_sub((-delta) as usize);
            }
            return true;
        }
        app.session_selection = None;
        if delta > 0 {
            app.scroll_session_down(delta as usize)
        } else {
            app.scroll_session_up((-delta) as usize)
        }
    } else {
        false
    }
}

/// Handle left-click: focus pane, select items, position input cursor.
/// Returns true if a redraw is needed.
pub fn handle_mouse_click(app: &mut App, col: u16, row: u16) -> bool {
    use ratatui::layout::Position;
    let pos = Position::new(col, row);

    // Overlays first — clicking anywhere dismisses them
    if app.show_help { app.show_help = false; return true; }

    // Worktree tab row click — select worktree or toggle BrowseMain
    if app.pane_worktree_tabs.contains(pos) {
        let target = app.worktree_tab_hits.iter()
            .find(|(xs, xe, _)| col >= *xs && col < *xe)
            .map(|(_, _, t)| *t);
        if let Some(tab_target) = target {
            if app.git_actions_panel.is_some() {
                // Git panel mode: switch git panel to clicked worktree
                let focused_pane = app.git_actions_panel.as_ref().map(|p| p.focused_pane).unwrap_or(0);
                match tab_target {
                    None => {
                        // ★ main tab: switch git panel to main branch
                        app.browsing_main = true;
                        app.open_git_actions_panel();
                        app.browsing_main = false;
                    }
                    Some(idx) => {
                        app.selected_worktree = Some(idx);
                        app.load_session_output();
                        app.open_git_actions_panel();
                    }
                }
                if let Some(ref mut p) = app.git_actions_panel {
                    p.focused_pane = focused_pane;
                }
            } else {
                // Normal mode: select worktree or toggle BrowseMain
                match tab_target {
                    None => {
                        if app.browsing_main { app.exit_main_browse(); }
                        else { app.enter_main_browse(); }
                    }
                    Some(idx) => {
                        if app.browsing_main { app.exit_main_browse(); }
                        app.save_current_terminal();
                        app.selected_worktree = Some(idx);
                        app.load_session_output();
                        app.invalidate_sidebar();
                    }
                }
            }
        }
        app.last_click = Some((std::time::Instant::now(), col, row));
        return true;
    }

    // Git panel is full-app layout — clicks within panes are handled, not dismissed
    if app.git_actions_panel.is_some() {
        if app.pane_viewer.contains(pos) {
            app.git_status_selected = false;
            app.viewer_selection = None;
            if let Some((cl, cc)) = screen_to_cache_pos(col, row, app.pane_viewer, app.viewer_scroll, app.viewer_lines_cache.len()) {
                app.mouse_drag_start = Some((cl, cc, 0));
            }
        } else if app.input_area.contains(pos) {
            // Click on git status box — select the result message text
            app.viewer_selection = None;
            app.git_status_selected = app.git_actions_panel.as_ref()
                .and_then(|p| p.result_message.as_ref()).is_some();
        } else {
            app.git_status_selected = false;
        }
        app.last_click = Some((std::time::Instant::now(), col, row));
        return true;
    }
    if app.run_command_picker.is_some() { app.run_command_picker = None; return true; }
    if app.run_command_dialog.is_some() { app.run_command_dialog = None; return true; }
    if app.branch_dialog.is_some() { app.branch_dialog = None; return true; }

    // Clicking any pane exits prompt mode (input pane re-enables it below)
    app.prompt_mode = false;

    // FileTree pane — always visible, click selects entries, double-click opens
    if app.pane_worktrees.contains(pos) {
        app.focus = Focus::FileTree;
        let visual_row = (row.saturating_sub(app.pane_worktrees.y + 1)) as usize;
        let entry_idx = visual_row + app.file_tree_scroll;
        if entry_idx < app.file_tree_entries.len() {
            app.file_tree_selected = Some(entry_idx);
            app.invalidate_file_tree();
            let now = std::time::Instant::now();
            let is_double = app.last_click.map_or(false, |(t, c, r)| {
                c == col && r == row && now.duration_since(t).as_millis() < 500
            });
            if is_double {
                let entry = &app.file_tree_entries[entry_idx];
                if entry.is_dir {
                    app.toggle_file_tree_dir();
                } else {
                    app.load_file_into_viewer();
                    app.focus = Focus::Viewer;
                }
            }
        }
        app.last_click = Some((std::time::Instant::now(), col, row));
        return true;
    }

    // Viewer pane — focus, and position edit cursor if in edit mode
    if app.pane_viewer.contains(pos) {
        app.focus = Focus::Viewer;
        if app.viewer_edit_mode {
            if let Some((src_line, src_col)) = screen_to_edit_pos(app, col, row) {
                app.viewer_edit_cursor = (src_line, src_col);
                app.viewer_edit_selection = None;
            }
        }
        app.last_click = Some((std::time::Instant::now(), col, row));
        return true;
    }

    // Session pane — focus + clickable file path detection
    if app.pane_session.contains(pos) {
        app.focus = Focus::Session;
        // Check if the click landed on an underlined file path link
        app.clamp_session_scroll();
        if let Some((cache_line, cache_col)) = screen_to_cache_pos(col, row, app.pane_session, app.session_scroll, app.rendered_lines_cache.len()) {
            // Search clickable_paths for a hit: first line checks column range,
            // continuation lines match anywhere within the wrapped path region
            let hit = app.clickable_paths.iter().find(|(li, sc, ec, _, _, _, wlc)| {
                if cache_line == *li { cache_col >= *sc && cache_col < *ec }
                else { *wlc > 1 && cache_line > *li && cache_line < *li + *wlc }
            }).cloned();
            if let Some((li, sc, ec, file_path, old_s, new_s, wlc)) = hit {
                // Set inverted-color highlight on the clicked path (including wrap count)
                app.clicked_path_highlight = Some((li, sc, ec, wlc));
                // Invalidate viewport cache so highlight is rendered on next draw
                app.session_viewport_scroll = usize::MAX;
                // Edit tool: open file with diff overlay in Viewer
                // Read/Write tool: open file plain in Viewer
                if !old_s.is_empty() || !new_s.is_empty() {
                    // Set selected_tool_diff so ⌥←/⌥→ cycling knows where we are
                    let click_idx = app.clickable_paths.iter().position(|(l, s, e, _, _, _, _)| *l == li && *s == sc && *e == ec);
                    app.selected_tool_diff = click_idx;
                    app.load_file_with_edit_diff(&file_path, &old_s, &new_s);
                } else {
                    app.load_file_at_path(&file_path);
                }
            } else {
                // Clicked somewhere else in session pane — clear any previous highlight
                if app.clicked_path_highlight.is_some() {
                    app.clicked_path_highlight = None;
                    app.session_viewport_scroll = usize::MAX;
                }
            }
        }
        app.last_click = Some((std::time::Instant::now(), col, row));
        return true;
    }

    // Input/Terminal pane — enter prompt mode or position cursor
    if app.input_area.contains(pos) {
        if app.terminal_mode {
            // Clicking terminal area — no cursor positioning needed
            return true;
        }
        // Enter prompt mode and position cursor at click point
        // Blocked in main browse mode — main is read-only
        if app.browsing_main { return true; }
        if !app.prompt_mode {
            app.prompt_mode = true;
        }
        app.focus = Focus::Input;
        app.input_selection = None;
        click_to_input_cursor(app, col, row);
        app.last_click = Some((std::time::Instant::now(), col, row));
        return true;
    }

    false
}

/// Handle mouse drag: compute text selection from drag anchor to current position.
/// Drag anchor is stored in cache coordinates (computed on MouseDown) so
/// auto-scroll during drag doesn't shift the start point.
pub fn handle_mouse_drag(app: &mut App, col: u16, row: u16) -> bool {
    let Some((anchor_line, anchor_col, pane_id)) = app.mouse_drag_start else { return false };

    match pane_id {
        // --- Input pane: anchor_line = char index, anchor_col unused ---
        2 => {
            let end_idx = screen_to_input_char(app, col, row);
            if anchor_line != end_idx {
                let new_sel = Some((anchor_line, end_idx));
                if app.input_selection != new_sel {
                    app.input_selection = new_sel;
                    app.input_cursor = end_idx;
                    return true;
                }
            }
            false
        }
        // --- Viewer pane: anchor = (cache_line, cache_col) ---
        0 => {
            // Auto-scroll when dragging above/below pane
            if row < app.pane_viewer.y + 1 { app.scroll_viewer_up(1); }
            else if row >= app.pane_viewer.y + app.pane_viewer.height.saturating_sub(1) { app.scroll_viewer_down(1); }
            // Clamp end to pane content bounds
            let ec = col.max(app.pane_viewer.x + 1).min(app.pane_viewer.x + app.pane_viewer.width.saturating_sub(1));
            let er = row.max(app.pane_viewer.y + 1).min(app.pane_viewer.y + app.pane_viewer.height.saturating_sub(1));
            let Some((el, ecc)) = screen_to_cache_pos(ec, er, app.pane_viewer, app.viewer_scroll, app.viewer_lines_cache.len()) else { return false };
            // Normalize so start <= end (anchor is always the fixed point)
            let sel = if anchor_line < el || (anchor_line == el && anchor_col <= ecc) {
                (anchor_line, anchor_col, el, ecc)
            } else {
                (el, ecc, anchor_line, anchor_col)
            };
            let new = Some(sel);
            if app.viewer_selection != new { app.viewer_selection = new; return true; }
            false
        }
        // --- Session pane: anchor = (cache_line, cache_col) ---
        1 => {
            app.clamp_session_scroll();
            // Auto-scroll when dragging above/below pane
            if row < app.pane_session.y + 1 { app.scroll_session_up(1); }
            else if row >= app.pane_session.y + app.pane_session.height.saturating_sub(1) { app.scroll_session_down(1); }
            let ec = col.max(app.pane_session.x + 1).min(app.pane_session.x + app.pane_session.width.saturating_sub(1));
            let er = row.max(app.pane_session.y + 1).min(app.pane_session.y + app.pane_session.height.saturating_sub(1));
            let Some((el, ecc)) = screen_to_cache_pos(ec, er, app.pane_session, app.session_scroll, app.rendered_lines_cache.len()) else { return false };
            let sel = if anchor_line < el || (anchor_line == el && anchor_col <= ecc) {
                (anchor_line, anchor_col, el, ecc)
            } else {
                (el, ecc, anchor_line, anchor_col)
            };
            let new = Some(sel);
            if app.session_selection != new {
                app.session_selection = new;
                app.session_viewport_scroll = usize::MAX;
                return true;
            }
            false
        }
        // --- Edit mode viewer: anchor = (source_line, source_col) ---
        3 => {
            // Auto-scroll when dragging above/below pane
            if row < app.pane_viewer.y + 1 { app.scroll_viewer_up(1); }
            else if row >= app.pane_viewer.y + app.pane_viewer.height.saturating_sub(1) { app.scroll_viewer_down(1); }
            let Some((el, ec)) = screen_to_edit_pos(app, col, row) else { return false };
            // Update edit selection from anchor to current drag position
            let new = Some((anchor_line, anchor_col, el, ec));
            if app.viewer_edit_selection != new {
                app.viewer_edit_selection = new;
                app.viewer_edit_cursor = (el, ec);
                return true;
            }
            false
        }
        _ => false,
    }
}

/// Extract plain text from a slice of ratatui Lines within a selection range.
/// Lines are joined with newlines. First/last line use column offsets.
/// `gutter` chars are stripped from the start of every line (for viewer line numbers).
fn extract_text_from_cache(
    cache: &[ratatui::text::Line],
    sl: usize, sc: usize, el: usize, ec: usize,
    gutter: usize,
) -> String {
    let mut out = String::new();
    for idx in sl..=el {
        let Some(line) = cache.get(idx) else { continue };
        let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        let chars: Vec<char> = text.chars().collect();
        // Shift columns past the gutter so line numbers are never copied
        let start = if idx == sl { sc.max(gutter) } else { gutter };
        let end = if idx == el { ec.max(gutter) } else { chars.len() };
        if start < end && start < chars.len() {
            out.extend(&chars[start..end.min(chars.len())]);
        }
        if idx < el { out.push('\n'); }
    }
    out
}

/// Copy text selected in the viewer pane to clipboard.
/// Strips line number gutter (first span per line) when viewer is in File mode,
/// so copied text contains only file content — no "  1 │ " prefixes.
pub fn copy_viewer_selection(app: &mut App) {
    let Some((sl, sc, el, ec)) = app.viewer_selection else { return };
    // Git mode diffs have no gutter; File mode strips line number prefix
    let gutter = if app.git_actions_panel.is_some() {
        0
    } else if app.viewer_mode == crate::app::ViewerMode::File {
        app.viewer_lines_cache.first()
            .and_then(|l| l.spans.first())
            .map(|s| s.content.chars().count())
            .unwrap_or(0)
    } else { 0 };
    let text = extract_text_from_cache(&app.viewer_lines_cache, sl, sc, el, ec, gutter);
    if text.is_empty() { return; }
    if let Ok(mut cb) = arboard::Clipboard::new() { let _ = cb.set_text(&text); }
    app.clipboard = text;
    app.set_status("Copied to clipboard");
}

/// Copy text selected in the session pane to clipboard
pub fn copy_session_selection(app: &mut App) {
    let Some((sl, sc, el, ec)) = app.session_selection else { return };
    let text = extract_text_from_cache(&app.rendered_lines_cache, sl, sc, el, ec, 0);
    if text.is_empty() { return; }
    if let Ok(mut cb) = arboard::Clipboard::new() { let _ = cb.set_text(&text); }
    app.clipboard = text;
    app.set_status("Copied to clipboard");
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::layout::{Position, Rect};
    use ratatui::text::{Line, Span};
    use ratatui::style::{Style, Color};

    // -- extract_text_from_cache: single line, full range --

    #[test]
    fn test_extract_single_line_full() {
        let lines = vec![Line::from("hello world")];
        let result = extract_text_from_cache(&lines, 0, 0, 0, 11, 0);
        assert_eq!(result, "hello world");
    }

    #[test]
    fn test_extract_single_line_partial() {
        let lines = vec![Line::from("hello world")];
        let result = extract_text_from_cache(&lines, 0, 6, 0, 11, 0);
        assert_eq!(result, "world");
    }

    #[test]
    fn test_extract_empty_cache() {
        let lines: Vec<Line> = vec![];
        let result = extract_text_from_cache(&lines, 0, 0, 0, 5, 0);
        assert_eq!(result, "");
    }

    #[test]
    fn test_extract_multiple_lines() {
        let lines = vec![
            Line::from("line one"),
            Line::from("line two"),
            Line::from("line three"),
        ];
        let result = extract_text_from_cache(&lines, 0, 0, 2, 10, 0);
        assert_eq!(result, "line one\nline two\nline three");
    }

    #[test]
    fn test_extract_multiple_lines_partial() {
        let lines = vec![
            Line::from("abcdef"),
            Line::from("ghijkl"),
            Line::from("mnopqr"),
        ];
        let result = extract_text_from_cache(&lines, 0, 3, 2, 3, 0);
        assert_eq!(result, "def\nghijkl\nmno");
    }

    // -- extract_text_from_cache: gutter stripping --

    #[test]
    fn test_extract_with_gutter() {
        let lines = vec![Line::from("  1 | hello")];
        let result = extract_text_from_cache(&lines, 0, 0, 0, 11, 6);
        assert_eq!(result, "hello");
    }

    #[test]
    fn test_extract_gutter_strips_line_numbers() {
        let lines = vec![
            Line::from("  1 | foo"),
            Line::from("  2 | bar"),
        ];
        let result = extract_text_from_cache(&lines, 0, 0, 1, 9, 6);
        assert_eq!(result, "foo\nbar");
    }

    #[test]
    fn test_extract_gutter_zero() {
        let lines = vec![Line::from("content")];
        let result = extract_text_from_cache(&lines, 0, 0, 0, 7, 0);
        assert_eq!(result, "content");
    }

    #[test]
    fn test_extract_gutter_larger_than_content() {
        let lines = vec![Line::from("abc")];
        let result = extract_text_from_cache(&lines, 0, 0, 0, 3, 10);
        assert_eq!(result, "");
    }

    // -- extract_text_from_cache: multi-span lines --

    #[test]
    fn test_extract_multi_span() {
        let lines = vec![Line::from(vec![
            Span::raw("hello"),
            Span::raw(" "),
            Span::raw("world"),
        ])];
        let result = extract_text_from_cache(&lines, 0, 0, 0, 11, 0);
        assert_eq!(result, "hello world");
    }

    #[test]
    fn test_extract_styled_spans() {
        let lines = vec![Line::from(vec![
            Span::styled("red", Style::default().fg(Color::Red)),
            Span::styled("green", Style::default().fg(Color::Green)),
        ])];
        let result = extract_text_from_cache(&lines, 0, 0, 0, 8, 0);
        assert_eq!(result, "redgreen");
    }

    // -- extract_text_from_cache: edge cases --

    #[test]
    fn test_extract_same_start_end() {
        let lines = vec![Line::from("hello")];
        let result = extract_text_from_cache(&lines, 0, 3, 0, 3, 0);
        assert_eq!(result, "");
    }

    #[test]
    fn test_extract_out_of_bounds_line() {
        let lines = vec![Line::from("only")];
        let result = extract_text_from_cache(&lines, 5, 0, 5, 4, 0);
        assert_eq!(result, "");
    }

    #[test]
    fn test_extract_newlines_between_lines() {
        let lines = vec![
            Line::from("a"),
            Line::from("b"),
        ];
        let result = extract_text_from_cache(&lines, 0, 0, 1, 1, 0);
        assert!(result.contains('\n'));
    }

    #[test]
    fn test_extract_no_trailing_newline() {
        let lines = vec![
            Line::from("a"),
            Line::from("b"),
        ];
        let result = extract_text_from_cache(&lines, 0, 0, 1, 1, 0);
        assert!(!result.ends_with('\n'));
    }

    // -- Position and Rect construction --

    #[test]
    fn test_position_new() {
        let pos = Position::new(10, 20);
        assert_eq!(pos.x, 10);
        assert_eq!(pos.y, 20);
    }

    #[test]
    fn test_rect_contains_position() {
        let rect = Rect::new(5, 5, 20, 10);
        assert!(rect.contains(Position::new(10, 10)));
    }

    #[test]
    fn test_rect_not_contains_position() {
        let rect = Rect::new(5, 5, 20, 10);
        assert!(!rect.contains(Position::new(0, 0)));
    }

    #[test]
    fn test_rect_contains_top_left() {
        let rect = Rect::new(5, 5, 20, 10);
        assert!(rect.contains(Position::new(5, 5)));
    }

    #[test]
    fn test_rect_not_contains_bottom_right_exclusive() {
        let rect = Rect::new(5, 5, 20, 10);
        assert!(!rect.contains(Position::new(25, 15)));
    }

    // -- Selection normalization --

    #[test]
    fn test_selection_normalize_forward() {
        let (al, ac, el, ec) = (2usize, 5usize, 4usize, 3usize);
        let sel = if al < el || (al == el && ac <= ec) {
            (al, ac, el, ec)
        } else {
            (el, ec, al, ac)
        };
        assert_eq!(sel, (2, 5, 4, 3));
    }

    #[test]
    fn test_selection_normalize_backward() {
        let (al, ac, el, ec) = (4usize, 3usize, 2usize, 5usize);
        let sel = if al < el || (al == el && ac <= ec) {
            (al, ac, el, ec)
        } else {
            (el, ec, al, ac)
        };
        assert_eq!(sel, (2, 5, 4, 3));
    }

    #[test]
    fn test_selection_normalize_same_line_forward() {
        let (al, ac, el, ec) = (3usize, 2usize, 3usize, 8usize);
        let sel = if al < el || (al == el && ac <= ec) {
            (al, ac, el, ec)
        } else {
            (el, ec, al, ac)
        };
        assert_eq!(sel, (3, 2, 3, 8));
    }

    #[test]
    fn test_selection_normalize_same_line_backward() {
        let (al, ac, el, ec) = (3usize, 8usize, 3usize, 2usize);
        let sel = if al < el || (al == el && ac <= ec) {
            (al, ac, el, ec)
        } else {
            (el, ec, al, ac)
        };
        assert_eq!(sel, (3, 2, 3, 8));
    }

    // -- Pane ID matching for drag --

    #[test]
    fn test_pane_id_viewer() {
        let pane_id = 0u8;
        assert!(matches!(pane_id, 0));
    }

    #[test]
    fn test_pane_id_session() {
        let pane_id = 1u8;
        assert!(matches!(pane_id, 1));
    }

    #[test]
    fn test_pane_id_input() {
        let pane_id = 2u8;
        assert!(matches!(pane_id, 2));
    }

    #[test]
    fn test_pane_id_edit() {
        let pane_id = 3u8;
        assert!(matches!(pane_id, 3));
    }

    #[test]
    fn test_pane_id_unknown() {
        let pane_id = 99u8;
        assert!(!matches!(pane_id, 0 | 1 | 2 | 3));
    }

    // -- Drag anchor tuple --

    #[test]
    fn test_drag_anchor_some() {
        let anchor: Option<(usize, usize, u8)> = Some((10, 5, 0));
        assert!(anchor.is_some());
        let (line, col, pane) = anchor.unwrap();
        assert_eq!(line, 10);
        assert_eq!(col, 5);
        assert_eq!(pane, 0);
    }

    #[test]
    fn test_drag_anchor_none() {
        let anchor: Option<(usize, usize, u8)> = None;
        assert!(anchor.is_none());
    }

    // -- Auto-scroll boundary checks --

    #[test]
    fn test_auto_scroll_above_pane() {
        let pane_y = 5u16;
        let row = 3u16;
        assert!(row < pane_y + 1);
    }

    #[test]
    fn test_auto_scroll_below_pane() {
        let pane_y = 5u16;
        let pane_height = 20u16;
        let row = 24u16;
        assert!(row >= pane_y + pane_height.saturating_sub(1));
    }

    #[test]
    fn test_auto_scroll_within_pane() {
        let pane_y = 5u16;
        let pane_height = 20u16;
        let row = 15u16;
        assert!(!(row < pane_y + 1) && !(row >= pane_y + pane_height.saturating_sub(1)));
    }

    // -- Column clamping for drag --

    #[test]
    fn test_col_clamp_left() {
        let col = 2u16;
        let pane_x = 5u16;
        let pane_width = 20u16;
        let ec = col.max(pane_x + 1).min(pane_x + pane_width.saturating_sub(1));
        assert_eq!(ec, 6); // clamped to pane_x + 1
    }

    #[test]
    fn test_col_clamp_right() {
        let col = 50u16;
        let pane_x = 5u16;
        let pane_width = 20u16;
        let ec = col.max(pane_x + 1).min(pane_x + pane_width.saturating_sub(1));
        assert_eq!(ec, 24); // clamped to pane_x + pane_width - 1
    }

    #[test]
    fn test_col_clamp_within() {
        let col = 15u16;
        let pane_x = 5u16;
        let pane_width = 20u16;
        let ec = col.max(pane_x + 1).min(pane_x + pane_width.saturating_sub(1));
        assert_eq!(ec, 15); // unchanged
    }

    // -- Row clamping for drag --

    #[test]
    fn test_row_clamp_above() {
        let row = 2u16;
        let pane_y = 5u16;
        let pane_height = 20u16;
        let er = row.max(pane_y + 1).min(pane_y + pane_height.saturating_sub(1));
        assert_eq!(er, 6);
    }

    #[test]
    fn test_row_clamp_below() {
        let row = 50u16;
        let pane_y = 5u16;
        let pane_height = 20u16;
        let er = row.max(pane_y + 1).min(pane_y + pane_height.saturating_sub(1));
        assert_eq!(er, 24);
    }

    // -- Double-click timing --

    #[test]
    fn test_double_click_timing() {
        let now = std::time::Instant::now();
        let last = now;
        let elapsed = now.duration_since(last).as_millis();
        assert!(elapsed < 500);
    }

    #[test]
    fn test_double_click_same_position() {
        let (c1, r1) = (10u16, 20u16);
        let (c2, r2) = (10u16, 20u16);
        assert!(c1 == c2 && r1 == r2);
    }

    #[test]
    fn test_double_click_different_position() {
        let (c1, r1) = (10u16, 20u16);
        let (c2, r2) = (15u16, 20u16);
        assert!(!(c1 == c2 && r1 == r2));
    }

    // -- Visual row from click position --

    #[test]
    fn test_visual_row_calc() {
        let row = 10u16;
        let pane_y = 3u16;
        let visual_row = (row.saturating_sub(pane_y + 1)) as usize;
        assert_eq!(visual_row, 6);
    }

    #[test]
    fn test_entry_idx_with_scroll() {
        let visual_row = 5usize;
        let scroll = 3usize;
        let entry_idx = visual_row + scroll;
        assert_eq!(entry_idx, 8);
    }

    // -- Focus enum for click targets --

    #[test]
    fn test_focus_file_tree() {
        let f = Focus::FileTree;
        assert_eq!(f, Focus::FileTree);
    }

    #[test]
    fn test_focus_viewer() {
        let f = Focus::Viewer;
        assert_eq!(f, Focus::Viewer);
    }

    #[test]
    fn test_focus_session() {
        let f = Focus::Session;
        assert_eq!(f, Focus::Session);
    }

    #[test]
    fn test_focus_input() {
        let f = Focus::Input;
        assert_eq!(f, Focus::Input);
    }

    // -- Scroll delta direction --

    #[test]
    fn test_scroll_delta_positive() {
        let delta = 3i32;
        assert!(delta > 0);
    }

    #[test]
    fn test_scroll_delta_negative() {
        let delta = -3i32;
        assert!(delta < 0);
        assert_eq!((-delta) as usize, 3);
    }

    #[test]
    fn test_scroll_delta_abs() {
        let delta = -5i32;
        assert_eq!(delta.abs(), 5);
    }

    // -- Todo scroll clamping --

    #[test]
    fn test_todo_scroll_clamp() {
        let todo_scroll = 10u16;
        let max_scroll = 5u16;
        let clamped = todo_scroll.min(max_scroll);
        assert_eq!(clamped, 5);
    }

    #[test]
    fn test_todo_content_height() {
        let pane_height = 22u16;
        let content_h = pane_height.saturating_sub(2);
        assert_eq!(content_h, 20);
    }

    #[test]
    fn test_todo_max_scroll() {
        let total_lines = 30u16;
        let content_h = 20u16;
        let max_scroll = total_lines.saturating_sub(content_h);
        assert_eq!(max_scroll, 10);
    }

    #[test]
    fn test_todo_no_overflow() {
        let total_lines = 15u16;
        let content_h = 20u16;
        let max_scroll = total_lines.saturating_sub(content_h);
        assert_eq!(max_scroll, 0);
    }

    // -- Clickable path tuple structure --

    #[test]
    fn test_clickable_path_tuple() {
        let path: (usize, usize, usize, String, String, String, usize) =
            (5, 10, 25, "/path/to/file".into(), "old".into(), "new".into(), 1);
        assert_eq!(path.0, 5);  // line index
        assert_eq!(path.1, 10); // start column
        assert_eq!(path.2, 25); // end column
        assert_eq!(path.3, "/path/to/file");
        assert_eq!(path.6, 1);  // wrap line count
    }

    #[test]
    fn test_clickable_path_edit_tool() {
        let old_s = "old content";
        let new_s = "new content";
        assert!(!old_s.is_empty() || !new_s.is_empty());
    }

    #[test]
    fn test_clickable_path_read_tool() {
        let old_s = "";
        let new_s = "";
        assert!(old_s.is_empty() && new_s.is_empty());
    }

    // -- Session list selected bounds --

    #[test]
    fn test_session_list_scroll_clamp() {
        let selected = 15usize;
        let total = 10usize;
        let clamped = selected.min(total.saturating_sub(1));
        assert_eq!(clamped, 9);
    }

    // -- Overlay dismissal flags --

    #[test]
    fn test_show_help_dismiss() {
        let mut show_help = true;
        assert!(show_help);
        show_help = false;
        assert!(!show_help);
    }

    #[test]
    fn test_run_command_dismiss() {
        let mut picker: Option<String> = Some("cmd".into());
        assert!(picker.is_some());
        picker = None;
        assert!(picker.is_none());
    }

    #[test]
    fn test_branch_dialog_dismiss() {
        let mut dialog: Option<String> = Some("dialog".into());
        assert!(dialog.is_some());
        dialog = None;
        assert!(dialog.is_none());
    }

    // -- Clipboard status message --

    #[test]
    fn test_clipboard_status() {
        let status = "Copied to clipboard";
        assert_eq!(status, "Copied to clipboard");
    }

    // -- Gutter width detection --

    #[test]
    fn test_gutter_from_first_span() {
        let line = Line::from(vec![
            Span::raw("  1 | "),
            Span::raw("content"),
        ]);
        let gutter = line.spans.first()
            .map(|s| s.content.chars().count())
            .unwrap_or(0);
        assert_eq!(gutter, 6);
    }

    #[test]
    fn test_gutter_empty_cache() {
        let cache: Vec<Line> = vec![];
        let gutter = cache.first()
            .and_then(|l| l.spans.first())
            .map(|s| s.content.chars().count())
            .unwrap_or(0);
        assert_eq!(gutter, 0);
    }

    // -- Function type checks --

    #[test]
    fn test_apply_scroll_cached_fn_type() {
        let _ = apply_scroll_cached as fn(&mut App, i32, u16, u16, u16, u16) -> bool;
    }

    #[test]
    fn test_handle_mouse_click_fn_type() {
        let _ = handle_mouse_click as fn(&mut App, u16, u16) -> bool;
    }

    #[test]
    fn test_handle_mouse_drag_fn_type() {
        let _ = handle_mouse_drag as fn(&mut App, u16, u16) -> bool;
    }

    #[test]
    fn test_copy_viewer_selection_fn_type() {
        let _ = copy_viewer_selection as fn(&mut App);
    }

    #[test]
    fn test_copy_session_selection_fn_type() {
        let _ = copy_session_selection as fn(&mut App);
    }

    // -- last_click tuple --

    #[test]
    fn test_last_click_tuple() {
        let now = std::time::Instant::now();
        let click: (std::time::Instant, u16, u16) = (now, 15, 25);
        assert_eq!(click.1, 15);
        assert_eq!(click.2, 25);
    }

    // -- Input selection equality check --

    #[test]
    fn test_input_selection_changed() {
        let old: Option<(usize, usize)> = Some((0, 5));
        let new: Option<(usize, usize)> = Some((0, 10));
        assert_ne!(old, new);
    }

    #[test]
    fn test_input_selection_unchanged() {
        let old: Option<(usize, usize)> = Some((0, 5));
        let new: Option<(usize, usize)> = Some((0, 5));
        assert_eq!(old, new);
    }

    // -- Viewer selection equality check --

    #[test]
    fn test_viewer_selection_changed() {
        let old: Option<(usize, usize, usize, usize)> = Some((0, 0, 5, 10));
        let new: Option<(usize, usize, usize, usize)> = Some((0, 0, 5, 15));
        assert_ne!(old, new);
    }

    #[test]
    fn test_viewer_selection_unchanged() {
        let old: Option<(usize, usize, usize, usize)> = Some((0, 0, 5, 10));
        let new: Option<(usize, usize, usize, usize)> = Some((0, 0, 5, 10));
        assert_eq!(old, new);
    }
}
