//! Wizard modal rendering

use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};

use crate::app::App;
use crate::wizard::{SessionCreationWizard, WizardStep};

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
        .title(format!(" New Session - {} ", wizard.step_title()))
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
        WizardStep::EnterPrompt => draw_wizard_prompt_input(f, wizard, content_area),
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

fn draw_wizard_prompt_input(f: &mut Frame, wizard: &SessionCreationWizard, area: Rect) {
    let instruction = Paragraph::new("Enter a prompt to start your Claude Code session:\n(This will be the initial message sent to Claude)")
        .style(Style::default().fg(Color::White))
        .wrap(Wrap { trim: true });
    f.render_widget(instruction, Rect { x: area.x, y: area.y, width: area.width, height: 3 });

    // Prompt input box
    let input_area = Rect { x: area.x, y: area.y + 4, width: area.width, height: 5 };
    let input = Paragraph::new(wizard.prompt.as_str())
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Prompt ")
                .border_style(Style::default().fg(Color::Yellow))
        )
        .wrap(Wrap { trim: false });
    f.render_widget(input, input_area);

    // Cursor
    let cursor_x = input_area.x + 1 + (wizard.prompt_cursor as u16 % (input_area.width - 2));
    let cursor_y = input_area.y + 1 + (wizard.prompt_cursor as u16 / (input_area.width - 2));
    f.set_cursor_position((cursor_x, cursor_y));

    // Session name preview
    if !wizard.session_name_preview.is_empty() {
        let preview_area = Rect { x: area.x, y: area.y + 10, width: area.width, height: 3 };
        let preview = Paragraph::new(format!("Session name: {}", wizard.session_name_preview))
            .style(Style::default().fg(Color::Cyan))
            .wrap(Wrap { trim: true });
        f.render_widget(preview, preview_area);
    }
}

fn draw_wizard_confirmation(f: &mut Frame, app: &App, wizard: &SessionCreationWizard, area: Rect) {
    let mut lines = vec![
        Line::from(vec![
            Span::styled("Ready to create session", Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
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
        Span::styled("Session name: ", Style::default().fg(Color::Gray)),
        Span::styled(&wizard.session_name_preview, Style::default().fg(Color::Cyan)),
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
        Span::styled("Press Enter to create and start the session", Style::default().fg(Color::Green)),
    ]));

    let confirmation = Paragraph::new(lines).wrap(Wrap { trim: false });
    f.render_widget(confirmation, area);
}
