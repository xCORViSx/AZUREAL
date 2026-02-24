//! Branch dialog, picker, and dialog input handling.
//!
//! Structural keys (nav, enter, esc, edit, delete, add) resolved via keybindings.rs
//! lookup functions. Text input keys (Char, Backspace, Left, Right) stay raw in dialogs.
//! Number quick-select (1-9/0) stays raw in pickers — not rebindable.
//! Confirm-delete (y/n) stays raw — transient sub-state, not an action.

use anyhow::Result;
use crossterm::event::{self, KeyCode};

use crate::app::{App, Focus, RunCommand};
use crate::claude::ClaudeProcess;
use crate::git::Git;
use crate::app::types::{CommandFieldMode, PresetPrompt, PresetPromptDialog, RunCommandDialog};
use super::keybindings::{lookup_branch_dialog_action, lookup_picker_action, Action};

/// Handle keyboard input when Branch dialog is focused.
/// Nav/Enter/Esc through keybindings; filter chars (Backspace, Char) stay raw.
pub fn handle_branch_dialog_input(key: event::KeyEvent, app: &mut App) -> Result<()> {
    if let Some(ref mut dialog) = app.branch_dialog {
        // Try centralized bindings first for structural keys
        if let Some(action) = lookup_branch_dialog_action(key.modifiers, key.code) {
            match action {
                Action::NavDown => dialog.select_next(),
                Action::NavUp => dialog.select_prev(),
                Action::Confirm => {
                    if let Some(branch) = dialog.selected_branch().cloned() {
                        if let Some(project) = app.current_project().cloned() {
                            // Strip remote prefix for worktree dir name (origin/foo → foo)
                            let local_name = if branch.contains('/') {
                                branch.split('/').skip(1).collect::<Vec<_>>().join("/")
                            } else {
                                branch.clone()
                            };
                            let worktree_path = project.worktrees_dir().join(&local_name);

                            // Use create_worktree_from_branch — these are existing branches,
                            // not new ones (list_available_branches returns already-existing refs)
                            match Git::create_worktree_from_branch(&project.path, &worktree_path, &branch) {
                                Ok(()) => {
                                    app.set_status(format!("Created worktree: {}", local_name));
                                    let _ = app.refresh_worktrees();
                                }
                                Err(e) => app.set_status(format!("Failed to create worktree: {}", e)),
                            }
                        }
                        app.close_branch_dialog();
                    }
                }
                Action::Escape => app.close_branch_dialog(),
                _ => {}
            }
            return Ok(());
        }

        // Raw text input for filter (not rebindable)
        match key.code {
            KeyCode::Backspace => dialog.filter_backspace(),
            KeyCode::Char(c) => dialog.filter_char(c),
            _ => {}
        }
    } else {
        app.focus = Focus::Worktrees;
    }
    Ok(())
}

