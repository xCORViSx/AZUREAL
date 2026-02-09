//! Context menu and branch dialog input handling

use anyhow::Result;
use crossterm::event::{self, KeyCode};

use crate::app::{App, Focus, RunCommand, SessionAction};
use crate::claude::ClaudeProcess;
use crate::git::Git;
use crate::app::types::{CommandFieldMode, RunCommandDialog};

/// Handle keyboard input when context menu is open
pub fn handle_context_menu_input(key: event::KeyEvent, app: &mut App, claude_process: &ClaudeProcess) -> Result<()> {
    match key.code {
        KeyCode::Char('j') | KeyCode::Down => app.context_menu_next(),
        KeyCode::Char('k') | KeyCode::Up => app.context_menu_prev(),
        KeyCode::Enter => {
            if let Some(action) = app.selected_action() {
                execute_action(app, claude_process, action)?;
            }
            app.close_context_menu();
        }
        KeyCode::Esc => app.close_context_menu(),
        _ => {}
    }
    Ok(())
}

/// Execute a session action from the context menu
fn execute_action(app: &mut App, _claude_process: &ClaudeProcess, action: SessionAction) -> Result<()> {
    match action {
        SessionAction::Start => {
            if let Some(session) = app.current_session() {
                if app.is_session_running(&session.branch_name) {
                    app.set_status("Claude already running in this session");
                } else {
                    app.focus = Focus::Input;
                    app.set_status("Enter your prompt");
                }
            }
        }
        SessionAction::Stop => {
            app.set_status("Stop action not yet implemented");
        }
        SessionAction::Archive => {
            if let Err(e) = app.archive_current_session() {
                app.set_status(format!("Failed to archive: {}", e));
            }
        }
        SessionAction::Delete => {
            app.set_status("Delete action not yet implemented - use CLI: azureal session delete");
        }
        SessionAction::ViewDiff => {
            if let Err(e) = app.load_diff() {
                app.set_status(format!("Failed to get diff: {}", e));
            }
        }
        SessionAction::RebaseFromMain => {
            if let Err(e) = app.rebase_current_session() {
                app.set_status(format!("Rebase failed: {}", e));
            }
        }
        SessionAction::OpenInEditor => {
            if let Some(session) = app.current_session() {
                if let Some(ref wt_path) = session.worktree_path {
                    let path = wt_path.display().to_string();
                    app.set_status(format!("Editor integration not implemented. Path: {}", path));
                }
            }
        }
        SessionAction::CopyWorktreePath => {
            if let Some(session) = app.current_session() {
                if let Some(ref wt_path) = session.worktree_path {
                    let path = wt_path.display().to_string();
                    app.set_status(format!("Copied to clipboard (not implemented): {}", path));
                }
            }
        }
    }
    Ok(())
}

/// Handle keyboard input when Branch dialog is focused
pub fn handle_branch_dialog_input(key: event::KeyEvent, app: &mut App) -> Result<()> {
    if let Some(ref mut dialog) = app.branch_dialog {
        match key.code {
            KeyCode::Down | KeyCode::Char('j') => dialog.select_next(),
            KeyCode::Up | KeyCode::Char('k') => dialog.select_prev(),
            KeyCode::Backspace => dialog.filter_backspace(),
            KeyCode::Enter => {
                if let Some(branch) = dialog.selected_branch().cloned() {
                    if let Some(project) = app.current_project().cloned() {
                        // Create worktree from existing branch
                        let worktree_name = branch.strip_prefix("azureal/").unwrap_or(&branch);
                        let worktree_path = project.worktrees_dir().join(worktree_name);

                        match Git::create_worktree(&project.path, &worktree_path, &branch) {
                            Ok(()) => {
                                app.set_status(format!("Created worktree: {}", worktree_name));
                                let _ = app.refresh_sessions();
                            }
                            Err(e) => app.set_status(format!("Failed to create worktree: {}", e)),
                        }
                    }
                    app.close_branch_dialog();
                }
            }
            KeyCode::Esc => app.close_branch_dialog(),
            KeyCode::Char(c) => dialog.filter_char(c),
            _ => {}
        }
    } else {
        app.focus = Focus::Worktrees;
    }
    Ok(())
}

