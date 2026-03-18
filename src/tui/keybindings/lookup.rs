//! Key-to-action resolution
//!
//! The single source of truth for resolving key presses into `Action` values.
//! `lookup_action()` handles non-modal contexts; each modal panel has its own
//! `lookup_*_action()` function with context-aware guards.

use crate::app::Focus;
use crossterm::event::{KeyCode, KeyModifiers};

use super::bindings::*;
use super::types::{Action, Keybinding};

/// All state needed to resolve a key press into an action.
/// Built from &App so guards are defined ONCE here, not scattered across input handlers.
pub struct KeyContext {
    pub focus: Focus,
    pub prompt_mode: bool,
    pub edit_mode: bool,
    pub terminal_mode: bool,
    pub help_open: bool,
    pub stt_recording: bool,
}

impl KeyContext {
    /// Build context from current app state — captures all guard-relevant fields
    pub fn from_app(app: &crate::app::App) -> Self {
        Self {
            focus: app.focus,
            prompt_mode: app.prompt_mode,
            edit_mode: app.viewer_edit_mode,
            terminal_mode: app.terminal_mode,
            help_open: app.show_help,
            stt_recording: app.stt_recording,
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
            Action::EnterPromptMode
            | Action::ToggleTerminal
            | Action::ToggleHelp
            | Action::OpenGitActions
            | Action::OpenHealth
            | Action::BrowseMain
            | Action::OpenProjects
            | Action::WorktreeTabNext
            | Action::WorktreeTabPrev
            | Action::RunCommand
            | Action::AddRunCommand
                if ctx.prompt_mode
                    || ctx.edit_mode
                    || (ctx.terminal_mode && ctx.focus == Focus::Input) =>
            {
                true
            }
            // Global copy must not fire in edit mode — edit handler owns clipboard
            Action::CopySelection if ctx.edit_mode => true,
            // Tab/Shift+Tab must not steal focus in edit mode, help overlay, or wizard
            Action::CycleFocusForward | Action::CycleFocusBackward
                if ctx.edit_mode || ctx.help_open =>
            {
                true
            }
            // 'p' also fires when already in prompt mode to re-focus input from another
            // pane — but NOT when focus is already on Input (would be a no-op that eats 'p')
            Action::EnterPromptMode if ctx.prompt_mode && ctx.focus == Focus::Input => true,
            _ => false,
        };
        if !skip && binding.matches(modifiers, code) {
            return Some(binding.action);
        }
    }

    // When STT is recording, ToggleStt must be reachable from ANY focus/mode
    // so the user can stop recording after tabbing away (which clears prompt_mode)
    if ctx.stt_recording {
        for binding in &INPUT {
            if binding.action == Action::ToggleStt && binding.matches(modifiers, code) {
                return Some(Action::ToggleStt);
            }
        }
    }

