//! Rebase status view
//!
//! Renders git rebase state: progress, branches, conflicts, and available
//! commands. Displayed when the user switches to ViewMode::Rebase.

use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};

use crate::app::App;
use crate::models::RebaseState;
use super::super::util::AZURE;

/// Build rebase status content lines for display in the convo pane
pub fn draw_rebase_content(app: &App) -> Vec<Line<'static>> {
    let mut lines = Vec::new();

    let Some(ref status) = app.rebase_status else {
        lines.push(Line::from("No rebase in progress"));
        return lines;
    };

    let state_color = status.state.color();
    lines.push(Line::from(vec![
        Span::styled("State: ", Style::default().add_modifier(Modifier::BOLD)),
        Span::styled(
            format!("{} {}", status.state.symbol(), status.state.as_str()),
            Style::default().fg(state_color),
        ),
    ]));

    if let (Some(current), Some(total)) = (status.current_step, status.total_steps) {
        lines.push(Line::from(vec![
            Span::styled("Progress: ", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(format!("{}/{}", current, total)),
        ]));
    }

    if let Some(ref head) = status.head_name {
        lines.push(Line::from(vec![
            Span::styled("Rebasing: ", Style::default().add_modifier(Modifier::BOLD)),
            Span::styled(head.clone(), Style::default().fg(Color::Green)),
        ]));
    }

    if let Some(ref onto) = status.onto_branch {
        lines.push(Line::from(vec![
            Span::styled("Onto: ", Style::default().add_modifier(Modifier::BOLD)),
            Span::styled(onto.clone(), Style::default().fg(AZURE)),
        ]));
    }

    if let Some(ref commit) = status.current_commit {
        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::styled("Current commit: ", Style::default().add_modifier(Modifier::BOLD)),
            Span::styled(commit.clone(), Style::default().fg(Color::Yellow)),
        ]));
        if let Some(ref msg) = status.current_commit_message {
            lines.push(Line::from(vec![
                Span::raw("  "),
                Span::raw(msg.clone()),
            ]));
        }
    }

    if !status.conflicted_files.is_empty() {
        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::styled(
                format!("Conflicts ({}):", status.conflicted_files.len()),
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            ),
        ]));
        for (idx, file) in status.conflicted_files.iter().enumerate() {
            let is_selected = app.selected_conflict == Some(idx);
            let style = if is_selected {
                Style::default().bg(Color::DarkGray).fg(Color::White)
            } else {
                Style::default().fg(Color::Red)
            };
            let prefix = if is_selected { "\u{25b8} " } else { "  " };
            lines.push(Line::from(vec![
                Span::raw(prefix),
                Span::styled(file.clone(), style),
            ]));
        }
        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::styled("Commands: ", Style::default().add_modifier(Modifier::BOLD)),
        ]));
        lines.push(Line::from("  o: use ours (keep our changes)"));
        lines.push(Line::from("  t: use theirs (accept incoming)"));
        lines.push(Line::from("  Enter: view conflict diff"));
        lines.push(Line::from("  c: continue rebase"));
        lines.push(Line::from("  s: skip this commit"));
        lines.push(Line::from("  A: abort rebase"));
    } else if status.state == RebaseState::InProgress {
        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::styled("No conflicts. ", Style::default().fg(Color::Green)),
            Span::raw("Press 'c' to continue."),
        ]));
    }

    lines
}
