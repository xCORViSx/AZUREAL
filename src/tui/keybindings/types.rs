//! Core keybinding types
//!
//! Defines the fundamental data structures used across the keybinding system:
//! `KeyCombo`, `Action`, `Keybinding`, and `HelpSection`.

use crossterm::event::{KeyCode, KeyModifiers};

/// A key combination (modifier + key)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyCombo {
    pub modifiers: KeyModifiers,
    pub code: KeyCode,
}

impl KeyCombo {
    pub const fn new(modifiers: KeyModifiers, code: KeyCode) -> Self {
        Self { modifiers, code }
    }

    pub const fn plain(code: KeyCode) -> Self {
        Self { modifiers: KeyModifiers::NONE, code }
    }

    pub const fn shift(code: KeyCode) -> Self {
        Self { modifiers: KeyModifiers::SHIFT, code }
    }

    pub const fn ctrl(code: KeyCode) -> Self {
        Self { modifiers: KeyModifiers::CONTROL, code }
    }

    pub const fn alt(code: KeyCode) -> Self {
        Self { modifiers: KeyModifiers::ALT, code }
    }

    pub const fn cmd(code: KeyCode) -> Self {
        Self { modifiers: KeyModifiers::SUPER, code }
    }

    /// Check if key event matches this combo
    #[inline]
    pub fn matches(&self, modifiers: KeyModifiers, code: KeyCode) -> bool {
        if self.modifiers == modifiers && self.code == code { return true; }
        // Shift+letter bindings use shift(Char('G')) but crossterm delivers
        // uppercase chars inconsistently depending on terminal + Kitty flags:
        //   - (NONE, Char('G'))  — no Kitty or legacy terminals
        //   - (SHIFT, Char('G')) — some Kitty implementations
        //   - (SHIFT, Char('g')) — DISAMBIGUATE without REPORT_ALL_KEYS
        // Match all three against a SHIFT + uppercase char binding.
        // Shift+letter bindings: crossterm delivers uppercase as either
        //   (NONE, Char('G')) or (SHIFT, Char('G')) or (SHIFT, Char('g'))
        // but NEVER (NONE, Char('g')) for a shifted press. So only match
        // when the pressed char is already uppercase (NONE modifier) or
        // SHIFT is held — reject plain lowercase to avoid t matching T.
        if self.modifiers == KeyModifiers::SHIFT {
            if let KeyCode::Char(c) = self.code {
                if c.is_ascii_uppercase() {
                    if let KeyCode::Char(pressed) = code {
                        if modifiers == KeyModifiers::SHIFT && pressed.to_ascii_uppercase() == c {
                            return true;
                        }
                        if modifiers == KeyModifiers::NONE && pressed == c {
                            return true;
                        }
                    }
                }
            }
        }
        false
    }

    /// Platform-appropriate display string (macOS symbols)
    pub fn display(&self) -> String {
        let mut s = String::new();
        if self.modifiers.contains(KeyModifiers::CONTROL) { s.push('⌃'); }
        if self.modifiers.contains(KeyModifiers::ALT) { s.push('⌥'); }
        // Only show ⇧ for non-char keys (arrows, enter, etc.) — uppercase chars imply Shift
        if self.modifiers.contains(KeyModifiers::SHIFT) && !matches!(self.code, KeyCode::Char(_)) {
            s.push('⇧');
        }
        if self.modifiers.contains(KeyModifiers::SUPER) { s.push('⌘'); }

        match self.code {
            KeyCode::Char(' ') => s.push_str("Space"),
            KeyCode::Char(c) => s.push(c),
            KeyCode::Enter => s.push_str("Enter"),
            KeyCode::Esc => s.push_str("Esc"),
            KeyCode::Tab => s.push_str("Tab"),
            KeyCode::BackTab => s.push_str("Tab"),
            KeyCode::Backspace => s.push('⌫'),
            KeyCode::Delete => s.push('⌦'),
            KeyCode::Up => s.push('↑'),
            KeyCode::Down => s.push('↓'),
            KeyCode::Left => s.push('←'),
            KeyCode::Right => s.push('→'),
            KeyCode::Home => s.push_str("Home"),
            KeyCode::End => s.push_str("End"),
            KeyCode::PageUp => s.push_str("PgUp"),
            KeyCode::PageDown => s.push_str("PgDn"),
            _ => s.push_str(&format!("{:?}", self.code)),
        }
        s
    }
}

