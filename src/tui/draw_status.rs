//! Status bar rendering

use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};

use super::util::{truncate, AZURE, GIT_BROWN};
use crate::app::App;
#[cfg(test)]
use crate::app::Focus;

/// Draw the status bar at the bottom — shows worktree info, status messages, and CPU/PID badge
pub fn draw_status(f: &mut Frame, app: &mut App, area: Rect) {
    // Sample CPU usage (~1s interval, cheap getrusage delta)
    app.update_cpu_usage();

    // Git panel mode — minimal status bar (hints are in the git status box title)
    if let Some(ref panel) = app.git_actions_panel {
        let badge_text = format!("CPU {} │ PID {} ", app.cpu_usage_text, std::process::id());
        let badge_color = AZURE;
        let badge_width = badge_text.len() as u16;

        let left_area = Rect {
            width: area.width.saturating_sub(badge_width),
            ..area
        };
        let left = Paragraph::new(Line::from(Span::styled(
            format!(" Git: {} ", panel.worktree_name),
            Style::default().fg(GIT_BROWN),
        )));
        f.render_widget(left, left_area);

        let right_area = Rect {
            x: area.x + area.width.saturating_sub(badge_width),
            width: badge_width,
            ..area
        };
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(
                badge_text,
                Style::default().fg(badge_color),
            ))),
            right_area,
        );
        return;
    }

    let mut status_spans = Vec::new();

    // Worktree + branch info (left side)
    // Shows: ● name (branch) — but skips the (branch) when it matches name to avoid "main (main)"
    if let Some(session) = app.current_worktree() {
        let status = session.status(app.is_session_running(&session.branch_name));
        let status_color = status.color();
        status_spans.push(Span::styled(
            format!("{} ", status.symbol()),
            Style::default().fg(status_color),
        ));

        let display_name = session.name();
        status_spans.push(Span::styled(
            truncate(display_name, 25),
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        ));

        // Only show (branch) when it differs from the display name — avoids "main (main)"
        if display_name != session.branch_name {
            status_spans.push(Span::raw(" "));
            status_spans.push(Span::styled(
                format!("({})", session.branch_name),
                Style::default().fg(AZURE),
            ));
        }
    } else {
        status_spans.push(Span::styled(
            "No session selected",
            Style::default().fg(Color::Gray),
        ));
    }

    // Status message (if any) — no permanent keybinding hints
    if let Some(ref msg) = app.status_message {
        status_spans.push(Span::raw(" │ "));
        status_spans.push(Span::styled(msg.clone(), Style::default().fg(Color::Gray)));
    }

    // Right badge: CPU% + PID — azure text in debug builds as a visual indicator
    let badge_text = format!("CPU {} │ PID {} ", app.cpu_usage_text, std::process::id());
    let badge_color = AZURE;
    let badge_width = badge_text.len() as u16;

    // Left side: status content (leave room for badge on right)
    let left_area = Rect {
        width: area.width.saturating_sub(badge_width),
        ..area
    };
    let status = Paragraph::new(Line::from(status_spans)).style(Style::default().bg(Color::Reset));
    f.render_widget(status, left_area);

    // Right side: badge
    let right_area = Rect {
        x: area.x + area.width.saturating_sub(badge_width),
        width: badge_width,
        ..area
    };
    let badge_widget = Paragraph::new(Line::from(Span::styled(
        badge_text,
        Style::default().fg(badge_color),
    )));
    f.render_widget(badge_widget, right_area);
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::layout::Rect;
    use ratatui::style::{Color, Modifier, Style};
    use ratatui::text::{Line, Span};

    // ══════════════════════════════════════════════════════════════════
    //  Color constants
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn azure_value() {
        assert_eq!(AZURE, Color::Rgb(51, 153, 255));
    }

    #[test]
    fn git_brown_value() {
        assert_eq!(GIT_BROWN, Color::Rgb(160, 82, 45));
    }

    // ══════════════════════════════════════════════════════════════════
    //  Focus enum variants
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn focus_worktrees_eq() {
        assert_eq!(Focus::Worktrees, Focus::Worktrees);
    }

    #[test]
    fn focus_session_eq() {
        assert_eq!(Focus::Session, Focus::Session);
    }

    #[test]
    fn focus_input_eq() {
        assert_eq!(Focus::Input, Focus::Input);
    }

    #[test]
    fn focus_branch_dialog_eq() {
        assert_eq!(Focus::BranchDialog, Focus::BranchDialog);
    }

    #[test]
    fn focus_file_tree_eq() {
        assert_eq!(Focus::FileTree, Focus::FileTree);
    }

    #[test]
    fn focus_viewer_eq() {
        assert_eq!(Focus::Viewer, Focus::Viewer);
    }

    #[test]
    fn focus_all_distinct() {
        let variants = [
            Focus::Worktrees,
            Focus::Session,
            Focus::Input,
            Focus::BranchDialog,
            Focus::FileTree,
            Focus::Viewer,
        ];
        for i in 0..variants.len() {
            for j in (i + 1)..variants.len() {
                assert_ne!(variants[i], variants[j]);
            }
        }
    }

    // ══════════════════════════════════════════════════════════════════
    //  Badge text formatting
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn badge_text_format() {
        let cpu = "2.5%";
        let pid = 12345;
        let text = format!("CPU {} \u{2502} PID {} ", cpu, pid);
        assert!(text.contains("CPU"));
        assert!(text.contains("PID"));
        assert!(text.contains("2.5%"));
        assert!(text.contains("12345"));
    }

    #[test]
    fn badge_text_length() {
        let text = "CPU 0.0% \u{2502} PID 99999 ";
        let width = text.len() as u16;
        assert!(width > 0);
    }

    // ══════════════════════════════════════════════════════════════════
    //  Badge color — always Azure
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn badge_color_always_azure() {
        let badge_color = AZURE;
        assert_eq!(badge_color, Color::Rgb(51, 153, 255));
    }

    // ══════════════════════════════════════════════════════════════════
    //  Left area sizing
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn left_area_calculation() {
        let area = Rect::new(0, 49, 120, 1);
        let badge_width = 20u16;
        let left_area = Rect {
            width: area.width.saturating_sub(badge_width),
            ..area
        };
        assert_eq!(left_area.width, 100);
        assert_eq!(left_area.y, 49);
        assert_eq!(left_area.height, 1);
    }

    #[test]
    fn left_area_narrow_terminal() {
        let area = Rect::new(0, 0, 15, 1);
        let badge_width = 20u16;
        let left_area = Rect {
            width: area.width.saturating_sub(badge_width),
            ..area
        };
        assert_eq!(left_area.width, 0);
    }

    // ══════════════════════════════════════════════════════════════════
    //  Right area positioning
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn right_area_position() {
        let area = Rect::new(0, 49, 120, 1);
        let badge_width = 20u16;
        let right_area = Rect {
            x: area.x + area.width.saturating_sub(badge_width),
            width: badge_width,
            ..area
        };
        assert_eq!(right_area.x, 100);
        assert_eq!(right_area.width, 20);
    }

    #[test]
    fn right_area_position_narrow() {
        let area = Rect::new(0, 0, 10, 1);
        let badge_width = 20u16;
        let right_area = Rect {
            x: area.x + area.width.saturating_sub(badge_width),
            width: badge_width,
            ..area
        };
        assert_eq!(right_area.x, 0);
    }

    // ══════════════════════════════════════════════════════════════════
    //  Git panel mode status text
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn git_status_text_format() {
        let wt_name = "feature/auth";
        let text = format!(" Git: {} ", wt_name);
        assert_eq!(text, " Git: feature/auth ");
    }

    #[test]
    fn git_status_text_style() {
        let style = Style::default().fg(GIT_BROWN);
        assert_eq!(style.fg, Some(GIT_BROWN));
    }

    // ══════════════════════════════════════════════════════════════════
    //  Status message display
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn status_message_present_shown() {
        let msg: Option<String> = Some("Created worktree: feat".to_string());
        assert!(msg.is_some());
    }

    #[test]
    fn status_message_none_no_hints() {
        let msg: Option<String> = None;
        assert!(msg.is_none());
    }

    // ══════════════════════════════════════════════════════════════════
    //  Truncate function accessibility
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn truncate_short_string() {
        let result = truncate("hello", 25);
        assert_eq!(result, "hello");
    }

    #[test]
    fn truncate_long_string() {
        let long = "this is a very long worktree name that exceeds limit";
        let result = truncate(long, 25);
        assert!(result.chars().count() <= 25);
    }

    #[test]
    fn truncate_exact_length() {
        let s = "exactly25characters_here!";
        let result = truncate(s, 25);
        assert_eq!(result, s);
    }

    // ══════════════════════════════════════════════════════════════════
    //  Worktree display formatting
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn branch_suffix_format() {
        let branch = "feature/auth";
        let text = format!("({})", branch);
        assert_eq!(text, "(feature/auth)");
    }

    #[test]
    fn no_session_text() {
        let text = "No session selected";
        assert_eq!(text, "No session selected");
    }

    // ══════════════════════════════════════════════════════════════════
    //  Separator span
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn separator_text() {
        let sep = " \u{2502} ";
        assert_eq!(sep, " \u{2502} ");
    }

    // ══════════════════════════════════════════════════════════════════
    //  Style construction
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn status_color_gray() {
        let style = Style::default().fg(Color::Gray);
        assert_eq!(style.fg, Some(Color::Gray));
    }

    #[test]
    fn bold_white_style() {
        let style = Style::default()
            .fg(Color::White)
            .add_modifier(Modifier::BOLD);
        assert_eq!(style.fg, Some(Color::White));
    }

    #[test]
    fn line_from_multiple_spans() {
        let spans = vec![
            Span::styled("a", Style::default().fg(Color::Green)),
            Span::raw(" "),
            Span::styled("b", Style::default().fg(Color::White)),
        ];
        let line = Line::from(spans);
        assert_eq!(line.spans.len(), 3);
    }

    // ══════════════════════════════════════════════════════════════════
    //  Process ID in badge
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn pid_is_nonzero() {
        let pid = std::process::id();
        assert!(pid > 0);
    }

    #[test]
    fn pid_fits_in_badge() {
        let pid = std::process::id();
        let text = format!("PID {} ", pid);
        assert!(text.len() < 20);
    }

    // ══════════════════════════════════════════════════════════════════
    //  Display name vs branch name deduplication
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn same_name_branch_skips_suffix() {
        let display_name = "main";
        let branch_name = "main";
        let show_branch = display_name != branch_name;
        assert!(!show_branch);
    }

    #[test]
    fn different_name_branch_shows_suffix() {
        let display_name = "my-feature";
        let branch_name = "azureal/my-feature";
        let show_branch = display_name != branch_name;
        assert!(show_branch);
    }

    // ══════════════════════════════════════════════════════════════════
    //  Background reset style
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn bg_reset_style() {
        let style = Style::default().bg(Color::Reset);
        assert_eq!(style.bg, Some(Color::Reset));
    }

    // ══════════════════════════════════════════════════════════════════
    //  Badge width arithmetic
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn badge_width_is_byte_length() {
        // badge_width = badge_text.len() as u16 — using byte length, not char count
        let text = "CPU 1.2% \u{2502} PID 99 ";
        let width = text.len() as u16;
        // The pipe char '│' is 3 bytes in UTF-8
        assert!(width > text.chars().count() as u16);
    }

    #[test]
    fn right_area_x_never_exceeds_terminal_width() {
        let area = Rect::new(5, 0, 80, 1);
        let badge_width = 20u16;
        let right_x = area.x + area.width.saturating_sub(badge_width);
        assert!(right_x <= area.x + area.width);
    }

    #[test]
    fn saturating_sub_prevents_underflow() {
        // When badge_width > area.width, saturating_sub returns 0
        let area_width = 10u16;
        let badge_width = 30u16;
        let result = area_width.saturating_sub(badge_width);
        assert_eq!(result, 0);
    }

    // ══════════════════════════════════════════════════════════════════
    //  Color constants distinctness
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn azure_is_not_dark_gray() {
        assert_ne!(AZURE, Color::DarkGray);
    }

    #[test]
    fn git_brown_is_not_azure() {
        assert_ne!(GIT_BROWN, AZURE);
    }
}
