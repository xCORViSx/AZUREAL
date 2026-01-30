//! Status bar rendering

use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};

use crate::app::{App, Focus, ViewMode};
use super::util::truncate;

/// Draw the status bar at the bottom
pub fn draw_status(f: &mut Frame, app: &App, area: Rect) {
    let mut status_spans = Vec::new();

    // Session info (left side)
    if let Some(session) = app.current_session() {
        let status = session.status(&app.running_sessions);
        let status_color = status.color();
        status_spans.push(Span::styled(
            format!("{} ", status.symbol()),
            Style::default().fg(status_color),
        ));

        status_spans.push(Span::styled(
            truncate(session.name(), 25),
            Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
        ));

        status_spans.push(Span::raw(" "));
        status_spans.push(Span::styled(
            format!("({})", session.branch_name),
            Style::default().fg(Color::Cyan),
        ));
    } else {
        status_spans.push(Span::styled(
            "No session selected",
            Style::default().fg(Color::Gray),
        ));
    }

    status_spans.push(Span::raw(" │ "));

    // View mode indicator
    let view_text = match app.view_mode {
        ViewMode::Output => "Output",
        ViewMode::Diff => "Diff",
        ViewMode::Messages => "Messages",
        ViewMode::Rebase => "Rebase",
    };
    status_spans.push(Span::styled(view_text, Style::default().fg(Color::Yellow)));

    status_spans.push(Span::raw(" │ "));

    // Help text or status message
    let help_text = if let Some(ref msg) = app.status_message {
        msg.clone()
    } else {
        match (app.focus, app.view_mode) {
            (Focus::Worktrees, _) => {
                if app.is_current_session_running() {
                    "?:help  Space:actions  n:new  b:branches  i:inprompt  s:stop  d:diff  r:rebase  R:status  a:archive  Tab/Shift+Tab:switch"
                } else {
                    "?:help  Space:actions  n:new  b:branches  d:diff  r:rebase  R:status  a:archive  Enter:start  Tab/Shift+Tab:switch"
                }
            }
            (Focus::Output, ViewMode::Diff) => "?:help  j/k:scroll  s:save  o:output  Esc:back",
            (Focus::Output, ViewMode::Rebase) => "?:help  j/k:select  o:ours  t:theirs  c:continue  s:skip  A:abort  Enter:diff  Esc:back",
            (Focus::Output, _) => "?:help  j/k:scroll  Ctrl+d/u:half-page  Ctrl+f/b:full-page  o:output  d:diff  R:rebase  Esc:back",
            (Focus::Input, _) => "?:help  Enter:submit  Esc:cancel  Tab/Shift+Tab:switch",
            (Focus::WorktreeCreation, _) => "Ctrl+Enter:submit  Esc:cancel  Enter:newline",
            (Focus::BranchDialog, _) => "j/k:select  Enter:confirm  Esc:cancel  type to filter",
            (Focus::FileTree, _) => "?:help  j/k:navigate  Enter:open  h/l:collapse/expand  Space:toggle  Tab:switch",
            (Focus::Viewer, _) => "?:help  j/k:scroll  Ctrl+d/u:half-page  g/G:top/bottom  Esc:close  Tab:switch",
        }.to_string()
    };
    status_spans.push(Span::styled(help_text, Style::default().fg(Color::Gray)));

    let status = Paragraph::new(Line::from(status_spans))
        .style(Style::default().bg(Color::Reset));

    f.render_widget(status, area);
}