/// All possible keybinding actions
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    // Global
    Quit,
    DumpDebug,
    CancelClaude,
    CopySelection,
    ToggleHelp,
    ToggleTerminal,
    EnterPromptMode,
    CycleFocusForward,
    CycleFocusBackward,
    CycleModel,

    // Navigation (shared across contexts)
    NavDown,
    NavUp,
    NavLeft,
    NavRight,
    PageDown,
    PageUp,
    GoToTop,
    GoToBottom,

    // Worktrees
    AddWorktree,
    BrowseBranches,
    RunCommand,
    AddRunCommand,
    ToggleArchiveWorktree,
    OpenHealth,
    OpenGitActions,
    OpenProjects,

    // FileTree
    ReturnToWorktrees,
    ToggleDir,
    OpenFile,
    AddFile,
    DeleteFile,
    RenameFile,
    CopyFile,
    MoveFile,

    // Viewer
    EnterEditMode,
    JumpNextEdit,
    JumpPrevEdit,
    SelectAll,
    ViewerTabCurrent,
    ViewerOpenTabDialog,
    ViewerCloseTab,

    // Viewer Edit Mode
    Save,
    Undo,
    Redo,

    // Output/Convo
    NewSession,
    ToggleSessionList,
    SearchSession,
    JumpNextBubble,
    JumpPrevBubble,
    JumpNextMessage,
    JumpPrevMessage,

    // Input
    Submit,
    InsertNewline,
    ExitPromptMode,
    WordLeft,
    WordRight,
    DeleteWord,
    HistoryPrev,
    HistoryNext,
    ToggleStt,

    // Terminal
    ResizeUp,
    ResizeDown,
    EnterTerminalType,

    // Dialogs
    Confirm,
    #[allow(dead_code)] // Kept for match exhaustiveness in actions.rs dialog handler
    Cancel,
    DeleteSelected,
    EditSelected,
    PresetPrompts,

    // Health Panel (modal)
    HealthSwitchTab,
    HealthToggleCheck,
    HealthToggleAll,
    HealthViewChecked,
    HealthScopeMode,
    HealthModularize,
    HealthDocToggleCheck,
    HealthDocToggleNon100,
    HealthDocSpawn,

    // Main branch browse (read-only inspection of main's files and sessions)
    BrowseMain,

    // Worktree tab row navigation (global)
    WorktreeTabPrev,
    WorktreeTabNext,

    // Git Actions Panel (modal)
    GitToggleFocus,
    GitSquashMerge,
    GitRebase,
    GitPull,
    GitViewDiff,
    GitRefresh,
    GitCommit,
    GitPush,
    GitAutoRebase,
    GitAutoResolveSettings,
    GitPrevWorktree,
    GitNextWorktree,
    GitPrevPage,
    GitNextPage,

    // FileTree Options overlay
    FileTreeOptions,

    // Projects Panel (modal, browse mode)
    ProjectsAdd,
    ProjectsDelete,
    ProjectsRename,
    ProjectsInit,

    // Generic
    Escape,
}

/// A keybinding with one or more key alternatives
#[derive(Debug, Clone)]
pub struct Keybinding {
    /// Primary key combo
    pub primary: KeyCombo,
    /// Alternative key combos (e.g., j AND Down for same action)
    pub alternatives: &'static [KeyCombo],
    /// Description for help dialog
    pub description: &'static str,
    /// Action identifier
    pub action: Action,
    /// When true, help panel merges this binding with the NEXT one onto a single line.
    /// Use for counterpart pairs like up/down, next/prev, expand/collapse.
    pub pair_with_next: bool,
}

impl Keybinding {
    pub const fn new(primary: KeyCombo, description: &'static str, action: Action) -> Self {
        Self { primary, alternatives: &[], description, action, pair_with_next: false }
    }

    pub const fn with_alt(
        primary: KeyCombo,
        alternatives: &'static [KeyCombo],
        description: &'static str,
        action: Action,
    ) -> Self {
        Self { primary, alternatives, description, action, pair_with_next: false }
    }

    /// Mark this binding to merge with the next one on a single help line
    pub const fn paired(mut self) -> Self {
        self.pair_with_next = true;
        self
    }

    /// Check if any key combo matches
    #[inline]
    pub fn matches(&self, modifiers: KeyModifiers, code: KeyCode) -> bool {
        self.primary.matches(modifiers, code)
            || self.alternatives.iter().any(|k| k.matches(modifiers, code))
    }

    /// Display string combining primary and alternatives (e.g., "j/↓").
    /// Skips macOS ⌥+letter unicode fallback chars (®, π, †, etc.) — those are
    /// internal matching alternatives, not meaningful to show in the help panel.
    pub fn display_keys(&self) -> String {
        if self.alternatives.is_empty() {
            return self.primary.display();
        }
        let mut s = self.primary.display();
        for alt in self.alternatives {
            // Skip bare unicode chars produced by macOS ⌥+letter (not ASCII, no modifiers)
            if let KeyCode::Char(c) = alt.code {
                if !c.is_ascii() && alt.modifiers == KeyModifiers::NONE { continue; }
            }
            s.push('/');
            s.push_str(&alt.display());
        }
        s
    }
}

/// Help section for UI display
pub struct HelpSection {
    pub title: &'static str,
    pub bindings: &'static [Keybinding],
}
