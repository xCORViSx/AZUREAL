//! Git panel rendering — full-app layout replacing all panes when active.
//!
//! Layout:
//! ┌──────────┬──────────────────────────┬──────────────┐
//! │ Actions  │                          │              │
//! │ (short)  │     Viewer/Detail        │   Commits    │
//! ├──────────┤  (diff, commit editor,   │  (git log)   │
//! │ Changed  │   conflict resolution)   │              │
//! │ Files    │                          │              │
//! │ (fills)  │                          │              │
//! ├──────────┴──────────────────────────┴──────────────┤
//! │         Status (operation result messages)         │
//! └────────────────────────────────────────────────────┘
//!
//! All panes use QuadrantOutside borders in Git orange (#F05032).
//! Focused pane border is bright orange; unfocused panes use brown.
//! Esc returns to normal layout.

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Paragraph, Wrap},
    Frame,
};

use crate::app::App;
use super::keybindings;
use super::util::{GIT_BROWN, GIT_ORANGE};

/// Entry point — replaces the normal ui() layout when git panel is open.
/// Called from run.rs via early return.
pub fn draw_git_layout(f: &mut Frame, app: &App) {
    let panel = match app.git_actions_panel {
        Some(ref p) => p,
        None => return,
    };
    let area = f.area();

    // Top-level split: body (fills) + status bar (1 row)
    let vert = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(4), Constraint::Length(1)])
        .split(area);
    let body = vert[0];
    let status_area = vert[1];

    // Body: left column (20%) | center viewer (55%) | right commits (25%)
    let horiz = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(20),
            Constraint::Percentage(55),
            Constraint::Percentage(25),
        ])
        .split(body);
    let left_col = horiz[0];
    let viewer_area = horiz[1];
    let commits_area = horiz[2];

    // Left column: actions (auto-sized) | files (fill remaining)
    // Actions: header(1) + actions(3-4) + auto-rebase(0-1) + 2 borders = ~7-8 rows
    let action_rows = if panel.is_on_main { 6 } else { 8 };
    let left_split = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(action_rows), Constraint::Min(4)])
        .split(left_col);
    let actions_area = left_split[0];
    let files_area = left_split[1];

    // Draw each pane
    draw_actions_pane(f, app, panel, actions_area);
    draw_files_pane(f, panel, files_area);
    draw_viewer_pane(f, panel, viewer_area);
    draw_commits_pane(f, panel, commits_area);
    draw_status_bar(f, panel, status_area);
}

/// Border style helper — focused pane gets bright orange, unfocused gets brown
fn pane_border(focused: bool) -> Style {
    if focused {
        Style::default().fg(GIT_ORANGE)
    } else {
        Style::default().fg(GIT_BROWN)
    }
}

/// Title style helper — focused gets bold orange, unfocused gets brown
fn pane_title_style(focused: bool) -> Style {
    if focused {
        Style::default().fg(GIT_ORANGE).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(GIT_BROWN).add_modifier(Modifier::BOLD)
    }
}

// ─── Actions pane (top-left) ─────────────────────────────────────────────────

fn draw_actions_pane(f: &mut Frame, app: &App, panel: &crate::app::types::GitActionsPanel, area: Rect) {
    let focused = panel.focused_pane == 0;
    let mut lines: Vec<Line> = Vec::new();

    let action_labels = keybindings::git_actions_labels(panel.is_on_main);
    for (i, (key, label)) in action_labels.iter().enumerate() {
        let selected = focused && i == panel.selected_action;
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
        lines.push(Line::from(vec![
            Span::styled(prefix, style),
            Span::styled(format!("[{}]", key), key_style),
            Span::styled(format!(" {}", label), style),
        ]));
    }

    // Auto-rebase toggle (feature branches only)
    if !panel.is_on_main {
        let enabled = app.auto_rebase_enabled.contains(&panel.worktree_name);
        let (indicator, ind_color) = if enabled {
            ("\u{25cf} ON", Color::Green)
        } else {
            ("\u{25cb} OFF", Color::DarkGray)
        };
        lines.push(Line::from(vec![
            Span::styled("   ", Style::default()),
            Span::styled("[a]", Style::default().fg(GIT_BROWN)),
            Span::styled(" Auto-rebase ", Style::default().fg(Color::White)),
            Span::styled(indicator, Style::default().fg(ind_color).add_modifier(Modifier::BOLD)),
        ]));
    }

    let block = Block::default()
        .title(Line::from(Span::styled(" Actions ", pane_title_style(focused))))
        .borders(Borders::ALL)
        .border_type(BorderType::QuadrantOutside)
        .border_style(pane_border(focused));

    f.render_widget(Paragraph::new(lines).block(block), area);
}

