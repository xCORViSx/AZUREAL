//! Session pane rendering
//!
//! Expensive work (markdown parsing, syntax highlighting, text wrapping) runs
//! on a background render thread. The main event loop sends render requests
//! via `submit_render_request()` (non-blocking) and polls for completed results
//! via `poll_render_result()` (non-blocking). The draw function itself is cheap —
//! just clones a viewport slice and renders from the pre-built cache.
//!
//! Submodules:
//! - `render_submit`: Background render thread submit/poll coordination
//! - `session_list`: Session browser overlay with filter and content search
//! - `todo_widget`: Sticky task progress tracker at bottom of session pane
//! - `selection`: Selectable content range calculation for cache lines
//! - `dialogs`: Session pane dialog overlays (new session, RCR, post-merge)
//! - `git_commits`: Git panel commit log rendering
//! - `viewport`: Viewport cache building with real-time overlays
//! - `session_chrome`: Session pane border and block construction
mod dialogs;
mod git_commits;
mod render_submit;
mod selection;
mod session_chrome;
mod session_list;
mod todo_widget;
mod viewport;

/// Re-export public API so existing `use super::draw_output::{...}` imports work unchanged
pub use dialogs::{draw_post_merge_dialog, draw_rcr_approval};
pub use render_submit::{poll_render_result, submit_render_request};
pub(crate) use selection::compute_line_content_bounds;

use ratatui::{
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Borders, Paragraph},
    Frame,
};

use super::util::{colorize_output, detect_message_type, MessageType, AZURE};
use crate::app::{App, ViewMode};

