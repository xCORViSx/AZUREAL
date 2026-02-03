//! Wizard modal rendering

use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};

use crate::app::App;
use crate::wizard::{
    CreationWizard, WizardTab, WorktreeWizard, WorktreeField, WorktreeStep,
    SessionWizard, SessionField, SessionStep,
};

/// Draw the wizard modal overlay
pub fn draw_wizard_modal(f: &mut Frame, app: &App) {
    let Some(ref wizard) = app.creation_wizard else { return; };

    let area = f.area();
    let modal_width = area.width.min(80);
    let modal_height = area.height.min(25);
    let modal_x = (area.width - modal_width) / 2;
    let modal_y = (area.height - modal_height) / 2;

    let modal_area = Rect { x: modal_x, y: modal_y, width: modal_width, height: modal_height };

    // Clear background
    let background = Block::default().style(Style::default().bg(Color::Reset));
    f.render_widget(background, area);

    // Modal frame
    let modal_block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan))
        .title(" New... ")
        .style(Style::default().bg(Color::Reset));
    f.render_widget(modal_block, modal_area);

    // Inner content area
    let inner = Rect {
        x: modal_area.x + 2,
        y: modal_area.y + 2,
        width: modal_area.width.saturating_sub(4),
        height: modal_area.height.saturating_sub(4),
    };

    // Draw tabs at top
    draw_tabs(f, wizard, Rect { x: inner.x, y: inner.y, width: inner.width, height: 1 });

    // Content area below tabs
    let content_area = Rect {
        x: inner.x,
        y: inner.y + 2,
        width: inner.width,
        height: inner.height.saturating_sub(5),
    };

    // Draw content based on active tab
    match wizard.active_tab {
        WizardTab::Project => draw_coming_soon(f, "Project creation", content_area),
        WizardTab::Branch => draw_coming_soon(f, "Branch creation", content_area),
        WizardTab::Worktree => draw_worktree_content(f, app, &wizard.worktree, content_area),
        WizardTab::Session => draw_session_content(f, app, &wizard.session, content_area),
    }

    // Error messages
    let errors = match wizard.active_tab {
        WizardTab::Worktree => &wizard.worktree.errors,
        WizardTab::Session => &wizard.session.errors,
        _ => return draw_help_text(f, wizard, inner),
    };

    if !errors.is_empty() {
        let error_area = Rect {
            x: inner.x,
            y: inner.y + inner.height.saturating_sub(3),
            width: inner.width,
            height: 2,
        };
        let error = Paragraph::new(errors.join(", "))
            .style(Style::default().fg(Color::Red))
            .wrap(Wrap { trim: true });
        f.render_widget(error, error_area);
    }

    draw_help_text(f, wizard, inner);
}

fn draw_tabs(f: &mut Frame, wizard: &CreationWizard, area: Rect) {
    let tabs: Vec<Span> = WizardTab::all().iter().map(|tab| {
        let is_active = *tab == wizard.active_tab;
        let style = if is_active {
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Gray)
        };
        Span::styled(format!(" {} ", tab.name()), style)
    }).collect();

    let tabs_line = Line::from(tabs);
    let tabs_widget = Paragraph::new(tabs_line);
    f.render_widget(tabs_widget, area);
}

fn draw_coming_soon(f: &mut Frame, feature: &str, area: Rect) {
    let lines = vec![
        Line::from(""),
        Line::from(vec![
            Span::styled(format!("{} coming soon", feature), Style::default().fg(Color::Gray)),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled("Use ⌥Tab to switch tabs", Style::default().fg(Color::DarkGray)),
        ]),
    ];
    let para = Paragraph::new(lines);
    f.render_widget(para, area);
}

fn draw_help_text(f: &mut Frame, wizard: &CreationWizard, inner: Rect) {
    let help_area = Rect {
        x: inner.x,
        y: inner.y + inner.height.saturating_sub(1),
        width: inner.width,
        height: 1,
    };
    let help = Paragraph::new(wizard.help_text()).style(Style::default().fg(Color::Gray));
    f.render_widget(help, help_area);
}

fn draw_worktree_content(f: &mut Frame, app: &App, wizard: &WorktreeWizard, area: Rect) {
    // Progress indicator
    let (current_step, total_steps) = wizard.step_progress();
    let progress = Paragraph::new(format!("Step {} of {} - {}", current_step, total_steps, wizard.step_title()))
        .style(Style::default().fg(Color::Gray));
    f.render_widget(progress, Rect { x: area.x, y: area.y, width: area.width, height: 1 });

    let content_area = Rect {
        x: area.x,
        y: area.y + 2,
        width: area.width,
        height: area.height.saturating_sub(2),
    };

    match wizard.step {
        WorktreeStep::SelectProject => {
            let project_info = if let Some(ref project) = app.project {
                format!("Project: {} ({})", project.name, project.path.display())
            } else {
                "No project loaded".to_string()
            };
            let info = Paragraph::new(project_info)
                .style(Style::default().fg(Color::Cyan));
            f.render_widget(info, content_area);
        }
        WorktreeStep::EnterDetails => draw_worktree_details_input(f, wizard, content_area),
        WorktreeStep::Confirm => draw_worktree_confirmation(f, app, wizard, content_area),
    }
}