// ─── Files pane (bottom-left) ────────────────────────────────────────────────

fn draw_files_pane(f: &mut Frame, panel: &crate::app::types::GitActionsPanel, area: Rect) {
    let focused = panel.focused_pane == 1;
    let inner_w = area.width.saturating_sub(2) as usize;
    let inner_h = area.height.saturating_sub(2) as usize;
    let mut lines: Vec<Line> = Vec::new();

    // Adjust scroll so selected file is visible
    let visible_files = inner_h;
    let scroll = if panel.selected_file < panel.file_scroll {
        panel.selected_file
    } else if panel.selected_file >= panel.file_scroll + visible_files {
        panel.selected_file.saturating_sub(visible_files.saturating_sub(1))
    } else {
        panel.file_scroll
    };

    for (i, file) in panel.changed_files.iter().enumerate().skip(scroll).take(visible_files) {
        let selected = focused && i == panel.selected_file;
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

        lines.push(Line::from(vec![
            Span::styled(prefix, Style::default()),
            Span::styled(format!("{} ", file.status), Style::default().fg(status_color)),
            Span::styled(path_display, path_style),
            Span::raw(" ".repeat(padding)),
            Span::styled(add_str, add_style),
            Span::styled("/", slash_style),
            Span::styled(del_str, del_style),
        ]));
    }

    // Title with file count and +/-
    let title = if panel.changed_files.is_empty() {
        " Files (none) ".to_string()
    } else {
        let total_add: usize = panel.changed_files.iter().map(|f| f.additions).sum();
        let total_del: usize = panel.changed_files.iter().map(|f| f.deletions).sum();
        format!(" Files ({}, +{}/-{}) ", panel.changed_files.len(), total_add, total_del)
    };

    let block = Block::default()
        .title(Line::from(Span::styled(title, pane_title_style(focused))))
        .borders(Borders::ALL)
        .border_type(BorderType::QuadrantOutside)
        .border_style(pane_border(focused));

    f.render_widget(Paragraph::new(lines).block(block), area);
}

// ─── Viewer pane (center) ────────────────────────────────────────────────────
// Shows: file diff, commit diff, commit editor (inline), or conflict overlay (inline)

fn draw_viewer_pane(f: &mut Frame, panel: &crate::app::types::GitActionsPanel, area: Rect) {
    // Commit overlay takes over the viewer pane
    if let Some(ref overlay) = panel.commit_overlay {
        draw_commit_editor(f, overlay, area);
        return;
    }

    // Conflict overlay takes over the viewer pane
    if let Some(ref ov) = panel.conflict_overlay {
        draw_conflict_inline(f, ov, area);
        return;
    }

    // Default: show diff content
    let title = match panel.viewer_diff_title {
        Some(ref t) => format!(" {} ", t),
        None => " Viewer ".to_string(),
    };

    let block = Block::default()
        .title(Line::from(Span::styled(title, Style::default().fg(GIT_ORANGE).add_modifier(Modifier::BOLD))))
        .borders(Borders::ALL)
        .border_type(BorderType::QuadrantOutside)
        .border_style(Style::default().fg(GIT_ORANGE));

    match panel.viewer_diff {
        Some(ref diff) => {
            let lines: Vec<Line> = diff.lines().map(|l| {
                let style = if l.starts_with('+') && !l.starts_with("+++") {
                    Style::default().fg(Color::Green)
                } else if l.starts_with('-') && !l.starts_with("---") {
                    Style::default().fg(Color::Red)
                } else if l.starts_with("@@") {
                    Style::default().fg(Color::Cyan)
                } else if l.starts_with("diff ") || l.starts_with("index ") {
                    Style::default().fg(GIT_BROWN)
                } else {
                    Style::default().fg(Color::White)
                };
                Line::from(Span::styled(format!(" {}", l), style))
            }).collect();
            f.render_widget(Paragraph::new(lines).block(block).wrap(Wrap { trim: false }), area);
        }
        None => {
            let hint = vec![
                Line::from(""),
                Line::from(Span::styled(
                    " Select a file or commit to view its diff",
                    Style::default().fg(GIT_BROWN),
                )),
            ];
            f.render_widget(Paragraph::new(hint).block(block), area);
        }
    }
}

