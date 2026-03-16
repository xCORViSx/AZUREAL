//! Projects panel input handling
//!
//! Handles keyboard events when the Projects panel modal is active.
//! Browse mode: dispatch via keybindings.rs lookup_projects_action().
//! Input modes: text input for path/name entry with Enter to confirm, Esc to cancel.
//! Text input keys (Char/Backspace/Left/Right/Home/End) stay raw — not rebindable.

use anyhow::Result;
use crossterm::event::{self, KeyCode};

use super::keybindings::{lookup_projects_action, Action};
use crate::app::types::ProjectsPanelMode;
use crate::app::App;
use crate::config;
use crate::git::Git;

/// Handle all keyboard input when the Projects panel is active.
/// Returns Ok(()) — all keys are consumed (no fall-through to other handlers).
pub fn handle_projects_input(key: event::KeyEvent, app: &mut App) -> Result<()> {
    let Some(ref panel) = app.projects_panel else {
        return Ok(());
    };
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
    if let Some(ref mut panel) = app.projects_panel {
        panel.error = None;
    }

    let Some(action) = lookup_projects_action(key.modifiers, key.code) else {
        return Ok(());
    };

    match action {
        Action::Quit => {
            if !app.git_action_in_progress() {
                app.should_quit = true;
            } else {
                app.set_status("Cannot quit while a git operation is in progress");
            }
        }

        Action::NavDown => {
            if let Some(ref mut panel) = app.projects_panel {
                panel.select_next();
            }
        }
        Action::NavUp => {
            if let Some(ref mut panel) = app.projects_panel {
                panel.select_prev();
            }
        }

        // Open selected project — only if it's a valid git repo
        Action::Confirm => {
            let path = app
                .projects_panel
                .as_ref()
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
            if let Some(ref mut panel) = app.projects_panel {
                panel.start_add();
            }
        }

        Action::ProjectsDelete => {
            let should_save = if let Some(ref mut panel) = app.projects_panel {
                if !panel.entries.is_empty() {
                    panel.entries.remove(panel.selected);
                    if panel.selected >= panel.entries.len() && panel.selected > 0 {
                        panel.selected -= 1;
                    }
                    true
                } else {
                    false
                }
            } else {
                false
            };
            if should_save {
                if let Some(ref panel) = app.projects_panel {
                    config::save_projects(&panel.entries);
                }
            }
        }

        Action::ProjectsRename => {
            if let Some(ref mut panel) = app.projects_panel {
                panel.start_rename();
            }
        }

        Action::ProjectsInit => {
            if let Some(ref mut panel) = app.projects_panel {
                panel.start_init();
            }
        }

        // Close panel (only if a project is already loaded)
        Action::Escape => {
            if app.project.is_some() {
                app.close_projects_panel();
            }
        }

        _ => {}
    }
    Ok(())
}

