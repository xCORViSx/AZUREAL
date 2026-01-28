//! Wizard input handling

use crossterm::event::KeyCode;

use crate::app::App;
use crate::claude::ClaudeProcess;
use crate::wizard::WizardStep;

/// Handle keyboard input for session creation wizard
pub fn handle_wizard_input(app: &mut App, key_code: KeyCode, claude_process: &ClaudeProcess) {
    let Some(wizard) = app.creation_wizard.as_mut() else { return; };

    match wizard.step {
        WizardStep::SelectProject => {
            // In single-project stateless mode, skip to next step
            match key_code {
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
                    let prompt = wizard.prompt.clone();

                    match app.create_new_session(prompt.clone()) {
                        Ok(session) => {
                            let branch_name = session.branch_name.clone();
                            app.set_status(format!("Created session: {}", session.name()));

                            // Start Claude in the new session
                            if let Some(ref wt_path) = session.worktree_path {
                                match claude_process.spawn(wt_path, &prompt, None) {
                                    Ok(rx) => {
                                        app.register_claude(branch_name.clone(), rx);
                                        // Find and select the new session
                                        if let Some(idx) = app.sessions.iter().position(|s| s.branch_name == branch_name) {
                                            app.selected_session = Some(idx);
                                            app.load_session_output();
                                        }
                                    }
                                    Err(e) => app.set_status(format!("Failed to start Claude: {}", e)),
                                }
                            }

                            app.cancel_wizard();
                        }
                        Err(e) => app.set_status(format!("Failed to create session: {}", e)),
                    }
                }
                KeyCode::Esc => wizard.prev_step(),
                _ => {}
            }
        }
    }
}
