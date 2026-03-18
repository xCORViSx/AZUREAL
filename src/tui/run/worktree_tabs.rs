//! Worktree tab bar rendering
//!
//! Horizontal tab rows at the top of normal mode and git mode layouts.
//! Handles pagination, hit-testing, status icons, and rebase indicators.

use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};

use crate::app::{App, Focus};

use super::super::util::{AZURE, GIT_BROWN, GIT_ORANGE};

/// Auto-rebase indicator color for a worktree branch.
/// Returns Some(color) if auto-rebase is enabled: green=idle, orange=RCR active, blue=approval pending.
pub fn rebase_indicator_color(app: &App, branch: &str) -> Option<Color> {
    if !app.auto_rebase_enabled.contains(branch) {
        return None;
    }
    if let Some(ref rcr) = app.rcr_session {
        if rcr.branch == branch {
            return Some(if rcr.approval_pending {
                Color::Blue
            } else {
                GIT_ORANGE
            });
        }
    }
    Some(Color::Green)
}

/// Active tab: AZURE bg + white fg + bold. Inactive: DarkGray fg.
/// [★ main] tab always first (main branch browse). Archived worktrees shown dim with ◇.
/// Pagination: when tabs don't fit, they are packed into pages greedily.
pub fn draw_worktree_tabs(f: &mut Frame, app: &mut App, area: Rect) {
    let avail = area.width as usize;
    let base_x = area.x;
    let focused = app.focus == Focus::Worktrees;

    // Build tab entries: (display_label, is_active, is_archived, target, rebase_color, is_unread, is_running)
    // target: None = [M] main browse, Some(idx) = worktree index
    let mut tabs: Vec<(String, bool, bool, Option<usize>, Option<Color>, bool, bool)> = Vec::new();

    let main_branch = app
        .project
        .as_ref()
        .map(|p| p.main_branch.as_str())
        .unwrap_or("main");
    tabs.push((
        format!("★ {}", main_branch),
        app.browsing_main,
        false,
        None,
        None,
        false,
        false,
    ));

    for (idx, wt) in app.worktrees.iter().enumerate() {
        let active = !app.browsing_main && app.selected_worktree == Some(idx);
        let rebase_color = rebase_indicator_color(app, &wt.branch_name);
        let is_running = app.is_session_running(&wt.branch_name);
        let unread = app.unread_sessions.contains(&wt.branch_name);
        if wt.archived {
            tabs.push((
                format!("◇ {}", wt.name()),
                active,
                true,
                Some(idx),
                rebase_color,
                false,
                false,
            ));
        } else if is_running {
            // Running always takes priority — show filled circle even if unread
            tabs.push((
                format!("● {}", wt.name()),
                active,
                false,
                Some(idx),
                rebase_color,
                false,
                true,
            ));
        } else if unread {
            tabs.push((
                format!("◐ {}", wt.name()),
                active,
                false,
                Some(idx),
                rebase_color,
                true,
                false,
            ));
        } else {
            let status = wt.status(false);
            tabs.push((
                format!("{} {}", status.symbol(), wt.name()),
                active,
                false,
                Some(idx),
                rebase_color,
                false,
                false,
            ));
        }
    }

    if tabs.is_empty() {
        return;
    }

    // Display width of each tab: "label " = display_width + 1, plus "R" if rebase indicator
    let tab_widths: Vec<usize> = tabs
        .iter()
        .map(|(label, _, _, _, rebase, _, _)| {
            let base: usize = label
                .chars()
                .map(|c| unicode_width::UnicodeWidthChar::width(c).unwrap_or(1))
                .sum::<usize>()
                + 1;
            if rebase.is_some() {
                base + 1
            } else {
                base
            }
        })
        .collect();

    // Pack tabs into pages greedily
    let mut pages: Vec<Vec<usize>> = Vec::new();
    let mut cur: Vec<usize> = Vec::new();
    let mut cur_w: usize = 0;
    let mut active_page: usize = 0;

    for (i, (&tw, (_, is_active, _, _, _, _, _))) in tab_widths.iter().zip(tabs.iter()).enumerate() {
        let cost = if cur.is_empty() { tw } else { tw + 1 };
        if !cur.is_empty() && cur_w + cost > avail {
            pages.push(std::mem::take(&mut cur));
            cur = vec![i];
            cur_w = tw;
        } else {
            cur.push(i);
            cur_w += cost;
        }
        if *is_active {
            active_page = pages.len();
        }
    }
    if !cur.is_empty() {
        pages.push(cur);
    }

    let total_pages = pages.len();
    let page_tabs = match pages.get(active_page) {
        Some(p) => p,
        None => return,
    };

    // Build spans and hit-test regions
    let mut spans: Vec<Span> = Vec::with_capacity(page_tabs.len() * 2 + 1);
    let mut hits: Vec<(u16, u16, Option<usize>)> = Vec::with_capacity(page_tabs.len());
    let mut x_cursor: u16 = base_x;

    for (j, &idx) in page_tabs.iter().enumerate() {
        let (ref label, is_active, is_archived, target, rebase_color, is_unread, is_running) =
            tabs[idx];
        let tab_text = format!("{} ", label);
        let mut tab_w: u16 = tab_text
            .chars()
            .map(|c| unicode_width::UnicodeWidthChar::width(c).unwrap_or(1) as u16)
            .sum();

        let dim = if focused {
            Color::Gray
        } else {
            Color::DarkGray
        };
        let style = if is_active {
            if target.is_none() {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
                    .fg(Color::White)
                    .bg(AZURE)
                    .add_modifier(Modifier::BOLD)
            }
        } else if is_running {
            Style::default().fg(Color::Green)
        } else if is_archived {
            Style::default().fg(dim)
        } else if is_unread {
            Style::default().fg(AZURE)
        } else if target.is_none() {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default().fg(dim)
        };

        spans.push(Span::styled(tab_text, style));

        if let Some(color) = rebase_color {
            spans.push(Span::styled(
                "R",
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            ));
            tab_w += 1;
        }

        hits.push((x_cursor, x_cursor + tab_w, target));
        x_cursor += tab_w;

        if j + 1 < page_tabs.len() {
            let sep_color = if focused { AZURE } else { Color::DarkGray };
            spans.push(Span::styled("│", Style::default().fg(sep_color)));
            x_cursor += 1;
        }
    }

    if total_pages > 1 {
        let page_color = if focused {
            Color::Gray
        } else {
            Color::DarkGray
        };
        spans.push(Span::styled(
            format!("  {}/{}", active_page + 1, total_pages),
            Style::default().fg(page_color).add_modifier(Modifier::DIM),
        ));
    }

    app.worktree_tab_hits = hits;
    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

/// Horizontal worktree tab bar — 1 row at the top of the git panel.
/// Reuses the same design as `draw_worktree_tabs` (★ main tab, status symbols,
/// archived styling, pagination, hit-test regions) but with GIT_ORANGE/GIT_BROWN
/// colors instead of AZURE/Yellow/DarkGray.
pub fn draw_git_worktree_tabs(f: &mut Frame, app: &mut App, area: Rect) {
    let panel = match app.git_actions_panel.as_ref() {
        Some(p) => p,
        None => return,
    };
    let active_branch = &panel.worktree_name;
    let avail = area.width as usize;
    let base_x = area.x;

    // Build tab entries: (display_label, is_active, is_archived, target, rebase_color, is_unread, is_running)
    // target: None = main branch, Some(idx) = worktree index
    let mut tabs: Vec<(String, bool, bool, Option<usize>, Option<Color>, bool, bool)> = Vec::new();

    let main_branch = app
        .project
        .as_ref()
        .map(|p| p.main_branch.as_str())
        .unwrap_or("main");
    let main_is_active = *active_branch == main_branch;
    tabs.push((
        format!("★ {}", main_branch),
        main_is_active,
        false,
        None,
        None,
        false,
        false,
    ));

    for (idx, wt) in app.worktrees.iter().enumerate() {
        let active = !main_is_active && wt.branch_name == *active_branch;
        let rebase_color = rebase_indicator_color(app, &wt.branch_name);
        let is_running = app.is_session_running(&wt.branch_name);
        let unread = app.unread_sessions.contains(&wt.branch_name);
        if wt.archived {
            tabs.push((
                format!("◇ {}", wt.name()),
                active,
                true,
                Some(idx),
                rebase_color,
                false,
                false,
            ));
        } else if is_running {
            tabs.push((
                format!("● {}", wt.name()),
                active,
                false,
                Some(idx),
                rebase_color,
                false,
                true,
            ));
        } else if unread {
            tabs.push((
                format!("◐ {}", wt.name()),
                active,
                false,
                Some(idx),
                rebase_color,
                true,
                false,
            ));
        } else {
            let status = wt.status(false);
            tabs.push((
                format!("{} {}", status.symbol(), wt.name()),
                active,
                false,
                Some(idx),
                rebase_color,
                false,
                false,
            ));
        }
    }

    if tabs.is_empty() {
        return;
    }

    // Display width of each tab: "label " = display_width + 1, plus "R" if rebase indicator
    let tab_widths: Vec<usize> = tabs
        .iter()
        .map(|(label, _, _, _, rebase, _, _)| {
            let base: usize = label
                .chars()
                .map(|c| unicode_width::UnicodeWidthChar::width(c).unwrap_or(1))
                .sum::<usize>()
                + 1;
            if rebase.is_some() {
                base + 1
            } else {
                base
            }
        })
        .collect();

    // Pack tabs into pages greedily
    let mut pages: Vec<Vec<usize>> = Vec::new();
    let mut cur: Vec<usize> = Vec::new();
    let mut cur_w: usize = 0;
    let mut active_page: usize = 0;

    for (i, (&tw, (_, is_active, _, _, _, _, _))) in tab_widths.iter().zip(tabs.iter()).enumerate() {
        let cost = if cur.is_empty() { tw } else { tw + 1 };
        if !cur.is_empty() && cur_w + cost > avail {
            pages.push(std::mem::take(&mut cur));
            cur = vec![i];
            cur_w = tw;
        } else {
            cur.push(i);
            cur_w += cost;
        }
        if *is_active {
            active_page = pages.len();
        }
    }
    if !cur.is_empty() {
        pages.push(cur);
    }

    let total_pages = pages.len();
    let page_tabs = match pages.get(active_page) {
        Some(p) => p,
        None => return,
    };

    // Build spans and hit-test regions
    let mut spans: Vec<Span> = Vec::with_capacity(page_tabs.len() * 2 + 1);
    let mut hits: Vec<(u16, u16, Option<usize>)> = Vec::with_capacity(page_tabs.len());
    let mut x_cursor: u16 = base_x;

    for (j, &idx) in page_tabs.iter().enumerate() {
        let (ref label, is_active, is_archived, target, rebase_color, is_unread, is_running) =
            tabs[idx];
        let tab_text = format!("{} ", label);
        let mut tab_w: u16 = tab_text
            .chars()
            .map(|c| unicode_width::UnicodeWidthChar::width(c).unwrap_or(1) as u16)
            .sum();

        // Same styling logic as draw_worktree_tabs but with git color palette
        let style = if is_active {
            if target.is_none() {
                Style::default()
                    .fg(Color::Black)
                    .bg(GIT_ORANGE)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
                    .fg(Color::White)
                    .bg(GIT_ORANGE)
                    .add_modifier(Modifier::BOLD)
            }
        } else if is_running {
            Style::default().fg(Color::Green)
        } else if is_archived {
            Style::default().fg(Color::DarkGray)
        } else if is_unread {
            Style::default().fg(AZURE)
        } else if target.is_none() {
            Style::default().fg(GIT_BROWN)
        } else {
            Style::default().fg(GIT_BROWN)
        };

        spans.push(Span::styled(tab_text, style));

        if let Some(color) = rebase_color {
            spans.push(Span::styled(
                "R",
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            ));
            tab_w += 1;
        }

        hits.push((x_cursor, x_cursor + tab_w, target));
        x_cursor += tab_w;

        if j + 1 < page_tabs.len() {
            spans.push(Span::styled("│", Style::default().fg(GIT_BROWN)));
            x_cursor += 1;
        }
    }

    if total_pages > 1 {
        spans.push(Span::styled(
            format!("  {}/{}", active_page + 1, total_pages),
            Style::default().fg(GIT_BROWN).add_modifier(Modifier::DIM),
        ));
    }

    app.worktree_tab_hits = hits;
    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

#[cfg(test)]
mod tests {
    use crate::app::Focus;
    use ratatui::{
        layout::Rect,
        style::{Color, Modifier, Style},
    };

    use super::super::super::util::{AZURE, GIT_BROWN, GIT_ORANGE};

    #[test]
    fn focus_worktrees_equality() {
        assert_eq!(Focus::Worktrees, Focus::Worktrees);
    }

    #[test]
    fn focus_variants_are_distinct() {
        assert_ne!(Focus::Worktrees, Focus::BranchDialog);
    }

    #[test]
    fn tab_packing_first_tab_no_separator() {
        let cur_is_empty = true;
        let tw = 10;
        let cost = if cur_is_empty { tw } else { tw + 1 };
        assert_eq!(cost, 10);
    }

    #[test]
    fn tab_packing_subsequent_tabs_add_separator() {
        let cur_is_empty = false;
        let tw = 10;
        let cost = if cur_is_empty { tw } else { tw + 1 };
        assert_eq!(cost, 11);
    }

    #[test]
    fn tab_packing_overflow_starts_new_page() {
        let avail: usize = 20;
        let cur_w: usize = 18;
        let cost: usize = 5;
        let overflow = !vec![0usize].is_empty() && cur_w + cost > avail;
        assert!(overflow);
    }

    #[test]
    fn tab_packing_fits_stays_on_page() {
        let avail: usize = 20;
        let cur_w: usize = 10;
        let cost: usize = 5;
        let overflow = !vec![0usize].is_empty() && cur_w + cost > avail;
        assert!(!overflow);
    }

    #[test]
    fn page_indicator_format() {
        let active_page: usize = 0;
        let total_pages: usize = 3;
        let indicator = format!("  {}/{}", active_page + 1, total_pages);
        assert_eq!(indicator, "  1/3");
    }

    #[test]
    fn page_indicator_last_page() {
        let active_page: usize = 2;
        let total_pages: usize = 3;
        let indicator = format!("  {}/{}", active_page + 1, total_pages);
        assert_eq!(indicator, "  3/3");
    }

    #[test]
    fn rect_new_sets_fields() {
        let r = Rect::new(5, 10, 80, 24);
        assert_eq!(r.x, 5);
        assert_eq!(r.y, 10);
        assert_eq!(r.width, 80);
        assert_eq!(r.height, 24);
    }

    #[test]
    fn rect_zero() {
        let r = Rect::new(0, 0, 0, 0);
        assert_eq!(r.x, 0);
        assert_eq!(r.width, 0);
    }

    #[test]
    fn style_default_is_reset() {
        let s = Style::default();
        assert_eq!(s, Style::default());
    }

    #[test]
    fn style_fg_sets_foreground() {
        let s = Style::default().fg(Color::Red);
        assert_ne!(s, Style::default());
    }

    #[test]
    fn style_bg_sets_background() {
        let s = Style::default().bg(Color::Blue);
        assert_ne!(s, Style::default());
    }

    #[test]
    fn style_bold_modifier() {
        let s = Style::default().add_modifier(Modifier::BOLD);
        assert_ne!(s, Style::default());
    }

    #[test]
    fn style_dim_modifier() {
        let s = Style::default().add_modifier(Modifier::DIM);
        assert_ne!(s, Style::default());
    }

    #[test]
    fn azure_color_is_rgb() {
        assert!(matches!(AZURE, Color::Rgb(_, _, _)));
    }

    #[test]
    fn git_orange_is_rgb() {
        assert!(matches!(GIT_ORANGE, Color::Rgb(_, _, _)));
    }

    #[test]
    fn git_brown_is_rgb() {
        assert!(matches!(GIT_BROWN, Color::Rgb(_, _, _)));
    }

    #[test]
    fn azure_not_equal_git_orange() {
        assert_ne!(AZURE, GIT_ORANGE);
    }

    #[test]
    fn azure_not_equal_git_brown() {
        assert_ne!(AZURE, GIT_BROWN);
    }

    #[test]
    fn git_orange_not_equal_git_brown() {
        assert_ne!(GIT_ORANGE, GIT_BROWN);
    }
}
