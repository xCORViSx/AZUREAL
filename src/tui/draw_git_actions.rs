//! Git panel rendering — centered modal overlay showing git operations
//! (squash-merge, commit) and changed files list.
//! Uses Git brand orange (#F05032) for border styling.

use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Paragraph},
    Frame,
};

use crate::app::App;
use super::keybindings;
use super::util::{GIT_BROWN, GIT_ORANGE};

/// Render the Git Actions panel as a centered modal overlay.
/// Called from ui() in run.rs when app.git_actions_panel.is_some().
pub fn draw_git_actions_panel(f: &mut Frame, app: &App) {
    let panel = match app.git_actions_panel {
        Some(ref p) => p,
        None => return,
    };
    let area = f.area();

    // Size the modal: 55% width (min 50), 70% height (min 16)
    let modal_w = (area.width * 55 / 100).max(50).min(area.width);
    let modal_h = (area.height * 70 / 100).max(16).min(area.height);
    let modal = Rect::new(
        area.x + (area.width.saturating_sub(modal_w)) / 2,
        area.y + (area.height.saturating_sub(modal_h)) / 2,
        modal_w, modal_h,
    );

    // Clear the area behind the modal so underlying panes don't bleed through
    f.render_widget(Clear, modal);

    let inner_w = modal.width.saturating_sub(4) as usize;
    let mut lines: Vec<Line> = Vec::new();

    // ── Actions section header ──
    lines.push(Line::from(Span::styled(
        "  ACTIONS",
        Style::default().fg(GIT_BROWN).add_modifier(Modifier::BOLD),
    )));

    // Each action row: "  ▸ [m] Squash merge to main" or "    [m] Squash merge to main"
    // Labels sourced from keybindings.rs so they stay in sync with actual key bindings.
    let action_labels = keybindings::git_actions_labels(panel.is_on_main);
    for (i, (key, label)) in action_labels.iter().enumerate() {
        let selected = panel.actions_focused && i == panel.selected_action;
        let prefix = if selected { "  \u{25b8} " } else { "    " };
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

    lines.push(Line::from(""));

    // ── Changed files section header ──
    let total_add: usize = panel.changed_files.iter().map(|f| f.additions).sum();
    let total_del: usize = panel.changed_files.iter().map(|f| f.deletions).sum();
    if panel.changed_files.is_empty() {
        lines.push(Line::from(Span::styled(
            "  CHANGED FILES (none)",
            Style::default().fg(GIT_BROWN).add_modifier(Modifier::BOLD),
        )));
    } else {
        let hdr_style = Style::default().fg(GIT_BROWN).add_modifier(Modifier::BOLD);
        lines.push(Line::from(vec![
            Span::styled(format!("  CHANGED FILES ({} files, ", panel.changed_files.len()), hdr_style),
            Span::styled(format!("+{}", total_add), Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
            Span::styled("/", hdr_style),
            Span::styled(format!("-{}", total_del), Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
            Span::styled(")", hdr_style),
        ]));
    }

    // Thin separator under the header
    let sep_len = inner_w.min(40);
    lines.push(Line::from(Span::styled(
        format!("  {}", "\u{2500}".repeat(sep_len)),
        Style::default().fg(GIT_BROWN),
    )));

    // How many file rows can we fit? Reserve lines for: actions header(1) + actions(3) +
    // blank(1) + files header(1) + separator(1) + result(2) + borders(2) = 11 fixed
    let visible_files = (modal_h as usize).saturating_sub(11);

    // Adjust scroll so selected file is visible
    let scroll = if panel.selected_file < panel.file_scroll {
        panel.selected_file
    } else if panel.selected_file >= panel.file_scroll + visible_files {
        panel.selected_file.saturating_sub(visible_files.saturating_sub(1))
    } else {
        panel.file_scroll
    };

    // Render each visible file row
    for (i, file) in panel.changed_files.iter().enumerate().skip(scroll).take(visible_files) {
        let selected = !panel.actions_focused && i == panel.selected_file;
        let prefix = if selected { "  \u{25b8} " } else { "    " };

        // Status character color: M=yellow, A=green, D=red, R=cyan, ?=magenta (untracked)
        let status_color = match file.status {
            'A' => Color::Green,
            'D' => Color::Red,
            'M' => Color::Yellow,
            'R' => Color::Cyan,
            '?' => Color::Magenta,
            _ => Color::White,
        };

        // Right-aligned +N/-N stats — green for additions, red for deletions
        let add_str = format!("+{}", file.additions);
        let del_str = format!("-{}", file.deletions);
        let stat_len = add_str.len() + 1 + del_str.len(); // "+N/-N" total width
        // How much space for the file path? prefix(4) + status(2) + padding(1+) + stats
        let path_budget = inner_w.saturating_sub(prefix.len() + 2 + stat_len + 1);
        let path_display = if file.path.len() > path_budget {
            format!("\u{2026}{}", &file.path[file.path.len().saturating_sub(path_budget.saturating_sub(1))..])
        } else {
            file.path.clone()
        };
        // Padding between path and stats to right-align
        let padding = inner_w.saturating_sub(prefix.len() + 2 + path_display.len() + stat_len);

        // Path style: underlined always, orange+bold when selected
        let path_style = if selected {
            Style::default().fg(GIT_ORANGE).add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
        } else {
            Style::default().fg(Color::White).add_modifier(Modifier::UNDERLINED)
        };
        // Selected rows use orange override; unselected uses semantic green/red
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

    // Scroll indicator when file list overflows
    if panel.changed_files.len() > visible_files && visible_files > 0 {
        let end = (scroll + visible_files).min(panel.changed_files.len());
        lines.push(Line::from(Span::styled(
            format!("    {}\u{2013}{} of {}", scroll + 1, end, panel.changed_files.len()),
            Style::default().fg(GIT_BROWN),
        )));
    }

    // ── Result message (green=success, red=error) ──
    if let Some((ref msg, is_error)) = panel.result_message {
        lines.push(Line::from(""));
        let color = if is_error { Color::Red } else { Color::Green };
        let truncated = if msg.len() > inner_w { &msg[..inner_w] } else { msg.as_str() };
        lines.push(Line::from(Span::styled(
            format!("  {}", truncated),
            Style::default().fg(color),
        )));
    }

    // ── Modal chrome: orange border with centered title ──
    let title = Line::from(vec![
        Span::styled(format!(" Git: {} ", panel.worktree_name), Style::default().fg(GIT_ORANGE).bold()),
    ]);
    let block = Block::default()
        .title(title)
        .title_alignment(ratatui::layout::Alignment::Center)
        .borders(Borders::ALL)
        .border_type(BorderType::QuadrantOutside)
        .border_style(Style::default().fg(GIT_ORANGE));

    let paragraph = Paragraph::new(lines).block(block);
    f.render_widget(paragraph, modal);

    // ── Commit message overlay (centered dialog on top of panel) ──
    if let Some(ref overlay) = panel.commit_overlay {
        // Size: 70% of panel width, 60% height — large enough for multi-line messages
        let dlg_w = (modal.width * 70 / 100).max(40).min(modal.width.saturating_sub(4));
        let dlg_h = (modal.height * 60 / 100).max(10).min(modal.height.saturating_sub(4));
        let dlg = Rect::new(
            modal.x + (modal.width.saturating_sub(dlg_w)) / 2,
            modal.y + (modal.height.saturating_sub(dlg_h)) / 2,
            dlg_w, dlg_h,
        );
        f.render_widget(Clear, dlg);

        // Inner content area height (inside borders)
        let inner_h = dlg_h.saturating_sub(2) as usize;

        let mut commit_lines: Vec<Line> = Vec::new();

        if overlay.generating {
            // Pulsating "Generating..." message while waiting for Claude
            let dots = ".".repeat((std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() / 500 % 4) as usize);
            commit_lines.push(Line::from(""));
            commit_lines.push(Line::from(Span::styled(
                format!("  Generating commit message{}", dots),
                Style::default().fg(GIT_ORANGE),
            )));
        } else {
            // Render the editable message with word-wrap and cursor indicator.
            // Split into logical lines, then wrap each to fit the dialog width.
            let msg_lines: Vec<&str> = overlay.message.lines().collect();
            let msg_lines: Vec<&str> = if overlay.message.ends_with('\n') {
                let mut v = msg_lines; v.push(""); v
            } else if msg_lines.is_empty() {
                vec![""]
            } else {
                msg_lines
            };

            // Available text width: dialog minus 2 border chars minus 1 prefix space
            let wrap_w = (dlg_w as usize).saturating_sub(3).max(1);

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

            // Wrap logical lines into display lines, tracking cursor position.
            // Each entry: (char vec for this display row, has_cursor, cursor_col_in_row)
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
                        let remaining = chars.len() - off;
                        // Word-based wrapping: find last space within the wrap width
                        // so lines break at word boundaries instead of mid-word
                        let end = if remaining <= wrap_w {
                            chars.len()
                        } else {
                            let window_end = off + wrap_w;
                            let mut break_at = None;
                            for j in (off..window_end).rev() {
                                if chars[j] == ' ' { break_at = Some(j + 1); break; }
                            }
                            // No space found — hard break (single long word)
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
                    // Cursor at end of line (past last char) — place on last sub-line
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
            // Auto-scroll to keep cursor visible
            let scroll = if cursor_display_row >= overlay.scroll + visible_h {
                cursor_display_row - visible_h + 1
            } else if cursor_display_row < overlay.scroll {
                cursor_display_row
            } else {
                overlay.scroll.min(wrapped.len().saturating_sub(visible_h))
            };

            for (chars, has_cursor, col) in wrapped.iter().skip(scroll).take(visible_h) {
                let prefix = " ";
                if *has_cursor {
                    let before: String = chars[..(*col).min(chars.len())].iter().collect();
                    let cursor_char = chars.get(*col).copied().unwrap_or(' ');
                    let after: String = if *col < chars.len() {
                        chars[*col + 1..].iter().collect()
                    } else {
                        String::new()
                    };
                    commit_lines.push(Line::from(vec![
                        Span::raw(prefix),
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
                        Span::raw(prefix),
                        Span::styled(text, Style::default().fg(Color::White)),
                    ]));
                }
            }

            // Pad remaining visible lines so the hint bar sits at the bottom
            while commit_lines.len() < visible_h {
                commit_lines.push(Line::from(""));
            }

            // Hint bar at the bottom of the dialog content
            commit_lines.push(Line::from(""));
            commit_lines.push(Line::from(vec![
                Span::styled(" Enter", Style::default().fg(GIT_ORANGE)),
                Span::styled(":commit  ", Style::default().fg(GIT_BROWN)),
                Span::styled("⌘P", Style::default().fg(GIT_ORANGE)),
                Span::styled(":commit+push  ", Style::default().fg(GIT_BROWN)),
                Span::styled("Shift+Enter", Style::default().fg(GIT_ORANGE)),
                Span::styled(":newline  ", Style::default().fg(GIT_BROWN)),
                Span::styled("Esc", Style::default().fg(GIT_ORANGE)),
                Span::styled(":cancel", Style::default().fg(GIT_BROWN)),
            ]));
        }

        let commit_block = Block::default()
            .title(Line::from(Span::styled(" Commit ", Style::default().fg(GIT_ORANGE).bold())))
            .title_alignment(ratatui::layout::Alignment::Center)
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(GIT_ORANGE));

        f.render_widget(Paragraph::new(commit_lines).block(commit_block), dlg);
    }

    // ── Conflict resolution overlay (shown when squash merge has conflicts) ──
    if let Some(ref ov) = panel.conflict_overlay {
        // Larger dialog — needs room for file lists and action prompt
        let dlg_w = (modal.width * 85 / 100).max(44).min(modal.width.saturating_sub(4));
        let dlg_h = (modal.height * 80 / 100).max(14).min(modal.height.saturating_sub(4));
        let dlg = Rect::new(
            modal.x + (modal.width.saturating_sub(dlg_w)) / 2,
            modal.y + (modal.height.saturating_sub(dlg_h)) / 2,
            dlg_w, dlg_h,
        );
        f.render_widget(Clear, dlg);

        let inner_w = dlg_w.saturating_sub(4) as usize;
        let mut lines: Vec<Line> = Vec::new();

        // Conflicted files section (red)
        lines.push(Line::from(Span::styled(
            format!("  {} CONFLICTED FILE{}", ov.conflicted_files.len(),
                if ov.conflicted_files.len() == 1 { "" } else { "S" }),
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        )));
        for cf in &ov.conflicted_files {
            let display = if cf.len() > inner_w.saturating_sub(4) {
                format!("    …{}", &cf[cf.len().saturating_sub(inner_w.saturating_sub(5))..])
            } else { format!("    {}", cf) };
            lines.push(Line::from(Span::styled(display, Style::default().fg(Color::Red))));
        }

        // Auto-merged files section (green, only shown if any)
        if !ov.auto_merged_files.is_empty() {
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                format!("  {} AUTO-MERGED", ov.auto_merged_files.len()),
                Style::default().fg(Color::Green).add_modifier(Modifier::BOLD),
            )));
            for am in &ov.auto_merged_files {
                let display = if am.len() > inner_w.saturating_sub(4) {
                    format!("    …{}", &am[am.len().saturating_sub(inner_w.saturating_sub(5))..])
                } else { format!("    {}", am) };
                lines.push(Line::from(Span::styled(display, Style::default().fg(Color::Green))));
            }
        }

        lines.push(Line::from(""));

        // [MCR] naming convention hint — mirrors [DH] and [GFM] hints
        lines.push(Line::from(Span::styled(
            "  Session will be prefixed [MCR] (Merge Conflict Resolution)",
            Style::default().fg(Color::DarkGray),
        )));
        lines.push(Line::from(""));

        // Action options with arrow selector
        let resolve_style = if ov.selected == 0 {
            Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)
        } else { Style::default().fg(Color::White) };
        let abort_style = if ov.selected == 1 {
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)
        } else { Style::default().fg(Color::White) };
        let arrow_0 = if ov.selected == 0 { " ▸ " } else { "   " };
        let arrow_1 = if ov.selected == 1 { " ▸ " } else { "   " };
        lines.push(Line::from(Span::styled(format!("{}[y] Resolve with Claude", arrow_0), resolve_style)));
        lines.push(Line::from(Span::styled(format!("{}[n] Abort merge", arrow_1), abort_style)));

        // Render with scroll and block
        let visible_h = dlg_h.saturating_sub(2) as usize;
        let skip = ov.scroll.min(lines.len().saturating_sub(visible_h));
        let visible: Vec<Line> = lines.into_iter().skip(skip).take(visible_h).collect();

        let conflict_block = Block::default()
            .title(Line::from(Span::styled(
                " Merge Conflicts ", Style::default().fg(Color::Red).bold(),
            )))
            .title_alignment(ratatui::layout::Alignment::Center)
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(Color::Red));

        f.render_widget(Paragraph::new(visible).block(conflict_block), dlg);

        // Footer hint overlaid on bottom border of conflict dialog
        let hint = " j/k:navigate  Enter/y:resolve  n/Esc:abort ";
        let hint_x = dlg.x + (dlg.width.saturating_sub(hint.len() as u16)) / 2;
        let hint_y = dlg.y + dlg.height.saturating_sub(1);
        if hint_y < area.height {
            let hr = Rect::new(hint_x, hint_y, hint.len() as u16, 1);
            f.render_widget(Paragraph::new(Line::from(Span::styled(
                hint, Style::default().fg(GIT_BROWN),
            ))), hr);
        }
    }

    // ── Footer hints rendered on top of the bottom border ──
    let footer = keybindings::git_actions_footer();
    let footer_y = modal.y + modal.height.saturating_sub(1);
    let footer_x = modal.x + (modal.width.saturating_sub(footer.len() as u16)) / 2;
    if footer_y < area.height && footer_x + (footer.len() as u16) <= area.x + area.width {
        let footer_rect = Rect::new(footer_x, footer_y, footer.len() as u16, 1);
        f.render_widget(Paragraph::new(Line::from(Span::styled(
            footer,
            Style::default().fg(GIT_BROWN),
        ))), footer_rect);
    }
}