/// Handle keyboard input when run command picker overlay is open.
/// Structural keys (nav/enter/esc/edit/delete/add) via keybindings.
/// Number quick-select (1-9) and confirm-delete (y/n) stay raw.
pub fn handle_run_command_picker_input(key: event::KeyEvent, app: &mut App) -> Result<()> {
    // Check if a delete confirmation is pending — only y confirms, anything else cancels
    if let Some(del_idx) = app.run_command_picker.as_ref().and_then(|p| p.confirm_delete) {
        match key.code {
            KeyCode::Char('y') | KeyCode::Char('Y') => {
                if del_idx < app.run_commands.len() {
                    let name = app.run_commands[del_idx].name.clone();
                    app.run_commands.remove(del_idx);
                    let _ = app.save_run_commands();
                    app.set_status(format!("Deleted run command: {}", name));
                    if app.run_commands.is_empty() {
                        app.run_command_picker = None;
                    } else if let Some(ref mut picker) = app.run_command_picker {
                        picker.confirm_delete = None;
                        if picker.selected >= app.run_commands.len() {
                            picker.selected = app.run_commands.len() - 1;
                        }
                    }
                }
            }
            _ => {
                if let Some(ref mut picker) = app.run_command_picker {
                    picker.confirm_delete = None;
                }
            }
        }
        return Ok(());
    }

    // Number quick-select (1-9) stays raw — mapping digits to indices isn't rebindable
    if let KeyCode::Char(c @ '1'..='9') = key.code {
        let idx = (c as usize) - ('1' as usize);
        if idx < app.run_commands.len() {
            app.run_command_picker = None;
            app.execute_run_command(idx);
        }
        return Ok(());
    }

    // Structural keys via centralized lookup
    let Some(action) = lookup_picker_action(key.modifiers, key.code) else {
        return Ok(());
    };

    let cmd_count = app.run_commands.len();
    match action {
        Action::NavDown => {
            if let Some(ref mut picker) = app.run_command_picker {
                if picker.selected + 1 < cmd_count { picker.selected += 1; }
            }
        }
        Action::NavUp => {
            if let Some(ref mut picker) = app.run_command_picker {
                if picker.selected > 0 { picker.selected -= 1; }
            }
        }
        Action::Confirm => {
            let idx = app.run_command_picker.as_ref().map(|p| p.selected).unwrap_or(0);
            app.run_command_picker = None;
            app.execute_run_command(idx);
        }
        Action::EditSelected => {
            let idx = app.run_command_picker.as_ref().map(|p| p.selected).unwrap_or(0);
            if let Some(cmd) = app.run_commands.get(idx) {
                app.run_command_dialog = Some(RunCommandDialog::edit(idx, cmd));
            }
            app.run_command_picker = None;
        }
        Action::DeleteSelected => {
            let idx = app.run_command_picker.as_ref().map(|p| p.selected).unwrap_or(0);
            if idx < cmd_count {
                if let Some(ref mut picker) = app.run_command_picker {
                    picker.confirm_delete = Some(idx);
                }
            }
        }
        Action::ProjectsAdd => {
            app.run_command_picker = None;
            app.open_run_command_dialog();
        }
        Action::Escape => { app.run_command_picker = None; }
        _ => {}
    }
    Ok(())
}

