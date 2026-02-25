//! Sidebar rendering for Worktrees panel

use ratatui::{
    layout::{Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, List, ListItem, Paragraph},
    Frame,
};

use crate::app::{App, Focus, SidebarRowAction};
use super::keybindings;
use super::util::{truncate, GIT_BROWN, GIT_ORANGE, AZURE};

/// Build sidebar items and row→action map for mouse click handling.
/// Flat list of worktrees — no session file dropdowns or chevrons.
/// Selected item gets a highlight box (blue bg, white text) like the file tree.
fn build_sidebar_items(app: &App) -> (Vec<ListItem<'static>>, Vec<SidebarRowAction>) {
    let mut items: Vec<ListItem> = Vec::new();
    let mut row_map: Vec<SidebarRowAction> = Vec::new();
    let filter = app.sidebar_filter.to_lowercase();

    let Some(ref project) = app.project else {
        return (items, row_map);
    };

    // If project name matches filter, show everything
    let project_matches = !filter.is_empty() && project.name.to_lowercase().contains(&filter);

    for (sess_idx, session) in app.worktrees.iter().enumerate() {
        // Filter: skip worktrees that don't match
        if !filter.is_empty() && !project_matches
            && !session.name().to_lowercase().contains(&filter)
        {
            continue;
        }

        let is_selected = app.selected_worktree == Some(sess_idx);
        let status = session.status(app.is_session_running(&session.branch_name));

        // Archived get diamond icon; active worktrees use status symbol
        let (status_symbol, status_color, name_style) = if session.archived {
            let ns = if is_selected {
                Style::default().bg(Color::Blue).fg(Color::DarkGray).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::DarkGray)
            };
            ("◇", Color::DarkGray, ns)
        } else {
            let ns = if is_selected {
                Style::default().bg(Color::Blue).fg(Color::White).add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            (status.symbol(), status.color(), ns)
        };

        // Determine R indicator: Blue (RCR approval), Orange (RCR active), Green (auto-rebase on)
        let r_color = if let Some(ref rcr) = app.rcr_session {
            if rcr.branch == session.branch_name {
                if rcr.approval_pending { Some(AZURE) } else { Some(Color::Rgb(255, 165, 0)) } // blue / orange
            } else { None }
        } else { None }
        .or_else(|| {
            if app.auto_rebase_enabled.contains(&session.branch_name) { Some(Color::Green) } else { None }
        });

        // inner_width = sidebar_width - 2 (borders). Prefix " ○ " = 4 chars. "R " = 2 chars.
        let inner_w = app.pane_worktrees.width.saturating_sub(2) as usize;
        let has_r = r_color.is_some();
        let name_max = if has_r { inner_w.saturating_sub(6) } else { inner_w.saturating_sub(4) };
        let name_max = name_max.min(36);
        let name_str = truncate(session.name(), name_max);

        let mut spans = vec![
            Span::raw(" "),
            Span::styled(status_symbol, Style::default().fg(status_color)),
            Span::raw(" "),
            Span::styled(name_str.clone(), name_style),
        ];

        if let Some(rc) = r_color {
            // Right-pad name, then append "R "
            let used = 4 + name_str.len(); // " ○ " + name
            let pad = inner_w.saturating_sub(used + 2); // 2 for "R "
            if pad > 0 {
                spans.push(Span::raw(" ".repeat(pad)));
            }
            let r_style = if is_selected {
                Style::default().bg(Color::Blue).fg(rc).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(rc).add_modifier(Modifier::BOLD)
            };
            spans.push(Span::styled("R", r_style));
            spans.push(Span::raw(" "));
        }

        items.push(ListItem::new(Line::from(spans)));
        row_map.push(SidebarRowAction::Worktree(sess_idx));
    }

    (items, row_map)
}

