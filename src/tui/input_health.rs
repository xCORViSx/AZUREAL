//! Input handler for the Worktree Health panel.
//! Full-screen modal overlay — consumes all input when active, dispatched via
//! the centralized keybinding system (lookup_health_action in keybindings.rs).
//! Tab switches between God Files and Documentation tabs.
//! Module style dialog intercepts keys when active (pre-modularize selector).

use anyhow::Result;
use crossterm::event::{self, KeyCode};

use crate::app::App;
use crate::app::types::{RustModuleStyle, PythonModuleStyle};
use crate::claude::ClaudeProcess;
use super::keybindings::{lookup_health_action, Action};

/// Handle keyboard input when the Worktree Health panel is active.
/// Module style dialog takes priority when shown (pre-modularize selector).
/// Otherwise all keys resolved through keybindings.rs.
pub fn handle_health_input(key: event::KeyEvent, app: &mut App, claude_process: &ClaudeProcess) -> Result<()> {
    // Module style dialog intercepts all input when active
    // (transient sub-state like confirm-delete y/n — raw key matching)
    if let Some(ref panel) = app.health_panel {
        if panel.module_style_dialog.is_some() {
            return handle_module_style_input(key, app, claude_process);
        }
    }

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
        Action::HealthScopeMode => { app.enter_god_file_scope_mode(); }
        // Start modularize — may show module style dialog first
        Action::HealthModularize => { app.god_file_start_modularize(claude_process); }

        // ── Shared — `v` opens checked files in Viewer from both tabs ──
        Action::HealthViewChecked => {
            match tab {
                crate::app::types::HealthTab::GodFiles => app.god_file_view_checked(),
                crate::app::types::HealthTab::Documentation => app.doc_view_checked(),
            }
        }

        // ── Documentation tab only ──
        Action::HealthDocToggleCheck => { app.doc_toggle_check(); }
        Action::HealthDocToggleNon100 => { app.doc_toggle_non100(); }
        Action::HealthDocSpawn => { app.doc_health_spawn(claude_process); }
        _ => {}
    }
    Ok(())
}

/// Handle input for the module style selector dialog.
/// Transient sub-state — raw key matching (same pattern as confirm-delete y/n).
///   j/k/Up/Down: move cursor between language rows
///   Space/Left/Right: toggle style for current language
///   Enter: confirm and spawn GFM sessions with chosen styles
///   Esc: cancel back to god files list
fn handle_module_style_input(key: event::KeyEvent, app: &mut App, claude_process: &ClaudeProcess) -> Result<()> {
    match key.code {
        // Navigate between language rows
        KeyCode::Char('j') | KeyCode::Down => {
            if let Some(ref mut panel) = app.health_panel {
                if let Some(ref mut d) = panel.module_style_dialog {
                    let max = if d.has_rust && d.has_python { 1 } else { 0 };
                    if d.selected < max { d.selected += 1; }
                }
            }
        }
        KeyCode::Char('k') | KeyCode::Up => {
            if let Some(ref mut panel) = app.health_panel {
                if let Some(ref mut d) = panel.module_style_dialog {
                    if d.selected > 0 { d.selected -= 1; }
                }
            }
        }
        // Toggle style for the selected language row
        KeyCode::Char(' ') | KeyCode::Left | KeyCode::Right => {
            if let Some(ref mut panel) = app.health_panel {
                if let Some(ref mut d) = panel.module_style_dialog {
                    // Map selected index to which language is on that row
                    let on_rust = d.has_rust && d.selected == 0;
                    let on_python = d.has_python && (d.selected == 1 || !d.has_rust);
                    if on_rust {
                        d.rust_style = match d.rust_style {
                            RustModuleStyle::FileBased => RustModuleStyle::ModRs,
                            RustModuleStyle::ModRs => RustModuleStyle::FileBased,
                        };
                    } else if on_python {
                        d.python_style = match d.python_style {
                            PythonModuleStyle::Package => PythonModuleStyle::SingleFile,
                            PythonModuleStyle::SingleFile => PythonModuleStyle::Package,
                        };
                    }
                }
            }
        }
        // Confirm — extract styles and spawn
        KeyCode::Enter => {
            let (rust_style, python_style) = match app.health_panel {
                Some(ref panel) => match panel.module_style_dialog {
                    Some(ref d) => (
                        if d.has_rust { Some(d.rust_style) } else { None },
                        if d.has_python { Some(d.python_style) } else { None },
                    ),
                    None => (None, None),
                },
                None => (None, None),
            };
            // Clear dialog before spawning (god_file_modularize closes the panel)
            if let Some(ref mut panel) = app.health_panel {
                panel.module_style_dialog = None;
            }
            app.god_file_modularize(claude_process, rust_style, python_style);
        }
        // Cancel — close dialog, return to god files list
        KeyCode::Esc => {
            if let Some(ref mut panel) = app.health_panel {
                panel.module_style_dialog = None;
            }
        }
        _ => {} // dialog eats unrecognized keys
    }
    Ok(())
}
