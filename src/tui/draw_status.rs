//! Status bar rendering

use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};

use crate::app::{App, Focus, ViewMode};
use super::util::{truncate, AZURE};

/// Draw the status bar at the bottom — shows worktree info, contextual help hints, and CPU/PID badge
pub fn draw_status(f: &mut Frame, app: &mut App, area: Rect) {
    // Sample CPU usage (~1s interval, cheap getrusage delta)
    app.update_cpu_usage();
    let mut status_spans = Vec::new();

    // Worktree + branch info (left side)
    // Shows: ● name (branch) — but skips the (branch) when it matches name to avoid "main (main)"
    if let Some(session) = app.current_session() {
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
            (Focus::Output, ViewMode::Diff) => "?:help  j/k:scroll  Esc:back",
            (Focus::Output, ViewMode::Rebase) => "?:help  j/k:select  o:ours  t:theirs  c:continue  s:skip  A:abort  Enter:diff  Esc:back",
            (Focus::Output, _) => "?:help  j/k:scroll  J/K:page  ⌥↑/↓:top/bottom  s:sessions  /:search  Esc:back",
            (Focus::Input, _) => "?:help  Enter:submit  Esc:cancel  Tab/⇧Tab:switch",
            (Focus::WorktreeCreation, _) => "⌃Enter:submit  Esc:cancel  Enter:newline",
            (Focus::BranchDialog, _) => "j/k:select  Enter:confirm  Esc:cancel  type to filter",
            (Focus::FileTree, _) => "?:help  j/k:navigate  Enter:open  h/l:collapse/expand  Space:toggle  f/Esc:back",
            (Focus::Viewer, _) => "?:help  j/k:scroll  J/K:page  ⌥↑/↓:top/bottom  e:edit  Esc:close  Tab:switch",
        }.to_string()
    };
    status_spans.push(Span::styled(help_text, Style::default().fg(Color::Gray)));

    // Right badge: CPU% + PID
    let badge_text = format!("CPU {} │ PID {} ", app.cpu_usage_text, std::process::id());
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
        Span::styled(badge_text, Style::default().fg(Color::DarkGray))
    ));
    f.render_widget(badge_widget, right_area);
}
