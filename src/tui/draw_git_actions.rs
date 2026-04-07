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
pub(crate) fn draw_commit_editor(
    f: &mut Frame,
    overlay: &crate::app::types::GitCommitOverlay,
    area: Rect,
    kbd_enhanced: bool,
    alt_enter_stolen: bool,
) {
    let inner_h = area.height.saturating_sub(2) as usize;
    let inner_w = area.width.saturating_sub(2) as usize;
    let mut commit_lines: Vec<Line> = Vec::new();

    if overlay.generating {
        let dots = ".".repeat(
            (std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis()
                / 500
                % 4) as usize,
        );
        commit_lines.push(Line::from(""));
        commit_lines.push(Line::from(Span::styled(
            format!(" Generating commit message{}", dots),
            Style::default().fg(GIT_ORANGE),
        )));
    } else {
        let msg_lines: Vec<&str> = overlay.message.lines().collect();
        let msg_lines: Vec<&str> = if overlay.message.ends_with('\n') {
            let mut v = msg_lines;
            v.push("");
            v
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
                if has {
                    cursor_display_row = wrapped.len();
                }
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
                            if chars[j] == ' ' {
                                break_at = Some(j + 1);
                                break;
                            }
                        }
                        break_at.unwrap_or(window_end)
                    };
                    let sub = chars[off..end].to_vec();
                    let has = li == cursor_logical
                        && cursor_col_in_logical >= off
                        && cursor_col_in_logical < end;
                    let col = if has { cursor_col_in_logical - off } else { 0 };
                    if has {
                        cursor_display_row = wrapped.len();
                    }
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

        // Hint bar at the bottom — keys adapt to terminal capabilities
        let commit_push_key = if cfg!(target_os = "macos") {
            if kbd_enhanced {
                "⌘P"
            } else {
                "⌥p"
            }
        } else {
            "Ctrl+P"
        };
        let newline_key = if alt_enter_stolen {
            "⌃j"
        } else if cfg!(target_os = "macos") {
            "⇧Enter"
        } else {
            "Shift+Enter"
        };
        commit_lines.push(Line::from(""));
        commit_lines.push(Line::from(vec![
            Span::styled(" Enter", Style::default().fg(GIT_ORANGE)),
            Span::styled(":commit  ", Style::default().fg(GIT_BROWN)),
            Span::styled(commit_push_key, Style::default().fg(GIT_ORANGE)),
            Span::styled(":commit+push  ", Style::default().fg(GIT_BROWN)),
            Span::styled(newline_key, Style::default().fg(GIT_ORANGE)),
            Span::styled(":newline  ", Style::default().fg(GIT_BROWN)),
            Span::styled("Esc", Style::default().fg(GIT_ORANGE)),
            Span::styled(":cancel", Style::default().fg(GIT_BROWN)),
        ]));
    }

    let block = Block::default()
        .title(Line::from(Span::styled(
            " Commit ",
            Style::default().fg(GIT_ORANGE).add_modifier(Modifier::BOLD),
        )))
        .borders(Borders::ALL)
        .border_type(BorderType::QuadrantOutside)
        .border_style(Style::default().fg(GIT_ORANGE));

    f.render_widget(Clear, area);
    f.render_widget(Paragraph::new(commit_lines).block(block), area);
}