/// Draw the sidebar showing project and sessions (or git actions + files in git mode)
pub fn draw_sidebar(f: &mut Frame, app: &mut App, area: Rect) {
    // Git panel mode — show Actions (top) + Changed Files (bottom) instead of worktree list
    if let Some(ref panel) = app.git_actions_panel {
        draw_git_sidebar(f, app, panel, area);
        return;
    }

    let is_focused = app.focus == Focus::Worktrees;

    // Only rebuild sidebar items if cache is dirty or focus changed (styling depends on focus)
    if app.sidebar_dirty || app.sidebar_focus_cached != is_focused {
        let (items, row_map) = build_sidebar_items(app);
        app.sidebar_cache = items;
        app.sidebar_row_map = row_map;
        app.sidebar_dirty = false;
        app.sidebar_focus_cached = is_focused;
    }

    // Split area: filter bar (1 line + borders = 3) at top when filter is active or has text
    let has_filter = app.sidebar_filter_active || !app.sidebar_filter.is_empty();
    let (filter_area, list_area) = if has_filter {
        let chunks = Layout::vertical([
            ratatui::layout::Constraint::Length(3),
            ratatui::layout::Constraint::Min(1),
        ]).split(area);
        (Some(chunks[0]), chunks[1])
    } else {
        (None, area)
    };

    // Draw filter input bar when active
    if let Some(fa) = filter_area {
        // Count visible worktrees (Session actions in row_map), not total rows
        let match_count = app.sidebar_row_map.iter()
            .filter(|a| matches!(a, SidebarRowAction::Worktree(_)))
            .count();
        let total = app.worktrees.len();
        let title = format!(" {}/{} ", match_count, total);
        let border_color = if app.sidebar_filter_active { Color::Yellow } else { Color::DarkGray };
        let filter_widget = Paragraph::new(app.sidebar_filter.clone())
            .block(Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(border_color))
                .title(Span::styled("🔍", Style::default()))
                .title(Line::from(Span::styled(title, Style::default().fg(Color::DarkGray))).alignment(ratatui::layout::Alignment::Right)),
            );
        f.render_widget(filter_widget, fa);

        // Show cursor in filter bar when actively typing
        if app.sidebar_filter_active {
            let cursor_x = fa.x + 1 + app.sidebar_filter.len() as u16;
            let cursor_y = fa.y + 1;
            if cursor_x < fa.right() {
                f.set_cursor_position((cursor_x, cursor_y));
            }
        }
    }

    // Show project name in border title, or main browse indicator
    let (title_text, title_color) = if app.browsing_main {
        let branch = app.project.as_ref().map(|p| p.main_branch.as_str()).unwrap_or("main");
        (format!(" ★ {} (read-only) ", branch), Color::Yellow)
    } else if let Some(ref project) = app.project {
        (format!(" {} ", project.name), if is_focused { AZURE } else { Color::White })
    } else {
        (" Worktrees ".to_string(), if is_focused { AZURE } else { Color::White })
    };

    let sidebar = List::new(app.sidebar_cache.clone())
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(if is_focused { BorderType::Double } else { BorderType::Plain })
                .title(Span::styled(title_text, Style::default().fg(title_color).add_modifier(if is_focused { Modifier::BOLD } else { Modifier::empty() })))
                .border_style(if is_focused {
                    Style::default().fg(AZURE).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::White)
                }),
        )
        .highlight_style(Style::default().add_modifier(Modifier::BOLD));

    f.render_widget(sidebar, list_area);
}

/// Draw the file tree overlay in the Worktrees pane (replaces sidebar when 'f' is pressed).
/// Delegates to draw_file_tree which handles its own caching and rendering.
pub fn draw_file_tree_overlay(f: &mut Frame, app: &mut App, area: Rect) {
    super::draw_file_tree::draw_file_tree(f, app, area);
}

