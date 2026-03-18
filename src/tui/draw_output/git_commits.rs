//! Git panel commit log rendering.
//!
//! Scrollable list of recent commits shown when the git actions panel
//! takes over the session pane area.

use ratatui::{
    layout::{Alignment, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Paragraph},
    Frame,
};

use crate::tui::util::{GIT_BROWN, GIT_ORANGE};

/// Git panel commit log — scrollable list of recent commits.
/// Returns the computed commit_scroll for writeback.
pub(super) fn draw_git_commits(
    f: &mut Frame,
    panel: &crate::app::types::GitActionsPanel,
    area: Rect,
) -> usize {
    let focused = panel.focused_pane == 2;
    let inner_h = area.height.saturating_sub(2) as usize;
    let inner_w = area.width.saturating_sub(2) as usize;
    let mut lines: Vec<Line> = Vec::new();

    let computed_scroll;
    if panel.commits.is_empty() {
        computed_scroll = 0;
        lines.push(Line::from(Span::styled(
            " No commits",
            Style::default().fg(Color::DarkGray),
        )));
    } else {
        // Adjust scroll so selected commit is visible
        computed_scroll = if panel.selected_commit < panel.commit_scroll {
            panel.selected_commit
        } else if panel.selected_commit >= panel.commit_scroll + inner_h {
            panel
                .selected_commit
                .saturating_sub(inner_h.saturating_sub(1))
        } else {
            panel.commit_scroll
        };
        let scroll = computed_scroll;

        for (i, commit) in panel.commits.iter().enumerate().skip(scroll).take(inner_h) {
            let selected = focused && i == panel.selected_commit;
            let prefix = if selected { " \u{25b8} " } else { "   " };

            // Green for unpushed, dim for pushed
            let hash_color = if !commit.is_pushed {
                Color::Green
            } else {
                Color::DarkGray
            };
            let subject_color = if selected {
                GIT_ORANGE
            } else if !commit.is_pushed {
                Color::Green
            } else {
                Color::White
            };
            let subject_mod = if selected {
                Modifier::BOLD
            } else {
                Modifier::empty()
            };

            // Truncate subject to fit: prefix(3) + hash(7) + space(1) + subject
            let subject_budget = inner_w.saturating_sub(prefix.len() + 8);
            let subject_display = if commit.subject.len() > subject_budget {
                format!(
                    "{}\u{2026}",
                    &commit.subject[..subject_budget.saturating_sub(1)]
                )
            } else {
                commit.subject.clone()
            };

            lines.push(Line::from(vec![
                Span::styled(prefix, Style::default()),
                Span::styled(&commit.hash, Style::default().fg(hash_color)),
                Span::raw(" "),
                Span::styled(
                    subject_display,
                    Style::default().fg(subject_color).add_modifier(subject_mod),
                ),
            ]));
        }
    }

    let title = format!(" Commits ({}) ", panel.commits.len());
    let border_style = if focused {
        Style::default().fg(GIT_ORANGE).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(GIT_BROWN)
    };
    let mut block = Block::default()
        .title(Span::styled(
            title,
            Style::default()
                .fg(if focused { GIT_ORANGE } else { GIT_BROWN })
                .add_modifier(if focused {
                    Modifier::BOLD
                } else {
                    Modifier::empty()
                }),
        ))
        .borders(Borders::ALL)
        .border_type(if focused {
            BorderType::Double
        } else {
            BorderType::Plain
        })
        .border_style(border_style);

    // Bottom border: divergence badges for main and remote
    let mut bottom_spans: Vec<Span> = Vec::new();
    // Main divergence (feature branches only)
    if !panel.is_on_main {
        let behind = panel.commits_behind_main;
        let ahead = panel.commits_ahead_main;
        if behind > 0 || ahead > 0 {
            let mut parts = Vec::new();
            if ahead > 0 {
                parts.push(format!("↑{}", ahead));
            }
            if behind > 0 {
                parts.push(format!("↓{}", behind));
            }
            let label = format!(" {} main ", parts.join(" "));
            let color = if behind > 0 { Color::Red } else { Color::Green };
            bottom_spans.push(Span::styled(
                label,
                Style::default()
                    .fg(Color::White)
                    .bg(color)
                    .add_modifier(Modifier::BOLD),
            ));
        }
    }
    // Remote divergence (any branch with upstream)
    {
        let behind = panel.commits_behind_remote;
        let ahead = panel.commits_ahead_remote;
        if behind > 0 || ahead > 0 {
            if !bottom_spans.is_empty() {
                bottom_spans.push(Span::raw(" "));
            }
            let mut parts = Vec::new();
            if ahead > 0 {
                parts.push(format!("↑{}", ahead));
            }
            if behind > 0 {
                parts.push(format!("↓{}", behind));
            }
            let label = format!(" {} remote ", parts.join(" "));
            let color = if behind > 0 {
                Color::Yellow
            } else {
                Color::Cyan
            };
            bottom_spans.push(Span::styled(
                label,
                Style::default()
                    .fg(Color::Black)
                    .bg(color)
                    .add_modifier(Modifier::BOLD),
            ));
        }
    }
    if !bottom_spans.is_empty() {
        block = block.title_bottom(Line::from(bottom_spans).alignment(Alignment::Right));
    }

    f.render_widget(Paragraph::new(lines).block(block), area);
    computed_scroll
}
