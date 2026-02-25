//! AZUREAL++ developer hub panel rendering — tabbed modal overlay with
//! Debug, Issues, and PRs tabs. Uses AZURE (#3399FF) accent color.

use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Paragraph},
    Frame,
};

use crate::app::App;
use crate::app::types::AzurealTab;
use super::util::AZURE;

const DIM: Color = Color::DarkGray;
const AZURE_DIM: Color = Color::Rgb(40, 100, 180);

/// Draw the AZUREAL++ panel as a centered modal overlay.
pub fn draw_azureal_panel(f: &mut Frame, app: &App) {
    let Some(ref panel) = app.azureal_panel else { return };
    let area = f.area();

    let modal_w = (area.width * 55 / 100).max(50).min(area.width);
    let modal_h = (area.height * 70 / 100).max(16).min(area.height);
    let modal = Rect::new(
        area.x + (area.width.saturating_sub(modal_w)) / 2,
        area.y + (area.height.saturating_sub(modal_h)) / 2,
        modal_w,
        modal_h,
    );
    f.render_widget(Clear, modal);

    let inner_w = modal.width.saturating_sub(4) as usize;
    let content_h = modal.height.saturating_sub(4) as usize; // border + tab bar + footer
    let mut lines: Vec<Line> = Vec::new();

    // ── Tab bar ──
    let tab_style = |t: AzurealTab| -> Style {
        if panel.tab == t {
            Style::default().fg(AZURE).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(DIM)
        }
    };
    lines.push(Line::from(vec![
        Span::raw("  "),
        Span::styled("[ Debug ]", tab_style(AzurealTab::Debug)),
        Span::raw("  "),
        Span::styled("[ Issues ]", tab_style(AzurealTab::Issues)),
        Span::raw("  "),
        Span::styled("[ PRs ]", tab_style(AzurealTab::PullRequests)),
    ]));
    lines.push(Line::from(""));

    // ── Tab content ──
    match panel.tab {
        AzurealTab::Debug => draw_debug_tab(&mut lines, panel, inner_w, content_h),
        AzurealTab::Issues => draw_issues_tab(&mut lines, panel, inner_w, content_h),
        AzurealTab::PullRequests => draw_prs_tab(&mut lines, panel, inner_w, content_h),
    }

    // ── Footer hints ──
    let footer = match panel.tab {
        AzurealTab::Debug => " Tab:switch  ⌃d/Enter:dump  v:view  d:delete  R:refresh  Esc ",
        AzurealTab::Issues => " Tab:switch  a:create  c:closed  o:browser  /:filter  R:refresh  Esc ",
        AzurealTab::PullRequests => " Tab:switch  a:create PR  o:browser  R:refresh  Esc ",
    };

    // Render
    let worktree_name = app.current_worktree().map(|w| w.name()).unwrap_or_default();
    let title = format!(" AZUREAL++: {} ", worktree_name);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::QuadrantOutside)
        .border_style(Style::default().fg(AZURE))
        .title(Span::styled(title, Style::default().fg(AZURE).add_modifier(Modifier::BOLD)))
        .title_bottom(Line::from(Span::styled(footer, Style::default().fg(AZURE_DIM))));

    let para = Paragraph::new(lines).block(block);
    f.render_widget(para, modal);
}