/// Handle keyboard input when run command picker overlay is open
pub fn handle_run_command_picker_input(key: event::KeyEvent, app: &mut App) -> Result<()> {
    let cmd_count = app.run_commands.len();
    match key.code {
        // Navigate selection
        KeyCode::Char('j') | KeyCode::Down => {
            if let Some(ref mut picker) = app.run_command_picker {
                if picker.selected + 1 < cmd_count { picker.selected += 1; }
            }
        }
        KeyCode::Char('k') | KeyCode::Up => {
            if let Some(ref mut picker) = app.run_command_picker {
                if picker.selected > 0 { picker.selected -= 1; }
            }
        }
        // Quick-select by number (1-9)
        KeyCode::Char(c @ '1'..='9') => {
            let idx = (c as usize) - ('1' as usize);
            if idx < cmd_count {
                app.run_command_picker = None;
                app.execute_run_command(idx);
            }
        }
        // Execute selected command
        KeyCode::Enter => {
            let idx = app.run_command_picker.as_ref().map(|p| p.selected).unwrap_or(0);
            app.run_command_picker = None;
            app.execute_run_command(idx);
        }
        // Edit selected command
        KeyCode::Char('e') => {
            let idx = app.run_command_picker.as_ref().map(|p| p.selected).unwrap_or(0);
            if let Some(cmd) = app.run_commands.get(idx) {
                app.run_command_dialog = Some(RunCommandDialog::edit(idx, cmd));
            }
            app.run_command_picker = None;
        }
        // Delete selected command
        KeyCode::Char('x') => {
            let idx = app.run_command_picker.as_ref().map(|p| p.selected).unwrap_or(0);
            if idx < cmd_count {
                let name = app.run_commands[idx].name.clone();
                app.run_commands.remove(idx);
                let _ = app.save_run_commands();
                app.set_status(format!("Deleted run command: {}", name));
                // Adjust selection after deletion
                if app.run_commands.is_empty() {
                    app.run_command_picker = None;
                } else if let Some(ref mut picker) = app.run_command_picker {
                    if picker.selected >= app.run_commands.len() {
                        picker.selected = app.run_commands.len() - 1;
                    }
                }
            }
        }
        // Add new command from picker
        KeyCode::Char('a') => {
            app.run_command_picker = None;
            app.open_run_command_dialog();
        }
        KeyCode::Esc => { app.run_command_picker = None; }
        _ => {}
    }
    Ok(())
}

/// Handle keyboard input when run command dialog (create/edit) is open.
/// In Command mode, Enter saves a raw shell command directly.
/// In Prompt mode, Enter spawns a Claude session on the main branch to generate the command.
pub fn handle_run_command_dialog_input(key: event::KeyEvent, app: &mut App, claude_process: &ClaudeProcess) -> Result<()> {
    let Some(ref mut dialog) = app.run_command_dialog else { return Ok(()) };

    match key.code {
        // Tab: in Name field → advance to Command/Prompt; in Command/Prompt → cycle mode
        KeyCode::Tab => {
            if dialog.editing_name {
                dialog.editing_name = false;
            } else {
                dialog.field_mode = match dialog.field_mode {
                    CommandFieldMode::Command => CommandFieldMode::Prompt,
                    CommandFieldMode::Prompt => CommandFieldMode::Command,
                };
            }
        }
        // Shift+Tab: go back to Name field from Command/Prompt
        KeyCode::BackTab => {
            if !dialog.editing_name { dialog.editing_name = true; }
        }
        // Enter: advance name→command, or save/generate when in command/prompt field
        KeyCode::Enter => {
            if dialog.editing_name {
                if dialog.name.trim().is_empty() {
                    app.set_status("Name is required");
                    return Ok(());
                }
                dialog.editing_name = false;
                return Ok(());
            }
            let name = dialog.name.trim().to_string();
            let content = dialog.command.trim().to_string();
            if name.is_empty() || content.is_empty() {
                let label = match dialog.field_mode {
                    CommandFieldMode::Command => "command",
                    CommandFieldMode::Prompt => "prompt",
                };
                app.set_status(format!("Both name and {} are required", label));
                return Ok(());
            }
            match dialog.field_mode {
                CommandFieldMode::Command => {
                    // Save the raw shell command directly
                    let editing_idx = dialog.editing_idx;
                    let cmd = RunCommand::new(name.clone(), content);
                    if let Some(idx) = editing_idx {
                        if idx < app.run_commands.len() { app.run_commands[idx] = cmd; }
                    } else {
                        app.run_commands.push(cmd);
                    }
                    app.run_command_dialog = None;
                    let _ = app.save_run_commands();
                    app.set_status(format!("Saved run command: {}", name));
                }
                CommandFieldMode::Prompt => {
                    // Spawn Claude on main branch to generate the run command
                    let prompt_text = content;
                    let cmd_name = name;
                    app.run_command_dialog = None;
                    spawn_run_command_prompt(app, claude_process, &cmd_name, &prompt_text);
                }
            }
        }
        KeyCode::Esc => { app.run_command_dialog = None; }
        // Text editing for the active field
        KeyCode::Backspace => {
            if dialog.editing_name {
                if dialog.name_cursor > 0 {
                    dialog.name_cursor -= 1;
                    dialog.name.remove(dialog.name_cursor);
                }
            } else if dialog.command_cursor > 0 {
                dialog.command_cursor -= 1;
                dialog.command.remove(dialog.command_cursor);
            }
        }
        KeyCode::Left => {
            if dialog.editing_name {
                dialog.name_cursor = dialog.name_cursor.saturating_sub(1);
            } else {
                dialog.command_cursor = dialog.command_cursor.saturating_sub(1);
            }
        }
        KeyCode::Right => {
            if dialog.editing_name {
                if dialog.name_cursor < dialog.name.len() { dialog.name_cursor += 1; }
            } else if dialog.command_cursor < dialog.command.len() {
                dialog.command_cursor += 1;
            }
        }
        KeyCode::Char(c) => {
            if dialog.editing_name {
                dialog.name.insert(dialog.name_cursor, c);
                dialog.name_cursor += 1;
            } else {
                dialog.command.insert(dialog.command_cursor, c);
                dialog.command_cursor += 1;
            }
        }
        _ => {}
    }
    Ok(())
}

