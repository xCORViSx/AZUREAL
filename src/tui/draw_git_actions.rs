//! Git panel overlay renderers — commit editor and conflict resolution dialogs.
//!
//! These are rendered as overlays on top of the viewer pane when active,
//! called from run.rs::ui() in the overlay section. The actual git panel
//! pane content (actions, files, commits, diff) is handled by the existing
//! draw_sidebar, draw_viewer, draw_output, and draw_status modules.

use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Paragraph},
    Frame,
};

use super::util::{GIT_BROWN, GIT_ORANGE};

/// Commit message editor rendered as an overlay on the viewer pane area
pub(crate) fn draw_commit_editor(f: &mut Frame, overlay: &crate::app::types::GitCommitOverlay, area: Rect) {
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

    f.render_widget(Clear, area);
    f.render_widget(Paragraph::new(commit_lines).block(block), area);
}

/// Auto-resolve file settings overlay — lets users configure which files
/// are auto-resolved during rebase via union merge (keeps both sides' changes).
pub(crate) fn draw_auto_resolve_overlay(f: &mut Frame, overlay: &crate::app::types::AutoResolveOverlay, area: Rect) {
    let inner_h = area.height.saturating_sub(2) as usize;
    let inner_w = area.width.saturating_sub(4) as usize;
    let mut lines: Vec<Line> = Vec::new();

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        " Files auto-resolved during rebase by",
        Style::default().fg(GIT_BROWN),
    )));
    lines.push(Line::from(Span::styled(
        " keeping both sides' changes (union merge):",
        Style::default().fg(GIT_BROWN),
    )));
    lines.push(Line::from(""));

    for (i, (name, enabled)) in overlay.files.iter().enumerate() {
        let selected = i == overlay.selected;
        let check = if *enabled { "[x]" } else { "[ ]" };
        let prefix = if selected { " \u{25b8} " } else { "   " };
        let display = if name.len() > inner_w.saturating_sub(10) {
            format!("\u{2026}{}", &name[name.len().saturating_sub(inner_w.saturating_sub(11))..])
        } else {
            name.clone()
        };
        let style = if selected {
            Style::default().fg(GIT_ORANGE).add_modifier(Modifier::BOLD)
        } else if *enabled {
            Style::default().fg(Color::White)
        } else {
            Style::default().fg(Color::DarkGray)
        };
        lines.push(Line::from(vec![
            Span::styled(prefix, style),
            Span::styled(check, style),
            Span::styled(format!(" {}", display), style),
        ]));
    }

    if overlay.files.is_empty() {
        lines.push(Line::from(Span::styled(
            "   (no files configured)",
            Style::default().fg(Color::DarkGray),
        )));
    }

    lines.push(Line::from(""));

    if overlay.adding {
        let cursor_char = overlay.input_buffer.chars().nth(overlay.input_cursor).unwrap_or(' ');
        let before: String = overlay.input_buffer.chars().take(overlay.input_cursor).collect();
        let after: String = overlay.input_buffer.chars().skip(overlay.input_cursor + 1).collect();
        let has_char = overlay.input_cursor < overlay.input_buffer.chars().count();
        lines.push(Line::from(vec![
            Span::styled(" > ", Style::default().fg(GIT_ORANGE)),
            Span::styled(before, Style::default().fg(Color::White)),
            Span::styled(
                if has_char { cursor_char.to_string() } else { " ".into() },
                Style::default().fg(Color::Black).bg(GIT_ORANGE),
            ),
            Span::styled(after, Style::default().fg(Color::White)),
        ]));
        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::styled(" Enter", Style::default().fg(GIT_ORANGE)),
            Span::styled(":add  ", Style::default().fg(GIT_BROWN)),
            Span::styled("Esc", Style::default().fg(GIT_ORANGE)),
            Span::styled(":cancel", Style::default().fg(GIT_BROWN)),
        ]));
    } else {
        lines.push(Line::from(vec![
            Span::styled(" a", Style::default().fg(GIT_ORANGE)),
            Span::styled(":add  ", Style::default().fg(GIT_BROWN)),
            Span::styled("d", Style::default().fg(GIT_ORANGE)),
            Span::styled(":remove  ", Style::default().fg(GIT_BROWN)),
            Span::styled("Space", Style::default().fg(GIT_ORANGE)),
            Span::styled(":toggle  ", Style::default().fg(GIT_BROWN)),
            Span::styled("Esc", Style::default().fg(GIT_ORANGE)),
            Span::styled(":save", Style::default().fg(GIT_BROWN)),
        ]));
    }

    let visible: Vec<Line> = lines.into_iter().take(inner_h).collect();

    let block = Block::default()
        .title(Line::from(Span::styled(
            " Auto-Resolve Files ",
            Style::default().fg(GIT_ORANGE).add_modifier(Modifier::BOLD),
        )))
        .borders(Borders::ALL)
        .border_type(BorderType::Double)
        .border_style(Style::default().fg(GIT_ORANGE));

    f.render_widget(Clear, area);
    f.render_widget(Paragraph::new(visible).block(block), area);
}

/// Conflict resolution UI rendered as an overlay on the viewer pane area
pub(crate) fn draw_conflict_inline(f: &mut Frame, ov: &crate::app::types::GitConflictOverlay, area: Rect) {
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

    f.render_widget(Clear, area);
    f.render_widget(Paragraph::new(visible).block(block), area);
}