fn draw_debug_tab(lines: &mut Vec<Line>, panel: &crate::app::types::AzurealPlusPlusPanel, inner_w: usize, _content_h: usize) {
    lines.push(Line::from(Span::styled(
        "  DEBUG DUMPS",
        Style::default().fg(AZURE_DIM).add_modifier(Modifier::BOLD),
    )));
    lines.push(Line::from(""));

    if panel.dump_files.is_empty() {
        lines.push(Line::from(Span::styled(
            "    No debug dumps found. Press ⌃d or Enter to create one.",
            Style::default().fg(DIM),
        )));
    } else {
        for (i, (name, size, modified)) in panel.dump_files.iter().enumerate() {
            let selected = i == panel.dump_selected;
            let prefix = if selected { "  \u{25b8} " } else { "    " };
            let name_style = if selected {
                Style::default().fg(AZURE).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            let meta_style = if selected {
                Style::default().fg(AZURE)
            } else {
                Style::default().fg(DIM)
            };

            let size_str = if *size > 1024 * 1024 {
                format!("{:.1}MB", *size as f64 / (1024.0 * 1024.0))
            } else if *size > 1024 {
                format!("{:.1}KB", *size as f64 / 1024.0)
            } else {
                format!("{}B", size)
            };

            // Truncate name to fit
            let max_name = inner_w.saturating_sub(30);
            let display_name = if name.len() > max_name {
                format!("{}…", &name[..max_name.saturating_sub(1)])
            } else {
                name.clone()
            };

            lines.push(Line::from(vec![
                Span::styled(prefix, name_style),
                Span::styled(display_name, name_style),
                Span::styled(format!("  {}  {}", size_str, modified), meta_style),
            ]));
        }
    }

    // Inline naming input
    if let Some(ref name) = panel.dump_naming {
        lines.push(Line::from(""));
        let filename = if name.is_empty() {
            "debug-output".to_string()
        } else {
            format!("debug-output_{}", name)
        };
        lines.push(Line::from(vec![
            Span::styled("  Save as: ", Style::default().fg(AZURE)),
            Span::styled(format!(".azureal/{}", filename), Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
            Span::styled("\u{258f}", Style::default().fg(AZURE)), // cursor
        ]));
        lines.push(Line::from(Span::styled(
            "  Enter:save  Esc:cancel",
            Style::default().fg(DIM),
        )));
    }
}

fn draw_issues_tab<'a>(lines: &mut Vec<Line<'a>>, panel: &'a crate::app::types::AzurealPlusPlusPanel, inner_w: usize, content_h: usize) {
    // Issue creation form takes over the tab
    if let Some(ref create) = panel.issue_create {
        draw_issue_create_form(lines, create, inner_w);
        return;
    }

    // Issue detail view
    if panel.issue_detail_view {
        if let Some(issue) = panel.issues.get(panel.issue_selected) {
            draw_issue_detail(lines, issue, inner_w, content_h, panel.issue_detail_scroll);
        }
        return;
    }

    // Header
    let repo_label = if panel.upstream_repo.is_empty() {
        "no repo detected".to_string()
    } else {
        panel.upstream_repo.clone()
    };
    let open_count = panel.issues.iter().filter(|i| i.state == "OPEN").count();
    let closed_count = panel.issues.iter().filter(|i| i.state == "CLOSED").count();

    lines.push(Line::from(vec![
        Span::styled("  ISSUES  ", Style::default().fg(AZURE_DIM).add_modifier(Modifier::BOLD)),
        Span::styled(repo_label, Style::default().fg(DIM)),
        Span::raw("  "),
        Span::styled(format!("Open: {}", open_count), Style::default().fg(Color::Green)),
        Span::raw("  "),
        Span::styled(format!("Closed: {}", closed_count), Style::default().fg(Color::Red)),
    ]));

    // Filter bar
    if let Some(ref filter) = panel.issue_filter {
        lines.push(Line::from(vec![
            Span::styled("  Filter: ", Style::default().fg(Color::Yellow)),
            Span::styled(filter.as_str(), Style::default().fg(Color::White)),
            Span::styled("\u{258f}", Style::default().fg(Color::Yellow)),
        ]));
    }

    lines.push(Line::from(""));

    if panel.issues_loading {
        lines.push(Line::from(Span::styled(
            "    Loading issues...",
            Style::default().fg(DIM),
        )));
        return;
    }

    if panel.issues.is_empty() {
        lines.push(Line::from(Span::styled(
            "    No issues found. Press 'a' to create one.",
            Style::default().fg(DIM),
        )));
        return;
    }

    // Filtered issue list
    let filter_lower = panel.issue_filter.as_ref().map(|f| f.to_lowercase());
    let mut visible_idx = 0usize;
    for issue in &panel.issues {
        if let Some(ref f) = filter_lower {
            if !f.is_empty() && !issue.title.to_lowercase().contains(f) {
                continue;
            }
        }

        let selected = visible_idx == panel.issue_selected;
        let prefix = if selected { "  \u{25b8} " } else { "    " };

        let (num_color, title_style) = if issue.state == "OPEN" {
            (Color::Green, if selected { Style::default().fg(AZURE).add_modifier(Modifier::BOLD) } else { Style::default().fg(Color::White) })
        } else {
            (Color::Red, if selected { Style::default().fg(AZURE) } else { Style::default().fg(DIM) })
        };

        let mut spans = vec![
            Span::styled(prefix, title_style),
            Span::styled(format!("#{:<5}", issue.number), Style::default().fg(num_color)),
            Span::raw(" "),
        ];

        // Title — truncate to fit
        let label_space = inner_w.saturating_sub(20);
        let title = if issue.title.len() > label_space {
            format!("{}…", &issue.title[..label_space.saturating_sub(1)])
        } else {
            issue.title.clone()
        };
        spans.push(Span::styled(title, title_style));

        // Labels
        for label in issue.labels.iter().take(2) {
            spans.push(Span::raw(" "));
            spans.push(Span::styled(
                format!("[{}]", label),
                Style::default().fg(Color::Cyan),
            ));
        }

        lines.push(Line::from(spans));
        visible_idx += 1;
    }
}

fn draw_issue_detail<'a>(lines: &mut Vec<Line<'a>>, issue: &'a crate::app::types::GitHubIssue, _inner_w: usize, content_h: usize, scroll: usize) {
    let state_color = if issue.state == "OPEN" { Color::Green } else { Color::Red };
    lines.push(Line::from(vec![
        Span::styled(format!("  #{} ", issue.number), Style::default().fg(state_color).add_modifier(Modifier::BOLD)),
        Span::styled(issue.title.as_str(), Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
    ]));
    lines.push(Line::from(vec![
        Span::styled(format!("  @{}", issue.author), Style::default().fg(DIM)),
        Span::raw("  "),
        Span::styled(issue.created_at.as_str(), Style::default().fg(DIM)),
    ]));
    for label in &issue.labels {
        lines.push(Line::from(vec![
            Span::raw("  "),
            Span::styled(format!("[{}]", label), Style::default().fg(Color::Cyan)),
        ]));
    }
    lines.push(Line::from(""));

    // Body with scroll
    let body_lines: Vec<&str> = issue.body.lines().collect();
    let max_body = content_h.saturating_sub(lines.len() + 2);
    for line in body_lines.iter().skip(scroll).take(max_body) {
        lines.push(Line::from(Span::styled(
            format!("  {}", line),
            Style::default().fg(Color::White),
        )));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "  Esc:back  o:open in browser  j/k:scroll",
        Style::default().fg(DIM),
    )));
}

