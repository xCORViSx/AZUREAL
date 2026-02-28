//! Key-to-action resolution
//!
//! The single source of truth for resolving key presses into `Action` values.
//! `lookup_action()` handles non-modal contexts; each modal panel has its own
//! `lookup_*_action()` function with context-aware guards.

use crossterm::event::{KeyCode, KeyModifiers};
use crate::app::Focus;

use super::types::{Action, Keybinding};
use super::bindings::*;

/// All state needed to resolve a key press into an action.
/// Built from &App so guards are defined ONCE here, not scattered across input handlers.
pub struct KeyContext {
    pub focus: Focus,
    pub prompt_mode: bool,
    pub edit_mode: bool,
    pub terminal_mode: bool,
    pub filter_active: bool,
    pub help_open: bool,
}

impl KeyContext {
    /// Build context from current app state — captures all guard-relevant fields
    pub fn from_app(app: &crate::app::App) -> Self {
        Self {
            focus: app.focus,
            prompt_mode: app.prompt_mode,
            edit_mode: app.viewer_edit_mode,
            terminal_mode: app.terminal_mode,
            filter_active: app.sidebar_filter_active,
            help_open: app.show_help,
        }
    }
}

/// Find matching action for current context.
/// This is the SINGLE SOURCE OF TRUTH for key → action resolution.
/// All guard logic lives here — callers never need to duplicate guards.
pub fn lookup_action(ctx: &KeyContext, modifiers: KeyModifiers, code: KeyCode) -> Option<Action> {
    // Global bindings checked first — with context-sensitive skips.
    // Each skip condition prevents the binding from firing in contexts where
    // it would conflict with text input or modal overlays.
    for binding in &GLOBAL {
        let skip = match binding.action {
            // Single-letter globals must not fire during text input, edit mode,
            // sidebar filter, or wizard — they'd steal keystrokes
            Action::EnterPromptMode | Action::ToggleTerminal | Action::ToggleHelp
            | Action::OpenGitActions | Action::OpenHealth
                if ctx.prompt_mode || ctx.edit_mode || ctx.terminal_mode
                   || ctx.filter_active => true,
            // ⌘C global copy must not fire in edit mode — edit handler owns clipboard
            Action::CopySelection if ctx.edit_mode => true,
            // Tab/Shift+Tab must not steal focus in edit mode, help overlay, or wizard
            Action::CycleFocusForward | Action::CycleFocusBackward
                if ctx.edit_mode || ctx.help_open => true,
            // 'p' also fires when already in prompt mode to re-focus input from another
            // pane — but NOT when focus is already on Input (would be a no-op that eats 'p')
            Action::EnterPromptMode
                if ctx.prompt_mode && ctx.focus == Focus::Input => true,
            _ => false,
        };
        if !skip && binding.matches(modifiers, code) {
            return Some(binding.action);
        }
    }

    // Context-specific bindings based on focus + mode
    let context_bindings: &[Keybinding] = match ctx.focus {
        Focus::Worktrees => &WORKTREES,
        Focus::FileTree => &FILE_TREE,
        Focus::Viewer if ctx.edit_mode => &EDIT_MODE,
        Focus::Viewer => &VIEWER,
        Focus::Session => &SESSION,
        Focus::Input if ctx.terminal_mode && !ctx.prompt_mode => &TERMINAL,
        Focus::Input if ctx.prompt_mode => &INPUT,
        _ => &[],
    };

    for binding in context_bindings {
        if binding.matches(modifiers, code) {
            return Some(binding.action);
        }
    }

    None
}

/// Resolve key → Action for the Health panel.
/// Checks shared bindings (Tab/nav/Esc) first, then tab-specific bindings.
pub fn lookup_health_action(
    tab: crate::app::types::HealthTab,
    modifiers: KeyModifiers,
    code: KeyCode,
) -> Option<Action> {
    for b in &HEALTH_SHARED { if b.matches(modifiers, code) { return Some(b.action); } }
    let tab_bindings: &[Keybinding] = match tab {
        crate::app::types::HealthTab::GodFiles => &HEALTH_GOD_FILES,
        crate::app::types::HealthTab::Documentation => &HEALTH_DOCS,
    };
    for b in tab_bindings { if b.matches(modifiers, code) { return Some(b.action); } }
    None
}

/// Resolve key → Action for the Git Actions panel.
/// Context-aware: squash-merge only on feature branches, pull only on main.
/// Git ops only fire when actions_focused=true; diff only when file list focused.
pub fn lookup_git_actions_action(
    focused_pane: u8,
    is_on_main: bool,
    modifiers: KeyModifiers,
    code: KeyCode,
) -> Option<Action> {
    let actions_focused = focused_pane == 0;
    for b in &GIT_ACTIONS {
        let skip = match b.action {
            // Squash merge + rebase + auto-rebase only available on feature branches (not main)
            Action::GitSquashMerge | Action::GitRebase | Action::GitAutoRebase if is_on_main || !actions_focused => true,
            // Pull only available on main branch
            Action::GitPull if !is_on_main || !actions_focused => true,
            // Commit + push + auto-resolve settings need actions focus
            Action::GitCommit | Action::GitPush | Action::GitAutoResolveSettings if !actions_focused => true,
            // Diff only from file list
            Action::GitViewDiff if actions_focused => true,
            _ => false,
        };
        if !skip && b.matches(modifiers, code) { return Some(b.action); }
    }
    None
}

/// Resolve key → Action for the Projects panel (browse mode only).
/// Text input modes (Add/Rename/Init) handle keys raw — don't call this for them.
pub fn lookup_projects_action(modifiers: KeyModifiers, code: KeyCode) -> Option<Action> {
    for b in &PROJECTS_BROWSE { if b.matches(modifiers, code) { return Some(b.action); } }
    None
}

/// Resolve key → Action for picker overlays (run commands, preset prompts).
/// Number quick-select and confirm-delete y/n stay raw in handlers.
pub fn lookup_picker_action(modifiers: KeyModifiers, code: KeyCode) -> Option<Action> {
    for b in &PICKER { if b.matches(modifiers, code) { return Some(b.action); } }
    None
}

/// Resolve key → Action for the branch dialog overlay.
/// Filter chars (typing to search) stay raw in the handler.
pub fn lookup_branch_dialog_action(modifiers: KeyModifiers, code: KeyCode) -> Option<Action> {
    for b in &BRANCH_DIALOG { if b.matches(modifiers, code) { return Some(b.action); } }
    None
}
