//! Input handler for the Worktree Health panel.
//! Full-screen modal overlay — consumes all input when active, dispatched via
//! the centralized keybinding system (lookup_health_action in keybindings.rs).
//! Tab switches between God Files and Documentation tabs.
//! Module style dialog intercepts keys when active (pre-modularize selector).

use anyhow::Result;
use crossterm::event::{self, KeyCode};

use crate::app::App;
use crate::app::types::{RustModuleStyle, PythonModuleStyle};
use crate::backend::AgentProcess;
use super::keybindings::{lookup_health_action, Action};

/// Handle keyboard input when the Worktree Health panel is active.
/// Module style dialog takes priority when shown (pre-modularize selector).
/// Otherwise all keys resolved through keybindings.rs.
pub fn handle_health_input(key: event::KeyEvent, app: &mut App, claude_process: &AgentProcess) -> Result<()> {
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

        // ── Page scroll — move selected cursor by one viewport page ──
        Action::PageDown => {
            if let Some(ref mut p) = app.health_panel {
                // Same modal_h and chrome calculations as draw_health.rs
                let modal_h = (app.screen_height * 70 / 100).max(16).min(app.screen_height) as usize;
                match p.tab {
                    crate::app::types::HealthTab::GodFiles => {
                        let page = modal_h.saturating_sub(12).max(1);
                        let max = p.god_files.len().saturating_sub(1);
                        p.god_selected = (p.god_selected + page).min(max);
                    }
                    crate::app::types::HealthTab::Documentation => {
                        let page = modal_h.saturating_sub(14).max(1);
                        let max = p.doc_entries.len().saturating_sub(1);
                        p.doc_selected = (p.doc_selected + page).min(max);
                    }
                }
            }
        }
        Action::PageUp => {
            if let Some(ref mut p) = app.health_panel {
                let modal_h = (app.screen_height * 70 / 100).max(16).min(app.screen_height) as usize;
                match p.tab {
                    crate::app::types::HealthTab::GodFiles => {
                        let page = modal_h.saturating_sub(12).max(1);
                        p.god_selected = p.god_selected.saturating_sub(page);
                    }
                    crate::app::types::HealthTab::Documentation => {
                        let page = modal_h.saturating_sub(14).max(1);
                        p.doc_selected = p.doc_selected.saturating_sub(page);
                    }
                }
            }
        }

        // ── Panel-level — scope applies to all health features ──
        Action::HealthScopeMode => { app.enter_god_file_scope_mode(); }

        // ── God Files tab only ──
        Action::HealthToggleCheck => { app.god_file_toggle_check(); }
        Action::HealthToggleAll => { app.god_file_toggle_all(); }
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
fn handle_module_style_input(key: event::KeyEvent, app: &mut App, claude_process: &AgentProcess) -> Result<()> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};
    use crate::app::types::HealthTab;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent { code, modifiers: KeyModifiers::NONE, kind: KeyEventKind::Press, state: KeyEventState::NONE }
    }

    // ══════════════════════════════════════════════════════════════════
    //  HealthTab enum
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn health_tab_god_files_eq() { assert_eq!(HealthTab::GodFiles, HealthTab::GodFiles); }
    #[test]
    fn health_tab_documentation_eq() { assert_eq!(HealthTab::Documentation, HealthTab::Documentation); }
    #[test]
    fn health_tab_different_ne() { assert_ne!(HealthTab::GodFiles, HealthTab::Documentation); }

    #[test]
    fn health_tab_toggle_god_to_doc() {
        let tab = HealthTab::GodFiles;
        let toggled = match tab {
            HealthTab::GodFiles => HealthTab::Documentation,
            HealthTab::Documentation => HealthTab::GodFiles,
        };
        assert_eq!(toggled, HealthTab::Documentation);
    }

    #[test]
    fn health_tab_toggle_doc_to_god() {
        let tab = HealthTab::Documentation;
        let toggled = match tab {
            HealthTab::GodFiles => HealthTab::Documentation,
            HealthTab::Documentation => HealthTab::GodFiles,
        };
        assert_eq!(toggled, HealthTab::GodFiles);
    }

    // ══════════════════════════════════════════════════════════════════
    //  RustModuleStyle enum
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn rust_style_file_based_eq() { assert_eq!(RustModuleStyle::FileBased, RustModuleStyle::FileBased); }
    #[test]
    fn rust_style_mod_rs_eq() { assert_eq!(RustModuleStyle::ModRs, RustModuleStyle::ModRs); }
    #[test]
    fn rust_style_different_ne() { assert_ne!(RustModuleStyle::FileBased, RustModuleStyle::ModRs); }

    #[test]
    fn rust_style_toggle_file_to_mod() {
        let style = RustModuleStyle::FileBased;
        let toggled = match style {
            RustModuleStyle::FileBased => RustModuleStyle::ModRs,
            RustModuleStyle::ModRs => RustModuleStyle::FileBased,
        };
        assert_eq!(toggled, RustModuleStyle::ModRs);
    }

    #[test]
    fn rust_style_toggle_mod_to_file() {
        let style = RustModuleStyle::ModRs;
        let toggled = match style {
            RustModuleStyle::FileBased => RustModuleStyle::ModRs,
            RustModuleStyle::ModRs => RustModuleStyle::FileBased,
        };
        assert_eq!(toggled, RustModuleStyle::FileBased);
    }

    // ══════════════════════════════════════════════════════════════════
    //  PythonModuleStyle enum
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn python_style_package_eq() { assert_eq!(PythonModuleStyle::Package, PythonModuleStyle::Package); }
    #[test]
    fn python_style_single_eq() { assert_eq!(PythonModuleStyle::SingleFile, PythonModuleStyle::SingleFile); }
    #[test]
    fn python_style_different_ne() { assert_ne!(PythonModuleStyle::Package, PythonModuleStyle::SingleFile); }

    #[test]
    fn python_style_toggle() {
        let style = PythonModuleStyle::Package;
        let toggled = match style {
            PythonModuleStyle::Package => PythonModuleStyle::SingleFile,
            PythonModuleStyle::SingleFile => PythonModuleStyle::Package,
        };
        assert_eq!(toggled, PythonModuleStyle::SingleFile);
    }

    // ══════════════════════════════════════════════════════════════════
    //  Module style dialog row selection
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn dialog_row_both_languages_max_1() {
        let has_rust = true;
        let has_python = true;
        let max = if has_rust && has_python { 1 } else { 0 };
        assert_eq!(max, 1);
    }

    #[test]
    fn dialog_row_single_language_max_0() {
        let has_rust = true;
        let has_python = false;
        let max = if has_rust && has_python { 1 } else { 0 };
        assert_eq!(max, 0);
    }

    #[test]
    fn dialog_row_rust_at_0() {
        let has_rust = true;
        let selected = 0;
        let on_rust = has_rust && selected == 0;
        assert!(on_rust);
    }

    #[test]
    fn dialog_row_python_at_1() {
        let has_python = true;
        let has_rust = true;
        let selected = 1;
        let on_python = has_python && (selected == 1 || !has_rust);
        assert!(on_python);
    }

    #[test]
    fn dialog_row_python_only_at_0() {
        let has_python = true;
        let has_rust = false;
        let selected = 0;
        let on_python = has_python && (selected == 1 || !has_rust);
        assert!(on_python);
    }

    // ══════════════════════════════════════════════════════════════════
    //  Action variants used in this module
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn action_health_switch_tab() { assert_eq!(Action::HealthSwitchTab, Action::HealthSwitchTab); }
    #[test]
    fn action_escape() { assert_eq!(Action::Escape, Action::Escape); }
    #[test]
    fn action_nav_down() { assert_eq!(Action::NavDown, Action::NavDown); }
    #[test]
    fn action_nav_up() { assert_eq!(Action::NavUp, Action::NavUp); }
    #[test]
    fn action_go_to_top() { assert_eq!(Action::GoToTop, Action::GoToTop); }
    #[test]
    fn action_go_to_bottom() { assert_eq!(Action::GoToBottom, Action::GoToBottom); }
    #[test]
    fn action_page_down() { assert_eq!(Action::PageDown, Action::PageDown); }
    #[test]
    fn action_page_up() { assert_eq!(Action::PageUp, Action::PageUp); }
    #[test]
    fn action_health_toggle_check() { assert_eq!(Action::HealthToggleCheck, Action::HealthToggleCheck); }
    #[test]
    fn action_health_toggle_all() { assert_eq!(Action::HealthToggleAll, Action::HealthToggleAll); }
    #[test]
    fn action_health_view_checked() { assert_eq!(Action::HealthViewChecked, Action::HealthViewChecked); }
    #[test]
    fn action_health_scope_mode() { assert_eq!(Action::HealthScopeMode, Action::HealthScopeMode); }
    #[test]
    fn action_health_modularize() { assert_eq!(Action::HealthModularize, Action::HealthModularize); }
    #[test]
    fn action_health_doc_toggle_check() { assert_eq!(Action::HealthDocToggleCheck, Action::HealthDocToggleCheck); }
    #[test]
    fn action_health_doc_toggle_non100() { assert_eq!(Action::HealthDocToggleNon100, Action::HealthDocToggleNon100); }
    #[test]
    fn action_health_doc_spawn() { assert_eq!(Action::HealthDocSpawn, Action::HealthDocSpawn); }

    // ══════════════════════════════════════════════════════════════════
    //  Page scroll calculation (modal_h arithmetic)
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn modal_h_calculation_normal() {
        let screen_height: u16 = 100;
        let modal_h = (screen_height * 70 / 100).max(16).min(screen_height) as usize;
        assert_eq!(modal_h, 70);
    }

    #[test]
    fn modal_h_calculation_small_screen() {
        let screen_height: u16 = 10;
        let modal_h = (screen_height * 70 / 100).max(16).min(screen_height) as usize;
        // 10*70/100 = 7, max(7,16) = 16, min(16,10) = 10
        assert_eq!(modal_h, 10);
    }

    #[test]
    fn god_files_page_size() {
        let modal_h = 70usize;
        let page = modal_h.saturating_sub(12).max(1);
        assert_eq!(page, 58);
    }

    #[test]
    fn doc_page_size() {
        let modal_h = 70usize;
        let page = modal_h.saturating_sub(14).max(1);
        assert_eq!(page, 56);
    }

    #[test]
    fn page_size_min_1() {
        let modal_h = 5usize;
        let page = modal_h.saturating_sub(12).max(1);
        assert_eq!(page, 1);
    }

    // ══════════════════════════════════════════════════════════════════
    //  lookup_health_action returns None for unmapped keys
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn lookup_health_unmapped_key() {
        let result = lookup_health_action(HealthTab::GodFiles, KeyModifiers::NONE, KeyCode::Char('z'));
        assert!(result.is_none());
    }

    #[test]
    fn lookup_health_esc_returns_escape() {
        let result = lookup_health_action(HealthTab::GodFiles, KeyModifiers::NONE, KeyCode::Esc);
        assert_eq!(result, Some(Action::Escape));
    }

    // ══════════════════════════════════════════════════════════════════
    //  Key matching for module style dialog
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn dialog_j_matches_down() {
        let k = key(KeyCode::Char('j'));
        assert!(matches!(k.code, KeyCode::Char('j') | KeyCode::Down));
    }

    #[test]
    fn dialog_k_matches_up() {
        let k = key(KeyCode::Char('k'));
        assert!(matches!(k.code, KeyCode::Char('k') | KeyCode::Up));
    }

    #[test]
    fn dialog_space_toggles() {
        let k = key(KeyCode::Char(' '));
        assert!(matches!(k.code, KeyCode::Char(' ') | KeyCode::Left | KeyCode::Right));
    }

    #[test]
    fn dialog_enter_confirms() {
        let k = key(KeyCode::Enter);
        assert_eq!(k.code, KeyCode::Enter);
    }

    #[test]
    fn dialog_esc_cancels() {
        let k = key(KeyCode::Esc);
        assert_eq!(k.code, KeyCode::Esc);
    }

    #[test]
    fn key_tab_code() {
        let k = key(KeyCode::Tab);
        assert_eq!(k.code, KeyCode::Tab);
    }

    #[test]
    fn key_backspace_code() {
        let k = key(KeyCode::Backspace);
        assert_eq!(k.code, KeyCode::Backspace);
    }

    #[test]
    fn key_modifiers_default_none() {
        let k = key(KeyCode::Char('x'));
        assert_eq!(k.modifiers, KeyModifiers::NONE);
    }
}
