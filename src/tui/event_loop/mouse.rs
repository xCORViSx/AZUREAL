//! Mouse event handling
//!
//! Handles left-click (focus, select, cursor placement), drag (text selection),
//! and scroll (pane-specific scrolling). Also handles clipboard copy from
//! viewer and convo selections.

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
        if app.show_file_tree {
            // File tree overlay is showing — scroll file tree items
            let old = app.file_tree_selected;
            if delta > 0 { for _ in 0..delta.abs() { app.file_tree_next(); } }
            else { for _ in 0..delta.abs() { app.file_tree_prev(); } }
            app.file_tree_selected != old
        } else {
            let old = app.selected_worktree;
            if delta > 0 { for _ in 0..delta.abs() { app.select_next_session(); } }
            else { for _ in 0..delta.abs() { app.select_prev_session(); } }
            app.selected_worktree != old
        }
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
    } else if app.pane_convo.contains(pos) {
        // Session list overlay: scroll selected item
        if app.show_session_list {
            let total: usize = app.sessions.iter().map(|s| {
                app.session_files.get(&s.branch_name).map(|f| f.len().max(1)).unwrap_or(1)
            }).sum();
            if delta > 0 {
                app.session_list_selected = (app.session_list_selected + delta as usize).min(total.saturating_sub(1));
            } else {
                app.session_list_selected = app.session_list_selected.saturating_sub((-delta) as usize);
            }
            return true;
        }
        app.output_selection = None;
        if delta > 0 {
            match app.view_mode {
                crate::app::ViewMode::Output => app.scroll_output_down(delta as usize),
                crate::app::ViewMode::Diff => app.scroll_diff_down(delta as usize),
                _ => false
            }
        } else {
            match app.view_mode {
                crate::app::ViewMode::Output => app.scroll_output_up((-delta) as usize),
                crate::app::ViewMode::Diff => app.scroll_diff_up((-delta) as usize),
                _ => false
            }
        }
    } else {
        false
    }
}

/// Handle left-click: focus pane, select items, position input cursor.
/// Returns true if a redraw is needed.
pub fn handle_mouse_click(app: &mut App, col: u16, row: u16) -> bool {
    use ratatui::layout::Position;
    use crate::app::SidebarRowAction;
    let pos = Position::new(col, row);

    // Overlays first — clicking anywhere dismisses them
    if app.show_help { app.show_help = false; return true; }
    if app.context_menu.is_some() { app.context_menu = None; return true; }
    if app.git_actions_panel.is_some() { app.git_actions_panel = None; return true; }
    if app.run_command_picker.is_some() { app.run_command_picker = None; return true; }
    if app.run_command_dialog.is_some() { app.run_command_dialog = None; return true; }
    if app.branch_dialog.is_some() { app.branch_dialog = None; return true; }
    if app.creation_wizard.is_some() { app.creation_wizard = None; app.focus = Focus::Worktrees; return true; }

    // Clicking any pane exits prompt mode (input pane re-enables it below)
    app.prompt_mode = false;

    // Worktrees/FileTree pane — when file tree overlay is active, click selects entries;
    // otherwise click selects worktrees or their Claude session files
    if app.pane_worktrees.contains(pos) {
        if app.show_file_tree {
            // File tree overlay: click to select, double-click to open/expand
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
        } else {
            // Worktree list: click to select worktree
            app.focus = Focus::Worktrees;
            let visual_row = (row.saturating_sub(app.pane_worktrees.y + 1)) as usize;
            let clicked_idx = app.sidebar_row_map.get(visual_row)
                .and_then(|a| if let SidebarRowAction::Worktree(i) = a { Some(*i) } else { None });
            if let Some(idx) = clicked_idx {
                if app.selected_worktree != Some(idx) {
                    app.save_current_terminal();
                    app.selected_worktree = Some(idx);
                    app.load_session_output();
                    app.invalidate_sidebar();
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

    // Convo pane — focus + clickable file path detection
    if app.pane_convo.contains(pos) {
        app.focus = Focus::Output;
        // Check if the click landed on an underlined file path link
        app.clamp_output_scroll();
        if let Some((cache_line, cache_col)) = screen_to_cache_pos(col, row, app.pane_convo, app.output_scroll, app.rendered_lines_cache.len()) {
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
                app.output_viewport_scroll = usize::MAX;
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
                // Clicked somewhere else in convo — clear any previous highlight
                if app.clicked_path_highlight.is_some() {
                    app.clicked_path_highlight = None;
                    app.output_viewport_scroll = usize::MAX;
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
        // --- Convo pane: anchor = (cache_line, cache_col) ---
        1 => {
            app.clamp_output_scroll();
            // Auto-scroll when dragging above/below pane
            if row < app.pane_convo.y + 1 { app.scroll_output_up(1); }
            else if row >= app.pane_convo.y + app.pane_convo.height.saturating_sub(1) { app.scroll_output_down(1); }
            let ec = col.max(app.pane_convo.x + 1).min(app.pane_convo.x + app.pane_convo.width.saturating_sub(1));
            let er = row.max(app.pane_convo.y + 1).min(app.pane_convo.y + app.pane_convo.height.saturating_sub(1));
            let Some((el, ecc)) = screen_to_cache_pos(ec, er, app.pane_convo, app.output_scroll, app.rendered_lines_cache.len()) else { return false };
            let sel = if anchor_line < el || (anchor_line == el && anchor_col <= ecc) {
                (anchor_line, anchor_col, el, ecc)
            } else {
                (el, ecc, anchor_line, anchor_col)
            };
            let new = Some(sel);
            if app.output_selection != new {
                app.output_selection = new;
                app.output_viewport_scroll = usize::MAX;
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
    // In File mode, first span of each cache line is the line number gutter
    let gutter = if app.viewer_mode == crate::app::ViewerMode::File {
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

/// Copy text selected in the convo pane to clipboard
pub fn copy_output_selection(app: &mut App) {
    let Some((sl, sc, el, ec)) = app.output_selection else { return };
    let text = extract_text_from_cache(&app.rendered_lines_cache, sl, sc, el, ec, 0);
    if text.is_empty() { return; }
    if let Ok(mut cb) = arboard::Clipboard::new() { let _ = cb.set_text(&text); }
    app.clipboard = text;
    app.set_status("Copied to clipboard");
}