/// Commit message editor rendered inline in the viewer pane area
fn draw_commit_editor(f: &mut Frame, overlay: &crate::app::types::GitCommitOverlay, area: Rect) {
    let inner_h = area.height.saturating_sub(2) as usize;
    let inner_w = area.width.saturating_sub(2) as usize;
    let mut commit_lines: Vec<Line> = Vec::new();

    if overlay.generating {
        let dots = ".".repeat((std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() / 500 % 4) as usize);
        commit_lines.push(Line::from(""));
        commit_lines.push(Line::from(Span::styled(
            format!(" Generating commit message{}", dots),
            Style::default().fg(GIT_ORANGE),
        )));
    } else {
        let msg_lines: Vec<&str> = overlay.message.lines().collect();
        let msg_lines: Vec<&str> = if overlay.message.ends_with('\n') {
            let mut v = msg_lines; v.push(""); v
        } else if msg_lines.is_empty() {
            vec![""]
        } else {
            msg_lines
        };

        let wrap_w = inner_w.saturating_sub(1).max(1);

        // Find cursor's logical line and column
        let mut cursor_logical = 0usize;
        let mut cursor_col_in_logical = 0usize;
        let mut chars_seen = 0usize;
        for (i, line) in msg_lines.iter().enumerate() {
            let lc = line.chars().count();
            if chars_seen + lc >= overlay.cursor {
                cursor_logical = i;
                cursor_col_in_logical = overlay.cursor - chars_seen;
                break;
            }
            chars_seen += lc + 1;
            cursor_logical = i + 1;
        }

        // Wrap logical lines into display lines, tracking cursor position
        let mut wrapped: Vec<(Vec<char>, bool, usize)> = Vec::new();
        let mut cursor_display_row = 0usize;
        for (li, line) in msg_lines.iter().enumerate() {
            let chars: Vec<char> = line.chars().collect();
            if chars.is_empty() {
                let has = li == cursor_logical && cursor_col_in_logical == 0;
                if has { cursor_display_row = wrapped.len(); }
                wrapped.push((vec![], has, 0));
            } else {
                let mut off = 0;
                while off < chars.len() {
                    let end = if chars.len() - off <= wrap_w {
                        chars.len()
                    } else {
                        let window_end = off + wrap_w;
                        let mut break_at = None;
                        for j in (off..window_end).rev() {
                            if chars[j] == ' ' { break_at = Some(j + 1); break; }
                        }
                        break_at.unwrap_or(window_end)
                    };
                    let sub = chars[off..end].to_vec();
                    let has = li == cursor_logical
                        && cursor_col_in_logical >= off
                        && cursor_col_in_logical < end;
                    let col = if has { cursor_col_in_logical - off } else { 0 };
                    if has { cursor_display_row = wrapped.len(); }
                    wrapped.push((sub, has, col));
                    off = end;
                }
                if li == cursor_logical && cursor_col_in_logical == chars.len() {
                    let last = wrapped.len() - 1;
                    wrapped[last].1 = true;
                    wrapped[last].2 = wrapped[last].0.len();
                    cursor_display_row = last;
                }
            }
        }

        // Reserve 2 lines for the bottom hint bar
        let visible_h = inner_h.saturating_sub(2);
        let scroll = if cursor_display_row >= overlay.scroll + visible_h {
            cursor_display_row - visible_h + 1
        } else if cursor_display_row < overlay.scroll {
            cursor_display_row
        } else {
            overlay.scroll.min(wrapped.len().saturating_sub(visible_h))
        };

        for (chars, has_cursor, col) in wrapped.iter().skip(scroll).take(visible_h) {
            if *has_cursor {
                let before: String = chars[..(*col).min(chars.len())].iter().collect();
                let cursor_char = chars.get(*col).copied().unwrap_or(' ');
                let after: String = if *col < chars.len() {
                    chars[*col + 1..].iter().collect()
                } else {
                    String::new()
                };
                commit_lines.push(Line::from(vec![
                    Span::raw(" "),
                    Span::styled(before, Style::default().fg(Color::White)),
                    Span::styled(
                        cursor_char.to_string(),
                        Style::default().fg(Color::Black).bg(GIT_ORANGE),
                    ),
                    Span::styled(after, Style::default().fg(Color::White)),
                ]));
            } else {
                let text: String = chars.iter().collect();
                commit_lines.push(Line::from(vec![
                    Span::raw(" "),
                    Span::styled(text, Style::default().fg(Color::White)),
                ]));
            }
        }

        while commit_lines.len() < visible_h {
            commit_lines.push(Line::from(""));
        }

        // Hint bar at the bottom
        commit_lines.push(Line::from(""));
        commit_lines.push(Line::from(vec![
            Span::styled(" Enter", Style::default().fg(GIT_ORANGE)),
            Span::styled(":commit  ", Style::default().fg(GIT_BROWN)),
            Span::styled("\u{2318}P", Style::default().fg(GIT_ORANGE)),
            Span::styled(":commit+push  ", Style::default().fg(GIT_BROWN)),
            Span::styled("Shift+Enter", Style::default().fg(GIT_ORANGE)),
            Span::styled(":newline  ", Style::default().fg(GIT_BROWN)),
            Span::styled("Esc", Style::default().fg(GIT_ORANGE)),
            Span::styled(":cancel", Style::default().fg(GIT_BROWN)),
        ]));
    }

    let block = Block::default()
        .title(Line::from(Span::styled(" Commit ", Style::default().fg(GIT_ORANGE).add_modifier(Modifier::BOLD))))
        .borders(Borders::ALL)
        .border_type(BorderType::QuadrantOutside)
        .border_style(Style::default().fg(GIT_ORANGE));

    f.render_widget(Paragraph::new(commit_lines).block(block), area);
}

