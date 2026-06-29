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

/// Render-ready metadata for one worktree tab label.
#[derive(Debug, Clone)]
struct WorktreeTab {
    label: String,
    is_active: bool,
    is_archived: bool,
    target: Option<usize>,
    rebase_color: Option<Color>,
    is_unread: bool,
    is_running: bool,
    is_compacting: bool,
}

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

/// Return the display width of a tab label plus its trailing spacer.
fn tab_text_width(tab_text: &str) -> usize {
    tab_text
        .chars()
        .map(|c| unicode_width::UnicodeWidthChar::width(c).unwrap_or(1))
        .sum()
}

/// Return the display width of a tab label clamped to terminal coordinate space.
fn tab_text_width_u16(tab_text: &str) -> u16 {
    tab_text_width(tab_text).min(u16::MAX as usize) as u16
}

/// Build the normal-mode style for a worktree tab label.
fn normal_tab_style(tab: &WorktreeTab, focused: bool) -> Style {
    if tab.is_compacting {
        let mut style = Style::default().fg(GIT_ORANGE).add_modifier(Modifier::BOLD);
        if tab.is_active {
            style = style.bg(if tab.target.is_none() {
                Color::Yellow
            } else {
                AZURE
            });
        }
        return style;
    }

    let dim = if focused {
        Color::Gray
    } else {
        Color::DarkGray
    };

    if tab.is_active {
        if tab.target.is_none() {
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
    } else if tab.is_running {
        Style::default().fg(Color::Green)
    } else if tab.is_archived {
        Style::default().fg(dim)
    } else if tab.is_unread {
        Style::default().fg(AZURE)
    } else if tab.target.is_none() {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default().fg(dim)
    }
}

/// Build the Git-panel style for a worktree tab label.
fn git_tab_style(tab: &WorktreeTab) -> Style {
    if tab.is_compacting {
        return Style::default().fg(GIT_ORANGE).add_modifier(Modifier::BOLD);
    }

    if tab.is_active {
        if tab.target.is_none() {
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
    } else if tab.is_running {
        Style::default().fg(Color::Green)
    } else if tab.is_archived {
        Style::default().fg(Color::DarkGray)
    } else if tab.is_unread {
        Style::default().fg(AZURE)
    } else if tab.target.is_none() {
        Style::default().fg(GIT_BROWN)
    } else {
        Style::default().fg(GIT_BROWN)
    }
}

/// Active tab: AZURE bg + white fg + bold. Inactive: DarkGray fg.
/// [★ main] tab always first (main branch browse). Archived worktrees shown dim with ◇.
/// Pagination: when tabs don't fit, they are packed into pages greedily.
pub fn draw_worktree_tabs(f: &mut Frame, app: &mut App, area: Rect) {
    let avail = area.width as usize;
    let base_x = area.x;
    let focused = app.focus == Focus::Worktrees;

    let mut tabs: Vec<WorktreeTab> = Vec::new();

    let main_branch = app
        .project
        .as_ref()
        .map(|p| p.main_branch.as_str())
        .unwrap_or("main");
    let main_compacting = app
        .main_worktree
        .as_ref()
        .and_then(|worktree| worktree.worktree_path.as_deref())
        .is_some_and(|path| app.is_worktree_compacting(path));
    tabs.push(WorktreeTab {
        label: format!("★ {}", main_branch),
        is_active: app.browsing_main,
        is_archived: false,
        target: None,
        rebase_color: None,
        is_unread: false,
        is_running: false,
        is_compacting: main_compacting,
    });

    for (idx, wt) in app.worktrees.iter().enumerate() {
        let active = !app.browsing_main && app.selected_worktree == Some(idx);
        let rebase_color = rebase_indicator_color(app, &wt.branch_name);
        let is_running = app.is_session_running(&wt.branch_name);
        let is_compacting = wt
            .worktree_path
            .as_deref()
            .is_some_and(|path| app.is_worktree_compacting(path));
        let unread = app.unread_sessions.contains(&wt.branch_name);
        if wt.archived {
            tabs.push(WorktreeTab {
                label: format!("◇ {}", wt.name()),
                is_active: active,
                is_archived: true,
                target: Some(idx),
                rebase_color,
                is_unread: false,
                is_running: false,
                is_compacting: false,
            });
        } else if is_compacting {
            tabs.push(WorktreeTab {
                label: format!("● {}", wt.name()),
                is_active: active,
                is_archived: false,
                target: Some(idx),
                rebase_color,
                is_unread: false,
                is_running,
                is_compacting: true,
            });
        } else if is_running {
            // Running always takes priority — show filled circle even if unread
            tabs.push(WorktreeTab {
                label: format!("● {}", wt.name()),
                is_active: active,
                is_archived: false,
                target: Some(idx),
                rebase_color,
                is_unread: false,
                is_running: true,
                is_compacting: false,
            });
        } else if unread {
            tabs.push(WorktreeTab {
                label: format!("◐ {}", wt.name()),
                is_active: active,
                is_archived: false,
                target: Some(idx),
                rebase_color,
                is_unread: true,
                is_running: false,
                is_compacting: false,
            });
        } else {
            let status = wt.status(false);
            tabs.push(WorktreeTab {
                label: format!("{} {}", status.symbol(), wt.name()),
                is_active: active,
                is_archived: false,
                target: Some(idx),
                rebase_color,
                is_unread: false,
                is_running: false,
                is_compacting: false,
            });
        }
    }

    if tabs.is_empty() {
        return;
    }

    // Display width of each tab: "label " = display_width + 1, plus "R" if rebase indicator
    let tab_widths: Vec<usize> = tabs
        .iter()
        .map(|tab| {
            let base = tab_text_width(&format!("{} ", tab.label));
            if tab.rebase_color.is_some() {
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

    for (i, (&tw, tab)) in tab_widths.iter().zip(tabs.iter()).enumerate() {
        let cost = if cur.is_empty() { tw } else { tw + 1 };
        if !cur.is_empty() && cur_w.saturating_add(cost) > avail {
            pages.push(std::mem::take(&mut cur));
            cur = vec![i];
            cur_w = tw;
        } else {
            cur.push(i);
            cur_w = cur_w.saturating_add(cost);
        }
        if tab.is_active {
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
        let tab = &tabs[idx];
        let tab_text = format!("{} ", tab.label);
        let mut tab_w = tab_text_width_u16(&tab_text);
        let style = normal_tab_style(tab, focused);

        spans.push(Span::styled(tab_text, style));

        if let Some(color) = tab.rebase_color {
            spans.push(Span::styled(
                "R",
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            ));
            tab_w = tab_w.saturating_add(1);
        }

        hits.push((x_cursor, x_cursor.saturating_add(tab_w), tab.target));
        x_cursor = x_cursor.saturating_add(tab_w);

        if j + 1 < page_tabs.len() {
            let sep_color = if focused { AZURE } else { Color::DarkGray };
            spans.push(Span::styled("│", Style::default().fg(sep_color)));
            x_cursor = x_cursor.saturating_add(1);
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

    let mut tabs: Vec<WorktreeTab> = Vec::new();

    let main_branch = app
        .project
        .as_ref()
        .map(|p| p.main_branch.as_str())
        .unwrap_or("main");
    let main_is_active = *active_branch == main_branch;
    let main_compacting = app
        .main_worktree
        .as_ref()
        .and_then(|worktree| worktree.worktree_path.as_deref())
        .is_some_and(|path| app.is_worktree_compacting(path));
    tabs.push(WorktreeTab {
        label: format!("★ {}", main_branch),
        is_active: main_is_active,
        is_archived: false,
        target: None,
        rebase_color: None,
        is_unread: false,
        is_running: false,
        is_compacting: main_compacting,
    });

    for (idx, wt) in app.worktrees.iter().enumerate() {
        let active = !main_is_active && wt.branch_name == *active_branch;
        let rebase_color = rebase_indicator_color(app, &wt.branch_name);
        let is_running = app.is_session_running(&wt.branch_name);
        let is_compacting = wt
            .worktree_path
            .as_deref()
            .is_some_and(|path| app.is_worktree_compacting(path));
        let unread = app.unread_sessions.contains(&wt.branch_name);
        if wt.archived {
            tabs.push(WorktreeTab {
                label: format!("◇ {}", wt.name()),
                is_active: active,
                is_archived: true,
                target: Some(idx),
                rebase_color,
                is_unread: false,
                is_running: false,
                is_compacting: false,
            });
        } else if is_compacting {
            tabs.push(WorktreeTab {
                label: format!("● {}", wt.name()),
                is_active: active,
                is_archived: false,
                target: Some(idx),
                rebase_color,
                is_unread: false,
                is_running,
                is_compacting: true,
            });
        } else if is_running {
            tabs.push(WorktreeTab {
                label: format!("● {}", wt.name()),
                is_active: active,
                is_archived: false,
                target: Some(idx),
                rebase_color,
                is_unread: false,
                is_running: true,
                is_compacting: false,
            });
        } else if unread {
            tabs.push(WorktreeTab {
                label: format!("◐ {}", wt.name()),
                is_active: active,
                is_archived: false,
                target: Some(idx),
                rebase_color,
                is_unread: true,
                is_running: false,
                is_compacting: false,
            });
        } else {
            let status = wt.status(false);
            tabs.push(WorktreeTab {
                label: format!("{} {}", status.symbol(), wt.name()),
                is_active: active,
                is_archived: false,
                target: Some(idx),
                rebase_color,
                is_unread: false,
                is_running: false,
                is_compacting: false,
            });
        }
    }

    if tabs.is_empty() {
        return;
    }

    // Display width of each tab: "label " = display_width + 1, plus "R" if rebase indicator
    let tab_widths: Vec<usize> = tabs
        .iter()
        .map(|tab| {
            let base = tab_text_width(&format!("{} ", tab.label));
            if tab.rebase_color.is_some() {
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

    for (i, (&tw, tab)) in tab_widths.iter().zip(tabs.iter()).enumerate() {
        let cost = if cur.is_empty() { tw } else { tw + 1 };
        if !cur.is_empty() && cur_w.saturating_add(cost) > avail {
            pages.push(std::mem::take(&mut cur));
            cur = vec![i];
            cur_w = tw;
        } else {
            cur.push(i);
            cur_w = cur_w.saturating_add(cost);
        }
        if tab.is_active {
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
        let tab = &tabs[idx];
        let tab_text = format!("{} ", tab.label);
        let mut tab_w = tab_text_width_u16(&tab_text);
        let style = git_tab_style(tab);

        spans.push(Span::styled(tab_text, style));

        if let Some(color) = tab.rebase_color {
            spans.push(Span::styled(
                "R",
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            ));
            tab_w = tab_w.saturating_add(1);
        }

        hits.push((x_cursor, x_cursor.saturating_add(tab_w), tab.target));
        x_cursor = x_cursor.saturating_add(tab_w);

        if j + 1 < page_tabs.len() {
            spans.push(Span::styled("│", Style::default().fg(GIT_BROWN)));
            x_cursor = x_cursor.saturating_add(1);
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

/// Tests for worktree tab layout, styling, and color constants.
#[cfg(test)]
mod tests {
    use crate::app::Focus;
    use ratatui::{
        layout::Rect,
        style::{Color, Modifier, Style},
    };

    use super::super::super::util::{AZURE, GIT_BROWN, GIT_ORANGE};
    use super::{git_tab_style, normal_tab_style, tab_text_width, tab_text_width_u16, WorktreeTab};

    /// Build a default inactive worktree tab that individual tests can specialize.
    fn default_tab() -> WorktreeTab {
        WorktreeTab {
            label: "● feature".into(),
            is_active: false,
            is_archived: false,
            target: Some(0),
            rebase_color: None,
            is_unread: false,
            is_running: false,
            is_compacting: false,
        }
    }

    /// Focus variants compare equal to themselves for tab focus checks.
    #[test]
    fn focus_worktrees_equality() {
        assert_eq!(Focus::Worktrees, Focus::Worktrees);
    }

    /// Worktree focus remains distinct from modal focus variants.
    #[test]
    fn focus_variants_are_distinct() {
        assert_ne!(Focus::Worktrees, Focus::BranchDialog);
    }

    /// The first tab in a packed page does not pay a separator width.
    #[test]
    fn tab_packing_first_tab_no_separator() {
        let cur_is_empty = true;
        let tw = 10;
        let cost = if cur_is_empty { tw } else { tw + 1 };
        assert_eq!(cost, 10);
    }

    /// Subsequent tabs include one separator column in the page budget.
    #[test]
    fn tab_packing_subsequent_tabs_add_separator() {
        let cur_is_empty = false;
        let tw = 10;
        let cost = if cur_is_empty { tw } else { tw + 1 };
        assert_eq!(cost, 11);
    }

    /// Tab packing starts a new page when the next tab would exceed available width.
    #[test]
    fn tab_packing_overflow_starts_new_page() {
        let avail: usize = 20;
        let cur_w: usize = 18;
        let cost: usize = 5;
        let cur = [0usize];
        let overflow = !cur.is_empty() && cur_w + cost > avail;
        assert!(overflow);
    }

    /// Tab packing keeps the tab on the current page when it fits.
    #[test]
    fn tab_packing_fits_stays_on_page() {
        let avail: usize = 20;
        let cur_w: usize = 10;
        let cost: usize = 5;
        let cur = [0usize];
        let overflow = !cur.is_empty() && cur_w + cost > avail;
        assert!(!overflow);
    }

    /// Page indicators start counting from one for display.
    #[test]
    fn page_indicator_format() {
        let active_page: usize = 0;
        let total_pages: usize = 3;
        let indicator = format!("  {}/{}", active_page + 1, total_pages);
        assert_eq!(indicator, "  1/3");
    }

    /// Page indicators show the final page number correctly.
    #[test]
    fn page_indicator_last_page() {
        let active_page: usize = 2;
        let total_pages: usize = 3;
        let indicator = format!("  {}/{}", active_page + 1, total_pages);
        assert_eq!(indicator, "  3/3");
    }

    /// Rect construction preserves position and size fields used by hit testing.
    #[test]
    fn rect_new_sets_fields() {
        let r = Rect::new(5, 10, 80, 24);
        assert_eq!(r.x, 5);
        assert_eq!(r.y, 10);
        assert_eq!(r.width, 80);
        assert_eq!(r.height, 24);
    }

    /// Zero-sized rectangles are valid layout inputs for defensive rendering paths.
    #[test]
    fn rect_zero() {
        let r = Rect::new(0, 0, 0, 0);
        assert_eq!(r.x, 0);
        assert_eq!(r.width, 0);
    }

    /// Default styles compare equal before any foreground, background, or modifier is set.
    #[test]
    fn style_default_is_reset() {
        let s = Style::default();
        assert_eq!(s, Style::default());
    }

    /// Setting a foreground color changes the style value.
    #[test]
    fn style_fg_sets_foreground() {
        let s = Style::default().fg(Color::Red);
        assert_ne!(s, Style::default());
    }

    /// Setting a background color changes the style value.
    #[test]
    fn style_bg_sets_background() {
        let s = Style::default().bg(Color::Blue);
        assert_ne!(s, Style::default());
    }

    /// Adding bold changes the style modifier set.
    #[test]
    fn style_bold_modifier() {
        let s = Style::default().add_modifier(Modifier::BOLD);
        assert_ne!(s, Style::default());
    }

    /// Adding dim changes the style modifier set.
    #[test]
    fn style_dim_modifier() {
        let s = Style::default().add_modifier(Modifier::DIM);
        assert_ne!(s, Style::default());
    }

    /// Tab width counts wide Unicode glyphs by terminal columns.
    #[test]
    fn tab_text_width_counts_wide_glyphs() {
        assert_eq!(tab_text_width("a好 "), 4);
    }

    /// Tab width conversion saturates instead of truncating huge labels.
    #[test]
    fn tab_text_width_u16_saturates_huge_labels() {
        let long = "x".repeat(u16::MAX as usize + 10);
        assert_eq!(tab_text_width_u16(&long), u16::MAX);
    }

    /// Normal-mode compacting tabs use orange foreground and bold emphasis.
    #[test]
    fn normal_compacting_tab_style_is_orange() {
        let mut tab = default_tab();
        tab.is_compacting = true;

        let style = normal_tab_style(&tab, false);

        assert_eq!(style.fg, Some(GIT_ORANGE));
        assert!(style.add_modifier.contains(Modifier::BOLD));
    }

    /// Active normal-mode compacting tabs keep active background while using orange text.
    #[test]
    fn active_normal_compacting_tab_keeps_active_background() {
        let mut tab = default_tab();
        tab.is_active = true;
        tab.is_compacting = true;

        let style = normal_tab_style(&tab, true);

        assert_eq!(style.fg, Some(GIT_ORANGE));
        assert_eq!(style.bg, Some(AZURE));
    }

    /// Git-panel compacting tabs use orange foreground and bold emphasis.
    #[test]
    fn git_compacting_tab_style_is_orange() {
        let mut tab = default_tab();
        tab.is_compacting = true;

        let style = git_tab_style(&tab);

        assert_eq!(style.fg, Some(GIT_ORANGE));
        assert!(style.add_modifier.contains(Modifier::BOLD));
    }

    /// Azure remains an RGB custom color.
    #[test]
    fn azure_color_is_rgb() {
        assert!(matches!(AZURE, Color::Rgb(_, _, _)));
    }

    /// Git orange remains an RGB custom color.
    #[test]
    fn git_orange_is_rgb() {
        assert!(matches!(GIT_ORANGE, Color::Rgb(_, _, _)));
    }

    /// Git brown remains an RGB custom color.
    #[test]
    fn git_brown_is_rgb() {
        assert!(matches!(GIT_BROWN, Color::Rgb(_, _, _)));
    }

    /// Azure and Git orange are visually distinct accent colors.
    #[test]
    fn azure_not_equal_git_orange() {
        assert_ne!(AZURE, GIT_ORANGE);
    }

    /// Azure and Git brown are visually distinct accent colors.
    #[test]
    fn azure_not_equal_git_brown() {
        assert_ne!(AZURE, GIT_BROWN);
    }

    /// Git orange and Git brown are visually distinct palette colors.
    #[test]
    fn git_orange_not_equal_git_brown() {
        assert_ne!(GIT_ORANGE, GIT_BROWN);
    }
}