/// Spawn a Claude session on the main branch to generate a run command from a prompt.
/// Claude reads/writes `.azureal/run_commands.json` and adds the new entry.
fn spawn_run_command_prompt(app: &mut App, claude_process: &ClaudeProcess, cmd_name: &str, user_prompt: &str) {
    let Some(project) = app.project.as_ref() else {
        app.set_status("No project loaded");
        return;
    };
    let main_branch = project.main_branch.clone();

    // Find the main worktree path (the repo root itself for the main branch)
    let main_wt_path = app.sessions.iter()
        .find(|s| s.branch_name == main_branch)
        .and_then(|s| s.worktree_path.clone());
    let Some(wt_path) = main_wt_path else {
        app.set_status("Main branch worktree not found");
        return;
    };

    // Build prompt with context about run_commands.json format and location
    let prompt = format!(
        "I need you to create a run command for my project.\n\n\
         Command name: {}\n\
         Description: {}\n\n\
         Run commands are stored in `.azureal/run_commands.json` in the project root.\n\
         Format: JSON array of objects with \"name\" and \"command\" fields, e.g.:\n\
         ```json\n\
         [\n\
           {{\"name\": \"Build\", \"command\": \"cargo build --release\"}}\n\
         ]\n\
         ```\n\n\
         Read the existing file if it exists. Determine the right shell command(s) based on my description, \
         and add a new entry with the name \"{}\". If the file doesn't exist, create it with just this entry.\n\
         Don't modify existing entries. Project directory: {}\n\
         Keep the response brief.",
        cmd_name, user_prompt, cmd_name, wt_path.display()
    );

    // Session name: [NewRunCmd] <name> — truncated to fit sidebar
    let display_name = if cmd_name.chars().count() > 30 {
        format!("[NewRunCmd] {}…", &cmd_name.chars().take(29).collect::<String>())
    } else {
        format!("[NewRunCmd] {}", cmd_name)
    };
    app.pending_session_name = Some((main_branch.clone(), display_name));

    // Select the main branch in sidebar so user sees the output
    if let Some(idx) = app.sessions.iter().position(|s| s.branch_name == main_branch) {
        app.selected_session = Some(idx);
        app.load_session_output();
    }

    // Spawn Claude on main branch (resume existing session if any)
    let resume_id = app.get_claude_session_id(&main_branch).cloned();
    match claude_process.spawn(&wt_path, &prompt, resume_id.as_deref()) {
        Ok(rx) => {
            app.register_claude(main_branch, rx);
            app.focus = Focus::Output;
            app.set_status(format!("Generating run command: {}...", cmd_name));
        }
        Err(e) => app.set_status(format!("Failed to start Claude: {}", e)),
    }
}