/// Git panel sidebar — Actions list (top) + Changed Files (bottom)
fn draw_git_sidebar(f: &mut Frame, app: &App, panel: &crate::app::types::GitActionsPanel, area: Rect) {
    // Split vertically: actions (auto-height) | files (fill)
    let action_rows = if panel.is_on_main { 8 } else { 10 };
    let splits = Layout::vertical([
        ratatui::layout::Constraint::Length(action_rows),
        ratatui::layout::Constraint::Min(4),
    ]).split(area);
    let actions_area = splits[0];
    let files_area = splits[1];

    // ─── Actions pane (top) ──────────────────────────────────────────────────
    let actions_focused = panel.focused_pane == 0;
    let mut action_lines: Vec<Line> = Vec::new();

    let action_labels = keybindings::git_actions_labels(panel.is_on_main);
    for (i, (key, label)) in action_labels.iter().enumerate() {
        let selected = actions_focused && i == panel.selected_action;
        let prefix = if selected { " \u{25b8} " } else { "   " };
        let style = if selected {
            Style::default().fg(GIT_ORANGE).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::White)
        };
        let key_style = if selected {
            Style::default().fg(GIT_ORANGE).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(GIT_BROWN)
        };
        action_lines.push(Line::from(vec![
            Span::styled(prefix, style),
            Span::styled(format!("[{}]", key), key_style),
            Span::styled(format!(" {}", label), style),
        ]));
    }

    // Divider + toggles (feature branches get a visual separator)
    if !panel.is_on_main {
        let inner_w = actions_area.width.saturating_sub(2) as usize;
        action_lines.push(Line::from(Span::styled(
            "\u{2500}".repeat(inner_w),
            Style::default().fg(GIT_BROWN),
        )));

        let enabled = app.auto_rebase_enabled.contains(&panel.worktree_name);
        let (indicator, ind_color) = if enabled {
            ("\u{25cf} ON", Color::Green)
        } else {
            ("\u{25cb} OFF", Color::DarkGray)
        };
        action_lines.push(Line::from(vec![
            Span::styled("   ", Style::default()),
            Span::styled("[a]", Style::default().fg(GIT_BROWN)),
            Span::styled(" Auto-rebase ", Style::default().fg(Color::White)),
            Span::styled(indicator, Style::default().fg(ind_color).add_modifier(Modifier::BOLD)),
        ]));
    }

    // Main branch also gets a divider before auto-resolve
    if panel.is_on_main {
        let inner_w = actions_area.width.saturating_sub(2) as usize;
        action_lines.push(Line::from(Span::styled(
            "\u{2500}".repeat(inner_w),
            Style::default().fg(GIT_BROWN),
        )));
    }

    // Auto-resolve files count
    let ar_count = panel.auto_resolve_files.len();
    action_lines.push(Line::from(vec![
        Span::styled("   ", Style::default()),
        Span::styled("[s]", Style::default().fg(GIT_BROWN)),
        Span::styled(format!(" Auto-resolve ({})", ar_count), Style::default().fg(Color::White)),
    ]));

    let actions_block = Block::default()
        .title(Span::styled(" Actions ", Style::default()
            .fg(if actions_focused { GIT_ORANGE } else { GIT_BROWN })
            .add_modifier(if actions_focused { Modifier::BOLD } else { Modifier::empty() })))
        .borders(Borders::ALL)
        .border_type(if actions_focused { BorderType::Double } else { BorderType::Plain })
        .border_style(if actions_focused {
            Style::default().fg(GIT_ORANGE).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(GIT_BROWN)
        });
    f.render_widget(Paragraph::new(action_lines).block(actions_block), actions_area);

    // ─── Changed Files pane (bottom) ─────────────────────────────────────────
    let files_focused = panel.focused_pane == 1;
    let inner_w = files_area.width.saturating_sub(2) as usize;
    let inner_h = files_area.height.saturating_sub(2) as usize;
    let mut file_lines: Vec<Line> = Vec::new();

    // Scroll so selected file is visible
    let visible_files = inner_h;
    let scroll = if panel.selected_file < panel.file_scroll {
        panel.selected_file
    } else if panel.selected_file >= panel.file_scroll + visible_files {
        panel.selected_file.saturating_sub(visible_files.saturating_sub(1))
    } else {
        panel.file_scroll
    };

    for (i, file) in panel.changed_files.iter().enumerate().skip(scroll).take(visible_files) {
        let selected = files_focused && i == panel.selected_file;
        let prefix = if selected { " \u{25b8} " } else { "   " };

        let status_color = match file.status {
            'A' => Color::Green,
            'D' => Color::Red,
            'M' => Color::Yellow,
            'R' => Color::Cyan,
            '?' => Color::Magenta,
            _ => Color::White,
        };

        let add_str = format!("+{}", file.additions);
        let del_str = format!("-{}", file.deletions);
        let stat_len = add_str.len() + 1 + del_str.len();
        let path_budget = inner_w.saturating_sub(prefix.len() + 2 + stat_len + 1);
        let path_display = if file.path.len() > path_budget {
            format!("\u{2026}{}", &file.path[file.path.len().saturating_sub(path_budget.saturating_sub(1))..])
        } else {
            file.path.clone()
        };
        let padding = inner_w.saturating_sub(prefix.len() + 2 + path_display.len() + stat_len);

        let path_style = if selected {
            Style::default().fg(GIT_ORANGE).add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
        } else {
            Style::default().fg(Color::White).add_modifier(Modifier::UNDERLINED)
        };
        let add_style = if selected { Style::default().fg(GIT_ORANGE) } else { Style::default().fg(Color::Green) };
        let del_style = if selected { Style::default().fg(GIT_ORANGE) } else { Style::default().fg(Color::Red) };
        let slash_style = if selected { Style::default().fg(GIT_ORANGE) } else { Style::default().fg(GIT_BROWN) };

        file_lines.push(Line::from(vec![
            Span::styled(prefix, Style::default()),
            Span::styled(format!("{} ", file.status), Style::default().fg(status_color)),
            Span::styled(path_display, path_style),
            Span::raw(" ".repeat(padding)),
            Span::styled(add_str, add_style),
            Span::styled("/", slash_style),
            Span::styled(del_str, del_style),
        ]));
    }

    // Title with file count and +/- stats
    let files_title = if panel.changed_files.is_empty() {
        " Changed Files (none) ".to_string()
    } else {
        let total_add: usize = panel.changed_files.iter().map(|f| f.additions).sum();
        let total_del: usize = panel.changed_files.iter().map(|f| f.deletions).sum();
        format!(" Changed Files ({}, +{}/-{}) ", panel.changed_files.len(), total_add, total_del)
    };

    let files_block = Block::default()
        .title(Span::styled(files_title, Style::default()
            .fg(if files_focused { GIT_ORANGE } else { GIT_BROWN })
            .add_modifier(if files_focused { Modifier::BOLD } else { Modifier::empty() })))
        .borders(Borders::ALL)
        .border_type(if files_focused { BorderType::Double } else { BorderType::Plain })
        .border_style(if files_focused {
            Style::default().fg(GIT_ORANGE).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(GIT_BROWN)
        });
    f.render_widget(Paragraph::new(file_lines).block(files_block), files_area);
}
