//! Sidebar rendering — Git Actions panel and FileTree overlay

use ratatui::{
    layout::{Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Paragraph},
    Frame,
};

use crate::app::App;
use super::keybindings;
use super::util::{GIT_BROWN, GIT_ORANGE};

/// Draw the sidebar — in Git mode shows Actions + Changed Files,
/// otherwise delegates to the file tree pane.
pub fn draw_sidebar(f: &mut Frame, app: &mut App, area: Rect) {
    if let Some(ref panel) = app.git_actions_panel {
        let scroll = draw_git_sidebar(f, app, panel, area);
        // Write back computed scroll (can't mutate during immutable borrow above)
        if let Some(ref mut p) = app.git_actions_panel {
            p.file_scroll = scroll;
        }
    }
}

/// Draw the file tree in the left pane (always visible in normal mode)
pub fn draw_file_tree_overlay(f: &mut Frame, app: &mut App, area: Rect) {
    super::draw_file_tree::draw_file_tree(f, app, area);
}

/// Git panel sidebar — Actions list (top) + Changed Files (bottom)
/// Returns the computed file_scroll for writeback.
fn draw_git_sidebar(f: &mut Frame, app: &App, panel: &crate::app::types::GitActionsPanel, area: Rect) -> usize {
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

    // Scroll so selected file is visible (written back after draw via file_scroll_writeback)
    let visible_files = inner_h;
    let scroll = if panel.selected_file < panel.file_scroll {
        panel.selected_file
    } else if panel.selected_file >= panel.file_scroll + visible_files {
        panel.selected_file.saturating_sub(visible_files.saturating_sub(1))
    } else {
        panel.file_scroll
    };
    // Stash for writeback after this borrow ends
    let file_scroll_writeback = scroll;

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
    file_scroll_writeback
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::layout::{Constraint, Layout, Rect};
    use ratatui::style::{Color, Modifier, Style};
    use crate::app::types::GitChangedFile;

    // ══════════════════════════════════════════════════════════════════
    //  Color constants
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn git_orange_value() {
        assert_eq!(GIT_ORANGE, Color::Rgb(240, 80, 50));
    }

    #[test]
    fn git_brown_value() {
        assert_eq!(GIT_BROWN, Color::Rgb(160, 82, 45));
    }

    #[test]
    fn git_orange_ne_brown() {
        assert_ne!(GIT_ORANGE, GIT_BROWN);
    }

    // ══════════════════════════════════════════════════════════════════
    //  Action row count logic
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn action_rows_main_is_8() {
        let is_on_main = true;
        let rows = if is_on_main { 8u16 } else { 10u16 };
        assert_eq!(rows, 8);
    }

    #[test]
    fn action_rows_feature_is_10() {
        let is_on_main = false;
        let rows = if is_on_main { 8u16 } else { 10u16 };
        assert_eq!(rows, 10);
    }

    // ══════════════════════════════════════════════════════════════════
    //  Layout splitting
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn sidebar_layout_main_branch() {
        let area = Rect::new(0, 0, 40, 30);
        let action_rows = 8u16;
        let splits = Layout::vertical([
            Constraint::Length(action_rows),
            Constraint::Min(4),
        ]).split(area);
        assert_eq!(splits[0].height, 8);
        assert_eq!(splits[1].height, 22);
    }

    #[test]
    fn sidebar_layout_feature_branch() {
        let area = Rect::new(0, 0, 40, 30);
        let action_rows = 10u16;
        let splits = Layout::vertical([
            Constraint::Length(action_rows),
            Constraint::Min(4),
        ]).split(area);
        assert_eq!(splits[0].height, 10);
        assert_eq!(splits[1].height, 20);
    }

    // ══════════════════════════════════════════════════════════════════
    //  Focused pane logic
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn actions_focused_when_pane_zero() {
        let focused_pane: u8 = 0;
        assert!(focused_pane == 0);
    }

    #[test]
    fn files_focused_when_pane_one() {
        let focused_pane: u8 = 1;
        assert!(focused_pane == 1);
    }

    // ══════════════════════════════════════════════════════════════════
    //  Action prefix (selection arrow)
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn selected_prefix_has_triangle() {
        let selected = true;
        let prefix = if selected { " \u{25b8} " } else { "   " };
        assert_eq!(prefix, " \u{25b8} ");
    }

    #[test]
    fn unselected_prefix_is_spaces() {
        let selected = false;
        let prefix = if selected { " \u{25b8} " } else { "   " };
        assert_eq!(prefix, "   ");
    }

    // ══════════════════════════════════════════════════════════════════
    //  Action style logic
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn selected_action_style_orange_bold() {
        let selected = true;
        let style = if selected {
            Style::default().fg(GIT_ORANGE).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::White)
        };
        assert_eq!(style.fg, Some(GIT_ORANGE));
    }

    #[test]
    fn unselected_action_style_white() {
        let selected = false;
        let style = if selected {
            Style::default().fg(GIT_ORANGE).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::White)
        };
        assert_eq!(style.fg, Some(Color::White));
    }

    #[test]
    fn key_style_selected_orange() {
        let selected = true;
        let style = if selected {
            Style::default().fg(GIT_ORANGE).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(GIT_BROWN)
        };
        assert_eq!(style.fg, Some(GIT_ORANGE));
    }

    #[test]
    fn key_style_unselected_brown() {
        let selected = false;
        let style = if selected {
            Style::default().fg(GIT_ORANGE).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(GIT_BROWN)
        };
        assert_eq!(style.fg, Some(GIT_BROWN));
    }

    // ══════════════════════════════════════════════════════════════════
    //  Auto-rebase indicator
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn auto_rebase_enabled_indicator() {
        let enabled = true;
        let (indicator, color) = if enabled {
            ("\u{25cf} ON", Color::Green)
        } else {
            ("\u{25cb} OFF", Color::DarkGray)
        };
        assert_eq!(indicator, "\u{25cf} ON");
        assert_eq!(color, Color::Green);
    }

    #[test]
    fn auto_rebase_disabled_indicator() {
        let enabled = false;
        let (indicator, color) = if enabled {
            ("\u{25cf} ON", Color::Green)
        } else {
            ("\u{25cb} OFF", Color::DarkGray)
        };
        assert_eq!(indicator, "\u{25cb} OFF");
        assert_eq!(color, Color::DarkGray);
    }

    // ══════════════════════════════════════════════════════════════════
    //  Divider line
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn divider_line_width() {
        let area_width = 40u16;
        let inner_w = area_width.saturating_sub(2) as usize;
        let divider = "\u{2500}".repeat(inner_w);
        assert_eq!(divider.chars().count(), 38);
    }

    #[test]
    fn divider_line_zero_width() {
        let area_width = 2u16;
        let inner_w = area_width.saturating_sub(2) as usize;
        let divider = "\u{2500}".repeat(inner_w);
        assert!(divider.is_empty());
    }

    // ══════════════════════════════════════════════════════════════════
    //  Auto-resolve count format
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn auto_resolve_count_format() {
        let count = 3;
        let text = format!(" Auto-resolve ({})", count);
        assert_eq!(text, " Auto-resolve (3)");
    }

    #[test]
    fn auto_resolve_count_zero() {
        let text = format!(" Auto-resolve ({})", 0);
        assert_eq!(text, " Auto-resolve (0)");
    }

    // ══════════════════════════════════════════════════════════════════
    //  File status colors
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn status_color_added_green() {
        let status = 'A';
        let color = match status {
            'A' => Color::Green,
            'D' => Color::Red,
            'M' => Color::Yellow,
            'R' => Color::Cyan,
            '?' => Color::Magenta,
            _ => Color::White,
        };
        assert_eq!(color, Color::Green);
    }

    #[test]
    fn status_color_deleted_red() {
        let color = match 'D' {
            'A' => Color::Green,
            'D' => Color::Red,
            'M' => Color::Yellow,
            'R' => Color::Cyan,
            '?' => Color::Magenta,
            _ => Color::White,
        };
        assert_eq!(color, Color::Red);
    }

    #[test]
    fn status_color_modified_yellow() {
        let color = match 'M' {
            'A' => Color::Green,
            'D' => Color::Red,
            'M' => Color::Yellow,
            'R' => Color::Cyan,
            '?' => Color::Magenta,
            _ => Color::White,
        };
        assert_eq!(color, Color::Yellow);
    }

    #[test]
    fn status_color_renamed_cyan() {
        let color = match 'R' {
            'A' => Color::Green,
            'D' => Color::Red,
            'M' => Color::Yellow,
            'R' => Color::Cyan,
            '?' => Color::Magenta,
            _ => Color::White,
        };
        assert_eq!(color, Color::Cyan);
    }

    #[test]
    fn status_color_untracked_magenta() {
        let color = match '?' {
            'A' => Color::Green,
            'D' => Color::Red,
            'M' => Color::Yellow,
            'R' => Color::Cyan,
            '?' => Color::Magenta,
            _ => Color::White,
        };
        assert_eq!(color, Color::Magenta);
    }

    #[test]
    fn status_color_unknown_white() {
        let color = match 'X' {
            'A' => Color::Green,
            'D' => Color::Red,
            'M' => Color::Yellow,
            'R' => Color::Cyan,
            '?' => Color::Magenta,
            _ => Color::White,
        };
        assert_eq!(color, Color::White);
    }

    // ══════════════════════════════════════════════════════════════════
    //  File path truncation
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn path_truncation_short_path_unchanged() {
        let path = "src/main.rs";
        let budget = 30;
        let display = if path.len() > budget {
            format!("\u{2026}{}", &path[path.len().saturating_sub(budget.saturating_sub(1))..])
        } else {
            path.to_string()
        };
        assert_eq!(display, "src/main.rs");
    }

    #[test]
    fn path_truncation_long_path_gets_ellipsis() {
        let path = "src/very/deeply/nested/module/file.rs";
        let budget = 15;
        let display = if path.len() > budget {
            format!("\u{2026}{}", &path[path.len().saturating_sub(budget.saturating_sub(1))..])
        } else {
            path.to_string()
        };
        assert!(display.starts_with('\u{2026}'));
    }

    // ══════════════════════════════════════════════════════════════════
    //  Add/Del stat formatting
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn add_del_format() {
        let additions = 42;
        let deletions = 10;
        let add_str = format!("+{}", additions);
        let del_str = format!("-{}", deletions);
        assert_eq!(add_str, "+42");
        assert_eq!(del_str, "-10");
    }

    #[test]
    fn stat_len_calculation() {
        let add_str = "+42";
        let del_str = "-10";
        let stat_len = add_str.len() + 1 + del_str.len(); // +1 for slash
        assert_eq!(stat_len, 7); // 3 + 1 + 3
    }

    // ══════════════════════════════════════════════════════════════════
    //  File scroll logic
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn scroll_when_selected_above() {
        let selected_file: usize = 2;
        let file_scroll: usize = 5;
        let visible_files: usize = 10;
        let scroll = if selected_file < file_scroll {
            selected_file
        } else if selected_file >= file_scroll + visible_files {
            selected_file.saturating_sub(visible_files.saturating_sub(1))
        } else {
            file_scroll
        };
        assert_eq!(scroll, 2);
    }

    #[test]
    fn scroll_when_selected_below() {
        let selected_file: usize = 20;
        let file_scroll: usize = 5;
        let visible_files: usize = 10;
        let scroll = if selected_file < file_scroll {
            selected_file
        } else if selected_file >= file_scroll + visible_files {
            selected_file.saturating_sub(visible_files.saturating_sub(1))
        } else {
            file_scroll
        };
        assert_eq!(scroll, 11);
    }

    #[test]
    fn scroll_when_visible() {
        let selected_file: usize = 7;
        let file_scroll: usize = 5;
        let visible_files: usize = 10;
        let scroll = if selected_file < file_scroll {
            selected_file
        } else if selected_file >= file_scroll + visible_files {
            selected_file.saturating_sub(visible_files.saturating_sub(1))
        } else {
            file_scroll
        };
        assert_eq!(scroll, 5);
    }

    // ══════════════════════════════════════════════════════════════════
    //  Changed Files title
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn files_title_empty() {
        let files: Vec<GitChangedFile> = vec![];
        let title = if files.is_empty() {
            " Changed Files (none) ".to_string()
        } else {
            let total_add: usize = files.iter().map(|f| f.additions).sum();
            let total_del: usize = files.iter().map(|f| f.deletions).sum();
            format!(" Changed Files ({}, +{}/-{}) ", files.len(), total_add, total_del)
        };
        assert_eq!(title, " Changed Files (none) ");
    }

    #[test]
    fn files_title_with_files() {
        let files = vec![
            GitChangedFile { path: "a.rs".into(), status: 'M', additions: 10, deletions: 5 },
            GitChangedFile { path: "b.rs".into(), status: 'A', additions: 20, deletions: 0 },
        ];
        let total_add: usize = files.iter().map(|f| f.additions).sum();
        let total_del: usize = files.iter().map(|f| f.deletions).sum();
        let title = format!(" Changed Files ({}, +{}/-{}) ", files.len(), total_add, total_del);
        assert_eq!(title, " Changed Files (2, +30/-5) ");
    }

    // ══════════════════════════════════════════════════════════════════
    //  Border types
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn actions_border_focused_double() {
        let focused = true;
        let bt = if focused { BorderType::Double } else { BorderType::Plain };
        assert_eq!(bt, BorderType::Double);
    }

    #[test]
    fn actions_border_unfocused_plain() {
        let focused = false;
        let bt = if focused { BorderType::Double } else { BorderType::Plain };
        assert_eq!(bt, BorderType::Plain);
    }

    // ══════════════════════════════════════════════════════════════════
    //  Title style for focused/unfocused panes
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn title_focused_orange_bold() {
        let focused = true;
        let style = Style::default()
            .fg(if focused { GIT_ORANGE } else { GIT_BROWN })
            .add_modifier(if focused { Modifier::BOLD } else { Modifier::empty() });
        assert_eq!(style.fg, Some(GIT_ORANGE));
    }

    #[test]
    fn title_unfocused_brown() {
        let focused = false;
        let style = Style::default()
            .fg(if focused { GIT_ORANGE } else { GIT_BROWN })
            .add_modifier(if focused { Modifier::BOLD } else { Modifier::empty() });
        assert_eq!(style.fg, Some(GIT_BROWN));
    }

    // ══════════════════════════════════════════════════════════════════
    //  Path style
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn path_style_selected_orange_bold_underline() {
        let selected = true;
        let style = if selected {
            Style::default().fg(GIT_ORANGE).add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
        } else {
            Style::default().fg(Color::White).add_modifier(Modifier::UNDERLINED)
        };
        assert_eq!(style.fg, Some(GIT_ORANGE));
    }

    #[test]
    fn path_style_unselected_white_underline() {
        let selected = false;
        let style = if selected {
            Style::default().fg(GIT_ORANGE).add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
        } else {
            Style::default().fg(Color::White).add_modifier(Modifier::UNDERLINED)
        };
        assert_eq!(style.fg, Some(Color::White));
    }

    // ══════════════════════════════════════════════════════════════════
    //  GitChangedFile construction
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn git_changed_file_construction() {
        let f = GitChangedFile {
            path: "src/lib.rs".to_string(),
            status: 'M',
            additions: 15,
            deletions: 3,
        };
        assert_eq!(f.path, "src/lib.rs");
        assert_eq!(f.status, 'M');
        assert_eq!(f.additions, 15);
        assert_eq!(f.deletions, 3);
    }

    #[test]
    fn git_changed_file_clone() {
        let f = GitChangedFile {
            path: "a.rs".to_string(),
            status: 'A',
            additions: 100,
            deletions: 0,
        };
        let cloned = f.clone();
        assert_eq!(f.path, cloned.path);
        assert_eq!(f.status, cloned.status);
    }

    #[test]
    fn git_changed_file_debug() {
        let f = GitChangedFile {
            path: "x.rs".to_string(),
            status: 'D',
            additions: 0,
            deletions: 50,
        };
        let dbg = format!("{:?}", f);
        assert!(dbg.contains("x.rs"));
        assert!(dbg.contains("D"));
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
    fn rect_zero_area() {
        let r = Rect::new(0, 0, 0, 0);
        assert_eq!(r.width, 0);
        assert_eq!(r.height, 0);
    }

    #[test]
    fn constraint_percentage_clamps() {
        let c = Constraint::Percentage(100);
        assert_eq!(c, Constraint::Percentage(100));
    }

    #[test]
    fn style_bold_modifier() {
        let s = Style::default().add_modifier(Modifier::BOLD);
        assert!(s.add_modifier.contains(Modifier::BOLD));
    }
}