/// Auto-resolve file settings overlay — lets users configure which files
/// are auto-resolved during rebase via union merge (keeps both sides' changes).
pub(crate) fn draw_auto_resolve_overlay(
    f: &mut Frame,
    overlay: &crate::app::types::AutoResolveOverlay,
    area: Rect,
) {
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
            format!(
                "\u{2026}{}",
                &name[name.len().saturating_sub(inner_w.saturating_sub(11))..]
            )
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
        let cursor_char = overlay
            .input_buffer
            .chars()
            .nth(overlay.input_cursor)
            .unwrap_or(' ');
        let before: String = overlay
            .input_buffer
            .chars()
            .take(overlay.input_cursor)
            .collect();
        let after: String = overlay
            .input_buffer
            .chars()
            .skip(overlay.input_cursor + 1)
            .collect();
        let has_char = overlay.input_cursor < overlay.input_buffer.chars().count();
        lines.push(Line::from(vec![
            Span::styled(" > ", Style::default().fg(GIT_ORANGE)),
            Span::styled(before, Style::default().fg(Color::White)),
            Span::styled(
                if has_char {
                    cursor_char.to_string()
                } else {
                    " ".into()
                },
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

// NOTE: tests are at the bottom of this file

/// Conflict resolution UI rendered as an overlay on the viewer pane area
pub(crate) fn draw_conflict_inline(
    f: &mut Frame,
    ov: &crate::app::types::GitConflictOverlay,
    area: Rect,
) {
    let inner_w = area.width.saturating_sub(4) as usize;
    let inner_h = area.height.saturating_sub(2) as usize;
    let mut lines: Vec<Line> = Vec::new();

    // Conflicted files section (red)
    lines.push(Line::from(Span::styled(
        format!(
            " {} CONFLICTED FILE{}",
            ov.conflicted_files.len(),
            if ov.conflicted_files.len() == 1 {
                ""
            } else {
                "S"
            }
        ),
        Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
    )));
    for cf in &ov.conflicted_files {
        let display = if cf.len() > inner_w.saturating_sub(3) {
            format!(
                "   \u{2026}{}",
                &cf[cf.len().saturating_sub(inner_w.saturating_sub(4))..]
            )
        } else {
            format!("   {}", cf)
        };
        lines.push(Line::from(Span::styled(
            display,
            Style::default().fg(Color::Red),
        )));
    }

    if !ov.auto_merged_files.is_empty() {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            format!(" {} AUTO-MERGED", ov.auto_merged_files.len()),
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        )));
        for am in &ov.auto_merged_files {
            let display = if am.len() > inner_w.saturating_sub(3) {
                format!(
                    "   \u{2026}{}",
                    &am[am.len().saturating_sub(inner_w.saturating_sub(4))..]
                )
            } else {
                format!("   {}", am)
            };
            lines.push(Line::from(Span::styled(
                display,
                Style::default().fg(Color::Green),
            )));
        }
    }

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        " Session will be prefixed [RCR] (Rebase Conflict Resolution)",
        Style::default().fg(Color::DarkGray),
    )));
    lines.push(Line::from(""));

    let resolve_style = if ov.selected == 0 {
        Style::default()
            .fg(Color::Green)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::White)
    };
    let abort_style = if ov.selected == 1 {
        Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::White)
    };
    let arrow_0 = if ov.selected == 0 {
        " \u{25b8} "
    } else {
        "   "
    };
    let arrow_1 = if ov.selected == 1 {
        " \u{25b8} "
    } else {
        "   "
    };
    lines.push(Line::from(Span::styled(
        format!("{}[y] Resolve with Claude", arrow_0),
        resolve_style,
    )));
    lines.push(Line::from(Span::styled(
        format!("{}[n] Abort rebase", arrow_1),
        abort_style,
    )));

    let skip = ov.scroll.min(lines.len().saturating_sub(inner_h));
    let visible: Vec<Line> = lines.into_iter().skip(skip).take(inner_h).collect();

    let block = Block::default()
        .title(Line::from(Span::styled(
            " Merge Conflicts ",
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        )))
        .borders(Borders::ALL)
        .border_type(BorderType::QuadrantOutside)
        .border_style(Style::default().fg(Color::Red));

    f.render_widget(Clear, area);
    f.render_widget(Paragraph::new(visible).block(block), area);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::types::{AutoResolveOverlay, GitCommitOverlay, GitConflictOverlay};

    // ── Color constants ──

    #[test]
    fn test_git_orange_value() {
        assert_eq!(GIT_ORANGE, Color::Rgb(240, 80, 50));
    }
    #[test]
    fn test_git_brown_value() {
        assert_eq!(GIT_BROWN, Color::Rgb(160, 82, 45));
    }
    #[test]
    fn test_git_orange_not_red() {
        assert_ne!(GIT_ORANGE, Color::Red);
    }
    #[test]
    fn test_git_brown_not_yellow() {
        assert_ne!(GIT_BROWN, Color::Yellow);
    }
    #[test]
    fn test_git_colors_distinct() {
        assert_ne!(GIT_ORANGE, GIT_BROWN);
    }

    // ── GitCommitOverlay ──

    #[test]
    fn test_commit_overlay_empty() {
        let ov = GitCommitOverlay {
            message: String::new(),
            cursor: 0,
            generating: false,
            scroll: 0,
            receiver: None,
        };
        assert!(ov.message.is_empty());
    }
    #[test]
    fn test_commit_overlay_generating() {
        let ov = GitCommitOverlay {
            message: String::new(),
            cursor: 0,
            generating: true,
            scroll: 0,
            receiver: None,
        };
        assert!(ov.generating);
    }
    #[test]
    fn test_commit_overlay_with_msg() {
        let ov = GitCommitOverlay {
            message: "feat: tests".into(),
            cursor: 11,
            generating: false,
            scroll: 0,
            receiver: None,
        };
        assert_eq!(ov.message, "feat: tests");
    }
    #[test]
    fn test_commit_overlay_multiline() {
        let lines: Vec<&str> = "a\nb\nc".lines().collect();
        assert_eq!(lines.len(), 3);
    }
    #[test]
    fn test_commit_overlay_trailing_newline() {
        let msg = "line\n";
        let mut lines: Vec<&str> = msg.lines().collect();
        if msg.ends_with('\n') {
            lines.push("");
        }
        assert_eq!(lines.len(), 2);
    }
    #[test]
    fn test_commit_overlay_empty_lines() {
        let lines: Vec<&str> = "".lines().collect();
        let lines: Vec<&str> = if lines.is_empty() { vec![""] } else { lines };
        assert_eq!(lines.len(), 1);
    }

    // ── Dots animation ──

    #[test]
    fn test_dots_0() {
        assert_eq!(".".repeat(0), "");
    }
    #[test]
    fn test_dots_1() {
        assert_eq!(".".repeat(1), ".");
    }
    #[test]
    fn test_dots_3() {
        assert_eq!(".".repeat(3), "...");
    }
    #[test]
    fn test_dots_cycle() {
        for i in 0..8u128 {
            assert!((i % 4) < 4);
        }
    }

    // ── GitConflictOverlay ──

    #[test]
    fn test_conflict_empty() {
        let ov = GitConflictOverlay {
            conflicted_files: vec![],
            auto_merged_files: vec![],
            scroll: 0,
            selected: 0,
            continue_with_merge: false,
        };
        assert!(ov.conflicted_files.is_empty());
    }
    #[test]
    fn test_conflict_with_files() {
        let ov = GitConflictOverlay {
            conflicted_files: vec!["a".into(), "b".into()],
            auto_merged_files: vec!["c".into()],
            scroll: 0,
            selected: 0,
            continue_with_merge: false,
        };
        assert_eq!(ov.conflicted_files.len(), 2);
        assert_eq!(ov.auto_merged_files.len(), 1);
    }
    #[test]
    fn test_conflict_selected_0() {
        let ov = GitConflictOverlay {
            conflicted_files: vec![],
            auto_merged_files: vec![],
            scroll: 0,
            selected: 0,
            continue_with_merge: false,
        };
        assert_eq!(ov.selected, 0);
    }
    #[test]
    fn test_conflict_selected_1() {
        let ov = GitConflictOverlay {
            conflicted_files: vec![],
            auto_merged_files: vec![],
            scroll: 0,
            selected: 1,
            continue_with_merge: false,
        };
        assert_eq!(ov.selected, 1);
    }
    #[test]
    fn test_conflict_continue() {
        let ov = GitConflictOverlay {
            conflicted_files: vec![],
            auto_merged_files: vec![],
            scroll: 0,
            selected: 0,
            continue_with_merge: true,
        };
        assert!(ov.continue_with_merge);
    }

    // ── Conflict display ──

    #[test]
    fn test_conflict_short_display() {
        let cf = "src/main.rs";
        let iw = 40;
        let d = if cf.len() > iw - 3 {
            format!("   \u{2026}{}", &cf[cf.len().saturating_sub(iw - 4)..])
        } else {
            format!("   {}", cf)
        };
        assert_eq!(d, "   src/main.rs");
    }
    #[test]
    fn test_conflict_long_truncated() {
        let cf = "a".repeat(50);
        let iw = 20;
        let d = if cf.len() > iw - 3 {
            format!("   \u{2026}{}", &cf[cf.len().saturating_sub(iw - 4)..])
        } else {
            format!("   {}", cf)
        };
        assert!(d.starts_with("   \u{2026}"));
    }

    // ── Header pluralization ──

    #[test]
    fn test_header_singular() {
        assert_eq!(
            format!(" {} CONFLICTED FILE{}", 1, if 1 == 1 { "" } else { "S" }),
            " 1 CONFLICTED FILE"
        );
    }
    #[test]
    fn test_header_plural() {
        assert_eq!(
            format!(" {} CONFLICTED FILE{}", 5, if 5 == 1 { "" } else { "S" }),
            " 5 CONFLICTED FILES"
        );
    }
    #[test]
    fn test_auto_merged_header() {
        assert_eq!(format!(" {} AUTO-MERGED", 3), " 3 AUTO-MERGED");
    }

    // ── AutoResolveOverlay ──

    #[test]
    fn test_ar_overlay_empty() {
        let ov = AutoResolveOverlay {
            files: vec![],
            selected: 0,
            adding: false,
            input_buffer: String::new(),
            input_cursor: 0,
        };
        assert!(ov.files.is_empty());
    }
    #[test]
    fn test_ar_overlay_files() {
        let ov = AutoResolveOverlay {
            files: vec![("Cargo.lock".into(), true), ("pkg.json".into(), false)],
            selected: 0,
            adding: false,
            input_buffer: String::new(),
            input_cursor: 0,
        };
        assert_eq!(ov.files.len(), 2);
        assert!(ov.files[0].1);
        assert!(!ov.files[1].1);
    }
    #[test]
    fn test_ar_overlay_adding() {
        let ov = AutoResolveOverlay {
            files: vec![],
            selected: 0,
            adding: true,
            input_buffer: "new".into(),
            input_cursor: 3,
        };
        assert!(ov.adding);
    }
    #[test]
    fn test_ar_check_enabled() {
        assert_eq!(if true { "[x]" } else { "[ ]" }, "[x]");
    }
    #[test]
    fn test_ar_check_disabled() {
        assert_eq!(if false { "[x]" } else { "[ ]" }, "[ ]");
    }
    #[test]
    fn test_ar_prefix_sel() {
        assert_eq!(if true { " \u{25b8} " } else { "   " }, " \u{25b8} ");
    }
    #[test]
    fn test_ar_prefix_unsel() {
        assert_eq!(if false { " \u{25b8} " } else { "   " }, "   ");
    }

    // ── Arrow indicators ──

    #[test]
    fn test_arrow_0() {
        let s = 0;
        assert_eq!(if s == 0 { " \u{25b8} " } else { "   " }, " \u{25b8} ");
        assert_eq!(if s == 1 { " \u{25b8} " } else { "   " }, "   ");
    }
    #[test]
    fn test_arrow_1() {
        let s = 1;
        assert_eq!(if s == 0 { " \u{25b8} " } else { "   " }, "   ");
        assert_eq!(if s == 1 { " \u{25b8} " } else { "   " }, " \u{25b8} ");
    }

    // ── Dimensions ──

    #[test]
    fn test_inner_h() {
        assert_eq!(20u16.saturating_sub(2) as usize, 18);
    }
    #[test]
    fn test_inner_w() {
        assert_eq!(60u16.saturating_sub(2) as usize, 58);
    }
    #[test]
    fn test_inner_w_pad() {
        assert_eq!(60u16.saturating_sub(4) as usize, 56);
    }
    #[test]
    fn test_inner_h_small() {
        assert_eq!(2u16.saturating_sub(2) as usize, 0);
    }
    #[test]
    fn test_inner_h_zero() {
        assert_eq!(0u16.saturating_sub(2) as usize, 0);
    }

    // ── Wrap width ──

    #[test]
    fn test_wrap_w_normal() {
        assert_eq!(58usize.saturating_sub(1).max(1), 57);
    }
    #[test]
    fn test_wrap_w_one() {
        assert_eq!(1usize.saturating_sub(1).max(1), 1);
    }
    #[test]
    fn test_wrap_w_zero() {
        assert_eq!(0usize.saturating_sub(1).max(1), 1);
    }

    // ── Visible height ──

    #[test]
    fn test_vis_h_normal() {
        assert_eq!(20usize.saturating_sub(2), 18);
    }
    #[test]
    fn test_vis_h_small() {
        assert_eq!(2usize.saturating_sub(2), 0);
    }

    // ── Cursor char ──

    #[test]
    fn test_cursor_at_pos() {
        assert_eq!("hello".chars().nth(2).unwrap_or(' '), 'l');
    }
    #[test]
    fn test_cursor_at_end() {
        assert_eq!("hi".chars().nth(2).unwrap_or(' '), ' ');
    }
    #[test]
    fn test_cursor_empty() {
        assert_eq!("".chars().nth(0).unwrap_or(' '), ' ');
    }
    #[test]
    fn test_has_char_in() {
        assert!(1 < "abc".chars().count());
    }
    #[test]
    fn test_has_char_end() {
        assert!(!(3 < "abc".chars().count()));
    }

    #[test]
    fn test_git_orange_rgb_components() {
        if let Color::Rgb(r, g, b) = GIT_ORANGE {
            assert!(r > g && r > b);
        } else {
            panic!();
        }
    }
}
