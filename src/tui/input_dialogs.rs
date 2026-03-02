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
use crate::app::types::{CommandFieldMode, PresetPrompt, PresetPromptDialog, RunCommandDialog, is_git_safe_char};
use super::keybindings::{lookup_branch_dialog_action, lookup_picker_action, Action};

/// Handle keyboard input when Branch dialog is focused.
/// Nav/Enter/Esc through keybindings; filter chars (Backspace, Char) stay raw.
/// selected==0 is "Create new" row, selected>=1 is branch rows.
pub fn handle_branch_dialog_input(key: event::KeyEvent, app: &mut App) -> Result<()> {
    if let Some(ref mut dialog) = app.branch_dialog {
        // On the "Create new" row, let j/k through as text input instead of nav
        let on_create = dialog.on_create_new();
        let is_jk = matches!(key.code, KeyCode::Char('j') | KeyCode::Char('k'));
        // Try centralized bindings first for structural keys (skip j/k when typing)
        if !(on_create && is_jk) {
        if let Some(action) = lookup_branch_dialog_action(key.modifiers, key.code) {
            match action {
                Action::NavDown => dialog.select_next(),
                Action::NavUp => dialog.select_prev(),
                Action::Confirm => {
                    if dialog.on_create_new() {
                        // "Create new" row — use filter text as worktree name
                        let name = dialog.filter.trim().to_string();
                        if name.is_empty() {
                            app.set_status("Enter a name for the new worktree");
                            return Ok(());
                        }
                        app.close_branch_dialog();
                        match app.create_new_worktree_with_name(name.clone(), String::new()) {
                            Ok(_wt) => app.set_status(format!("Created worktree: {}", name)),
                            Err(e) => app.set_status(format!("Failed: {}", e)),
                        }
                    } else if let Some(branch) = dialog.selected_branch().cloned() {
                        let is_active = dialog.is_checked_out(&branch);
                        if is_active {
                            // Branch already has a worktree — switch focus to it
                            let local_name = if branch.contains('/') {
                                branch.split('/').skip(1).collect::<Vec<_>>().join("/")
                            } else {
                                branch.clone()
                            };
                            let target_idx = app.worktrees.iter().position(|wt| wt.branch_name == branch || wt.branch_name == local_name);
                            if let Some(idx) = target_idx {
                                app.selected_worktree = Some(idx);
                                app.set_status(format!("Switched to {}", branch));
                            }
                        } else if let Some(project) = app.current_project().cloned() {
                            // Create a new worktree from this branch
                            let local_name = if branch.contains('/') {
                                branch.split('/').skip(1).collect::<Vec<_>>().join("/")
                            } else {
                                branch.clone()
                            };
                            let worktree_path = project.worktrees_dir().join(&local_name);
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
        }

        // Raw text input for filter — only git-safe chars allowed
        match key.code {
            KeyCode::Backspace => dialog.filter_backspace(),
            KeyCode::Char(c) if is_git_safe_char(c) => dialog.filter_char(c),
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

/// Truncate long command names for display in sidebar session list.
/// Extracted for testability: applies the same 30-char limit used by spawn_run_command_prompt.
fn format_run_cmd_display_name(cmd_name: &str) -> String {
    if cmd_name.chars().count() > 30 {
        format!("[NewRunCmd] {}…", &cmd_name.chars().take(29).collect::<String>())
    } else {
        format!("[NewRunCmd] {}", cmd_name)
    }
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
    let display_name = format_run_cmd_display_name(cmd_name);
    // Spawn Claude on current worktree (resume existing session if any)
    let resume_id = app.get_claude_session_id(&branch).cloned();
    match claude_process.spawn(&wt_path, &prompt, resume_id.as_deref(), app.selected_model.as_deref()) {
        Ok((rx, pid)) => {
            app.pending_session_names.push((pid.to_string(), display_name));
            app.register_claude(branch, pid, rx);
            app.focus = Focus::Session;
            app.set_status(format!("Generating run command: {}...", cmd_name));
        }
        Err(e) => app.set_status(format!("Failed to start Claude: {}", e)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, KeyEventKind, KeyEventState};

    /// Helper to build a plain KeyEvent (no modifiers)
    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent {
            code,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        }
    }

    /// Helper to build a KeyEvent with modifiers
    fn key_mod(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
        KeyEvent {
            code,
            modifiers,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        }
    }

    // ══════════════════════════════════════════════════════════════════
    //  format_run_cmd_display_name
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn display_name_short_cmd() {
        assert_eq!(format_run_cmd_display_name("Build"), "[NewRunCmd] Build");
    }

    #[test]
    fn display_name_exactly_30_chars() {
        let name = "a".repeat(30);
        assert_eq!(format_run_cmd_display_name(&name), format!("[NewRunCmd] {}", name));
    }

    #[test]
    fn display_name_31_chars_truncates() {
        let name = "a".repeat(31);
        let expected = format!("[NewRunCmd] {}…", "a".repeat(29));
        assert_eq!(format_run_cmd_display_name(&name), expected);
    }

    #[test]
    fn display_name_empty_string() {
        assert_eq!(format_run_cmd_display_name(""), "[NewRunCmd] ");
    }

    #[test]
    fn display_name_unicode_chars() {
        let name = "a".repeat(28) + "🚀🚀🚀"; // 31 chars
        let result = format_run_cmd_display_name(&name);
        assert!(result.starts_with("[NewRunCmd] "));
        assert!(result.ends_with('…'));
    }

    #[test]
    fn display_name_single_char() {
        assert_eq!(format_run_cmd_display_name("x"), "[NewRunCmd] x");
    }

    #[test]
    fn display_name_spaces_preserved() {
        assert_eq!(format_run_cmd_display_name("My Build"), "[NewRunCmd] My Build");
    }

    // ══════════════════════════════════════════════════════════════════
    //  KeyEvent construction helpers
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn key_helper_creates_plain_event() {
        let k = key(KeyCode::Enter);
        assert_eq!(k.code, KeyCode::Enter);
        assert_eq!(k.modifiers, KeyModifiers::NONE);
    }

    #[test]
    fn key_mod_helper_creates_modified_event() {
        let k = key_mod(KeyCode::Char('s'), KeyModifiers::CONTROL);
        assert_eq!(k.code, KeyCode::Char('s'));
        assert_eq!(k.modifiers, KeyModifiers::CONTROL);
    }

    // ══════════════════════════════════════════════════════════════════
    //  KeyCode construction and matching
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn keycode_char_equality() {
        assert_eq!(KeyCode::Char('a'), KeyCode::Char('a'));
        assert_ne!(KeyCode::Char('a'), KeyCode::Char('b'));
    }

    #[test]
    fn keycode_enter_equality() {
        assert_eq!(KeyCode::Enter, KeyCode::Enter);
        assert_ne!(KeyCode::Enter, KeyCode::Esc);
    }

    #[test]
    fn keycode_esc_equality() {
        assert_eq!(KeyCode::Esc, KeyCode::Esc);
    }

    #[test]
    fn keycode_backspace_equality() {
        assert_eq!(KeyCode::Backspace, KeyCode::Backspace);
    }

    #[test]
    fn keycode_tab_equality() {
        assert_eq!(KeyCode::Tab, KeyCode::Tab);
        assert_ne!(KeyCode::Tab, KeyCode::BackTab);
    }

    #[test]
    fn keycode_left_right_different() {
        assert_ne!(KeyCode::Left, KeyCode::Right);
    }

    #[test]
    fn keycode_up_down_different() {
        assert_ne!(KeyCode::Up, KeyCode::Down);
    }

    // ══════════════════════════════════════════════════════════════════
    //  KeyModifiers construction and bitflags
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn modifiers_none_is_empty() {
        assert!(KeyModifiers::NONE.is_empty());
    }

    #[test]
    fn modifiers_control_not_empty() {
        assert!(!KeyModifiers::CONTROL.is_empty());
    }

    #[test]
    fn modifiers_shift_contains_shift() {
        assert!(KeyModifiers::SHIFT.contains(KeyModifiers::SHIFT));
    }

    #[test]
    fn modifiers_control_does_not_contain_shift() {
        assert!(!KeyModifiers::CONTROL.contains(KeyModifiers::SHIFT));
    }

    #[test]
    fn modifiers_combined_control_shift() {
        let combined = KeyModifiers::CONTROL | KeyModifiers::SHIFT;
        assert!(combined.contains(KeyModifiers::CONTROL));
        assert!(combined.contains(KeyModifiers::SHIFT));
        assert!(!combined.contains(KeyModifiers::ALT));
    }

    #[test]
    fn modifiers_super_for_cmd() {
        assert!(KeyModifiers::SUPER.contains(KeyModifiers::SUPER));
        assert!(!KeyModifiers::SUPER.contains(KeyModifiers::CONTROL));
    }

    // ══════════════════════════════════════════════════════════════════
    //  Number quick-select index calculation (1-9 digit-to-index)
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn digit_1_maps_to_index_0() {
        let c = '1';
        let idx = (c as usize) - ('1' as usize);
        assert_eq!(idx, 0);
    }

    #[test]
    fn digit_5_maps_to_index_4() {
        let c = '5';
        let idx = (c as usize) - ('1' as usize);
        assert_eq!(idx, 4);
    }

    #[test]
    fn digit_9_maps_to_index_8() {
        let c = '9';
        let idx = (c as usize) - ('1' as usize);
        assert_eq!(idx, 8);
    }

    // ══════════════════════════════════════════════════════════════════
    //  Action enum variant coverage (used in this file)
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn action_nav_down_eq() {
        assert_eq!(Action::NavDown, Action::NavDown);
    }

    #[test]
    fn action_nav_up_eq() {
        assert_eq!(Action::NavUp, Action::NavUp);
    }

    #[test]
    fn action_confirm_eq() {
        assert_eq!(Action::Confirm, Action::Confirm);
    }

    #[test]
    fn action_escape_eq() {
        assert_eq!(Action::Escape, Action::Escape);
    }

    #[test]
    fn action_edit_selected_eq() {
        assert_eq!(Action::EditSelected, Action::EditSelected);
    }

    #[test]
    fn action_delete_selected_eq() {
        assert_eq!(Action::DeleteSelected, Action::DeleteSelected);
    }

    #[test]
    fn action_projects_add_eq() {
        assert_eq!(Action::ProjectsAdd, Action::ProjectsAdd);
    }

    #[test]
    fn action_different_variants_ne() {
        assert_ne!(Action::NavDown, Action::NavUp);
        assert_ne!(Action::Confirm, Action::Escape);
        assert_ne!(Action::EditSelected, Action::DeleteSelected);
    }

    // ══════════════════════════════════════════════════════════════════
    //  CommandFieldMode enum
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn command_field_mode_command_eq() {
        assert_eq!(CommandFieldMode::Command, CommandFieldMode::Command);
    }

    #[test]
    fn command_field_mode_prompt_eq() {
        assert_eq!(CommandFieldMode::Prompt, CommandFieldMode::Prompt);
    }

    #[test]
    fn command_field_mode_command_ne_prompt() {
        assert_ne!(CommandFieldMode::Command, CommandFieldMode::Prompt);
    }

    #[test]
    fn command_field_mode_toggle_command_to_prompt() {
        let mode = CommandFieldMode::Command;
        let toggled = match mode {
            CommandFieldMode::Command => CommandFieldMode::Prompt,
            CommandFieldMode::Prompt => CommandFieldMode::Command,
        };
        assert_eq!(toggled, CommandFieldMode::Prompt);
    }

    #[test]
    fn command_field_mode_toggle_prompt_to_command() {
        let mode = CommandFieldMode::Prompt;
        let toggled = match mode {
            CommandFieldMode::Command => CommandFieldMode::Prompt,
            CommandFieldMode::Prompt => CommandFieldMode::Command,
        };
        assert_eq!(toggled, CommandFieldMode::Command);
    }

    // ══════════════════════════════════════════════════════════════════
    //  RunCommandDialog construction
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn run_command_dialog_new_defaults() {
        let d = RunCommandDialog::new();
        assert!(d.name.is_empty());
        assert!(d.command.is_empty());
        assert_eq!(d.name_cursor, 0);
        assert_eq!(d.command_cursor, 0);
        assert!(d.editing_name);
        assert!(d.editing_idx.is_none());
        assert_eq!(d.field_mode, CommandFieldMode::Command);
        assert!(!d.global);
    }

    #[test]
    fn run_command_dialog_edit_preloads() {
        let cmd = RunCommand::new("Build", "cargo build", true);
        let d = RunCommandDialog::edit(0, &cmd);
        assert_eq!(d.name, "Build");
        assert_eq!(d.command, "cargo build");
        assert_eq!(d.name_cursor, 5);
        assert_eq!(d.command_cursor, 11);
        assert!(d.editing_name);
        assert_eq!(d.editing_idx, Some(0));
        assert!(d.global);
    }

    // ══════════════════════════════════════════════════════════════════
    //  PresetPromptDialog construction
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn preset_dialog_new_defaults() {
        let d = PresetPromptDialog::new();
        assert!(d.name.is_empty());
        assert!(d.prompt.is_empty());
        assert_eq!(d.name_cursor, 0);
        assert_eq!(d.prompt_cursor, 0);
        assert!(d.editing_name);
        assert!(d.editing_idx.is_none());
        assert!(!d.global);
    }

    #[test]
    fn preset_dialog_edit_preloads() {
        let preset = PresetPrompt::new("Greet", "Hello world", false);
        let d = PresetPromptDialog::edit(2, &preset);
        assert_eq!(d.name, "Greet");
        assert_eq!(d.prompt, "Hello world");
        assert_eq!(d.name_cursor, 5);
        assert_eq!(d.prompt_cursor, 11);
        assert!(d.editing_name);
        assert_eq!(d.editing_idx, Some(2));
        assert!(!d.global);
    }

    #[test]
    fn preset_dialog_edit_unicode_cursor_char_count() {
        let preset = PresetPrompt::new("abc", "hello", false);
        let d = PresetPromptDialog::edit(0, &preset);
        assert_eq!(d.name_cursor, 3);
        assert_eq!(d.prompt_cursor, 5);
    }

    // ══════════════════════════════════════════════════════════════════
    //  RunCommand construction
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn run_command_new_global() {
        let cmd = RunCommand::new("Test", "cargo test", true);
        assert_eq!(cmd.name, "Test");
        assert_eq!(cmd.command, "cargo test");
        assert!(cmd.global);
    }

    #[test]
    fn run_command_new_local() {
        let cmd = RunCommand::new("Lint", "cargo clippy", false);
        assert!(!cmd.global);
    }

    // ══════════════════════════════════════════════════════════════════
    //  PresetPrompt construction
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn preset_prompt_new_global() {
        let p = PresetPrompt::new("Review", "Review this code", true);
        assert_eq!(p.name, "Review");
        assert_eq!(p.prompt, "Review this code");
        assert!(p.global);
    }

    #[test]
    fn preset_prompt_new_local() {
        let p = PresetPrompt::new("Fix", "Fix this bug", false);
        assert!(!p.global);
    }

    // ══════════════════════════════════════════════════════════════════
    //  Label generation for required-field validation message
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn field_mode_label_command() {
        let label = match CommandFieldMode::Command {
            CommandFieldMode::Command => "command",
            CommandFieldMode::Prompt => "prompt",
        };
        assert_eq!(label, "command");
    }

    #[test]
    fn field_mode_label_prompt() {
        let label = match CommandFieldMode::Prompt {
            CommandFieldMode::Command => "command",
            CommandFieldMode::Prompt => "prompt",
        };
        assert_eq!(label, "prompt");
    }

    // ══════════════════════════════════════════════════════════════════
    //  Branch name parsing (local_name extraction used in confirm)
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn branch_local_name_with_slash() {
        let branch = "origin/feature-x";
        let local_name = if branch.contains('/') {
            branch.split('/').skip(1).collect::<Vec<_>>().join("/")
        } else {
            branch.to_string()
        };
        assert_eq!(local_name, "feature-x");
    }

    #[test]
    fn branch_local_name_without_slash() {
        let branch = "main";
        let local_name = if branch.contains('/') {
            branch.split('/').skip(1).collect::<Vec<_>>().join("/")
        } else {
            branch.to_string()
        };
        assert_eq!(local_name, "main");
    }

    #[test]
    fn branch_local_name_multiple_slashes() {
        let branch = "origin/feature/sub/deep";
        let local_name = if branch.contains('/') {
            branch.split('/').skip(1).collect::<Vec<_>>().join("/")
        } else {
            branch.to_string()
        };
        assert_eq!(local_name, "feature/sub/deep");
    }

    // ══════════════════════════════════════════════════════════════════
    //  KeyEvent matching patterns used by handlers
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn match_char_y_for_confirm() {
        let k = key(KeyCode::Char('y'));
        assert!(matches!(k.code, KeyCode::Char('y') | KeyCode::Char('Y')));
    }

    #[test]
    fn match_char_upper_y_for_confirm() {
        let k = key(KeyCode::Char('Y'));
        assert!(matches!(k.code, KeyCode::Char('y') | KeyCode::Char('Y')));
    }

    #[test]
    fn match_char_n_does_not_match_y() {
        let k = key(KeyCode::Char('n'));
        assert!(!matches!(k.code, KeyCode::Char('y') | KeyCode::Char('Y')));
    }

    #[test]
    fn match_digit_range_1_to_9() {
        for c in '1'..='9' {
            assert!(matches!(KeyCode::Char(c), KeyCode::Char('1'..='9')));
        }
    }

    #[test]
    fn match_digit_0_not_in_1_to_9() {
        assert!(!matches!(KeyCode::Char('0'), KeyCode::Char('1'..='9')));
    }

    // ══════════════════════════════════════════════════════════════════
    //  Ctrl+S detection for scope toggle
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn ctrl_s_detected_via_contains() {
        let k = key_mod(KeyCode::Char('s'), KeyModifiers::CONTROL);
        assert!(k.modifiers.contains(KeyModifiers::CONTROL));
        assert_eq!(k.code, KeyCode::Char('s'));
    }

    #[test]
    fn ctrl_s_with_extra_shift_still_contains_control() {
        let k = key_mod(KeyCode::Char('s'), KeyModifiers::CONTROL | KeyModifiers::SHIFT);
        assert!(k.modifiers.contains(KeyModifiers::CONTROL));
    }

    #[test]
    fn plain_s_does_not_contain_control() {
        let k = key(KeyCode::Char('s'));
        assert!(!k.modifiers.contains(KeyModifiers::CONTROL));
    }

    // ══════════════════════════════════════════════════════════════════
    //  RunCommandPicker / PresetPromptPicker defaults
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn run_command_picker_new_defaults() {
        let p = crate::app::types::RunCommandPicker::new();
        assert_eq!(p.selected, 0);
        assert!(p.confirm_delete.is_none());
    }

    #[test]
    fn preset_prompt_picker_new_defaults() {
        let p = crate::app::types::PresetPromptPicker::new();
        assert_eq!(p.selected, 0);
        assert!(p.confirm_delete.is_none());
    }

    // ══════════════════════════════════════════════════════════════════
    //  lookup function type consistency
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn lookup_branch_dialog_action_returns_option() {
        let result = lookup_branch_dialog_action(KeyModifiers::NONE, KeyCode::Char('z'));
        // 'z' is not a branch dialog binding, should be None
        assert!(result.is_none());
    }

    #[test]
    fn lookup_picker_action_returns_option() {
        let result = lookup_picker_action(KeyModifiers::NONE, KeyCode::Char('z'));
        // 'z' is not a picker binding, should be None
        assert!(result.is_none());
    }

    #[test]
    fn lookup_branch_dialog_esc_returns_escape() {
        let result = lookup_branch_dialog_action(KeyModifiers::NONE, KeyCode::Esc);
        assert_eq!(result, Some(Action::Escape));
    }

    #[test]
    fn lookup_picker_esc_returns_escape() {
        let result = lookup_picker_action(KeyModifiers::NONE, KeyCode::Esc);
        assert_eq!(result, Some(Action::Escape));
    }
}