/// Handle keyboard input when run command dialog (create/edit) is open.
/// In Command mode, Enter saves a raw shell command directly.
/// In Prompt mode, Enter spawns a Claude session on the main branch to generate the command.
/// ⌃s toggles global/project scope (works from any field).
/// Text input keys stay raw — not rebindable.
pub fn handle_run_command_dialog_input(key: event::KeyEvent, app: &mut App, claude_process: &ClaudeProcess) -> Result<()> {
    let Some(ref mut dialog) = app.run_command_dialog else { return Ok(()) };

    // ⌃s toggles global/project scope (works from any field)
    if key.modifiers.contains(event::KeyModifiers::CONTROL) && key.code == KeyCode::Char('s') {
        dialog.global = !dialog.global;
        return Ok(());
    }

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
                    let editing_idx = dialog.editing_idx;
                    let is_global = dialog.global;
                    let cmd = RunCommand::new(name.clone(), content, is_global);
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

/// Handle keyboard input when preset prompt picker overlay is open.
/// Structural keys via keybindings. Number quick-select (1-9, 0) and
/// confirm-delete (y/n) stay raw.
pub fn handle_preset_prompt_picker_input(key: event::KeyEvent, app: &mut App) -> Result<()> {
    // Check if a delete confirmation is pending — only y confirms, anything else cancels
    if let Some(del_idx) = app.preset_prompt_picker.as_ref().and_then(|p| p.confirm_delete) {
        match key.code {
            KeyCode::Char('y') | KeyCode::Char('Y') => {
                if del_idx < app.preset_prompts.len() {
                    let name = app.preset_prompts[del_idx].name.clone();
                    app.preset_prompts.remove(del_idx);
                    let _ = app.save_preset_prompts();
                    app.set_status(format!("Deleted preset: {}", name));
                    if app.preset_prompts.is_empty() {
                        app.preset_prompt_picker = None;
                    } else if let Some(ref mut picker) = app.preset_prompt_picker {
                        picker.confirm_delete = None;
                        if picker.selected >= app.preset_prompts.len() {
                            picker.selected = app.preset_prompts.len() - 1;
                        }
                    }
                }
            }
            _ => {
                if let Some(ref mut picker) = app.preset_prompt_picker {
                    picker.confirm_delete = None;
                }
            }
        }
        return Ok(());
    }

    // Number quick-select: 1-9 for indices 0-8, 0 for index 9 — stays raw
    let count = app.preset_prompts.len();
    match key.code {
        KeyCode::Char(c @ '1'..='9') => {
            let idx = (c as usize) - ('1' as usize);
            if idx < count { app.select_preset_prompt(idx); }
            return Ok(());
        }
        KeyCode::Char('0') => {
            if count > 9 { app.select_preset_prompt(9); }
            return Ok(());
        }
        _ => {}
    }

    // Structural keys via centralized lookup
    let Some(action) = lookup_picker_action(key.modifiers, key.code) else {
        return Ok(());
    };

    match action {
        Action::NavDown => {
            if let Some(ref mut picker) = app.preset_prompt_picker {
                if picker.selected + 1 < count { picker.selected += 1; }
            }
        }
        Action::NavUp => {
            if let Some(ref mut picker) = app.preset_prompt_picker {
                if picker.selected > 0 { picker.selected -= 1; }
            }
        }
        Action::Confirm => {
            let idx = app.preset_prompt_picker.as_ref().map(|p| p.selected).unwrap_or(0);
            app.select_preset_prompt(idx);
        }
        Action::EditSelected => {
            let idx = app.preset_prompt_picker.as_ref().map(|p| p.selected).unwrap_or(0);
            if let Some(preset) = app.preset_prompts.get(idx) {
                app.preset_prompt_dialog = Some(PresetPromptDialog::edit(idx, preset));
            }
            app.preset_prompt_picker = None;
        }
        Action::DeleteSelected => {
            let idx = app.preset_prompt_picker.as_ref().map(|p| p.selected).unwrap_or(0);
            if idx < count {
                if let Some(ref mut picker) = app.preset_prompt_picker {
                    picker.confirm_delete = Some(idx);
                }
            }
        }
        Action::ProjectsAdd => {
            app.preset_prompt_picker = None;
            app.preset_prompt_dialog = Some(PresetPromptDialog::new());
        }
        Action::Escape => { app.preset_prompt_picker = None; }
        _ => {}
    }
    Ok(())
}

/// Handle keyboard input when preset prompt dialog (create/edit) is open.
/// Tab toggles between name and prompt fields, ⌃s toggles global scope,
/// Enter saves, Esc cancels. Text input keys stay raw.
pub fn handle_preset_prompt_dialog_input(key: event::KeyEvent, app: &mut App) -> Result<()> {
    let Some(ref mut dialog) = app.preset_prompt_dialog else { return Ok(()) };

    // ⌃s toggles global/project scope (works from any field)
    if key.modifiers.contains(event::KeyModifiers::CONTROL) && key.code == KeyCode::Char('s') {
        dialog.global = !dialog.global;
        return Ok(());
    }

    match key.code {
        // Tab: advance from name to prompt field
        KeyCode::Tab => {
            if dialog.editing_name { dialog.editing_name = false; }
        }
        // Shift+Tab: go back to name field
        KeyCode::BackTab => {
            if !dialog.editing_name { dialog.editing_name = true; }
        }
        // Enter: advance name→prompt, or save when in prompt field
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
            let prompt = dialog.prompt.trim().to_string();
            if name.is_empty() || prompt.is_empty() {
                app.set_status("Both name and prompt are required");
                return Ok(());
            }
            let editing_idx = dialog.editing_idx;
            let is_global = dialog.global;
            let preset = PresetPrompt::new(name.clone(), prompt, is_global);
            if let Some(idx) = editing_idx {
                if idx < app.preset_prompts.len() { app.preset_prompts[idx] = preset; }
            } else {
                if app.preset_prompts.len() >= 10 {
                    app.set_status("Maximum 10 preset prompts reached");
                    return Ok(());
                }
                app.preset_prompts.push(preset);
            }
            app.preset_prompt_dialog = None;
            let _ = app.save_preset_prompts();
            app.set_status(format!("Saved preset: {}", name));
            // Reopen picker if we have presets now
            if !app.preset_prompts.is_empty() {
                app.preset_prompt_picker = Some(crate::app::types::PresetPromptPicker::new());
            }
        }
        KeyCode::Esc => {
            app.preset_prompt_dialog = None;
        }
        // Text editing for the active field
        KeyCode::Backspace => {
            if dialog.editing_name {
                if dialog.name_cursor > 0 {
                    let byte_pos = dialog.name.char_indices().nth(dialog.name_cursor - 1).map(|(i, _)| i).unwrap_or(0);
                    dialog.name.remove(byte_pos);
                    dialog.name_cursor -= 1;
                }
            } else if dialog.prompt_cursor > 0 {
                let byte_pos = dialog.prompt.char_indices().nth(dialog.prompt_cursor - 1).map(|(i, _)| i).unwrap_or(0);
                dialog.prompt.remove(byte_pos);
                dialog.prompt_cursor -= 1;
            }
        }
        KeyCode::Left => {
            if dialog.editing_name {
                dialog.name_cursor = dialog.name_cursor.saturating_sub(1);
            } else {
                dialog.prompt_cursor = dialog.prompt_cursor.saturating_sub(1);
            }
        }
        KeyCode::Right => {
            if dialog.editing_name {
                if dialog.name_cursor < dialog.name.chars().count() { dialog.name_cursor += 1; }
            } else if dialog.prompt_cursor < dialog.prompt.chars().count() {
                dialog.prompt_cursor += 1;
            }
        }
        KeyCode::Char(c) => {
            if dialog.editing_name {
                let byte_pos = dialog.name.char_indices().nth(dialog.name_cursor).map(|(i, _)| i).unwrap_or(dialog.name.len());
                dialog.name.insert(byte_pos, c);
                dialog.name_cursor += 1;
            } else {
                let byte_pos = dialog.prompt.char_indices().nth(dialog.prompt_cursor).map(|(i, _)| i).unwrap_or(dialog.prompt.len());
                dialog.prompt.insert(byte_pos, c);
                dialog.prompt_cursor += 1;
            }
        }
        _ => {}
    }
    Ok(())
}

