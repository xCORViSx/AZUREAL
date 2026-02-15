//! Wizard input handling

use crossterm::event::{KeyCode, KeyEvent};

use crate::app::App;
use crate::claude::ClaudeProcess;
use crate::wizard::{WizardTab, WorktreeStep, SessionStep};

/// Handle keyboard input for creation wizard
/// Note: Tab cycling (Alt+Tab, Shift+Tab, [, ]) is handled in event_loop.rs before this
pub fn handle_wizard_input(app: &mut App, key: KeyEvent, claude_process: &ClaudeProcess) {
    let Some(wizard) = app.creation_wizard.as_mut() else { return; };

    // Handle input based on active tab
    match wizard.active_tab {
        WizardTab::Project | WizardTab::Branch => {
            // Not implemented yet - just allow tab cycling and escape
            if key.code == KeyCode::Esc {
                app.cancel_wizard();
            }
        }
        WizardTab::Worktree => handle_worktree_input(app, key.code, claude_process),
        WizardTab::Session => handle_session_input(app, key.code, claude_process),
    }
}

fn handle_worktree_input(app: &mut App, key_code: KeyCode, claude_process: &ClaudeProcess) {
    let Some(wizard) = app.creation_wizard.as_mut() else { return; };

    match wizard.worktree.step {
        WorktreeStep::SelectProject => {
            match key_code {
                KeyCode::Enter => { wizard.worktree.next_step(); }
                KeyCode::Esc => app.cancel_wizard(),
                _ => {}
            }
        }
        WorktreeStep::EnterDetails => {
            match key_code {
                KeyCode::Tab => wizard.worktree.toggle_field(),
                KeyCode::Char(c) => wizard.worktree.input_char(c),
                KeyCode::Backspace => wizard.worktree.input_backspace(),
                KeyCode::Delete => wizard.worktree.input_delete(),
                KeyCode::Left => wizard.worktree.cursor_left(),
                KeyCode::Right => wizard.worktree.cursor_right(),
                KeyCode::Home => wizard.worktree.cursor_home(),
                KeyCode::End => wizard.worktree.cursor_end(),
                KeyCode::Enter => { wizard.worktree.next_step(); }
                KeyCode::Esc => wizard.worktree.prev_step(),
                _ => {}
            }
        }
        WorktreeStep::Confirm => {
            match key_code {
                KeyCode::Enter => {
                    let prompt = wizard.worktree.prompt.clone();
                    let worktree_name = wizard.worktree.final_worktree_name();

                    match app.create_new_worktree_with_name(worktree_name, prompt.clone()) {
                        Ok(worktree) => {
                            let branch_name = worktree.branch_name.clone();
                            app.set_status(format!("Created worktree: {}", worktree.name()));

                            // Start Claude in the new worktree
                            if let Some(ref wt_path) = worktree.worktree_path {
                                match claude_process.spawn(wt_path, &prompt, None) {
                                    Ok((rx, pid)) => {
                                        app.register_claude(branch_name.clone(), pid, rx);
                                        // Find and select the new worktree
                                        if let Some(idx) = app.worktrees.iter().position(|s| s.branch_name == branch_name) {
                                            app.selected_worktree = Some(idx);
                                            app.load_session_output();
                                        }
                                    }
                                    Err(e) => app.set_status(format!("Failed to start Claude: {}", e)),
                                }
                            }

                            app.cancel_wizard();
                        }
                        Err(e) => app.set_status(format!("Failed to create worktree: {}", e)),
                    }
                }
                KeyCode::Esc => wizard.worktree.prev_step(),
                _ => {}
            }
        }
    }
}

fn handle_session_input(app: &mut App, key_code: KeyCode, claude_process: &ClaudeProcess) {
    let Some(wizard) = app.creation_wizard.as_mut() else { return; };
    let num_sessions = app.worktrees.len();

    match wizard.session.step {
        SessionStep::SelectWorktree => {
            match key_code {
                KeyCode::Char('j') | KeyCode::Down => wizard.session.select_next(num_sessions),
                KeyCode::Char('k') | KeyCode::Up => wizard.session.select_prev(),
                KeyCode::Enter => {
                    if num_sessions > 0 {
                        wizard.session.next_step();
                    }
                }
                KeyCode::Esc => app.cancel_wizard(),
                _ => {}
            }
        }
        SessionStep::EnterDetails => {
            match key_code {
                KeyCode::Tab => wizard.session.toggle_field(),
                KeyCode::Char(c) => wizard.session.input_char(c),
                KeyCode::Backspace => wizard.session.input_backspace(),
                KeyCode::Delete => wizard.session.input_delete(),
                KeyCode::Left => wizard.session.cursor_left(),
                KeyCode::Right => wizard.session.cursor_right(),
                KeyCode::Home => wizard.session.cursor_home(),
                KeyCode::End => wizard.session.cursor_end(),
                KeyCode::Enter => { wizard.session.next_step(); }
                KeyCode::Esc => wizard.session.prev_step(),
                _ => {}
            }
        }
        SessionStep::Confirm => {
            match key_code {
                KeyCode::Enter => {
                    let session_name = wizard.session.session_name.clone();
                    let prompt = wizard.session.prompt.clone();
                    let worktree_idx = wizard.session.selected_worktree_idx;

                    // Get the selected worktree
                    if let Some(session) = app.worktrees.get(worktree_idx).cloned() {
                        if let Some(ref wt_path) = session.worktree_path {
                            // Start Claude in the worktree
                            match claude_process.spawn(wt_path, &prompt, None) {
                                Ok((rx, pid)) => {
                                    // Store pending session name keyed by slot (PID)
                                    app.pending_session_names.push((pid.to_string(), session_name.clone()));
                                    app.register_claude(session.branch_name.clone(), pid, rx);
                                    app.selected_worktree = Some(worktree_idx);
                                    app.load_session_output();
                                    app.set_status(format!("Started session '{}' in {}", session_name, session.name()));
                                }
                                Err(e) => app.set_status(format!("Failed to start Claude: {}", e)),
                            }
                        }
                    }

                    app.cancel_wizard();
                }
                KeyCode::Esc => wizard.session.prev_step(),
                _ => {}
            }
        }
    }
}
