//! Projects panel input handling
//!
//! Handles keyboard events when the Projects panel modal is active.
//! Browse mode: dispatch via keybindings.rs lookup_projects_action().
//! Input modes: text input for path/name entry with Enter to confirm, Esc to cancel.
//! Text input keys (Char/Backspace/Left/Right/Home/End) stay raw — not rebindable.

use anyhow::Result;
use crossterm::event::{self, KeyCode};

use crate::app::App;
use crate::app::types::ProjectsPanelMode;
use crate::config;
use crate::git::Git;
use super::keybindings::{lookup_projects_action, Action};

/// Handle all keyboard input when the Projects panel is active.
/// Returns Ok(()) — all keys are consumed (no fall-through to other handlers).
pub fn handle_projects_input(key: event::KeyEvent, app: &mut App) -> Result<()> {
    let Some(ref panel) = app.projects_panel else { return Ok(()) };
    let mode = panel.mode;

    match mode {
        ProjectsPanelMode::Browse => handle_browse(key, app),
        ProjectsPanelMode::AddPath | ProjectsPanelMode::Rename | ProjectsPanelMode::Init => {
            handle_text_input(key, app)
        }
    }
}

/// Browse mode: resolve key via centralized bindings, then dispatch on Action
fn handle_browse(key: event::KeyEvent, app: &mut App) -> Result<()> {
    // Clear any previous error on next keypress so it doesn't persist
    if let Some(ref mut panel) = app.projects_panel { panel.error = None; }

    let Some(action) = lookup_projects_action(key.modifiers, key.code) else {
        return Ok(());
    };

    match action {
        Action::Quit => {
            if !app.git_action_in_progress() { app.should_quit = true; }
            else { app.set_status("Cannot quit while a git operation is in progress"); }
        }

        Action::NavDown => {
            if let Some(ref mut panel) = app.projects_panel { panel.select_next(); }
        }
        Action::NavUp => {
            if let Some(ref mut panel) = app.projects_panel { panel.select_prev(); }
        }

        // Open selected project — only if it's a valid git repo
        Action::Confirm => {
            let path = app.projects_panel.as_ref()
                .and_then(|p| p.entries.get(p.selected))
                .map(|e| e.path.clone());
            if let Some(path) = path {
                if !path.exists() {
                    if let Some(ref mut panel) = app.projects_panel {
                        panel.error = Some("Directory does not exist".to_string());
                    }
                } else if !Git::is_git_repo(&path) {
                    if let Some(ref mut panel) = app.projects_panel {
                        panel.error = Some("Not a git repository".to_string());
                    }
                } else {
                    // Deferred project switch — show loading while git ops + session reload run
                    app.loading_indicator = Some("Switching project…".into());
                    app.deferred_action = Some(crate::app::DeferredAction::SwitchProject { path });
                }
            }
        }

        Action::ProjectsAdd => {
            if let Some(ref mut panel) = app.projects_panel { panel.start_add(); }
        }

        Action::ProjectsDelete => {
            let should_save = if let Some(ref mut panel) = app.projects_panel {
                if !panel.entries.is_empty() {
                    panel.entries.remove(panel.selected);
                    if panel.selected >= panel.entries.len() && panel.selected > 0 {
                        panel.selected -= 1;
                    }
                    true
                } else { false }
            } else { false };
            if should_save {
                if let Some(ref panel) = app.projects_panel {
                    config::save_projects(&panel.entries);
                }
            }
        }

        Action::ProjectsRename => {
            if let Some(ref mut panel) = app.projects_panel { panel.start_rename(); }
        }

        Action::ProjectsInit => {
            if let Some(ref mut panel) = app.projects_panel { panel.start_init(); }
        }

        // Close panel (only if a project is already loaded)
        Action::Escape => {
            if app.project.is_some() { app.close_projects_panel(); }
        }

        _ => {}
    }
    Ok(())
}

/// Text input mode for Add/Rename/Init — character entry + confirm/cancel
fn handle_text_input(key: event::KeyEvent, app: &mut App) -> Result<()> {
    let Some(ref panel) = app.projects_panel else { return Ok(()) };
    let mode = panel.mode;

    match key.code {
        // Cancel back to Browse mode
        KeyCode::Esc => {
            if let Some(ref mut panel) = app.projects_panel { panel.cancel_input(); }
        }

        // Confirm the action
        KeyCode::Enter => {
            match mode {
                ProjectsPanelMode::AddPath => confirm_add(app),
                ProjectsPanelMode::Rename => confirm_rename(app),
                ProjectsPanelMode::Init => confirm_init(app),
                _ => {}
            }
        }

        // Text editing
        KeyCode::Backspace => {
            if let Some(ref mut panel) = app.projects_panel { panel.input_backspace(); }
        }
        KeyCode::Delete => {
            if let Some(ref mut panel) = app.projects_panel { panel.input_delete(); }
        }
        KeyCode::Left => {
            if let Some(ref mut panel) = app.projects_panel { panel.cursor_left(); }
        }
        KeyCode::Right => {
            if let Some(ref mut panel) = app.projects_panel { panel.cursor_right(); }
        }
        KeyCode::Home => {
            if let Some(ref mut panel) = app.projects_panel { panel.cursor_home(); }
        }
        KeyCode::End => {
            if let Some(ref mut panel) = app.projects_panel { panel.cursor_end(); }
        }
        KeyCode::Char(c) => {
            if let Some(ref mut panel) = app.projects_panel { panel.input_char(c); }
        }

        _ => {}
    }
    Ok(())
}

