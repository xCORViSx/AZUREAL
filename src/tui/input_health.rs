//! Input handler for the Worktree Health panel.
//! Full-screen modal overlay — consumes all input when active, dispatched via
//! the centralized keybinding system (lookup_health_action in keybindings.rs).
//! Tab switches between God Files and Documentation tabs.

use anyhow::Result;
use crossterm::event;

use crate::app::App;
use crate::claude::ClaudeProcess;
use super::keybindings::{lookup_health_action, Action};

/// Handle keyboard input when the Worktree Health panel is active.
/// All keys resolved through keybindings.rs — no hardcoded KeyCode matching.
pub fn handle_health_input(key: event::KeyEvent, app: &mut App, claude_process: &ClaudeProcess) -> Result<()> {
    let tab = match app.health_panel {
        Some(ref p) => p.tab,
        None => return Ok(()),
    };

    // Resolve key → action via centralized binding arrays
    let Some(action) = lookup_health_action(tab, key.modifiers, key.code) else {
        return Ok(()); // modal eats unrecognized keys
    };

    match action {
        // ── Shared across both tabs ──
        Action::HealthSwitchTab => {
            if let Some(ref mut p) = app.health_panel {
                p.tab = match p.tab {
                    crate::app::types::HealthTab::GodFiles => crate::app::types::HealthTab::Documentation,
                    crate::app::types::HealthTab::Documentation => crate::app::types::HealthTab::GodFiles,
                };
            }
        }
        Action::Escape => { app.close_health_panel(); }
        Action::NavDown => {
            if let Some(ref mut p) = app.health_panel {
                match p.tab {
                    crate::app::types::HealthTab::GodFiles => {
                        if !p.god_files.is_empty() && p.god_selected + 1 < p.god_files.len() {
                            p.god_selected += 1;
                        }
                    }
                    crate::app::types::HealthTab::Documentation => {
                        if !p.doc_entries.is_empty() && p.doc_selected + 1 < p.doc_entries.len() {
                            p.doc_selected += 1;
                        }
                    }
                }
            }
        }
        Action::NavUp => {
            if let Some(ref mut p) = app.health_panel {
                match p.tab {
                    crate::app::types::HealthTab::GodFiles => {
                        if p.god_selected > 0 { p.god_selected -= 1; }
                    }
                    crate::app::types::HealthTab::Documentation => {
                        if p.doc_selected > 0 { p.doc_selected -= 1; }
                    }
                }
            }
        }
        Action::GoToTop => {
            if let Some(ref mut p) = app.health_panel {
                match p.tab {
                    crate::app::types::HealthTab::GodFiles => { p.god_selected = 0; }
                    crate::app::types::HealthTab::Documentation => { p.doc_selected = 0; }
                }
            }
        }
        Action::GoToBottom => {
            if let Some(ref mut p) = app.health_panel {
                match p.tab {
                    crate::app::types::HealthTab::GodFiles => {
                        if !p.god_files.is_empty() { p.god_selected = p.god_files.len() - 1; }
                    }
                    crate::app::types::HealthTab::Documentation => {
                        if !p.doc_entries.is_empty() { p.doc_selected = p.doc_entries.len() - 1; }
                    }
                }
            }
        }

        // ── God Files tab only ──
        Action::HealthToggleCheck => { app.god_file_toggle_check(); }
        Action::HealthToggleAll => { app.god_file_toggle_all(); }
        Action::HealthViewChecked => { app.god_file_view_checked(); }
        Action::HealthScopeMode => { app.enter_god_file_scope_mode(); }
        Action::HealthModularize => { app.god_file_modularize(claude_process); }

        // ── Documentation tab only ──
        Action::Confirm => {
            if let Some(ref p) = app.health_panel {
                if let Some(entry) = p.doc_entries.get(p.doc_selected) {
                    let path_str = entry.path.display().to_string();
                    app.health_panel = None;
                    app.load_file_at_path(&path_str);
                }
            }
        }
        _ => {}
    }
    Ok(())
}