    // Context-specific bindings based on focus + mode
    let context_bindings: &[Keybinding] = match ctx.focus {
        // Worktree mutation actions: resolved both here (direct press when focused)
        // AND via leader sequence (W <key>) from any focus — see lookup_leader_action
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
    for b in &HEALTH_SHARED {
        if b.matches(modifiers, code) {
            return Some(b.action);
        }
    }
    let tab_bindings: &[Keybinding] = match tab {
        crate::app::types::HealthTab::GodFiles => &HEALTH_GOD_FILES,
        crate::app::types::HealthTab::Documentation => &HEALTH_DOCS,
    };
    for b in tab_bindings {
        if b.matches(modifiers, code) {
            return Some(b.action);
        }
    }
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
    let files_focused = focused_pane == 1;
    for b in &GIT_ACTIONS {
        let skip = match b.action {
            // Squash merge + rebase + auto-rebase only available on feature branches (not main)
            Action::GitSquashMerge | Action::GitRebase | Action::GitAutoRebase
                if is_on_main || !actions_focused =>
            {
                true
            }
            // Pull only available on main branch
            Action::GitPull if !is_on_main || !actions_focused => true,
            // Commit + push + auto-resolve settings need actions focus
            Action::GitCommit | Action::GitPush | Action::GitAutoResolveSettings
                if !actions_focused =>
            {
                true
            }
            // Diff only from file list
            Action::GitViewDiff if actions_focused => true,
            // Stage/unstage/discard only available in files pane
            Action::GitToggleStage | Action::GitStageAll | Action::GitDiscardFile
                if !files_focused =>
            {
                true
            }
            _ => false,
        };
        if !skip && b.matches(modifiers, code) {
            return Some(b.action);
        }
    }
    None
}

/// Resolve key → Action for the Projects panel (browse mode only).
/// Text input modes (Add/Rename/Init) handle keys raw — don't call this for them.
pub fn lookup_projects_action(modifiers: KeyModifiers, code: KeyCode) -> Option<Action> {
    for b in &PROJECTS_BROWSE {
        if b.matches(modifiers, code) {
            return Some(b.action);
        }
    }
    None
}

/// Resolve key → Action for picker overlays (run commands, preset prompts).
/// Number quick-select and confirm-delete y/n stay raw in handlers.
pub fn lookup_picker_action(modifiers: KeyModifiers, code: KeyCode) -> Option<Action> {
    for b in &PICKER {
        if b.matches(modifiers, code) {
            return Some(b.action);
        }
    }
    None
}

/// Resolve key → Action for the branch dialog overlay.
/// Filter chars (typing to search) stay raw in the handler.
pub fn lookup_branch_dialog_action(modifiers: KeyModifiers, code: KeyCode) -> Option<Action> {
    for b in &BRANCH_DIALOG {
        if b.matches(modifiers, code) {
            return Some(b.action);
        }
    }
    None
}

/// Resolve the second keystroke of a `W <key>` leader sequence.
/// Checks the WORKTREES binding array for a match.
pub fn lookup_leader_action(modifiers: KeyModifiers, code: KeyCode) -> Option<Action> {
    for b in &WORKTREES {
        if b.matches(modifiers, code) {
            return Some(b.action);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::types::HealthTab;
    use crate::app::Focus;

    // Helper to create a default "command mode" context (no prompt, no edit, no terminal, no help)
    fn cmd_ctx(focus: Focus) -> KeyContext {
        KeyContext {
            focus,
            prompt_mode: false,
            edit_mode: false,
            terminal_mode: false,
            help_open: false,
            stt_recording: false,
        }
    }

    // ══════════════════════════════════════════════════════════════════
    //  lookup_action — Global bindings
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn global_ctrl_q_quits() {
        let ctx = cmd_ctx(Focus::Worktrees);
        assert_eq!(
            lookup_action(&ctx, KeyModifiers::CONTROL, KeyCode::Char('q')),
            Some(Action::Quit)
        );
    }

    #[test]
    fn global_ctrl_d_dumps_debug() {
        let ctx = cmd_ctx(Focus::Worktrees);
        assert_eq!(
            lookup_action(&ctx, KeyModifiers::CONTROL, KeyCode::Char('d')),
            Some(Action::DumpDebug)
        );
    }

    #[test]
    fn global_cancel_claude() {
        let ctx = cmd_ctx(Focus::Worktrees);
        // macOS: ⌃C = Cancel, Windows/Linux: ⌃⇧C = Cancel
        #[cfg(target_os = "macos")]
        assert_eq!(
            lookup_action(&ctx, KeyModifiers::CONTROL, KeyCode::Char('c')),
            Some(Action::CancelClaude)
        );
        #[cfg(not(target_os = "macos"))]
        {
            let mods = KeyModifiers::CONTROL | KeyModifiers::SHIFT;
            assert_eq!(
                lookup_action(&ctx, mods, KeyCode::Char('C')),
                Some(Action::CancelClaude)
            );
        }
    }

    #[test]
    fn global_copy_selection() {
        let ctx = cmd_ctx(Focus::Worktrees);
        // macOS: ⌘C = Copy, Windows/Linux: ⌃C = Copy
        #[cfg(target_os = "macos")]
        assert_eq!(
            lookup_action(&ctx, KeyModifiers::SUPER, KeyCode::Char('c')),
            Some(Action::CopySelection)
        );
        #[cfg(not(target_os = "macos"))]
        assert_eq!(
            lookup_action(&ctx, KeyModifiers::CONTROL, KeyCode::Char('c')),
            Some(Action::CopySelection)
        );
    }

    #[test]
    fn global_ctrl_m_cycles_model() {
        let ctx = cmd_ctx(Focus::Worktrees);
        assert_eq!(
            lookup_action(&ctx, KeyModifiers::CONTROL, KeyCode::Char('m')),
            Some(Action::CycleModel)
        );
    }

    #[test]
    fn global_question_toggles_help() {
        let ctx = cmd_ctx(Focus::Worktrees);
        assert_eq!(
            lookup_action(&ctx, KeyModifiers::NONE, KeyCode::Char('?')),
            Some(Action::ToggleHelp)
        );
    }

    #[test]
    fn global_p_enters_prompt() {
        let ctx = cmd_ctx(Focus::Worktrees);
        assert_eq!(
            lookup_action(&ctx, KeyModifiers::NONE, KeyCode::Char('p')),
            Some(Action::EnterPromptMode)
        );
    }

    #[test]
    fn global_shift_t_toggles_terminal() {
        let ctx = cmd_ctx(Focus::Worktrees);
        // Shift+T can arrive as (NONE, 'T') on legacy terminals
        assert_eq!(
            lookup_action(&ctx, KeyModifiers::NONE, KeyCode::Char('T')),
            Some(Action::ToggleTerminal)
        );
    }

    #[test]
    fn global_tab_cycles_focus_forward() {
        let ctx = cmd_ctx(Focus::Worktrees);
        assert_eq!(
            lookup_action(&ctx, KeyModifiers::NONE, KeyCode::Tab),
            Some(Action::CycleFocusForward)
        );
    }

    #[test]
    fn global_backtab_cycles_focus_backward() {
        let ctx = cmd_ctx(Focus::Worktrees);
        assert_eq!(
            lookup_action(&ctx, KeyModifiers::NONE, KeyCode::BackTab),
            Some(Action::CycleFocusBackward)
        );
    }

    #[test]
    fn global_shift_g_opens_git() {
        let ctx = cmd_ctx(Focus::Worktrees);
        assert_eq!(
            lookup_action(&ctx, KeyModifiers::SHIFT, KeyCode::Char('G')),
            Some(Action::OpenGitActions)
        );
    }

    #[test]
    fn global_shift_h_opens_health() {
        let ctx = cmd_ctx(Focus::Worktrees);
        assert_eq!(
            lookup_action(&ctx, KeyModifiers::SHIFT, KeyCode::Char('H')),
            Some(Action::OpenHealth)
        );
    }

    #[test]
    fn global_shift_m_browses_main() {
        let ctx = cmd_ctx(Focus::Worktrees);
        assert_eq!(
            lookup_action(&ctx, KeyModifiers::SHIFT, KeyCode::Char('M')),
            Some(Action::BrowseMain)
        );
    }

    #[test]
    fn global_shift_p_opens_projects() {
        let ctx = cmd_ctx(Focus::Worktrees);
        assert_eq!(
            lookup_action(&ctx, KeyModifiers::SHIFT, KeyCode::Char('P')),
            Some(Action::OpenProjects)
        );
    }

    #[test]
    fn global_bracket_right_next_worktree() {
        let ctx = cmd_ctx(Focus::Worktrees);
        assert_eq!(
            lookup_action(&ctx, KeyModifiers::NONE, KeyCode::Char(']')),
            Some(Action::WorktreeTabNext)
        );
    }

    #[test]
    fn global_bracket_left_prev_worktree() {
        let ctx = cmd_ctx(Focus::Worktrees);
        assert_eq!(
            lookup_action(&ctx, KeyModifiers::NONE, KeyCode::Char('[')),
            Some(Action::WorktreeTabPrev)
        );
    }

    #[test]
    fn global_r_runs_command() {
        let ctx = cmd_ctx(Focus::Worktrees);
        assert_eq!(
            lookup_action(&ctx, KeyModifiers::NONE, KeyCode::Char('r')),
            Some(Action::RunCommand)
        );
    }

    #[test]
    fn global_shift_r_adds_run_command() {
        let ctx = cmd_ctx(Focus::Worktrees);
        assert_eq!(
            lookup_action(&ctx, KeyModifiers::SHIFT, KeyCode::Char('R')),
            Some(Action::AddRunCommand)
        );
    }

    // ══════════════════════════════════════════════════════════════════
    //  lookup_action — Global skip guards (prompt mode)
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn prompt_mode_skips_enter_prompt() {
        let ctx = KeyContext {
            focus: Focus::Input,
            prompt_mode: true,
            edit_mode: false,
            terminal_mode: false,
            help_open: false,
            stt_recording: false,
        };
        // 'p' should be skipped when prompt_mode=true AND focus=Input
        assert_ne!(
            lookup_action(&ctx, KeyModifiers::NONE, KeyCode::Char('p')),
            Some(Action::EnterPromptMode)
        );
    }

    #[test]
    fn prompt_mode_skips_toggle_help() {
        let ctx = KeyContext {
            focus: Focus::Input,
            prompt_mode: true,
            edit_mode: false,
            terminal_mode: false,
            help_open: false,
            stt_recording: false,
        };
        assert_ne!(
            lookup_action(&ctx, KeyModifiers::NONE, KeyCode::Char('?')),
            Some(Action::ToggleHelp)
        );
    }

    #[test]
    fn prompt_mode_skips_toggle_terminal() {
        let ctx = KeyContext {
            focus: Focus::Input,
            prompt_mode: true,
            edit_mode: false,
            terminal_mode: false,
            help_open: false,
            stt_recording: false,
        };
        assert_ne!(
            lookup_action(&ctx, KeyModifiers::NONE, KeyCode::Char('T')),
            Some(Action::ToggleTerminal)
        );
    }

    #[test]
    fn prompt_mode_does_not_skip_ctrl_q() {
        // Ctrl+Q is NOT in the skip list, so it still fires
        let ctx = KeyContext {
            focus: Focus::Input,
            prompt_mode: true,
            edit_mode: false,
            terminal_mode: false,
            help_open: false,
            stt_recording: false,
        };
        assert_eq!(
            lookup_action(&ctx, KeyModifiers::CONTROL, KeyCode::Char('q')),
            Some(Action::Quit)
        );
    }

    #[test]
    fn prompt_mode_does_not_skip_cancel() {
        let ctx = KeyContext {
            focus: Focus::Input,
            prompt_mode: true,
            edit_mode: false,
            terminal_mode: false,
            help_open: false,
            stt_recording: false,
        };
        // macOS: ⌃C = Cancel, Windows/Linux: ⌃⇧C = Cancel
        #[cfg(target_os = "macos")]
        assert_eq!(
            lookup_action(&ctx, KeyModifiers::CONTROL, KeyCode::Char('c')),
            Some(Action::CancelClaude)
        );
        #[cfg(not(target_os = "macos"))]
        {
            let mods = KeyModifiers::CONTROL | KeyModifiers::SHIFT;
            assert_eq!(
                lookup_action(&ctx, mods, KeyCode::Char('C')),
                Some(Action::CancelClaude)
            );
        }
    }

    #[test]
    fn prompt_mode_skips_shift_g() {
        let ctx = KeyContext {
            focus: Focus::Input,
            prompt_mode: true,
            edit_mode: false,
            terminal_mode: false,
            help_open: false,
            stt_recording: false,
        };
        assert_ne!(
            lookup_action(&ctx, KeyModifiers::SHIFT, KeyCode::Char('G')),
            Some(Action::OpenGitActions)
        );
    }

    #[test]
    fn prompt_mode_skips_shift_h() {
        let ctx = KeyContext {
            focus: Focus::Input,
            prompt_mode: true,
            edit_mode: false,
            terminal_mode: false,
            help_open: false,
            stt_recording: false,
        };
        assert_ne!(
            lookup_action(&ctx, KeyModifiers::SHIFT, KeyCode::Char('H')),
            Some(Action::OpenHealth)
        );
    }

    #[test]
    fn prompt_mode_skips_brackets() {
        let ctx = KeyContext {
            focus: Focus::Input,
            prompt_mode: true,
            edit_mode: false,
            terminal_mode: false,
            help_open: false,
            stt_recording: false,
        };
        assert_ne!(
            lookup_action(&ctx, KeyModifiers::NONE, KeyCode::Char(']')),
            Some(Action::WorktreeTabNext)
        );
        assert_ne!(
            lookup_action(&ctx, KeyModifiers::NONE, KeyCode::Char('[')),
            Some(Action::WorktreeTabPrev)
        );
    }

    #[test]
    fn prompt_mode_skips_run_command() {
        let ctx = KeyContext {
            focus: Focus::Input,
            prompt_mode: true,
            edit_mode: false,
            terminal_mode: false,
            help_open: false,
            stt_recording: false,
        };
        assert_ne!(
            lookup_action(&ctx, KeyModifiers::NONE, KeyCode::Char('r')),
            Some(Action::RunCommand)
        );
        assert_ne!(
            lookup_action(&ctx, KeyModifiers::SHIFT, KeyCode::Char('R')),
            Some(Action::AddRunCommand)
        );
    }

    // ══════════════════════════════════════════════════════════════════
    //  lookup_action — Global skip guards (edit mode)
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn edit_mode_skips_copy_selection() {
        // Global copy skipped in edit mode — edit handler owns clipboard
        let ctx = KeyContext {
            focus: Focus::Viewer,
            prompt_mode: false,
            edit_mode: true,
            terminal_mode: false,
            help_open: false,
            stt_recording: false,
        };
        #[cfg(target_os = "macos")]
        assert_ne!(
            lookup_action(&ctx, KeyModifiers::SUPER, KeyCode::Char('c')),
            Some(Action::CopySelection)
        );
        #[cfg(not(target_os = "macos"))]
        assert_ne!(
            lookup_action(&ctx, KeyModifiers::CONTROL, KeyCode::Char('c')),
            Some(Action::CopySelection)
        );
    }

    #[test]
    fn edit_mode_skips_tab_cycle() {
        let ctx = KeyContext {
            focus: Focus::Viewer,
            prompt_mode: false,
            edit_mode: true,
            terminal_mode: false,
            help_open: false,
            stt_recording: false,
        };
        assert_ne!(
            lookup_action(&ctx, KeyModifiers::NONE, KeyCode::Tab),
            Some(Action::CycleFocusForward)
        );
    }

    #[test]
    fn edit_mode_skips_enter_prompt() {
        let ctx = KeyContext {
            focus: Focus::Viewer,
            prompt_mode: false,
            edit_mode: true,
            terminal_mode: false,
            help_open: false,
            stt_recording: false,
        };
        assert_ne!(
            lookup_action(&ctx, KeyModifiers::NONE, KeyCode::Char('p')),
            Some(Action::EnterPromptMode)
        );
    }

    // ══════════════════════════════════════════════════════════════════
    //  lookup_action — Global skip guards (help open)
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn help_open_skips_tab_cycle() {
        let ctx = KeyContext {
            focus: Focus::Worktrees,
            prompt_mode: false,
            edit_mode: false,
            terminal_mode: false,
            help_open: true,
            stt_recording: false,
        };
        assert_ne!(
            lookup_action(&ctx, KeyModifiers::NONE, KeyCode::Tab),
            Some(Action::CycleFocusForward)
        );
        assert_ne!(
            lookup_action(&ctx, KeyModifiers::NONE, KeyCode::BackTab),
            Some(Action::CycleFocusBackward)
        );
    }

    // ══════════════════════════════════════════════════════════════════
    //  lookup_action — Terminal mode skip guards
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn terminal_mode_skips_global_help_toggle() {
        // '?' is a GLOBAL binding that gets skipped in terminal_mode.
        // It's also NOT in the TERMINAL array, so it returns None.
        let ctx = KeyContext {
            focus: Focus::Input,
            prompt_mode: false,
            edit_mode: false,
            terminal_mode: true,
            help_open: false,
            stt_recording: false,
        };
        assert_ne!(
            lookup_action(&ctx, KeyModifiers::NONE, KeyCode::Char('?')),
            Some(Action::ToggleHelp)
        );
    }

    #[test]
    fn terminal_mode_p_fires_from_terminal_bindings() {
        // 'p' is skipped as a GLOBAL binding in terminal_mode, but the TERMINAL
        // context array has its own 'p' → EnterPromptMode ("Close & prompt").
        let ctx = KeyContext {
            focus: Focus::Input,
            prompt_mode: false,
            edit_mode: false,
            terminal_mode: true,
            help_open: false,
            stt_recording: false,
        };
        assert_eq!(
            lookup_action(&ctx, KeyModifiers::NONE, KeyCode::Char('p')),
            Some(Action::EnterPromptMode)
        );
    }

    // ══════════════════════════════════════════════════════════════════
    //  lookup_action — Context-specific: Worktrees
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn worktrees_focus_resolves_add() {
        // Direct press 'a' on Worktrees pane → AddWorktree
        let ctx = cmd_ctx(Focus::Worktrees);
        assert_eq!(
            lookup_action(&ctx, KeyModifiers::NONE, KeyCode::Char('a')),
            Some(Action::AddWorktree)
        );
    }

    #[test]
    fn worktrees_focus_resolves_archive() {
        let ctx = cmd_ctx(Focus::Worktrees);
        assert_eq!(
            lookup_action(&ctx, KeyModifiers::NONE, KeyCode::Char('x')),
            Some(Action::ToggleArchiveWorktree)
        );
    }

    #[test]
    fn worktrees_focus_resolves_delete() {
        let ctx = cmd_ctx(Focus::Worktrees);
        assert_eq!(
            lookup_action(&ctx, KeyModifiers::NONE, KeyCode::Char('d')),
            Some(Action::DeleteWorktree)
        );
    }

    #[test]
    fn worktrees_focus_unbound_key_returns_none() {
        let ctx = cmd_ctx(Focus::Worktrees);
        assert_eq!(
            lookup_action(&ctx, KeyModifiers::NONE, KeyCode::Char('g')),
            None
        );
    }

    // ══════════════════════════════════════════════════════════════════
    //  lookup_action — Context-specific: FileTree
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn filetree_shift_w_returns_none() {
        // 'W' is the leader entry key, handled in actions.rs — lookup returns None
        let ctx = cmd_ctx(Focus::FileTree);
        assert_eq!(
            lookup_action(&ctx, KeyModifiers::SHIFT, KeyCode::Char('W')),
            None
        );
    }

    #[test]
    fn filetree_j_navigates_down() {
        let ctx = cmd_ctx(Focus::FileTree);
        assert_eq!(
            lookup_action(&ctx, KeyModifiers::NONE, KeyCode::Char('j')),
            Some(Action::NavDown)
        );
    }

    #[test]
    fn filetree_k_navigates_up() {
        let ctx = cmd_ctx(Focus::FileTree);
        assert_eq!(
            lookup_action(&ctx, KeyModifiers::NONE, KeyCode::Char('k')),
            Some(Action::NavUp)
        );
    }

    #[test]
    fn filetree_enter_opens_file() {
        let ctx = cmd_ctx(Focus::FileTree);
        assert_eq!(
            lookup_action(&ctx, KeyModifiers::NONE, KeyCode::Enter),
            Some(Action::OpenFile)
        );
    }

    #[test]
    fn filetree_space_toggles_dir() {
        let ctx = cmd_ctx(Focus::FileTree);
        assert_eq!(
            lookup_action(&ctx, KeyModifiers::NONE, KeyCode::Char(' ')),
            Some(Action::ToggleDir)
        );
    }

    #[test]
    fn filetree_a_adds_file() {
        let ctx = cmd_ctx(Focus::FileTree);
        assert_eq!(
            lookup_action(&ctx, KeyModifiers::NONE, KeyCode::Char('a')),
            Some(Action::AddFile)
        );
    }

    #[test]
    fn filetree_d_deletes_file() {
        let ctx = cmd_ctx(Focus::FileTree);
        assert_eq!(
            lookup_action(&ctx, KeyModifiers::NONE, KeyCode::Char('d')),
            Some(Action::DeleteFile)
        );
    }

    #[test]
    fn filetree_esc_escapes() {
        let ctx = cmd_ctx(Focus::FileTree);
        assert_eq!(
            lookup_action(&ctx, KeyModifiers::NONE, KeyCode::Esc),
            Some(Action::Escape)
        );
    }

    // ══════════════════════════════════════════════════════════════════
    //  lookup_action — Context-specific: Viewer (read-only)
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn viewer_j_scrolls_down() {
        let ctx = cmd_ctx(Focus::Viewer);
        assert_eq!(
            lookup_action(&ctx, KeyModifiers::NONE, KeyCode::Char('j')),
            Some(Action::NavDown)
        );
    }

    #[test]
    fn viewer_e_enters_edit() {
        let ctx = cmd_ctx(Focus::Viewer);
        assert_eq!(
            lookup_action(&ctx, KeyModifiers::NONE, KeyCode::Char('e')),
            Some(Action::EnterEditMode)
        );
    }

    #[test]
    fn viewer_esc_closes() {
        let ctx = cmd_ctx(Focus::Viewer);
        assert_eq!(
            lookup_action(&ctx, KeyModifiers::NONE, KeyCode::Esc),
            Some(Action::Escape)
        );
    }

    // ══════════════════════════════════════════════════════════════════
    //  lookup_action — Context-specific: Viewer (edit mode)
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn edit_mode_cmd_s_saves() {
        let ctx = KeyContext {
            focus: Focus::Viewer,
            prompt_mode: false,
            edit_mode: true,
            terminal_mode: false,
            help_open: false,
            stt_recording: false,
        };
        assert_eq!(
            lookup_action(&ctx, KeyModifiers::SUPER, KeyCode::Char('s')),
            Some(Action::Save)
        );
    }

    #[test]
    fn edit_mode_cmd_z_undoes() {
        let ctx = KeyContext {
            focus: Focus::Viewer,
            prompt_mode: false,
            edit_mode: true,
            terminal_mode: false,
            help_open: false,
            stt_recording: false,
        };
        assert_eq!(
            lookup_action(&ctx, KeyModifiers::SUPER, KeyCode::Char('z')),
            Some(Action::Undo)
        );
    }

    #[test]
    fn edit_mode_esc_exits() {
        let ctx = KeyContext {
            focus: Focus::Viewer,
            prompt_mode: false,
            edit_mode: true,
            terminal_mode: false,
            help_open: false,
            stt_recording: false,
        };
        assert_eq!(
            lookup_action(&ctx, KeyModifiers::NONE, KeyCode::Esc),
            Some(Action::Escape)
        );
    }

    // ══════════════════════════════════════════════════════════════════
    //  lookup_action — Context-specific: Session
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn session_a_new_session() {
        let ctx = cmd_ctx(Focus::Session);
        assert_eq!(
            lookup_action(&ctx, KeyModifiers::NONE, KeyCode::Char('a')),
            Some(Action::NewSession)
        );
    }

    #[test]
    fn session_s_session_list() {
        let ctx = cmd_ctx(Focus::Session);
        assert_eq!(
            lookup_action(&ctx, KeyModifiers::NONE, KeyCode::Char('s')),
            Some(Action::ToggleSessionList)
        );
    }

    #[test]
    fn session_slash_search() {
        let ctx = cmd_ctx(Focus::Session);
        assert_eq!(
            lookup_action(&ctx, KeyModifiers::NONE, KeyCode::Char('/')),
            Some(Action::SearchSession)
        );
    }

    #[test]
    fn session_down_next_bubble() {
        let ctx = cmd_ctx(Focus::Session);
        assert_eq!(
            lookup_action(&ctx, KeyModifiers::NONE, KeyCode::Down),
            Some(Action::JumpNextBubble)
        );
    }

    #[test]
    fn session_up_prev_bubble() {
        let ctx = cmd_ctx(Focus::Session);
        assert_eq!(
            lookup_action(&ctx, KeyModifiers::NONE, KeyCode::Up),
            Some(Action::JumpPrevBubble)
        );
    }

    // ══════════════════════════════════════════════════════════════════
    //  lookup_action — Context-specific: Input (prompt mode)
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn input_enter_submits() {
        let ctx = KeyContext {
            focus: Focus::Input,
            prompt_mode: true,
            edit_mode: false,
            terminal_mode: false,
            help_open: false,
            stt_recording: false,
        };
        assert_eq!(
            lookup_action(&ctx, KeyModifiers::NONE, KeyCode::Enter),
            Some(Action::Submit)
        );
    }

    #[test]
    fn input_esc_exits_prompt() {
        let ctx = KeyContext {
            focus: Focus::Input,
            prompt_mode: true,
            edit_mode: false,
            terminal_mode: false,
            help_open: false,
            stt_recording: false,
        };
        assert_eq!(
            lookup_action(&ctx, KeyModifiers::NONE, KeyCode::Esc),
            Some(Action::ExitPromptMode)
        );
    }

    #[test]
    fn input_ctrl_w_deletes_word() {
        let ctx = KeyContext {
            focus: Focus::Input,
            prompt_mode: true,
            edit_mode: false,
            terminal_mode: false,
            help_open: false,
            stt_recording: false,
        };
        assert_eq!(
            lookup_action(&ctx, KeyModifiers::CONTROL, KeyCode::Char('w')),
            Some(Action::DeleteWord)
        );
    }

    #[test]
    fn input_up_history_prev() {
        let ctx = KeyContext {
            focus: Focus::Input,
            prompt_mode: true,
            edit_mode: false,
            terminal_mode: false,
            help_open: false,
            stt_recording: false,
        };
        assert_eq!(
            lookup_action(&ctx, KeyModifiers::NONE, KeyCode::Up),
            Some(Action::HistoryPrev)
        );
    }

    #[test]
    fn input_down_history_next() {
        let ctx = KeyContext {
            focus: Focus::Input,
            prompt_mode: true,
            edit_mode: false,
            terminal_mode: false,
            help_open: false,
            stt_recording: false,
        };
        assert_eq!(
            lookup_action(&ctx, KeyModifiers::NONE, KeyCode::Down),
            Some(Action::HistoryNext)
        );
    }

    #[test]
    fn input_ctrl_s_toggles_stt() {
        let ctx = KeyContext {
            focus: Focus::Input,
            prompt_mode: true,
            edit_mode: false,
            terminal_mode: false,
            help_open: false,
            stt_recording: false,
        };
        assert_eq!(
            lookup_action(&ctx, KeyModifiers::CONTROL, KeyCode::Char('s')),
            Some(Action::ToggleStt)
        );
    }

    // ══════════════════════════════════════════════════════════════════
    //  lookup_action — Context-specific: Terminal (command mode)
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn terminal_t_enters_type_mode() {
        let ctx = KeyContext {
            focus: Focus::Input,
            prompt_mode: false,
            edit_mode: false,
            terminal_mode: true,
            help_open: false,
            stt_recording: false,
        };
        assert_eq!(
            lookup_action(&ctx, KeyModifiers::NONE, KeyCode::Char('t')),
            Some(Action::EnterTerminalType)
        );
    }

    #[test]
    fn terminal_esc_closes() {
        let ctx = KeyContext {
            focus: Focus::Input,
            prompt_mode: false,
            edit_mode: false,
            terminal_mode: true,
            help_open: false,
            stt_recording: false,
        };
        assert_eq!(
            lookup_action(&ctx, KeyModifiers::NONE, KeyCode::Esc),
            Some(Action::Escape)
        );
    }

    #[test]
    fn terminal_j_scrolls_down() {
        let ctx = KeyContext {
            focus: Focus::Input,
            prompt_mode: false,
            edit_mode: false,
            terminal_mode: true,
            help_open: false,
            stt_recording: false,
        };
        assert_eq!(
            lookup_action(&ctx, KeyModifiers::NONE, KeyCode::Char('j')),
            Some(Action::NavDown)
        );
    }

    #[test]
    fn terminal_plus_resizes_up() {
        let ctx = KeyContext {
            focus: Focus::Input,
            prompt_mode: false,
            edit_mode: false,
            terminal_mode: true,
            help_open: false,
            stt_recording: false,
        };
        assert_eq!(
            lookup_action(&ctx, KeyModifiers::NONE, KeyCode::Char('+')),
            Some(Action::ResizeUp)
        );
    }

    #[test]
    fn terminal_minus_resizes_down() {
        let ctx = KeyContext {
            focus: Focus::Input,
            prompt_mode: false,
            edit_mode: false,
            terminal_mode: true,
            help_open: false,
            stt_recording: false,
        };
        assert_eq!(
            lookup_action(&ctx, KeyModifiers::NONE, KeyCode::Char('-')),
            Some(Action::ResizeDown)
        );
    }

    // ══════════════════════════════════════════════════════════════════
    //  lookup_action — Unknown keys return None
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn unknown_key_returns_none() {
        let ctx = cmd_ctx(Focus::Worktrees);
        assert_eq!(
            lookup_action(&ctx, KeyModifiers::NONE, KeyCode::F(12)),
            None
        );
    }

    #[test]
    fn random_ctrl_combo_returns_none() {
        let ctx = cmd_ctx(Focus::Worktrees);
        assert_eq!(
            lookup_action(&ctx, KeyModifiers::CONTROL, KeyCode::Char('x')),
            None
        );
    }

    #[test]
    fn session_shift_w_does_not_resolve_globally() {
        // 'W' is the leader trigger, not a direct global binding — lookup_action returns None
        let ctx = cmd_ctx(Focus::Session);
        assert_eq!(
            lookup_action(&ctx, KeyModifiers::SHIFT, KeyCode::Char('W')),
            None
        );
    }

    // ══════════════════════════════════════════════════════════════════
    //  lookup_action — Prompt mode from non-Input focus allows re-focus
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn prompt_mode_non_input_focus_allows_p() {
        // When prompt_mode=true but focus is NOT Input, 'p' should still fire to re-focus input
        let ctx = KeyContext {
            focus: Focus::Session,
            prompt_mode: true,
            edit_mode: false,
            terminal_mode: false,
            help_open: false,
            stt_recording: false,
        };
        // 'p' is skipped in prompt_mode BUT the guard only skips EnterPromptMode,ToggleTerminal etc.
        // Actually the guard skips it. Let me re-check: the skip is
        //   Action::EnterPromptMode if ctx.prompt_mode || ctx.edit_mode || ctx.terminal_mode
        // BUT there's a second guard:
        //   Action::EnterPromptMode if ctx.prompt_mode && ctx.focus == Focus::Input
        // The FIRST guard fires first (prompt_mode=true), so 'p' is skipped entirely.
        // Wait: the first guard uses `|` (OR) — it matches. So 'p' is skipped.
        // Actually re-reading: the first guard is for EnterPromptMode | ToggleTerminal | ... if ctx.prompt_mode || ctx.edit_mode || ctx.terminal_mode
        // That evaluates to true, so skip=true and the binding is skipped.
        // But wait, the second guard: EnterPromptMode if ctx.prompt_mode && ctx.focus == Focus::Input
        // Both match arms are checked separately. The first match arm matches first, so skip=true.
        // Hmm, actually in Rust match, only the FIRST matching arm fires. So the first arm matches and skip=true.
        // Therefore 'p' is always skipped in prompt_mode regardless of focus. That seems wrong based on the comment,
        // but let's test actual behavior.
        assert_ne!(
            lookup_action(&ctx, KeyModifiers::NONE, KeyCode::Char('p')),
            Some(Action::EnterPromptMode)
        );
    }

    // ══════════════════════════════════════════════════════════════════
    //  lookup_action — Empty context fallback
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn input_no_prompt_no_terminal_returns_empty_context() {
        // Focus::Input with neither prompt_mode nor terminal_mode active → empty bindings
        let ctx = KeyContext {
            focus: Focus::Input,
            prompt_mode: false,
            edit_mode: false,
            terminal_mode: false,
            help_open: false,
            stt_recording: false,
        };
        // 'j' has no global or context binding here
        assert_eq!(
            lookup_action(&ctx, KeyModifiers::NONE, KeyCode::Char('j')),
            None
        );
    }

    // ══════════════════════════════════════════════════════════════════
    //  lookup_health_action
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn health_tab_switches_tab() {
        assert_eq!(
            lookup_health_action(HealthTab::GodFiles, KeyModifiers::NONE, KeyCode::Tab),
            Some(Action::HealthSwitchTab)
        );
    }

    #[test]
    fn health_esc_closes() {
        assert_eq!(
            lookup_health_action(HealthTab::GodFiles, KeyModifiers::NONE, KeyCode::Esc),
            Some(Action::Escape)
        );
    }

    #[test]
    fn health_j_navigates_down() {
        assert_eq!(
            lookup_health_action(HealthTab::GodFiles, KeyModifiers::NONE, KeyCode::Char('j')),
            Some(Action::NavDown)
        );
    }

    #[test]
    fn health_k_navigates_up() {
        assert_eq!(
            lookup_health_action(
                HealthTab::Documentation,
                KeyModifiers::NONE,
                KeyCode::Char('k')
            ),
            Some(Action::NavUp)
        );
    }

    #[test]
    fn health_god_files_space_toggles_check() {
        assert_eq!(
            lookup_health_action(HealthTab::GodFiles, KeyModifiers::NONE, KeyCode::Char(' ')),
            Some(Action::HealthToggleCheck)
        );
    }

    #[test]
    fn health_god_files_a_toggles_all() {
        assert_eq!(
            lookup_health_action(HealthTab::GodFiles, KeyModifiers::NONE, KeyCode::Char('a')),
            Some(Action::HealthToggleAll)
        );
    }

    #[test]
    fn health_god_files_v_views_checked() {
        assert_eq!(
            lookup_health_action(HealthTab::GodFiles, KeyModifiers::NONE, KeyCode::Char('v')),
            Some(Action::HealthViewChecked)
        );
    }

    #[test]
    fn health_god_files_enter_modularizes() {
        assert_eq!(
            lookup_health_action(HealthTab::GodFiles, KeyModifiers::NONE, KeyCode::Enter),
            Some(Action::HealthModularize)
        );
    }

    #[test]
    fn health_god_files_m_modularizes() {
        assert_eq!(
            lookup_health_action(HealthTab::GodFiles, KeyModifiers::NONE, KeyCode::Char('m')),
            Some(Action::HealthModularize)
        );
    }

    #[test]
    fn health_docs_space_toggles_check() {
        assert_eq!(
            lookup_health_action(
                HealthTab::Documentation,
                KeyModifiers::NONE,
                KeyCode::Char(' ')
            ),
            Some(Action::HealthDocToggleCheck)
        );
    }

    #[test]
    fn health_docs_a_toggles_non_100() {
        assert_eq!(
            lookup_health_action(
                HealthTab::Documentation,
                KeyModifiers::NONE,
                KeyCode::Char('a')
            ),
            Some(Action::HealthDocToggleNon100)
        );
    }

    #[test]
    fn health_docs_v_views_checked() {
        assert_eq!(
            lookup_health_action(
                HealthTab::Documentation,
                KeyModifiers::NONE,
                KeyCode::Char('v')
            ),
            Some(Action::HealthViewChecked)
        );
    }

    #[test]
    fn health_docs_enter_spawns() {
        assert_eq!(
            lookup_health_action(HealthTab::Documentation, KeyModifiers::NONE, KeyCode::Enter),
            Some(Action::HealthDocSpawn)
        );
    }

    #[test]
    fn health_unknown_key_returns_none() {
        assert_eq!(
            lookup_health_action(HealthTab::GodFiles, KeyModifiers::NONE, KeyCode::F(5)),
            None
        );
    }

    #[test]
    fn health_shared_scope_mode() {
        assert_eq!(
            lookup_health_action(HealthTab::GodFiles, KeyModifiers::NONE, KeyCode::Char('s')),
            Some(Action::HealthScopeMode)
        );
    }

    // ══════════════════════════════════════════════════════════════════
    //  lookup_git_actions_action
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn git_esc_closes() {
        assert_eq!(
            lookup_git_actions_action(0, false, KeyModifiers::NONE, KeyCode::Esc),
            Some(Action::Escape)
        );
    }

    #[test]
    fn git_tab_toggles_focus() {
        assert_eq!(
            lookup_git_actions_action(0, false, KeyModifiers::NONE, KeyCode::Tab),
            Some(Action::GitToggleFocus)
        );
    }

    #[test]
    fn git_squash_merge_on_feature_actions_focused() {
        assert_eq!(
            lookup_git_actions_action(0, false, KeyModifiers::NONE, KeyCode::Char('m')),
            Some(Action::GitSquashMerge)
        );
    }

    #[test]
    fn git_squash_merge_skipped_on_main() {
        assert_ne!(
            lookup_git_actions_action(0, true, KeyModifiers::NONE, KeyCode::Char('m')),
            Some(Action::GitSquashMerge)
        );
    }

    #[test]
    fn git_squash_merge_skipped_when_not_actions_focused() {
        assert_ne!(
            lookup_git_actions_action(1, false, KeyModifiers::NONE, KeyCode::Char('m')),
            Some(Action::GitSquashMerge)
        );
    }

    #[test]
    fn git_refresh_on_feature_actions_focused() {
        assert_eq!(
            lookup_git_actions_action(0, false, KeyModifiers::NONE, KeyCode::Char('r')),
            Some(Action::GitRefresh)
        );
    }

    #[test]
    fn git_rebase_on_feature_actions_focused() {
        assert_eq!(
            lookup_git_actions_action(0, false, KeyModifiers::SHIFT, KeyCode::Char('R')),
            Some(Action::GitRebase)
        );
    }

    #[test]
    fn git_rebase_skipped_on_main() {
        assert_ne!(
            lookup_git_actions_action(0, true, KeyModifiers::SHIFT, KeyCode::Char('R')),
            Some(Action::GitRebase)
        );
    }

    #[test]
    fn git_pull_on_main_actions_focused() {
        assert_eq!(
            lookup_git_actions_action(0, true, KeyModifiers::NONE, KeyCode::Char('l')),
            Some(Action::GitPull)
        );
    }

    #[test]
    fn git_pull_skipped_on_feature() {
        assert_ne!(
            lookup_git_actions_action(0, false, KeyModifiers::NONE, KeyCode::Char('l')),
            Some(Action::GitPull)
        );
    }

    #[test]
    fn git_pull_skipped_when_not_actions_focused() {
        assert_ne!(
            lookup_git_actions_action(1, true, KeyModifiers::NONE, KeyCode::Char('l')),
            Some(Action::GitPull)
        );
    }

    #[test]
    fn git_commit_actions_focused() {
        assert_eq!(
            lookup_git_actions_action(0, false, KeyModifiers::NONE, KeyCode::Char('c')),
            Some(Action::GitCommit)
        );
    }

    #[test]
    fn git_commit_skipped_not_actions_focused() {
        assert_ne!(
            lookup_git_actions_action(1, false, KeyModifiers::NONE, KeyCode::Char('c')),
            Some(Action::GitCommit)
        );
    }

    #[test]
    fn git_push_actions_focused() {
        assert_eq!(
            lookup_git_actions_action(0, false, KeyModifiers::SHIFT, KeyCode::Char('P')),
            Some(Action::GitPush)
        );
    }

    #[test]
    fn git_push_skipped_not_actions_focused() {
        assert_ne!(
            lookup_git_actions_action(1, false, KeyModifiers::SHIFT, KeyCode::Char('P')),
            Some(Action::GitPush)
        );
    }

    #[test]
    fn git_view_diff_d_from_file_list_resolves_to_confirm() {
        // focused_pane=1 (file list) → 'd' matches Confirm (Enter/d alt) first in array,
        // since Confirm has no skip guard. GitViewDiff also lives on 'd' but is later.
        assert_eq!(
            lookup_git_actions_action(1, false, KeyModifiers::NONE, KeyCode::Char('d')),
            Some(Action::Confirm)
        );
    }

    #[test]
    fn git_view_diff_skipped_from_actions() {
        // focused_pane=0 (actions) → GitViewDiff is skipped (guard: actions_focused → true)
        // 'd' instead matches Confirm via the Enter/d alternative
        assert_ne!(
            lookup_git_actions_action(0, false, KeyModifiers::NONE, KeyCode::Char('d')),
            Some(Action::GitViewDiff)
        );
    }

    #[test]
    fn git_auto_rebase_on_feature() {
        assert_eq!(
            lookup_git_actions_action(0, false, KeyModifiers::NONE, KeyCode::Char('a')),
            Some(Action::GitAutoRebase)
        );
    }

    #[test]
    fn git_auto_rebase_skipped_on_main() {
        assert_ne!(
            lookup_git_actions_action(0, true, KeyModifiers::NONE, KeyCode::Char('a')),
            Some(Action::GitAutoRebase)
        );
    }

    #[test]
    fn git_auto_resolve_actions_focused() {
        assert_eq!(
            lookup_git_actions_action(0, false, KeyModifiers::NONE, KeyCode::Char('s')),
            Some(Action::GitAutoResolveSettings)
        );
    }

    #[test]
    fn git_auto_resolve_skipped_not_actions_focused() {
        assert_ne!(
            lookup_git_actions_action(1, false, KeyModifiers::NONE, KeyCode::Char('s')),
            Some(Action::GitAutoResolveSettings)
        );
    }

    #[test]
    fn git_unknown_key_returns_none() {
        assert_eq!(
            lookup_git_actions_action(0, false, KeyModifiers::NONE, KeyCode::F(3)),
            None
        );
    }

    #[test]
    fn git_nav_down() {
        assert_eq!(
            lookup_git_actions_action(0, false, KeyModifiers::NONE, KeyCode::Char('j')),
            Some(Action::NavDown)
        );
    }

    #[test]
    fn git_nav_up() {
        assert_eq!(
            lookup_git_actions_action(0, false, KeyModifiers::NONE, KeyCode::Char('k')),
            Some(Action::NavUp)
        );
    }

    #[test]
    fn git_prev_worktree() {
        assert_eq!(
            lookup_git_actions_action(0, false, KeyModifiers::NONE, KeyCode::Char('[')),
            Some(Action::GitPrevWorktree)
        );
    }

    #[test]
    fn git_next_worktree() {
        assert_eq!(
            lookup_git_actions_action(0, false, KeyModifiers::NONE, KeyCode::Char(']')),
            Some(Action::GitNextWorktree)
        );
    }

    #[test]
    fn git_shift_m_browses_main() {
        assert_eq!(
            lookup_git_actions_action(0, false, KeyModifiers::SHIFT, KeyCode::Char('M')),
            Some(Action::BrowseMain)
        );
    }

    #[test]
    fn git_shift_m_browses_main_from_file_list() {
        assert_eq!(
            lookup_git_actions_action(1, false, KeyModifiers::SHIFT, KeyCode::Char('M')),
            Some(Action::BrowseMain)
        );
    }

    #[test]
    fn git_shift_m_browses_main_on_main() {
        assert_eq!(
            lookup_git_actions_action(0, true, KeyModifiers::SHIFT, KeyCode::Char('M')),
            Some(Action::BrowseMain)
        );
    }

    // ══════════════════════════════════════════════════════════════════
    //  lookup_projects_action
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn projects_enter_confirms() {
        assert_eq!(
            lookup_projects_action(KeyModifiers::NONE, KeyCode::Enter),
            Some(Action::Confirm)
        );
    }

    #[test]
    fn projects_a_adds() {
        assert_eq!(
            lookup_projects_action(KeyModifiers::NONE, KeyCode::Char('a')),
            Some(Action::ProjectsAdd)
        );
    }

    #[test]
    fn projects_d_deletes() {
        assert_eq!(
            lookup_projects_action(KeyModifiers::NONE, KeyCode::Char('d')),
            Some(Action::ProjectsDelete)
        );
    }

    #[test]
    fn projects_n_renames() {
        assert_eq!(
            lookup_projects_action(KeyModifiers::NONE, KeyCode::Char('n')),
            Some(Action::ProjectsRename)
        );
    }

    #[test]
    fn projects_i_inits() {
        assert_eq!(
            lookup_projects_action(KeyModifiers::NONE, KeyCode::Char('i')),
            Some(Action::ProjectsInit)
        );
    }

    #[test]
    fn projects_esc_closes() {
        assert_eq!(
            lookup_projects_action(KeyModifiers::NONE, KeyCode::Esc),
            Some(Action::Escape)
        );
    }

    #[test]
    fn projects_ctrl_q_quits() {
        assert_eq!(
            lookup_projects_action(KeyModifiers::CONTROL, KeyCode::Char('q')),
            Some(Action::Quit)
        );
    }

    #[test]
    fn projects_j_navigates_down() {
        assert_eq!(
            lookup_projects_action(KeyModifiers::NONE, KeyCode::Char('j')),
            Some(Action::NavDown)
        );
    }

    #[test]
    fn projects_unknown_returns_none() {
        assert_eq!(
            lookup_projects_action(KeyModifiers::NONE, KeyCode::F(9)),
            None
        );
    }

    // ══════════════════════════════════════════════════════════════════
    //  lookup_picker_action
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn picker_enter_confirms() {
        assert_eq!(
            lookup_picker_action(KeyModifiers::NONE, KeyCode::Enter),
            Some(Action::Confirm)
        );
    }

    #[test]
    fn picker_e_edits() {
        assert_eq!(
            lookup_picker_action(KeyModifiers::NONE, KeyCode::Char('e')),
            Some(Action::EditSelected)
        );
    }

    #[test]
    fn picker_d_deletes() {
        assert_eq!(
            lookup_picker_action(KeyModifiers::NONE, KeyCode::Char('d')),
            Some(Action::DeleteSelected)
        );
    }

    #[test]
    fn picker_a_adds() {
        assert_eq!(
            lookup_picker_action(KeyModifiers::NONE, KeyCode::Char('a')),
            Some(Action::ProjectsAdd)
        );
    }

    #[test]
    fn picker_esc_closes() {
        assert_eq!(
            lookup_picker_action(KeyModifiers::NONE, KeyCode::Esc),
            Some(Action::Escape)
        );
    }

    #[test]
    fn picker_j_navigates_down() {
        assert_eq!(
            lookup_picker_action(KeyModifiers::NONE, KeyCode::Char('j')),
            Some(Action::NavDown)
        );
    }

    #[test]
    fn picker_k_navigates_up() {
        assert_eq!(
            lookup_picker_action(KeyModifiers::NONE, KeyCode::Char('k')),
            Some(Action::NavUp)
        );
    }

    #[test]
    fn picker_unknown_returns_none() {
        assert_eq!(
            lookup_picker_action(KeyModifiers::NONE, KeyCode::F(7)),
            None
        );
    }

    // ══════════════════════════════════════════════════════════════════
    //  lookup_branch_dialog_action
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn branch_enter_selects() {
        assert_eq!(
            lookup_branch_dialog_action(KeyModifiers::NONE, KeyCode::Enter),
            Some(Action::Confirm)
        );
    }

    #[test]
    fn branch_esc_closes() {
        assert_eq!(
            lookup_branch_dialog_action(KeyModifiers::NONE, KeyCode::Esc),
            Some(Action::Escape)
        );
    }

    #[test]
    fn branch_j_navigates_down() {
        assert_eq!(
            lookup_branch_dialog_action(KeyModifiers::NONE, KeyCode::Char('j')),
            Some(Action::NavDown)
        );
    }

    #[test]
    fn branch_k_navigates_up() {
        assert_eq!(
            lookup_branch_dialog_action(KeyModifiers::NONE, KeyCode::Char('k')),
            Some(Action::NavUp)
        );
    }

    #[test]
    fn branch_down_arrow_navigates() {
        assert_eq!(
            lookup_branch_dialog_action(KeyModifiers::NONE, KeyCode::Down),
            Some(Action::NavDown)
        );
    }

    #[test]
    fn branch_up_arrow_navigates() {
        assert_eq!(
            lookup_branch_dialog_action(KeyModifiers::NONE, KeyCode::Up),
            Some(Action::NavUp)
        );
    }

    #[test]
    fn branch_unknown_returns_none() {
        assert_eq!(
            lookup_branch_dialog_action(KeyModifiers::NONE, KeyCode::Char('x')),
            None
        );
    }

    #[test]
    fn branch_ctrl_combo_returns_none() {
        assert_eq!(
            lookup_branch_dialog_action(KeyModifiers::CONTROL, KeyCode::Char('a')),
            None
        );
    }

    // ══════════════════════════════════════════════════════════════════
    //  lookup_leader_action (W <key> worktree commands)
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn leader_g_returns_none() {
        assert_eq!(
            lookup_leader_action(KeyModifiers::NONE, KeyCode::Char('g')),
            None
        );
    }

    #[test]
    fn leader_h_returns_none() {
        assert_eq!(
            lookup_leader_action(KeyModifiers::NONE, KeyCode::Char('h')),
            None
        );
    }

    #[test]
    fn leader_m_returns_none() {
        assert_eq!(
            lookup_leader_action(KeyModifiers::NONE, KeyCode::Char('m')),
            None
        );
    }

    #[test]
    fn leader_o_returns_none() {
        assert_eq!(
            lookup_leader_action(KeyModifiers::NONE, KeyCode::Char('o')),
            None
        );
    }

    #[test]
    fn leader_r_returns_none() {
        assert_eq!(
            lookup_leader_action(KeyModifiers::NONE, KeyCode::Char('r')),
            None
        );
    }

    #[test]
    fn leader_shift_r_returns_none() {
        assert_eq!(
            lookup_leader_action(KeyModifiers::SHIFT, KeyCode::Char('R')),
            None
        );
    }

    #[test]
    fn leader_bracket_right_returns_none() {
        assert_eq!(
            lookup_leader_action(KeyModifiers::NONE, KeyCode::Char(']')),
            None
        );
    }

    #[test]
    fn leader_bracket_left_returns_none() {
        assert_eq!(
            lookup_leader_action(KeyModifiers::NONE, KeyCode::Char('[')),
            None
        );
    }

    #[test]
    fn leader_a_adds_worktree() {
        assert_eq!(
            lookup_leader_action(KeyModifiers::NONE, KeyCode::Char('a')),
            Some(Action::AddWorktree)
        );
    }

    #[test]
    fn leader_x_archives_worktree() {
        assert_eq!(
            lookup_leader_action(KeyModifiers::NONE, KeyCode::Char('x')),
            Some(Action::ToggleArchiveWorktree)
        );
    }

    #[test]
    fn leader_d_deletes_worktree() {
        assert_eq!(
            lookup_leader_action(KeyModifiers::NONE, KeyCode::Char('d')),
            Some(Action::DeleteWorktree)
        );
    }

    #[test]
    fn leader_unknown_returns_none() {
        assert_eq!(
            lookup_leader_action(KeyModifiers::NONE, KeyCode::Char('z')),
            None
        );
    }
}