/// Conflict resolution UI rendered inline in the viewer pane area
fn draw_conflict_inline(f: &mut Frame, ov: &crate::app::types::GitConflictOverlay, area: Rect) {
    let inner_w = area.width.saturating_sub(4) as usize;
    let inner_h = area.height.saturating_sub(2) as usize;
    let mut lines: Vec<Line> = Vec::new();

    // Conflicted files section (red)
    lines.push(Line::from(Span::styled(
        format!(" {} CONFLICTED FILE{}", ov.conflicted_files.len(),
            if ov.conflicted_files.len() == 1 { "" } else { "S" }),
        Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
    )));
    for cf in &ov.conflicted_files {
        let display = if cf.len() > inner_w.saturating_sub(3) {
            format!("   \u{2026}{}", &cf[cf.len().saturating_sub(inner_w.saturating_sub(4))..])
        } else { format!("   {}", cf) };
        lines.push(Line::from(Span::styled(display, Style::default().fg(Color::Red))));
    }

    if !ov.auto_merged_files.is_empty() {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            format!(" {} AUTO-MERGED", ov.auto_merged_files.len()),
            Style::default().fg(Color::Green).add_modifier(Modifier::BOLD),
        )));
        for am in &ov.auto_merged_files {
            let display = if am.len() > inner_w.saturating_sub(3) {
                format!("   \u{2026}{}", &am[am.len().saturating_sub(inner_w.saturating_sub(4))..])
            } else { format!("   {}", am) };
            lines.push(Line::from(Span::styled(display, Style::default().fg(Color::Green))));
        }
    }

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        " Session will be prefixed [RCR] (Rebase Conflict Resolution)",
        Style::default().fg(Color::DarkGray),
    )));
    lines.push(Line::from(""));

    let resolve_style = if ov.selected == 0 {
        Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)
    } else { Style::default().fg(Color::White) };
    let abort_style = if ov.selected == 1 {
        Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)
    } else { Style::default().fg(Color::White) };
    let arrow_0 = if ov.selected == 0 { " \u{25b8} " } else { "   " };
    let arrow_1 = if ov.selected == 1 { " \u{25b8} " } else { "   " };
    lines.push(Line::from(Span::styled(format!("{}[y] Resolve with Claude", arrow_0), resolve_style)));
    lines.push(Line::from(Span::styled(format!("{}[n] Abort rebase", arrow_1), abort_style)));

    let skip = ov.scroll.min(lines.len().saturating_sub(inner_h));
    let visible: Vec<Line> = lines.into_iter().skip(skip).take(inner_h).collect();

    let block = Block::default()
        .title(Line::from(Span::styled(
            " Merge Conflicts ", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        )))
        .borders(Borders::ALL)
        .border_type(BorderType::QuadrantOutside)
        .border_style(Style::default().fg(Color::Red));

    f.render_widget(Paragraph::new(visible).block(block), area);
}