fn draw_worktree_details_input(f: &mut Frame, wizard: &WorktreeWizard, area: Rect) {
    // Worktree name field
    let name_focused = wizard.focused_field == WorktreeField::Name;
    let name_border_color = if name_focused { Color::Yellow } else { Color::Gray };
    let name_area = Rect { x: area.x, y: area.y, width: area.width, height: 3 };
    let name_input = Paragraph::new(wizard.worktree_name.as_str())
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Worktree Name ")
                .border_style(Style::default().fg(name_border_color))
        );
    f.render_widget(name_input, name_area);

    // Prompt field
    let prompt_focused = wizard.focused_field == WorktreeField::Prompt;
    let prompt_border_color = if prompt_focused { Color::Yellow } else { Color::Gray };
    let prompt_area = Rect { x: area.x, y: area.y + 4, width: area.width, height: 6 };
    let prompt_input = Paragraph::new(wizard.prompt.as_str())
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Initial Prompt ")
                .border_style(Style::default().fg(prompt_border_color))
        )
        .wrap(Wrap { trim: false });
    f.render_widget(prompt_input, prompt_area);

    // Cursor position based on focused field
    match wizard.focused_field {
        WorktreeField::Name => {
            let cursor_x = name_area.x + 1 + wizard.name_cursor as u16;
            f.set_cursor_position((cursor_x.min(name_area.x + name_area.width - 2), name_area.y + 1));
        }
        WorktreeField::Prompt => {
            let inner_width = prompt_area.width.saturating_sub(2) as usize;
            let cursor_x = prompt_area.x + 1 + (wizard.prompt_cursor % inner_width) as u16;
            let cursor_y = prompt_area.y + 1 + (wizard.prompt_cursor / inner_width) as u16;
            f.set_cursor_position((cursor_x, cursor_y.min(prompt_area.y + prompt_area.height - 2)));
        }
    }

    // Hint text
    let hint_area = Rect { x: area.x, y: area.y + 11, width: area.width, height: 1 };
    let hint = Paragraph::new("Tab to switch fields • Only alphanumeric, -, _ allowed in name")
        .style(Style::default().fg(Color::DarkGray));
    f.render_widget(hint, hint_area);
}

fn draw_worktree_confirmation(f: &mut Frame, app: &App, wizard: &WorktreeWizard, area: Rect) {
    let mut lines = vec![
        Line::from(vec![
            Span::styled("Ready to create worktree", Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
        ]),
        Line::from(""),
    ];

    if let Some(ref project) = app.project {
        lines.push(Line::from(vec![
            Span::styled("Project: ", Style::default().fg(Color::Gray)),
            Span::styled(&project.name, Style::default().fg(Color::Cyan)),
        ]));
        lines.push(Line::from(""));
    }

    lines.push(Line::from(vec![
        Span::styled("Worktree name: ", Style::default().fg(Color::Gray)),
        Span::styled(&wizard.worktree_name, Style::default().fg(Color::Cyan)),
    ]));
    lines.push(Line::from(vec![
        Span::styled("Branch: ", Style::default().fg(Color::Gray)),
        Span::styled(format!("azural/{}", wizard.final_worktree_name()), Style::default().fg(Color::Cyan)),
    ]));
    lines.push(Line::from(""));

    lines.push(Line::from(vec![
        Span::styled("Initial prompt:", Style::default().fg(Color::Gray)),
    ]));
    lines.push(Line::from(""));

    let prompt_wrapped = textwrap::wrap(&wizard.prompt, (area.width as usize).saturating_sub(4));
    for line in prompt_wrapped {
        lines.push(Line::from(vec![
            Span::styled(format!("  {}", line), Style::default().fg(Color::White)),
        ]));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled("Press Enter to create worktree and start Claude", Style::default().fg(Color::Green)),
    ]));

    let confirmation = Paragraph::new(lines).wrap(Wrap { trim: false });
    f.render_widget(confirmation, area);
}

fn draw_session_content(f: &mut Frame, app: &App, wizard: &SessionWizard, area: Rect) {
    // Progress indicator
    let (current_step, total_steps) = wizard.step_progress();
    let progress = Paragraph::new(format!("Step {} of {} - {}", current_step, total_steps, wizard.step_title()))
        .style(Style::default().fg(Color::Gray));
    f.render_widget(progress, Rect { x: area.x, y: area.y, width: area.width, height: 1 });

    let content_area = Rect {
        x: area.x,
        y: area.y + 2,
        width: area.width,
        height: area.height.saturating_sub(2),
    };

    match wizard.step {
        SessionStep::SelectWorktree => draw_session_worktree_select(f, app, wizard, content_area),
        SessionStep::EnterDetails => draw_session_details_input(f, wizard, content_area),
        SessionStep::Confirm => draw_session_confirmation(f, app, wizard, content_area),
    }
}