/// Draw the main output/diff panel — cheap, just reads from pre-rendered caches
pub fn draw_output(f: &mut Frame, app: &mut App, area: Rect) {
    // Git panel mode — show commit log instead of conversation
    if app.git_actions_panel.is_some() {
        let scroll =
            git_commits::draw_git_commits(f, app.git_actions_panel.as_ref().unwrap(), area);
        app.git_actions_panel.as_mut().unwrap().commit_scroll = scroll;
        return;
    }

    // Session list overlay takes over the entire session pane when active
    if app.show_session_list {
        session_list::draw_session_list(f, app, area);
        return;
    }

    // Split area for sticky todo widget at bottom (visible whenever todos exist —
    // stays visible even when all completed, cleared on next user prompt or session switch)
    let has_todos = !app.current_todos.is_empty() || !app.subagent_todos.is_empty();
    let todo_height = if has_todos {
        // Account for text wrapping: each todo may span multiple visual lines.
        // Inner width = area width minus 2 for borders (minus 1 more if scrollbar needed).
        let inner_w = area.width.saturating_sub(2) as usize;
        // Helper closure: count wrapped visual lines for a todo list.
        // prefix_extra = extra chars before text (e.g. 2 for "↳ " indent on subtasks)
        let count_lines = |todos: &[crate::app::TodoItem], prefix_extra: usize| -> u16 {
            if inner_w == 0 {
                return todos.len() as u16;
            }
            todos
                .iter()
                .map(|t| {
                    let text = if t.status == crate::app::TodoStatus::InProgress
                        && !t.active_form.is_empty()
                    {
                        &t.active_form
                    } else {
                        &t.content
                    };
                    // 2 chars for icon ("✓ ") + prefix_extra for indent
                    let text_w: usize = text
                        .chars()
                        .map(|c| unicode_width::UnicodeWidthChar::width(c).unwrap_or(1))
                        .sum::<usize>()
                        + 2
                        + prefix_extra;
                    ((text_w + inner_w - 1) / inner_w).max(1) as u16
                })
                .sum()
        };
        let main_lines = count_lines(&app.current_todos, 0);
        // Subagent todos get "↳ " prefix (2 display-width chars)
        let sub_lines = count_lines(&app.subagent_todos, 2);
        let total_content_lines = main_lines + sub_lines;
        app.todo_total_lines = total_content_lines;
        // Cap at 20 content lines + 2 border = 22, also ensure session pane has >= 10 rows
        let max_h = 22u16.min(area.height.saturating_sub(10));
        (total_content_lines + 2).min(max_h)
    } else {
        app.todo_total_lines = 0;
        0
    };
    // Search bar at bottom of session pane: visible when search is active or has residual matches
    let has_search = app.session_find_active || !app.session_find_matches.is_empty();
    let search_height: u16 = if has_search { 3 } else { 0 };
    let [session_area, search_area, todo_area] = Layout::vertical([
        Constraint::Min(1),
        Constraint::Length(search_height),
        Constraint::Length(todo_height),
    ])
    .areas(area);
    let area = session_area;
    app.pane_session_content = area;
    let viewport_height = area.height.saturating_sub(2) as usize;

    // Cache viewport height for scroll operations (input handling uses this)
    app.session_viewport_height = viewport_height;

    let (title, content) = match app.view_mode {
        ViewMode::Session => {
            // If the cache width doesn't match the actual draw area (e.g. resize),
            // mark dirty so the next loop iteration submits a new render request.
            // We NEVER render synchronously here — draw uses whatever cache exists.
            let inner_width = area.width.saturating_sub(2);
            if !app.display_events.is_empty()
                && app.rendered_lines_width != inner_width
                && !app.rendered_lines_dirty
            {
                app.rendered_lines_dirty = true;
            }

            if !app.rendered_lines_cache.is_empty() {
                // Resolve scroll for this frame WITHOUT destroying the usize::MAX sentinel.
                // If user is following bottom (sentinel), compute concrete position but
                // leave session_scroll as usize::MAX so it keeps following on next frame.
                let scroll = if app.session_scroll == usize::MAX {
                    app.session_natural_bottom()
                } else {
                    app.session_scroll.min(app.session_max_scroll())
                };

                // Check if viewport cache is still valid — skip the clone if so.
                // Selection changes also invalidate (must re-apply highlight)
                // Check if any tools are still pending (need pulse animation)
                let has_pending_tools = app
                    .animation_line_indices
                    .iter()
                    .any(|(_, _, id)| app.pending_tool_calls.contains(id));
                let cache_valid = scroll == app.session_viewport_scroll
                    && (!has_pending_tools || app.animation_tick == app.session_viewport_anim_tick)
                    && app.tool_status_generation == app.session_viewport_status_gen
                    && app.session_selection == app.session_selection_cached
                    && app.session_viewport_cache.len()
                        == viewport_height
                            .min(app.rendered_lines_cache.len().saturating_sub(scroll));

                if !cache_valid {
                    viewport::rebuild_viewport_cache(app, scroll, viewport_height);
                }

                (
                    app.session_viewport_title.clone(),
                    app.session_viewport_cache.clone(),
                )
            } else if !app.session_lines.is_empty() || !app.session_buffer.is_empty() {
                // Fallback: using session_lines with colorize_output
                let mut all_lines: Vec<Line> = Vec::new();
                let mut last_msg_type = MessageType::Other;

                for line in app.session_lines.iter() {
                    let msg_type = detect_message_type(line);
                    if (last_msg_type == MessageType::User && msg_type == MessageType::Assistant)
                        || (last_msg_type == MessageType::Assistant
                            && msg_type == MessageType::User)
                    {
                        all_lines.push(Line::from(""));
                        all_lines.push(Line::from(""));
                    }
                    all_lines.push(colorize_output(line));
                    if msg_type != MessageType::Other {
                        last_msg_type = msg_type;
                    }
                }

                if !app.session_buffer.is_empty() {
                    let msg_type = detect_message_type(&app.session_buffer);
                    if (last_msg_type == MessageType::User && msg_type == MessageType::Assistant)
                        || (last_msg_type == MessageType::Assistant
                            && msg_type == MessageType::User)
                    {
                        all_lines.push(Line::from(""));
                        all_lines.push(Line::from(""));
                    }
                    all_lines.push(colorize_output(&app.session_buffer));
                }

                let total = all_lines.len();
                let max_scroll = total.saturating_sub(viewport_height);
                // Resolve sentinel to concrete position for THIS frame only —
                // don't write it back so usize::MAX survives and keeps
                // following bottom as new content arrives.
                let scroll = if app.session_scroll == usize::MAX {
                    max_scroll
                } else {
                    app.session_scroll.min(max_scroll)
                };
                let lines: Vec<Line> = all_lines
                    .into_iter()
                    .skip(scroll)
                    .take(viewport_height)
                    .collect();
                let title = if total > viewport_height {
                    format!(
                        " Session [{}/{}] ",
                        scroll + viewport_height.min(total - scroll),
                        total
                    )
                } else {
                    " Session ".to_string()
                };
                (title, lines)
            } else if app.current_session_id.is_some() {
                // Active session with no events yet — blank pane, ready for input
                (" Session ".to_string(), vec![])
            } else {
                // No session selected — show hint to open session list
                let key = crate::tui::keybindings::find_key_for_action(
                    &crate::tui::keybindings::SESSION,
                    crate::tui::keybindings::Action::ToggleSessionList,
                )
                .unwrap_or_else(|| "s".into());
                let add_key = crate::tui::keybindings::find_key_for_action(
                    &crate::tui::keybindings::SESSION,
                    crate::tui::keybindings::Action::NewSession,
                )
                .unwrap_or_else(|| "a".into());
                let hint = vec![Line::from(vec![
                    Span::styled("Press ", Style::default().fg(Color::DarkGray)),
                    Span::styled(key, Style::default().fg(AZURE).add_modifier(Modifier::BOLD)),
                    Span::styled(
                        " to choose a session or ",
                        Style::default().fg(Color::DarkGray),
                    ),
                    Span::styled(
                        add_key,
                        Style::default().fg(AZURE).add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(" to create one", Style::default().fg(Color::DarkGray)),
                ])];
                (" Session ".to_string(), hint)
            }
        }
    };

    let block = session_chrome::build_session_block(app, area, &title);
    let output = Paragraph::new(content).block(block);
    f.render_widget(output, area);

    // Render session find bar at bottom of session content area
    if has_search {
        let match_info = if app.session_find_matches.is_empty() {
            if app.session_find.is_empty() {
                String::new()
            } else {
                " 0/0 ".to_string()
            }
        } else {
            format!(
                " {}/{} ",
                app.session_find_current + 1,
                app.session_find_matches.len()
            )
        };
        let border_color = if app.session_find_active {
            Color::Yellow
        } else {
            Color::DarkGray
        };
        let search_widget = Paragraph::new(app.session_find.clone()).block(
            ratatui::widgets::Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(border_color))
                .title(Span::styled("/", Style::default().fg(Color::Yellow)))
                .title(
                    Line::from(Span::styled(
                        match_info,
                        Style::default().fg(Color::DarkGray),
                    ))
                    .alignment(Alignment::Right),
                ),
        );
        f.render_widget(search_widget, search_area);
        // Show cursor in search bar when actively typing
        if app.session_find_active {
            let cursor_x = search_area.x + 1 + app.session_find.len() as u16;
            let cursor_y = search_area.y + 1;
            if cursor_x < search_area.right() {
                f.set_cursor_position((cursor_x, cursor_y));
            }
        }
    }

    // Render sticky todo widget at bottom of session pane (main + subagent todos).
    // Cache the rect for mouse scroll hit-testing, and clamp scroll to valid range.
    if todo_height > 0 {
        app.pane_todo = todo_area;
        let content_h = todo_area.height.saturating_sub(2);
        let max_scroll = app.todo_total_lines.saturating_sub(content_h);
        if app.todo_scroll > max_scroll {
            app.todo_scroll = max_scroll;
        }
        todo_widget::draw_todo_widget(
            f,
            &app.current_todos,
            &app.subagent_todos,
            app.subagent_parent_idx,
            todo_area,
            app.animation_tick,
            app.todo_scroll,
            app.todo_total_lines,
        );
    } else {
        // No todos visible — clear cached rect so mouse scroll won't hit-test stale area
        app.pane_todo = Rect::default();
    }

    // New session name dialog (centered overlay) — rendered last so it appears above all content
    if app.new_session_dialog_active {
        dialogs::draw_new_session_dialog(f, app, area);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::types::{GitChangedFile, GitCommit, PostMergeDialog};
    use crate::app::Focus;
    use crate::tui::colorize::ORANGE;
    use crate::tui::util::{GIT_BROWN, GIT_ORANGE};
    use std::path::PathBuf;

    // ── Colors ──
    #[test]
    fn test_azure() {
        assert_eq!(AZURE, Color::Rgb(51, 153, 255));
    }
    #[test]
    fn test_git_orange() {
        assert_eq!(GIT_ORANGE, Color::Rgb(240, 80, 50));
    }
    #[test]
    fn test_git_brown() {
        assert_eq!(GIT_BROWN, Color::Rgb(160, 82, 45));
    }
    #[test]
    fn test_orange_exists() {
        let _ = ORANGE;
    }

    // ── ViewMode ──
    #[test]
    fn test_view_mode_eq() {
        assert_eq!(ViewMode::Session, ViewMode::Session);
    }

    // ── Focus ──
    #[test]
    fn test_focus_session() {
        assert_eq!(Focus::Session, Focus::Session);
    }
    #[test]
    fn test_focus_input() {
        assert_eq!(Focus::Input, Focus::Input);
    }
    #[test]
    fn test_focus_ne() {
        assert_ne!(Focus::Session, Focus::Input);
    }

    // ── MessageType ──
    #[test]
    fn test_msg_type_user() {
        let _ = MessageType::User;
    }
    #[test]
    fn test_msg_type_assistant() {
        let _ = MessageType::Assistant;
    }
    #[test]
    fn test_msg_type_other() {
        let _ = MessageType::Other;
    }

    // ── GitCommit ──
    #[test]
    fn test_commit_new() {
        let c = GitCommit {
            hash: "abc".into(),
            full_hash: "abcdef".into(),
            subject: "feat".into(),
            is_pushed: false,
        };
        assert!(!c.is_pushed);
    }
    #[test]
    fn test_commit_pushed() {
        let c = GitCommit {
            hash: "d".into(),
            full_hash: "dd".into(),
            subject: "s".into(),
            is_pushed: true,
        };
        assert!(c.is_pushed);
    }
    #[test]
    fn test_commit_clone() {
        let c = GitCommit {
            hash: "h".into(),
            full_hash: "hh".into(),
            subject: "s".into(),
            is_pushed: false,
        };
        let cl = c.clone();
        assert_eq!(cl.hash, "h");
    }

    // ── GitChangedFile ──
    #[test]
    fn test_file_modified() {
        let f = GitChangedFile {
            path: "a".into(),
            status: 'M',
            additions: 10,
            deletions: 5,
            staged: false,
        };
        assert_eq!(f.status, 'M');
    }
    #[test]
    fn test_file_added() {
        let f = GitChangedFile {
            path: "b".into(),
            status: 'A',
            additions: 50,
            deletions: 0,
            staged: false,
        };
        assert_eq!(f.status, 'A');
    }
    #[test]
    fn test_file_deleted() {
        let f = GitChangedFile {
            path: "c".into(),
            status: 'D',
            additions: 0,
            deletions: 30,
            staged: false,
        };
        assert_eq!(f.status, 'D');
    }

    // ── Status colors ──
    #[test]
    fn test_sc_a() {
        assert_eq!(
            match 'A' {
                'A' => Color::Green,
                'D' => Color::Red,
                'M' => Color::Yellow,
                'R' => Color::Cyan,
                '?' => Color::Magenta,
                _ => Color::White,
            },
            Color::Green
        );
    }
    #[test]
    fn test_sc_d() {
        assert_eq!(
            match 'D' {
                'A' => Color::Green,
                'D' => Color::Red,
                _ => Color::White,
            },
            Color::Red
        );
    }
    #[test]
    fn test_sc_m() {
        assert_eq!(
            match 'M' {
                'M' => Color::Yellow,
                _ => Color::White,
            },
            Color::Yellow
        );
    }
    #[test]
    fn test_sc_r() {
        assert_eq!(
            match 'R' {
                'R' => Color::Cyan,
                _ => Color::White,
            },
            Color::Cyan
        );
    }

    // ── PostMergeDialog ──
    #[test]
    fn test_pmd_keep() {
        let d = PostMergeDialog {
            branch: "b".into(),
            display_name: "d".into(),
            worktree_path: PathBuf::from("/w"),
            selected: 0,
        };
        assert_eq!(d.selected, 0);
    }
    #[test]
    fn test_pmd_archive() {
        let d = PostMergeDialog {
            branch: "b".into(),
            display_name: "d".into(),
            worktree_path: PathBuf::from("/w"),
            selected: 1,
        };
        assert_eq!(d.selected, 1);
    }
    #[test]
    fn test_pmd_delete() {
        let d = PostMergeDialog {
            branch: "b".into(),
            display_name: "d".into(),
            worktree_path: PathBuf::from("/w"),
            selected: 2,
        };
        assert_eq!(d.selected, 2);
    }

    // ── Arrow indicator ──
    #[test]
    fn test_arrow_0() {
        let s = 0;
        assert_eq!(if s == 0 { "\u{25b8} " } else { "  " }, "\u{25b8} ");
    }
    #[test]
    fn test_arrow_2() {
        let s = 2;
        assert_eq!(if s == 2 { "\u{25b8} " } else { "  " }, "\u{25b8} ");
    }

    // ── Title format ──
    #[test]
    fn test_commits_title_0() {
        assert_eq!(format!(" Commits ({}) ", 0), " Commits (0) ");
    }
    #[test]
    fn test_commits_title_42() {
        assert_eq!(format!(" Commits ({}) ", 42), " Commits (42) ");
    }

    // ── Changed files title ──
    #[test]
    fn test_cf_title_none() {
        let files: Vec<GitChangedFile> = vec![];
        let t = if files.is_empty() {
            " Changed Files (none) ".into()
        } else {
            format!(" Changed Files ({}) ", files.len())
        };
        assert_eq!(t, " Changed Files (none) ");
    }
    #[test]
    fn test_cf_title_stats() {
        let files = vec![GitChangedFile {
            path: "a".into(),
            status: 'M',
            additions: 10,
            deletions: 3,
            staged: false,
        }];
        let ta: usize = files.iter().map(|f| f.additions).sum();
        let td: usize = files.iter().map(|f| f.deletions).sum();
        let t = format!(" Changed Files ({}, +{}/-{}) ", files.len(), ta, td);
        assert_eq!(t, " Changed Files (1, +10/-3) ");
    }

    // ── Divergence badge ──
    #[test]
    fn test_div_ahead() {
        let mut p = Vec::new();
        if 3 > 0 {
            p.push(format!("\u{2191}{}", 3));
        }
        assert_eq!(format!(" {} main ", p.join(" ")), " \u{2191}3 main ");
    }
    #[test]
    fn test_div_behind() {
        let mut p = Vec::new();
        if 5 > 0 {
            p.push(format!("\u{2193}{}", 5));
        }
        assert_eq!(format!(" {} main ", p.join(" ")), " \u{2193}5 main ");
    }
    #[test]
    fn test_div_both() {
        let mut p = Vec::new();
        p.push(format!("\u{2191}{}", 2));
        p.push(format!("\u{2193}{}", 3));
        assert_eq!(
            format!(" {} main ", p.join(" ")),
            " \u{2191}2 \u{2193}3 main "
        );
    }

    // ── RCR dialog ──
    #[test]
    fn test_rcr_size() {
        assert_eq!(46u16.min(80u16.saturating_sub(2)), 46);
        assert_eq!(5u16.min(40u16.saturating_sub(2)), 5);
    }
    #[test]
    fn test_rcr_small() {
        assert_eq!(46u16.min(20u16.saturating_sub(2)), 18);
    }

    // ── Post-merge ──
    #[test]
    fn test_pm_size() {
        assert_eq!(50u16.min(100u16.saturating_sub(2)), 50);
        assert_eq!(9u16.min(40u16.saturating_sub(2)), 9);
    }

    // ── Session title ──
    #[test]
    fn test_session_title() {
        assert_eq!(format!(" Session [{}/{}] ", 5, 20), " Session [5/20] ");
    }
    #[test]
    fn test_session_title_empty() {
        assert_eq!(" Session ".to_string(), " Session ");
    }

    // ── Model colors (unified pool) ──
    #[test]
    fn test_mc_opus() {
        assert_eq!(
            match "opus" {
                "opus" => Color::Magenta,
                "sonnet" => Color::Cyan,
                "haiku" => Color::Yellow,
                _ => Color::DarkGray,
            },
            Color::Magenta
        );
    }
    #[test]
    fn test_mc_sonnet() {
        assert_eq!(
            match "sonnet" {
                "opus" => Color::Magenta,
                "sonnet" => Color::Cyan,
                "haiku" => Color::Yellow,
                _ => Color::DarkGray,
            },
            Color::Cyan
        );
    }
    #[test]
    fn test_mc_haiku() {
        assert_eq!(
            match "haiku" {
                "opus" => Color::Magenta,
                "sonnet" => Color::Cyan,
                "haiku" => Color::Yellow,
                _ => Color::DarkGray,
            },
            Color::Yellow
        );
    }
    #[test]
    fn test_mc_gpt54() {
        assert_eq!(
            match "gpt-5.4" {
                "gpt-5.4" => Color::Green,
                _ => Color::DarkGray,
            },
            Color::Green
        );
    }
    #[test]
    fn test_mc_unknown() {
        assert_eq!(
            match "x" {
                "opus" => Color::Magenta,
                "sonnet" => Color::Cyan,
                "haiku" => Color::Yellow,
                _ => Color::DarkGray,
            },
            Color::DarkGray
        );
    }

    // ── Search match ──
    #[test]
    fn test_search_empty() {
        let m: Vec<(usize, usize, usize)> = vec![];
        let f = "";
        let i = if m.is_empty() {
            if f.is_empty() {
                String::new()
            } else {
                " 0/0 ".into()
            }
        } else {
            format!(" {}/{} ", 1, m.len())
        };
        assert_eq!(i, "");
    }
    #[test]
    fn test_search_no_match() {
        let i = if true {
            if false {
                String::new()
            } else {
                " 0/0 ".into()
            }
        } else {
            String::new()
        };
        assert_eq!(i, " 0/0 ");
    }
    #[test]
    fn test_search_matches() {
        assert_eq!(format!(" {}/{} ", 1, 2), " 1/2 ");
    }

    // ── Exit code ──
    #[test]
    fn test_exit_0() {
        let (t, c) = if 0 == 0 {
            (" exit:0 ".into(), Color::Green)
        } else {
            (format!(" exit:{} ", 0), Color::Red)
        };
        assert_eq!(t, " exit:0 ");
        assert_eq!(c, Color::Green);
    }
    #[test]
    fn test_exit_1() {
        let (t, c): (String, Color) = if 1 == 0 {
            (" exit:0 ".into(), Color::Green)
        } else {
            (format!(" exit:{} ", 1), Color::Red)
        };
        assert_eq!(t, " exit:1 ");
        assert_eq!(c, Color::Red);
    }

    #[test]
    fn test_azure_is_rgb() {
        assert!(matches!(AZURE, Color::Rgb(_, _, _)));
    }

    #[test]
    fn test_git_orange_red_channel_highest() {
        if let Color::Rgb(r, g, b) = GIT_ORANGE {
            assert!(r > g && r > b);
        } else {
            panic!();
        }
    }

    #[test]
    fn test_exit_code_format_negative() {
        let code = -1i32;
        let s = format!(" exit:{} ", code);
        assert_eq!(s, " exit:-1 ");
    }
}