// ─── Commits pane (right) ────────────────────────────────────────────────────

fn draw_commits_pane(f: &mut Frame, panel: &crate::app::types::GitActionsPanel, area: Rect) {
    let focused = panel.focused_pane == 2;
    let inner_h = area.height.saturating_sub(2) as usize;
    let inner_w = area.width.saturating_sub(2) as usize;
    let mut lines: Vec<Line> = Vec::new();

    if panel.commits.is_empty() {
        lines.push(Line::from(Span::styled(
            " No commits",
            Style::default().fg(GIT_BROWN),
        )));
    } else {
        // Adjust scroll so selected commit is visible
        let scroll = if panel.selected_commit < panel.commit_scroll {
            panel.selected_commit
        } else if panel.selected_commit >= panel.commit_scroll + inner_h {
            panel.selected_commit.saturating_sub(inner_h.saturating_sub(1))
        } else {
            panel.commit_scroll
        };

        for (i, commit) in panel.commits.iter().enumerate().skip(scroll).take(inner_h) {
            let selected = focused && i == panel.selected_commit;
            let prefix = if selected { " \u{25b8} " } else { "   " };

            // Green for unpushed, white for pushed
            let hash_color = if !commit.is_pushed { Color::Green } else { Color::DarkGray };
            let subject_color = if selected {
                GIT_ORANGE
            } else if !commit.is_pushed {
                Color::Green
            } else {
                Color::White
            };
            let subject_mod = if selected { Modifier::BOLD } else { Modifier::empty() };

            // Truncate subject to fit: prefix(3) + hash(7) + space(1) + subject
            let subject_budget = inner_w.saturating_sub(prefix.len() + 8);
            let subject_display = if commit.subject.len() > subject_budget {
                format!("{}\u{2026}", &commit.subject[..subject_budget.saturating_sub(1)])
            } else {
                commit.subject.clone()
            };

            lines.push(Line::from(vec![
                Span::styled(prefix, Style::default()),
                Span::styled(&commit.hash, Style::default().fg(hash_color)),
                Span::raw(" "),
                Span::styled(subject_display, Style::default().fg(subject_color).add_modifier(subject_mod)),
            ]));
        }
    }

    let title = format!(" Commits ({}) ", panel.commits.len());
    let block = Block::default()
        .title(Line::from(Span::styled(title, pane_title_style(focused))))
        .borders(Borders::ALL)
        .border_type(BorderType::QuadrantOutside)
        .border_style(pane_border(focused));

    f.render_widget(Paragraph::new(lines).block(block), area);
}

// ─── Status bar (bottom) ─────────────────────────────────────────────────────

fn draw_status_bar(f: &mut Frame, panel: &crate::app::types::GitActionsPanel, area: Rect) {
    let mut spans: Vec<Span> = Vec::new();

    // Show result message if present, otherwise show footer keybinding hints
    if let Some((ref msg, is_error)) = panel.result_message {
        let color = if is_error { Color::Red } else { Color::Green };
        spans.push(Span::styled(format!(" {} ", msg), Style::default().fg(color)));
    } else {
        let footer = keybindings::git_actions_footer();
        spans.push(Span::styled(
            format!(" Git: {} {}", panel.worktree_name, footer),
            Style::default().fg(GIT_BROWN),
        ));
    }

    f.render_widget(Paragraph::new(Line::from(spans)), area);
}