fn draw_issue_create_form<'a>(lines: &mut Vec<Line<'a>>, create: &'a crate::app::types::IssueCreateState, _inner_w: usize) {
    lines.push(Line::from(Span::styled(
        "  CREATE ISSUE",
        Style::default().fg(AZURE_DIM).add_modifier(Modifier::BOLD),
    )));
    lines.push(Line::from(""));

    let title_style = if create.cursor_in_title {
        Style::default().fg(AZURE).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::White)
    };
    let body_style = if !create.cursor_in_title {
        Style::default().fg(AZURE).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::White)
    };

    lines.push(Line::from(vec![
        Span::styled("  Title: ", Style::default().fg(DIM)),
        Span::styled(create.title.as_str(), title_style),
        if create.cursor_in_title { Span::styled("\u{258f}", Style::default().fg(AZURE)) } else { Span::raw("") },
    ]));
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled("  Body:", Style::default().fg(DIM))));
    for line in create.body.lines() {
        lines.push(Line::from(Span::styled(format!("  {}", line), body_style)));
    }
    if !create.cursor_in_title {
        if create.body.is_empty() || create.body.ends_with('\n') {
            lines.push(Line::from(Span::styled("  \u{258f}", Style::default().fg(AZURE))));
        }
    }

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "  Tab:switch field  ⌃Enter:submit  Esc:cancel",
        Style::default().fg(DIM),
    )));
}