fn draw_session_worktree_select(f: &mut Frame, app: &App, wizard: &SessionWizard, area: Rect) {
    let mut lines = vec![
        Line::from(vec![
            Span::styled("Select worktree for new session:", Style::default().fg(Color::White)),
        ]),
        Line::from(""),
    ];

    for (idx, session) in app.sessions.iter().enumerate() {
        let is_selected = idx == wizard.selected_worktree_idx;
        let prefix = if is_selected { "> " } else { "  " };
        let style = if is_selected {
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Gray)
        };
        lines.push(Line::from(vec![
            Span::styled(format!("{}{}", prefix, session.name()), style),
        ]));
    }

    if app.sessions.is_empty() {
        lines.push(Line::from(vec![
            Span::styled("  (No worktrees available)", Style::default().fg(Color::DarkGray)),
        ]));
    }

    let list = Paragraph::new(lines);
    f.render_widget(list, area);
}

fn draw_session_details_input(f: &mut Frame, wizard: &SessionWizard, area: Rect) {
    // Session name field
    let name_focused = wizard.focused_field == SessionField::Name;
    let name_border_color = if name_focused { Color::Yellow } else { Color::Gray };
    let name_area = Rect { x: area.x, y: area.y, width: area.width, height: 3 };
    let name_input = Paragraph::new(wizard.session_name.as_str())
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Session Name ")
                .border_style(Style::default().fg(name_border_color))
        );
    f.render_widget(name_input, name_area);

    // Prompt field
    let prompt_focused = wizard.focused_field == SessionField::Prompt;
    let prompt_border_color = if prompt_focused { Color::Yellow } else { Color::Gray };
    let prompt_area = Rect { x: area.x, y: area.y + 4, width: area.width, height: 6 };
    let prompt_input = Paragraph::new(wizard.prompt.as_str())
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Initial Prompt ")
                .border_style(Style::default().fg(prompt_border_color))
        )
        .wrap(Wrap { trim: false });
    f.render_widget(prompt_input, prompt_area);

    // Cursor position based on focused field
    match wizard.focused_field {
        SessionField::Name => {
            let cursor_x = name_area.x + 1 + wizard.name_cursor as u16;
            f.set_cursor_position((cursor_x.min(name_area.x + name_area.width - 2), name_area.y + 1));
        }
        SessionField::Prompt => {
            let inner_width = prompt_area.width.saturating_sub(2) as usize;
            let cursor_x = prompt_area.x + 1 + (wizard.prompt_cursor % inner_width) as u16;
            let cursor_y = prompt_area.y + 1 + (wizard.prompt_cursor / inner_width) as u16;
            f.set_cursor_position((cursor_x, cursor_y.min(prompt_area.y + prompt_area.height - 2)));
        }
    }

    // Hint text
    let hint_area = Rect { x: area.x, y: area.y + 11, width: area.width, height: 1 };
    let hint = Paragraph::new("Tab to switch fields • Name will be stored in .azureal/sessions.toml")
        .style(Style::default().fg(Color::DarkGray));
    f.render_widget(hint, hint_area);
}

fn draw_session_confirmation(f: &mut Frame, app: &App, wizard: &SessionWizard, area: Rect) {
    let worktree_name = app.sessions.get(wizard.selected_worktree_idx)
        .map(|s| s.name())
        .unwrap_or("Unknown");

    let mut lines = vec![
        Line::from(vec![
            Span::styled("Ready to create session", Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled("Worktree: ", Style::default().fg(Color::Gray)),
            Span::styled(worktree_name, Style::default().fg(Color::Cyan)),
        ]),
        Line::from(vec![
            Span::styled("Session name: ", Style::default().fg(Color::Gray)),
            Span::styled(&wizard.session_name, Style::default().fg(Color::Cyan)),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled("Initial prompt:", Style::default().fg(Color::Gray)),
        ]),
        Line::from(""),
    ];

    let prompt_wrapped = textwrap::wrap(&wizard.prompt, (area.width as usize).saturating_sub(4));
    for line in prompt_wrapped {
        lines.push(Line::from(vec![
            Span::styled(format!("  {}", line), Style::default().fg(Color::White)),
        ]));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled("Press Enter to create session and start Claude", Style::default().fg(Color::Green)),
    ]));

    let confirmation = Paragraph::new(lines).wrap(Wrap { trim: false });
    f.render_widget(confirmation, area);
}
