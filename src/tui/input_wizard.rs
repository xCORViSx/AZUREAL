//! Wizard input handling

use crossterm::event::KeyCode;

use crate::app::App;
use crate::claude::ClaudeProcess;
use crate::session::SessionManager;
use crate::wizard::WizardStep;

/// Handle keyboard input for session creation wizard
pub fn handle_wizard_input(app: &mut App, key_code: KeyCode, claude_process: &ClaudeProcess) {
    let Some(wizard) = app.creation_wizard.as_mut() else { return; };

    match wizard.step {
        WizardStep::SelectProject => {
            match key_code {
                KeyCode::Char('j') | KeyCode::Down => wizard.select_next_project(app.projects.len()),
                KeyCode::Char('k') | KeyCode::Up => wizard.select_prev_project(),
                KeyCode::Enter => { wizard.next_step(); }
                KeyCode::Esc => app.cancel_wizard(),
                _ => {}
            }
        }
        WizardStep::EnterPrompt => {
            match key_code {
                KeyCode::Char(c) => wizard.input_char(c),
                KeyCode::Backspace => wizard.input_backspace(),
                KeyCode::Delete => wizard.input_delete(),
                KeyCode::Left => wizard.cursor_left(),
                KeyCode::Right => wizard.cursor_right(),
                KeyCode::Home => wizard.cursor_home(),
                KeyCode::End => wizard.cursor_end(),
                KeyCode::Enter => { wizard.next_step(); }
                KeyCode::Esc => wizard.prev_step(),
                _ => {}
            }
        }
        WizardStep::Confirm => {
            match key_code {
                KeyCode::Enter => {
                    if let Some(project_idx) = wizard.selected_project_idx {
                        if let Some(project) = app.projects.get(project_idx).cloned() {
                            let prompt = wizard.prompt.clone();

                            match SessionManager::new(&app.db).create_session(&project, &prompt) {
                                Ok(session) => {
                                    let _ = app.refresh_sessions();
                                    app.set_status(format!("Created session: {}", session.name));

                                    match claude_process.spawn(&session.worktree_path, &session.initial_prompt, None) {
                                        Ok(rx) => {
                                            app.register_claude(session.id, rx);
                                            app.selected_session = Some(0);
                                            app.load_session_output();
                                        }
                                        Err(e) => app.set_status(format!("Failed to start: {}", e)),
                                    }

                                    app.cancel_wizard();
                                }
                                Err(e) => app.set_status(format!("Failed to create session: {}", e)),
                            }
                        }
                    }
                }
                KeyCode::Esc => wizard.prev_step(),
                _ => {}
            }
        }
    }
}