/// Spawn a Claude session on the main branch to generate a run command from a prompt.
/// Claude reads/writes `.azureal/runcmds` and adds the new entry.
fn spawn_run_command_prompt(app: &mut App, claude_process: &ClaudeProcess, cmd_name: &str, user_prompt: &str) {
    if app.project.is_none() {
        app.set_status("No project loaded");
        return;
    }
    // Spawn on the current worktree (changes merge back to main via squash-merge)
    let Some(session) = app.current_worktree() else {
        app.set_status("No active worktree");
        return;
    };
    let Some(wt_path) = session.worktree_path.clone() else {
        app.set_status("No worktree path");
        return;
    };
    let branch = session.branch_name.clone();

    // Build prompt with context about runcmds format and location
    let prompt = format!(
        "I need you to create a run command for my project.\n\n\
         Command name: {}\n\
         Description: {}\n\n\
         Run commands are stored in `.azureal/runcmds` in the project root.\n\
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
    // Spawn Claude on current worktree (resume existing session if any)
    let resume_id = app.get_claude_session_id(&branch).cloned();
    match claude_process.spawn(&wt_path, &prompt, resume_id.as_deref()) {
        Ok((rx, pid)) => {
            app.pending_session_names.push((pid.to_string(), display_name));
            app.register_claude(branch, pid, rx);
            app.focus = Focus::Output;
            app.set_status(format!("Generating run command: {}...", cmd_name));
        }
        Err(e) => app.set_status(format!("Failed to start Claude: {}", e)),
    }
}
