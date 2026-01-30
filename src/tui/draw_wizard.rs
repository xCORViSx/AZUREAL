//! Wizard modal rendering

use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};

use crate::app::App;
use crate::wizard::{SessionCreationWizard, WizardField, WizardStep};

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
        .title(format!(" New Worktree - {} ", wizard.step_title()))
        .style(Style::default().bg(Color::Reset));
    f.render_widget(modal_block, modal_area);

    // Inner content area
    let inner = Rect {
        x: modal_area.x + 2,
        y: modal_area.y + 2,
        width: modal_area.width.saturating_sub(4),
        height: modal_area.height.saturating_sub(4),
    };

    // Progress indicator
    let (current_step, total_steps) = wizard.step_progress();
    let progress = Paragraph::new(format!("Step {} of {}", current_step, total_steps))
        .style(Style::default().fg(Color::Gray));
    f.render_widget(progress, Rect { x: inner.x, y: inner.y, width: inner.width, height: 1 });

    // Content area
    let content_area = Rect {
        x: inner.x,
        y: inner.y + 2,
        width: inner.width,
        height: inner.height.saturating_sub(5),
    };

    match wizard.step {
        WizardStep::SelectProject => {
            // In single-project mode, show project info
            let project_info = if let Some(ref project) = app.project {
                format!("Project: {} ({})", project.name, project.path.display())
            } else {
                "No project loaded".to_string()
            };
            let info = Paragraph::new(project_info)
                .style(Style::default().fg(Color::Cyan));
            f.render_widget(info, content_area);
        }
        WizardStep::EnterDetails => draw_wizard_details_input(f, wizard, content_area),
        WizardStep::Confirm => draw_wizard_confirmation(f, app, wizard, content_area),
    }

    // Error messages
    if !wizard.errors.is_empty() {
        let error_area = Rect {
            x: inner.x,
            y: inner.y + inner.height.saturating_sub(3),
            width: inner.width,
            height: 2,
        };
        let error = Paragraph::new(wizard.errors.join(", "))
            .style(Style::default().fg(Color::Red))
            .wrap(Wrap { trim: true });
        f.render_widget(error, error_area);
    }

    // Help text
    let help_area = Rect {
        x: inner.x,
        y: inner.y + inner.height.saturating_sub(1),
        width: inner.width,
        height: 1,
    };
    let help = Paragraph::new(wizard.help_text()).style(Style::default().fg(Color::Gray));
    f.render_widget(help, help_area);
}

fn draw_wizard_details_input(f: &mut Frame, wizard: &SessionCreationWizard, area: Rect) {
    // Worktree name field
    let name_focused = wizard.focused_field == WizardField::Name;
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
    let prompt_focused = wizard.focused_field == WizardField::Prompt;
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
        WizardField::Name => {
            let cursor_x = name_area.x + 1 + wizard.name_cursor as u16;
            f.set_cursor_position((cursor_x.min(name_area.x + name_area.width - 2), name_area.y + 1));
        }
        WizardField::Prompt => {
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

fn draw_wizard_confirmation(f: &mut Frame, app: &App, wizard: &SessionCreationWizard, area: Rect) {
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
