//! Status bar rendering

use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};

use crate::app::{App, Focus};
use super::util::{truncate, GIT_BROWN, AZURE};

/// Draw the status bar at the bottom — shows worktree info, contextual help hints, and CPU/PID badge
pub fn draw_status(f: &mut Frame, app: &mut App, area: Rect) {
    // Sample CPU usage (~1s interval, cheap getrusage delta)
    app.update_cpu_usage();

    // Git panel mode — minimal status bar (hints are in the git status box title)
    if let Some(ref panel) = app.git_actions_panel {
        let badge_text = format!("CPU {} │ PID {} ", app.cpu_usage_text, std::process::id());
        let badge_color = if cfg!(debug_assertions) { AZURE } else { Color::DarkGray };
        let badge_width = badge_text.len() as u16;

        let left_area = Rect { width: area.width.saturating_sub(badge_width), ..area };
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
        f.render_widget(Paragraph::new(Line::from(
            Span::styled(badge_text, Style::default().fg(badge_color))
        )), right_area);
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
            Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
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

    status_spans.push(Span::raw(" │ "));

    // Contextual help hints — shows relevant keybindings for current focus/mode
    let help_text = if let Some(ref msg) = app.status_message {
        msg.clone()
    } else {
        match (app.focus, app.view_mode) {
            (Focus::Worktrees, _) => {
                if app.is_current_session_running() {
                    "?:help  f:files  n:new  b:branches  r:run  g:godfiles  G:git  P:projects  ⌃c:cancel  Tab:switch"
                } else {
                    "?:help  f:files  n:new  b:branches  r:run  g:godfiles  G:git  P:projects  Enter:start  Tab:switch"
                }
            }
            (Focus::Output, _) => "?:help  j/k:scroll  J/K:page  ⌥↑/↓:top/bottom  s:sessions  /:search  Esc:back",
            (Focus::Input, _) => "?:help  Enter:submit  Esc:cancel  Tab/⇧Tab:switch",
            (Focus::WorktreeCreation, _) => "Enter:create  Esc:cancel",
            (Focus::BranchDialog, _) => "j/k:select  Enter:switch/create  Esc:cancel  type to filter",
            (Focus::FileTree, _) => "?:help  j/k:navigate  Enter:open  h/l:collapse/expand  Space:toggle  f/Esc:back",
            (Focus::Viewer, _) => "?:help  j/k:scroll  J/K:page  ⌥↑/↓:top/bottom  e:edit  Esc:close  Tab:switch",
        }.to_string()
    };
    status_spans.push(Span::styled(help_text, Style::default().fg(Color::Gray)));

    // Right badge: CPU% + PID — azure text in debug builds as a visual indicator
    let badge_text = format!("CPU {} │ PID {} ", app.cpu_usage_text, std::process::id());
    let badge_color = if cfg!(debug_assertions) { AZURE } else { Color::DarkGray };
    let badge_width = badge_text.len() as u16;

    // Left side: status content (leave room for badge on right)
    let left_area = Rect { width: area.width.saturating_sub(badge_width), ..area };
    let status = Paragraph::new(Line::from(status_spans))
        .style(Style::default().bg(Color::Reset));
    f.render_widget(status, left_area);

    // Right side: badge
    let right_area = Rect {
        x: area.x + area.width.saturating_sub(badge_width),
        width: badge_width,
        ..area
    };
    let badge_widget = Paragraph::new(Line::from(
        Span::styled(badge_text, Style::default().fg(badge_color))
    ));
    f.render_widget(badge_widget, right_area);
}