fn draw_prs_tab<'a>(lines: &mut Vec<Line<'a>>, panel: &'a crate::app::types::AzurealPlusPlusPanel, inner_w: usize, _content_h: usize) {
    // PR creation form takes over the tab
    if let Some(ref create) = panel.pr_create {
        draw_pr_create_form(lines, create, inner_w);
        return;
    }

    // Header
    let fork_status = if let Some(ref owner) = panel.fork_owner {
        format!("Fork ({}) → {}", owner, panel.upstream_repo)
    } else if panel.upstream_repo.is_empty() {
        "No repo detected".to_string()
    } else {
        format!("Official repo: {}", panel.upstream_repo)
    };

    lines.push(Line::from(vec![
        Span::styled("  PULL REQUESTS  ", Style::default().fg(AZURE_DIM).add_modifier(Modifier::BOLD)),
        Span::styled(fork_status, Style::default().fg(DIM)),
    ]));
    lines.push(Line::from(""));

    if panel.prs_loading {
        lines.push(Line::from(Span::styled("    Loading PRs...", Style::default().fg(DIM))));
        return;
    }

    if panel.prs.is_empty() {
        lines.push(Line::from(Span::styled(
            "    No PRs found. Press 'a' to create one.",
            Style::default().fg(DIM),
        )));
        return;
    }

    for (i, pr) in panel.prs.iter().enumerate() {
        let selected = i == panel.pr_selected;
        let prefix = if selected { "  \u{25b8} " } else { "    " };

        let (state_color, state_label) = match pr.state.as_str() {
            "OPEN" => (Color::Green, "open"),
            "MERGED" => (Color::Magenta, "merged"),
            "CLOSED" => (Color::Red, "closed"),
            _ => (DIM, "?"),
        };

        let title_style = if selected {
            Style::default().fg(AZURE).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::White)
        };

        let max_title = inner_w.saturating_sub(30);
        let title = if pr.title.len() > max_title {
            format!("{}…", &pr.title[..max_title.saturating_sub(1)])
        } else {
            pr.title.clone()
        };

        lines.push(Line::from(vec![
            Span::styled(prefix, title_style),
            Span::styled(format!("#{:<5}", pr.number), Style::default().fg(state_color)),
            Span::styled(format!("[{}] ", state_label), Style::default().fg(state_color)),
            Span::styled(title, title_style),
            Span::raw(" "),
            Span::styled(pr.head_branch.as_str(), Style::default().fg(DIM)),
        ]));
    }
}

fn draw_pr_create_form<'a>(lines: &mut Vec<Line<'a>>, create: &'a crate::app::types::PrCreateState, _inner_w: usize) {
    lines.push(Line::from(Span::styled(
        "  CREATE PULL REQUEST",
        Style::default().fg(AZURE_DIM).add_modifier(Modifier::BOLD),
    )));
    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled("  Head: ", Style::default().fg(DIM)),
        Span::styled(create.head_branch.as_str(), Style::default().fg(Color::Cyan)),
    ]));
    lines.push(Line::from(""));

    let title_style = if create.cursor_in_title {
        Style::default().fg(AZURE).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::White)
    };
    let body_style = if !create.cursor_in_title {
        Style::default().fg(AZURE).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::White)
    };

    lines.push(Line::from(vec![
        Span::styled("  Title: ", Style::default().fg(DIM)),
        Span::styled(create.title.as_str(), title_style),
        if create.cursor_in_title { Span::styled("\u{258f}", Style::default().fg(AZURE)) } else { Span::raw("") },
    ]));
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled("  Body:", Style::default().fg(DIM))));
    for line in create.body.lines() {
        lines.push(Line::from(Span::styled(format!("  {}", line), body_style)));
    }
    if !create.cursor_in_title {
        if create.body.is_empty() || create.body.ends_with('\n') {
            lines.push(Line::from(Span::styled("  \u{258f}", Style::default().fg(AZURE))));
        }
    }

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "  Tab:switch field  ⌃Enter:submit  Esc:cancel",
        Style::default().fg(DIM),
    )));
}