/// Validate and add a project path
fn confirm_add(app: &mut App) {
    let input = match app.projects_panel.as_ref() {
        Some(p) => p.input.trim().to_string(),
        None => return,
    };

    if input.is_empty() {
        if let Some(ref mut panel) = app.projects_panel {
            panel.error = Some("Path cannot be empty".to_string());
        }
        return;
    }

    // Resolve ~ and canonicalize
    let path = resolve_input_path(&input);

    // Verify it's a git repo
    if !Git::is_git_repo(&path) {
        if let Some(ref mut panel) = app.projects_panel {
            panel.error = Some("Not a git repository".to_string());
        }
        return;
    }

    // Check for duplicates
    let canonical = std::fs::canonicalize(&path).unwrap_or(path.clone());
    if let Some(ref panel) = app.projects_panel {
        if panel.entries.iter().any(|e| e.path == canonical) {
            if let Some(ref mut panel) = app.projects_panel {
                panel.error = Some("Project already registered".to_string());
            }
            return;
        }
    }

    // Add and save
    config::register_project(&canonical);
    if let Some(ref mut panel) = app.projects_panel {
        let entries = config::load_projects();
        panel.entries = entries;
        panel.cancel_input();
    }
}

/// Update the display name of the selected project
fn confirm_rename(app: &mut App) {
    let (input, selected) = match app.projects_panel.as_ref() {
        Some(p) => (p.input.trim().to_string(), p.selected),
        None => return,
    };

    if input.is_empty() {
        if let Some(ref mut panel) = app.projects_panel {
            panel.error = Some("Name cannot be empty".to_string());
        }
        return;
    }

    if let Some(ref mut panel) = app.projects_panel {
        if let Some(entry) = panel.entries.get_mut(selected) {
            entry.display_name = input;
        }
        config::save_projects(&panel.entries);
        panel.cancel_input();
    }
}

/// Initialize a new git repo at the given path (or cwd if blank)
fn confirm_init(app: &mut App) {
    let input = match app.projects_panel.as_ref() {
        Some(p) => p.input.trim().to_string(),
        None => return,
    };

    // Blank = use current working directory
    let path = if input.is_empty() {
        std::env::current_dir().unwrap_or_default()
    } else {
        resolve_input_path(&input)
    };

    // Guard: don't re-init an existing git repo — user should use 'a' (add) instead
    if path.exists() && Git::is_git_repo(&path) {
        if let Some(ref mut panel) = app.projects_panel {
            panel.error = Some("Already a git repo — use 'a' to add it".to_string());
        }
        return;
    }

    // Create directory if it doesn't exist
    if !path.exists() {
        if let Err(e) = std::fs::create_dir_all(&path) {
            if let Some(ref mut panel) = app.projects_panel {
                panel.error = Some(format!("Cannot create directory: {}", e));
            }
            return;
        }
    }

    // Run git init
    let result = std::process::Command::new("git")
        .args(["init"])
        .current_dir(&path)
        .output();

    match result {
        Ok(output) if output.status.success() => {
            // Register the new repo and refresh
            let canonical = std::fs::canonicalize(&path).unwrap_or(path);
            config::register_project(&canonical);
            if let Some(ref mut panel) = app.projects_panel {
                panel.entries = config::load_projects();
                panel.cancel_input();
            }
            app.set_status("Git repository initialized");
        }
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            if let Some(ref mut panel) = app.projects_panel {
                panel.error = Some(format!("git init failed: {}", stderr.trim()));
            }
        }
        Err(e) => {
            if let Some(ref mut panel) = app.projects_panel {
                panel.error = Some(format!("Cannot run git: {}", e));
            }
        }
    }
}

/// Expand ~ prefix and return a PathBuf
fn resolve_input_path(raw: &str) -> std::path::PathBuf {
    if let Some(rest) = raw.strip_prefix("~/") {
        dirs::home_dir().unwrap_or_default().join(rest)
    } else if raw == "~" {
        dirs::home_dir().unwrap_or_default()
    } else {
        std::path::PathBuf::from(raw)
    }
}