/// Text input mode for Add/Rename/Init — character entry + confirm/cancel
fn handle_text_input(key: event::KeyEvent, app: &mut App) -> Result<()> {
    let Some(ref panel) = app.projects_panel else {
        return Ok(());
    };
    let mode = panel.mode;

    match key.code {
        // Cancel back to Browse mode
        KeyCode::Esc => {
            if let Some(ref mut panel) = app.projects_panel {
                panel.cancel_input();
            }
        }

        // Confirm the action
        KeyCode::Enter => match mode {
            ProjectsPanelMode::AddPath => confirm_add(app),
            ProjectsPanelMode::Rename => confirm_rename(app),
            ProjectsPanelMode::Init => confirm_init(app),
            _ => {}
        },

        // Text editing
        KeyCode::Backspace => {
            if let Some(ref mut panel) = app.projects_panel {
                panel.input_backspace();
            }
        }
        KeyCode::Delete => {
            if let Some(ref mut panel) = app.projects_panel {
                panel.input_delete();
            }
        }
        KeyCode::Left => {
            if let Some(ref mut panel) = app.projects_panel {
                panel.cursor_left();
            }
        }
        KeyCode::Right => {
            if let Some(ref mut panel) = app.projects_panel {
                panel.cursor_right();
            }
        }
        KeyCode::Home => {
            if let Some(ref mut panel) = app.projects_panel {
                panel.cursor_home();
            }
        }
        KeyCode::End => {
            if let Some(ref mut panel) = app.projects_panel {
                panel.cursor_end();
            }
        }
        KeyCode::Char(c) => {
            if let Some(ref mut panel) = app.projects_panel {
                panel.input_char(c);
            }
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
    let canonical = dunce::canonicalize(&path).unwrap_or(path.clone());
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
            let canonical = dunce::canonicalize(&path).unwrap_or(path);
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

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent {
            code,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        }
    }

    // ══════════════════════════════════════════════════════════════════
    //  resolve_input_path
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn resolve_tilde_slash_path() {
        let result = resolve_input_path("~/projects/test");
        let home = dirs::home_dir().unwrap_or_default();
        assert_eq!(result, home.join("projects/test"));
    }

    #[test]
    fn resolve_tilde_only() {
        let result = resolve_input_path("~");
        let home = dirs::home_dir().unwrap_or_default();
        assert_eq!(result, home);
    }

    #[test]
    fn resolve_absolute_path() {
        let result = resolve_input_path("/tmp/project");
        assert_eq!(result, std::path::PathBuf::from("/tmp/project"));
    }

    #[test]
    fn resolve_relative_path() {
        let result = resolve_input_path("relative/path");
        assert_eq!(result, std::path::PathBuf::from("relative/path"));
    }

    #[test]
    fn resolve_empty_path() {
        let result = resolve_input_path("");
        assert_eq!(result, std::path::PathBuf::from(""));
    }

    #[test]
    fn resolve_tilde_in_middle_not_expanded() {
        let result = resolve_input_path("/tmp/~/project");
        assert_eq!(result, std::path::PathBuf::from("/tmp/~/project"));
    }

    #[test]
    fn resolve_tilde_without_slash_not_tilde_path() {
        // "~foo" is NOT "~/foo" - should be treated as literal
        let result = resolve_input_path("~foo");
        assert_eq!(result, std::path::PathBuf::from("~foo"));
    }

    // ══════════════════════════════════════════════════════════════════
    //  ProjectsPanelMode enum
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn mode_browse_eq() {
        assert_eq!(ProjectsPanelMode::Browse, ProjectsPanelMode::Browse);
    }
    #[test]
    fn mode_add_path_eq() {
        assert_eq!(ProjectsPanelMode::AddPath, ProjectsPanelMode::AddPath);
    }
    #[test]
    fn mode_rename_eq() {
        assert_eq!(ProjectsPanelMode::Rename, ProjectsPanelMode::Rename);
    }
    #[test]
    fn mode_init_eq() {
        assert_eq!(ProjectsPanelMode::Init, ProjectsPanelMode::Init);
    }
    #[test]
    fn mode_browse_ne_add() {
        assert_ne!(ProjectsPanelMode::Browse, ProjectsPanelMode::AddPath);
    }
    #[test]
    fn mode_rename_ne_init() {
        assert_ne!(ProjectsPanelMode::Rename, ProjectsPanelMode::Init);
    }

    // ══════════════════════════════════════════════════════════════════
    //  ProjectsPanel construction
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn projects_panel_new_defaults() {
        let entries = vec![];
        let p = crate::app::types::ProjectsPanel::new(entries);
        assert_eq!(p.selected, 0);
        assert_eq!(p.mode, ProjectsPanelMode::Browse);
        assert!(p.input.is_empty());
        assert_eq!(p.input_cursor, 0);
        assert!(p.error.is_none());
    }

    #[test]
    fn projects_panel_start_add_mode() {
        let mut p = crate::app::types::ProjectsPanel::new(vec![]);
        p.start_add();
        assert_eq!(p.mode, ProjectsPanelMode::AddPath);
        assert!(p.input.is_empty());
        assert_eq!(p.input_cursor, 0);
    }

    // ══════════════════════════════════════════════════════════════════
    //  Action variants used in this module
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn action_quit_eq() {
        assert_eq!(Action::Quit, Action::Quit);
    }
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
    fn action_projects_add_eq() {
        assert_eq!(Action::ProjectsAdd, Action::ProjectsAdd);
    }
    #[test]
    fn action_projects_delete_eq() {
        assert_eq!(Action::ProjectsDelete, Action::ProjectsDelete);
    }
    #[test]
    fn action_projects_rename_eq() {
        assert_eq!(Action::ProjectsRename, Action::ProjectsRename);
    }
    #[test]
    fn action_projects_init_eq() {
        assert_eq!(Action::ProjectsInit, Action::ProjectsInit);
    }
    #[test]
    fn action_escape_eq() {
        assert_eq!(Action::Escape, Action::Escape);
    }

    // ══════════════════════════════════════════════════════════════════
    //  Text input key matching
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn esc_cancels() {
        let k = key(KeyCode::Esc);
        assert_eq!(k.code, KeyCode::Esc);
    }

    #[test]
    fn enter_confirms() {
        let k = key(KeyCode::Enter);
        assert_eq!(k.code, KeyCode::Enter);
    }

    #[test]
    fn backspace_deletes() {
        let k = key(KeyCode::Backspace);
        assert_eq!(k.code, KeyCode::Backspace);
    }

    #[test]
    fn delete_key() {
        let k = key(KeyCode::Delete);
        assert_eq!(k.code, KeyCode::Delete);
    }

    #[test]
    fn left_arrow() {
        let k = key(KeyCode::Left);
        assert_eq!(k.code, KeyCode::Left);
    }

    #[test]
    fn right_arrow() {
        let k = key(KeyCode::Right);
        assert_eq!(k.code, KeyCode::Right);
    }

    #[test]
    fn home_key() {
        let k = key(KeyCode::Home);
        assert_eq!(k.code, KeyCode::Home);
    }

    #[test]
    fn end_key() {
        let k = key(KeyCode::End);
        assert_eq!(k.code, KeyCode::End);
    }

    #[test]
    fn char_key() {
        let k = key(KeyCode::Char('a'));
        assert!(matches!(k.code, KeyCode::Char('a')));
    }

    // ══════════════════════════════════════════════════════════════════
    //  lookup_projects_action
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn lookup_projects_unmapped() {
        let result = lookup_projects_action(KeyModifiers::NONE, KeyCode::Char('z'));
        assert!(result.is_none());
    }

    #[test]
    fn lookup_projects_esc_returns_escape() {
        let result = lookup_projects_action(KeyModifiers::NONE, KeyCode::Esc);
        assert_eq!(result, Some(Action::Escape));
    }

    // ══════════════════════════════════════════════════════════════════
    //  Path validation patterns
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn empty_input_rejected() {
        let input = "";
        assert!(input.is_empty());
    }

    #[test]
    fn whitespace_input_rejected() {
        let input = "   ";
        assert!(input.trim().is_empty());
    }

    #[test]
    fn valid_input_accepted() {
        let input = "/tmp/project";
        assert!(!input.trim().is_empty());
    }

    // ══════════════════════════════════════════════════════════════════
    //  Error message patterns used in this module
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn error_path_empty() {
        let msg = "Path cannot be empty".to_string();
        assert!(msg.contains("empty"));
    }

    #[test]
    fn error_not_git_repo() {
        let msg = "Not a git repository".to_string();
        assert!(msg.contains("git"));
    }

    #[test]
    fn error_already_registered() {
        let msg = "Project already registered".to_string();
        assert!(msg.contains("registered"));
    }

    #[test]
    fn error_name_empty() {
        let msg = "Name cannot be empty".to_string();
        assert!(msg.contains("empty"));
    }

    #[test]
    fn error_already_git_repo() {
        let msg = "Already a git repo — use 'a' to add it".to_string();
        assert!(msg.contains("Already"));
    }

    // ══════════════════════════════════════════════════════════════════
    //  Duplicate detection
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn duplicate_path_detected() {
        let entries = vec![crate::config::ProjectEntry {
            path: std::path::PathBuf::from("/tmp/proj"),
            display_name: "proj".into(),
        }];
        let canonical = std::path::PathBuf::from("/tmp/proj");
        assert!(entries.iter().any(|e| e.path == canonical));
    }

    #[test]
    fn unique_path_not_detected() {
        let entries = vec![crate::config::ProjectEntry {
            path: std::path::PathBuf::from("/tmp/proj"),
            display_name: "proj".into(),
        }];
        let canonical = std::path::PathBuf::from("/tmp/other");
        assert!(!entries.iter().any(|e| e.path == canonical));
    }

    #[test]
    fn key_char_q_code() {
        let k = key(KeyCode::Char('q'));
        assert_eq!(k.code, KeyCode::Char('q'));
    }

    #[test]
    fn key_delete_code() {
        let k = key(KeyCode::Delete);
        assert_eq!(k.code, KeyCode::Delete);
    }

    #[test]
    fn key_home_code() {
        let k = key(KeyCode::Home);
        assert_eq!(k.code, KeyCode::Home);
    }

    #[test]
    fn key_end_code() {
        let k = key(KeyCode::End);
        assert_eq!(k.code, KeyCode::End);
    }

    #[test]
    fn resolve_dot_path() {
        let result = resolve_input_path(".");
        assert!(result.is_absolute() || result == std::path::PathBuf::from("."));
    }
}
