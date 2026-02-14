//! Centralized keybinding definitions
//!
//! All keybindings are defined once here and referenced by:
//! - Input handlers (for executing actions)
//! - Help dialog (for display)

use crossterm::event::{KeyCode, KeyModifiers};
use crate::app::Focus;

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
    Restart,
    DumpDebug,
    CancelClaude,
    CopySelection,
    ToggleHelp,
    ToggleTerminal,
    EnterPromptMode,
    CycleFocusForward,
    CycleFocusBackward,

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
    ToggleFileTree,
    SearchFilter,
    NewWorktree,
    BrowseBranches,
    RunCommand,
    AddRunCommand,
    ArchiveWorktree,
    StartResume,
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
    ViewerNextTab,
    ViewerPrevTab,
    ViewerCloseTab,

    // Viewer Edit Mode
    Save,
    Undo,
    Redo,

    // Output/Convo
    ToggleSessionList,
    SearchConvo,
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

    // Wizard
    WizardNextTab,
    WizardPrevTab,
    WizardNextField,

    // Dialogs
    Confirm,
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

    // Git Actions Panel (modal)
    GitToggleFocus,
    GitRebase,
    GitMerge,
    GitFetch,
    GitPull,
    GitPush,
    GitViewDiff,
    GitRefresh,
    GitToggleDotGit,

    // Projects Panel (modal, browse mode)
    ProjectsAdd,
    ProjectsDelete,
    ProjectsRename,
    ProjectsInit,

    // Dialog structural
    DialogToggleScope,

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

// Static alternative key arrays for dual-key bindings
// Enter/m alternative for health panel modularize action
static ALT_CHAR_M: [KeyCombo; 1] = [KeyCombo { modifiers: KeyModifiers::NONE, code: KeyCode::Char('m') }];
// Enter/d alternative for git panel view-diff action
static ALT_CHAR_D: [KeyCombo; 1] = [KeyCombo { modifiers: KeyModifiers::NONE, code: KeyCode::Char('d') }];
static ALT_DOWN: [KeyCombo; 1] = [KeyCombo { modifiers: KeyModifiers::NONE, code: KeyCode::Down }];
static ALT_UP: [KeyCombo; 1] = [KeyCombo { modifiers: KeyModifiers::NONE, code: KeyCode::Up }];
static ALT_LEFT: [KeyCombo; 1] = [KeyCombo { modifiers: KeyModifiers::NONE, code: KeyCode::Left }];
static ALT_RIGHT: [KeyCombo; 1] = [KeyCombo { modifiers: KeyModifiers::NONE, code: KeyCode::Right }];
// ⌃← alternative for ⌥← (word nav in prompt input)
static ALT_CTRL_LEFT: [KeyCombo; 1] = [KeyCombo { modifiers: KeyModifiers::CONTROL, code: KeyCode::Left }];
static ALT_CTRL_RIGHT: [KeyCombo; 1] = [KeyCombo { modifiers: KeyModifiers::CONTROL, code: KeyCode::Right }];
// ⌃Backspace alternative for ⌃w delete word (non-macOS)
static ALT_DELETE_WORD: [KeyCombo; 1] = [
    KeyCombo { modifiers: KeyModifiers::CONTROL, code: KeyCode::Backspace },
];
// PageUp/PageDown/Home/End alternatives for viewer scroll
static ALT_PGDN: [KeyCombo; 1] = [KeyCombo { modifiers: KeyModifiers::NONE, code: KeyCode::PageDown }];
static ALT_PGUP: [KeyCombo; 1] = [KeyCombo { modifiers: KeyModifiers::NONE, code: KeyCode::PageUp }];
static ALT_HOME: [KeyCombo; 1] = [KeyCombo { modifiers: KeyModifiers::NONE, code: KeyCode::Home }];
static ALT_END: [KeyCombo; 1] = [KeyCombo { modifiers: KeyModifiers::NONE, code: KeyCode::End }];
// macOS ⌥r produces '®' (unicode) instead of ALT+r — add as alternative
static ALT_MACOS_R: [KeyCombo; 1] = [KeyCombo { modifiers: KeyModifiers::NONE, code: KeyCode::Char('®') }];
// macOS ⌥p produces 'π' (unicode) instead of ALT+p — add as alternative
static ALT_MACOS_P: [KeyCombo; 1] = [KeyCombo { modifiers: KeyModifiers::NONE, code: KeyCode::Char('π') }];
static ALT_MACOS_T: [KeyCombo; 1] = [KeyCombo { modifiers: KeyModifiers::NONE, code: KeyCode::Char('†') }];

// Cmd+Shift modifier combo
const CMD_SHIFT: KeyModifiers = KeyModifiers::from_bits_truncate(
    KeyModifiers::SUPER.bits() | KeyModifiers::SHIFT.bits()
);

/// Global keybindings (always active, checked first)
pub static GLOBAL: [Keybinding; 12] = [
    Keybinding::new(KeyCombo::ctrl(KeyCode::Char('q')), "Quit azureal", Action::Quit),
    Keybinding::new(KeyCombo::ctrl(KeyCode::Char('r')), "Restart azureal", Action::Restart),
    Keybinding::new(KeyCombo::ctrl(KeyCode::Char('d')), "Dump debug output", Action::DumpDebug),
    Keybinding::new(KeyCombo::ctrl(KeyCode::Char('c')), "Cancel agent", Action::CancelClaude),
    Keybinding::new(KeyCombo::cmd(KeyCode::Char('c')), "Copy selection", Action::CopySelection),
    Keybinding::new(KeyCombo::plain(KeyCode::Char('?')), "Toggle help", Action::ToggleHelp),
    Keybinding::new(KeyCombo::plain(KeyCode::Char('p')), "Enter prompt mode", Action::EnterPromptMode),
    Keybinding::new(KeyCombo::shift(KeyCode::Char('T')), "Toggle terminal", Action::ToggleTerminal),
    Keybinding::new(KeyCombo::shift(KeyCode::Char('G')), "Git actions", Action::OpenGitActions),
    Keybinding::new(KeyCombo::shift(KeyCode::Char('H')), "Worktree health", Action::OpenHealth),
    Keybinding::new(KeyCombo::plain(KeyCode::Tab), "Cycle focus forward", Action::CycleFocusForward),
    Keybinding::new(KeyCombo::shift(KeyCode::BackTab), "Cycle focus backward", Action::CycleFocusBackward),
];

/// Worktrees context bindings — flat list, no expand/collapse
pub static WORKTREES: [Keybinding; 13] = [
    Keybinding::new(KeyCombo::plain(KeyCode::Char('f')), "Browse files", Action::ToggleFileTree),
    Keybinding::new(KeyCombo::plain(KeyCode::Char('/')), "Search/filter", Action::SearchFilter),
    Keybinding::with_alt(KeyCombo::plain(KeyCode::Char('j')), &ALT_DOWN, "Select worktree", Action::NavDown).paired(),
    Keybinding::with_alt(KeyCombo::plain(KeyCode::Char('k')), &ALT_UP, "Select worktree", Action::NavUp),
    Keybinding::new(KeyCombo::alt(KeyCode::Up), "Jump to top", Action::GoToTop).paired(),
    Keybinding::new(KeyCombo::alt(KeyCode::Down), "Jump to bottom", Action::GoToBottom),
    Keybinding::new(KeyCombo::plain(KeyCode::Enter), "Start/resume", Action::StartResume),
    Keybinding::new(KeyCombo::plain(KeyCode::Char('n')), "New...", Action::NewWorktree),
    Keybinding::new(KeyCombo::plain(KeyCode::Char('b')), "Browse branches", Action::BrowseBranches),
    Keybinding::new(KeyCombo::plain(KeyCode::Char('r')), "Run command", Action::RunCommand),
    Keybinding::with_alt(KeyCombo::alt(KeyCode::Char('r')), &ALT_MACOS_R, "Add run command", Action::AddRunCommand),
    Keybinding::new(KeyCombo::plain(KeyCode::Char('a')), "Archive worktree", Action::ArchiveWorktree),
    Keybinding::new(KeyCombo::shift(KeyCode::Char('P')), "Projects", Action::OpenProjects),
];

/// FileTree bindings
pub static FILE_TREE: [Keybinding; 15] = [
    Keybinding::new(KeyCombo::plain(KeyCode::Char('w')), "Back to worktrees", Action::ReturnToWorktrees),
    Keybinding::with_alt(KeyCombo::plain(KeyCode::Char('j')), &ALT_DOWN, "Navigate", Action::NavDown).paired(),
    Keybinding::with_alt(KeyCombo::plain(KeyCode::Char('k')), &ALT_UP, "Navigate", Action::NavUp),
    Keybinding::with_alt(KeyCombo::plain(KeyCode::Char('h')), &ALT_LEFT, "Collapse", Action::NavLeft).paired(),
    Keybinding::with_alt(KeyCombo::plain(KeyCode::Char('l')), &ALT_RIGHT, "Expand", Action::NavRight),
    Keybinding::new(KeyCombo::alt(KeyCode::Up), "First in folder", Action::GoToTop).paired(),
    Keybinding::new(KeyCombo::alt(KeyCode::Down), "Last in folder", Action::GoToBottom),
    Keybinding::new(KeyCombo::plain(KeyCode::Enter), "Open/toggle", Action::OpenFile),
    Keybinding::new(KeyCombo::plain(KeyCode::Char(' ')), "Toggle dir", Action::ToggleDir),
    Keybinding::new(KeyCombo::plain(KeyCode::Char('a')), "Add file/dir", Action::AddFile),
    Keybinding::new(KeyCombo::plain(KeyCode::Char('d')), "Delete", Action::DeleteFile),
    Keybinding::new(KeyCombo::plain(KeyCode::Char('r')), "Rename", Action::RenameFile),
    Keybinding::new(KeyCombo::plain(KeyCode::Char('c')), "Copy", Action::CopyFile),
    Keybinding::new(KeyCombo::plain(KeyCode::Char('m')), "Move", Action::MoveFile),
    Keybinding::new(KeyCombo::plain(KeyCode::Esc), "Back to Worktrees", Action::Escape),
];

/// Viewer bindings (read-only mode)
pub static VIEWER: [Keybinding; 16] = [
    Keybinding::with_alt(KeyCombo::plain(KeyCode::Char('j')), &ALT_DOWN, "Scroll line", Action::NavDown).paired(),
    Keybinding::with_alt(KeyCombo::plain(KeyCode::Char('k')), &ALT_UP, "Scroll line", Action::NavUp),
    Keybinding::with_alt(KeyCombo::shift(KeyCode::Char('J')), &ALT_PGDN, "Page down", Action::PageDown).paired(),
    Keybinding::with_alt(KeyCombo::shift(KeyCode::Char('K')), &ALT_PGUP, "Page up", Action::PageUp),
    Keybinding::with_alt(KeyCombo::alt(KeyCode::Up), &ALT_HOME, "Top", Action::GoToTop).paired(),
    Keybinding::with_alt(KeyCombo::alt(KeyCode::Down), &ALT_END, "Bottom", Action::GoToBottom),
    Keybinding::new(KeyCombo::alt(KeyCode::Right), "Next Edit", Action::JumpNextEdit).paired(),
    Keybinding::new(KeyCombo::alt(KeyCode::Left), "Prev Edit", Action::JumpPrevEdit),
    Keybinding::new(KeyCombo::plain(KeyCode::Char('e')), "Edit file", Action::EnterEditMode),
    Keybinding::new(KeyCombo::plain(KeyCode::Esc), "Close viewer", Action::Escape),
    Keybinding::new(KeyCombo::cmd(KeyCode::Char('a')), "Select all", Action::SelectAll),
    Keybinding::new(KeyCombo::plain(KeyCode::Char('t')), "Tab file", Action::ViewerTabCurrent),
    Keybinding::with_alt(KeyCombo::alt(KeyCode::Char('t')), &ALT_MACOS_T, "Tab dialog", Action::ViewerOpenTabDialog),
    Keybinding::new(KeyCombo::plain(KeyCode::Char(']')), "Next tab", Action::ViewerNextTab).paired(),
    Keybinding::new(KeyCombo::plain(KeyCode::Char('[')), "Prev tab", Action::ViewerPrevTab),
    Keybinding::new(KeyCombo::plain(KeyCode::Char('x')), "Close tab", Action::ViewerCloseTab),
];

/// Edit mode bindings
pub static EDIT_MODE: [Keybinding; 5] = [
    Keybinding::new(KeyCombo::cmd(KeyCode::Char('s')), "Save file", Action::Save),
    Keybinding::new(KeyCombo::cmd(KeyCode::Char('z')), "Undo", Action::Undo).paired(),
    Keybinding::new(KeyCombo::new(CMD_SHIFT, KeyCode::Char('Z')), "Redo", Action::Redo),
    Keybinding::new(KeyCombo::ctrl(KeyCode::Char('s')), "Speech input", Action::ToggleStt),
    Keybinding::new(KeyCombo::plain(KeyCode::Esc), "Exit edit mode", Action::Escape),
];

/// Convo/Output bindings
pub static OUTPUT: [Keybinding; 13] = [
    Keybinding::new(KeyCombo::plain(KeyCode::Char('s')), "Session list", Action::ToggleSessionList),
    Keybinding::new(KeyCombo::plain(KeyCode::Char('/')), "Search", Action::SearchConvo),
    Keybinding::new(KeyCombo::plain(KeyCode::Char('j')), "Scroll line", Action::NavDown).paired(),
    Keybinding::new(KeyCombo::plain(KeyCode::Char('k')), "Scroll line", Action::NavUp),
    Keybinding::new(KeyCombo::plain(KeyCode::Down), "Next prompt", Action::JumpNextBubble).paired(),
    Keybinding::new(KeyCombo::plain(KeyCode::Up), "Prev prompt", Action::JumpPrevBubble),
    Keybinding::new(KeyCombo::shift(KeyCode::Down), "Next message", Action::JumpNextMessage).paired(),
    Keybinding::new(KeyCombo::shift(KeyCode::Up), "Prev message", Action::JumpPrevMessage),
    Keybinding::new(KeyCombo::shift(KeyCode::Char('J')), "Page down", Action::PageDown).paired(),
    Keybinding::new(KeyCombo::shift(KeyCode::Char('K')), "Page up", Action::PageUp),
    Keybinding::new(KeyCombo::alt(KeyCode::Up), "Top", Action::GoToTop).paired(),
    Keybinding::new(KeyCombo::alt(KeyCode::Down), "Bottom", Action::GoToBottom),
    Keybinding::new(KeyCombo::plain(KeyCode::Esc), "Back to Worktrees", Action::Escape),
];

/// Input mode bindings — keys that work in Claude prompt type mode
/// Word nav uses standard macOS shortcuts (⌥← / ⌥→), not ⌃z/⌃x which conflict with clipboard
/// Newline: ⇧Enter (Kitty keyboard protocol makes this distinguishable from bare Enter)
pub static INPUT: [Keybinding; 10] = [
    Keybinding::new(KeyCombo::plain(KeyCode::Enter), "Submit prompt", Action::Submit),
    Keybinding::new(KeyCombo::shift(KeyCode::Enter), "Insert newline", Action::InsertNewline),
    Keybinding::new(KeyCombo::plain(KeyCode::Esc), "Exit to COMMAND", Action::ExitPromptMode),
    Keybinding::with_alt(KeyCombo::alt(KeyCode::Left), &ALT_CTRL_LEFT, "Word left", Action::WordLeft).paired(),
    Keybinding::with_alt(KeyCombo::alt(KeyCode::Right), &ALT_CTRL_RIGHT, "Word right", Action::WordRight),
    Keybinding::with_alt(KeyCombo::ctrl(KeyCode::Char('w')), &ALT_DELETE_WORD, "Delete word", Action::DeleteWord),
    Keybinding::new(KeyCombo::plain(KeyCode::Up), "History prev", Action::HistoryPrev).paired(),
    Keybinding::new(KeyCombo::plain(KeyCode::Down), "History next", Action::HistoryNext),
    Keybinding::new(KeyCombo::ctrl(KeyCode::Char('s')), "Speech input", Action::ToggleStt),
    Keybinding::with_alt(KeyCombo::alt(KeyCode::Char('p')), &ALT_MACOS_P, "Preset prompts", Action::PresetPrompts),
];

/// Terminal bindings (command mode) — ALL terminal keybindings live here
/// so title bar hints can source from them dynamically
pub static TERMINAL: [Keybinding; 11] = [
    Keybinding::new(KeyCombo::plain(KeyCode::Char('t')), "Enter type mode", Action::EnterTerminalType),
    Keybinding::new(KeyCombo::plain(KeyCode::Char('p')), "Close & prompt", Action::EnterPromptMode),
    Keybinding::new(KeyCombo::plain(KeyCode::Esc), "Close terminal", Action::Escape),
    Keybinding::with_alt(KeyCombo::plain(KeyCode::Char('j')), &ALT_DOWN, "Scroll line", Action::NavDown).paired(),
    Keybinding::with_alt(KeyCombo::plain(KeyCode::Char('k')), &ALT_UP, "Scroll line", Action::NavUp),
    Keybinding::new(KeyCombo::shift(KeyCode::Char('J')), "Scroll page", Action::PageDown).paired(),
    Keybinding::new(KeyCombo::shift(KeyCode::Char('K')), "Scroll page", Action::PageUp),
    Keybinding::new(KeyCombo::alt(KeyCode::Up), "Scroll to top", Action::GoToTop).paired(),
    Keybinding::new(KeyCombo::alt(KeyCode::Down), "Scroll to bottom", Action::GoToBottom),
    Keybinding::new(KeyCombo::plain(KeyCode::Char('+')), "Resize up", Action::ResizeUp).paired(),
    Keybinding::new(KeyCombo::plain(KeyCode::Char('-')), "Resize down", Action::ResizeDown),
];

/// Wizard/New dialog bindings
pub static WIZARD: [Keybinding; 3] = [
    Keybinding::new(KeyCombo::plain(KeyCode::Char(']')), "Next tab", Action::WizardNextTab),
    Keybinding::new(KeyCombo::plain(KeyCode::Char('[')), "Prev tab", Action::WizardPrevTab),
    Keybinding::new(KeyCombo::plain(KeyCode::Tab), "Next field", Action::WizardNextField),
];

// ─── Modal panel binding arrays ───────────────────────────────────────────────
// These are NOT resolved via lookup_action() — modals intercept all input before
// the non-modal system runs. Each modal has its own lookup_*_action() function below.

/// Health Panel — bindings shared across both tabs (Tab, nav, Esc)
pub static HEALTH_SHARED: [Keybinding; 6] = [
    Keybinding::new(KeyCombo::plain(KeyCode::Tab), "Switch tab", Action::HealthSwitchTab),
    Keybinding::with_alt(KeyCombo::plain(KeyCode::Char('j')), &ALT_DOWN, "Navigate", Action::NavDown).paired(),
    Keybinding::with_alt(KeyCombo::plain(KeyCode::Char('k')), &ALT_UP, "Navigate", Action::NavUp),
    Keybinding::new(KeyCombo::alt(KeyCode::Up), "Jump to top", Action::GoToTop).paired(),
    Keybinding::new(KeyCombo::alt(KeyCode::Down), "Jump to bottom", Action::GoToBottom),
    Keybinding::new(KeyCombo::plain(KeyCode::Esc), "Close", Action::Escape),
];

/// Health Panel — God Files tab actions (Space/a/v/s/Enter/m)
pub static HEALTH_GOD_FILES: [Keybinding; 5] = [
    Keybinding::new(KeyCombo::plain(KeyCode::Char(' ')), "Toggle check", Action::HealthToggleCheck),
    Keybinding::new(KeyCombo::plain(KeyCode::Char('a')), "Toggle all", Action::HealthToggleAll),
    Keybinding::new(KeyCombo::plain(KeyCode::Char('v')), "View checked", Action::HealthViewChecked),
    Keybinding::new(KeyCombo::plain(KeyCode::Char('s')), "Scope mode", Action::HealthScopeMode),
    Keybinding::with_alt(KeyCombo::plain(KeyCode::Enter), &ALT_CHAR_M, "Modularize", Action::HealthModularize),
];

/// Health Panel — Documentation tab actions.
/// Space checks, `a` toggles all non-100%, `v` views in Viewer, Enter spawns [DH] sessions.
pub static HEALTH_DOCS: [Keybinding; 4] = [
    Keybinding::new(KeyCombo::plain(KeyCode::Char(' ')), "Toggle check", Action::HealthDocToggleCheck),
    Keybinding::new(KeyCombo::plain(KeyCode::Char('a')), "Check non-100%", Action::HealthDocToggleNon100),
    Keybinding::new(KeyCombo::plain(KeyCode::Char('v')), "View checked", Action::HealthViewChecked),
    Keybinding::new(KeyCombo::plain(KeyCode::Enter), "Spawn doc sessions", Action::HealthDocSpawn),
];

/// Git Actions Panel — all keys for the git modal overlay.
/// Guard note: git ops (r/m/f/l/P) only fire when actions_focused=true,
/// diff view (d) only fires when actions_focused=false. Guards live in
/// lookup_git_actions_action(), not here.
pub static GIT_ACTIONS: [Keybinding; 15] = [
    Keybinding::new(KeyCombo::plain(KeyCode::Esc), "Close", Action::Escape),
    Keybinding::new(KeyCombo::plain(KeyCode::Tab), "Switch focus", Action::GitToggleFocus),
    Keybinding::with_alt(KeyCombo::plain(KeyCode::Char('j')), &ALT_DOWN, "Navigate", Action::NavDown).paired(),
    Keybinding::with_alt(KeyCombo::plain(KeyCode::Char('k')), &ALT_UP, "Navigate", Action::NavUp),
    Keybinding::new(KeyCombo::alt(KeyCode::Up), "Jump to top", Action::GoToTop).paired(),
    Keybinding::new(KeyCombo::alt(KeyCode::Down), "Jump to bottom", Action::GoToBottom),
    Keybinding::new(KeyCombo::plain(KeyCode::Char('r')), "Rebase from main", Action::GitRebase),
    Keybinding::new(KeyCombo::plain(KeyCode::Char('m')), "Merge from main", Action::GitMerge),
    Keybinding::new(KeyCombo::plain(KeyCode::Char('f')), "Fetch", Action::GitFetch),
    Keybinding::new(KeyCombo::plain(KeyCode::Char('l')), "Pull", Action::GitPull),
    Keybinding::new(KeyCombo::plain(KeyCode::Char('P')), "Push", Action::GitPush),
    Keybinding::with_alt(KeyCombo::plain(KeyCode::Enter), &ALT_CHAR_D, "Exec/view diff", Action::Confirm),
    Keybinding::new(KeyCombo::shift(KeyCode::Char('R')), "Refresh", Action::GitRefresh),
    Keybinding::new(KeyCombo::shift(KeyCode::Char('H')), "Toggle .git", Action::GitToggleDotGit),
    Keybinding::new(KeyCombo::plain(KeyCode::Char('d')), "View diff", Action::GitViewDiff),
];

/// Projects Panel — browse mode bindings (text input modes stay raw)
pub static PROJECTS_BROWSE: [Keybinding; 9] = [
    Keybinding::new(KeyCombo::ctrl(KeyCode::Char('q')), "Quit", Action::Quit),
    Keybinding::with_alt(KeyCombo::plain(KeyCode::Char('j')), &ALT_DOWN, "Navigate", Action::NavDown).paired(),
    Keybinding::with_alt(KeyCombo::plain(KeyCode::Char('k')), &ALT_UP, "Navigate", Action::NavUp),
    Keybinding::new(KeyCombo::plain(KeyCode::Enter), "Open project", Action::Confirm),
    Keybinding::new(KeyCombo::plain(KeyCode::Char('a')), "Add project", Action::ProjectsAdd),
    Keybinding::new(KeyCombo::plain(KeyCode::Char('d')), "Delete", Action::ProjectsDelete),
    Keybinding::new(KeyCombo::plain(KeyCode::Char('n')), "Rename", Action::ProjectsRename),
    Keybinding::new(KeyCombo::plain(KeyCode::Char('i')), "Init git repo", Action::ProjectsInit),
    Keybinding::new(KeyCombo::plain(KeyCode::Esc), "Close", Action::Escape),
];

/// Picker — shared bindings for run command + preset prompt pickers.
/// Number quick-select (1-9/0) stays raw in handlers — not rebindable.
pub static PICKER: [Keybinding; 7] = [
    Keybinding::with_alt(KeyCombo::plain(KeyCode::Char('j')), &ALT_DOWN, "Navigate", Action::NavDown).paired(),
    Keybinding::with_alt(KeyCombo::plain(KeyCode::Char('k')), &ALT_UP, "Navigate", Action::NavUp),
    Keybinding::new(KeyCombo::plain(KeyCode::Enter), "Select", Action::Confirm),
    Keybinding::new(KeyCombo::plain(KeyCode::Char('e')), "Edit", Action::EditSelected),
    Keybinding::new(KeyCombo::plain(KeyCode::Char('d')), "Delete", Action::DeleteSelected),
    Keybinding::new(KeyCombo::plain(KeyCode::Char('a')), "Add new", Action::ProjectsAdd),
    Keybinding::new(KeyCombo::plain(KeyCode::Esc), "Close", Action::Escape),
];

/// Context Menu — simple nav + select
pub static CONTEXT_MENU: [Keybinding; 4] = [
    Keybinding::with_alt(KeyCombo::plain(KeyCode::Char('j')), &ALT_DOWN, "Navigate", Action::NavDown).paired(),
    Keybinding::with_alt(KeyCombo::plain(KeyCode::Char('k')), &ALT_UP, "Navigate", Action::NavUp),
    Keybinding::new(KeyCombo::plain(KeyCode::Enter), "Select", Action::Confirm),
    Keybinding::new(KeyCombo::plain(KeyCode::Esc), "Close", Action::Escape),
];

/// Branch Dialog — nav + select (filter chars stay raw)
pub static BRANCH_DIALOG: [Keybinding; 4] = [
    Keybinding::with_alt(KeyCombo::plain(KeyCode::Char('j')), &ALT_DOWN, "Navigate", Action::NavDown).paired(),
    Keybinding::with_alt(KeyCombo::plain(KeyCode::Char('k')), &ALT_UP, "Navigate", Action::NavUp),
    Keybinding::new(KeyCombo::plain(KeyCode::Enter), "Select", Action::Confirm),
    Keybinding::new(KeyCombo::plain(KeyCode::Esc), "Close", Action::Escape),
];

/// All state needed to resolve a key press into an action.
/// Built from &App so guards are defined ONCE here, not scattered across input handlers.
pub struct KeyContext {
    pub focus: Focus,
    pub prompt_mode: bool,
    pub edit_mode: bool,
    pub terminal_mode: bool,
    pub filter_active: bool,
    pub has_context_menu: bool,
    pub wizard_active: bool,
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
            has_context_menu: app.context_menu.is_some(),
            wizard_active: app.is_wizard_active(),
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
            // sidebar filter, context menu, or wizard — they'd steal keystrokes
            Action::EnterPromptMode | Action::ToggleTerminal | Action::ToggleHelp
            | Action::OpenGitActions | Action::OpenHealth
                if ctx.prompt_mode || ctx.edit_mode || ctx.terminal_mode
                   || ctx.filter_active || ctx.has_context_menu || ctx.wizard_active => true,
            // ⌘C global copy must not fire in edit mode — edit handler owns clipboard
            Action::CopySelection if ctx.edit_mode => true,
            // Tab/Shift+Tab must not steal focus in edit mode, help overlay, or wizard
            Action::CycleFocusForward | Action::CycleFocusBackward
                if ctx.edit_mode || ctx.help_open || ctx.wizard_active => true,
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

    // Wizard bindings checked before focus-specific (wizard is a modal overlay)
    if ctx.wizard_active {
        for binding in &WIZARD {
            if binding.matches(modifiers, code) {
                return Some(binding.action);
            }
        }
    }

    // Context-specific bindings based on focus + mode
    let context_bindings: &[Keybinding] = match ctx.focus {
        Focus::Worktrees => &WORKTREES,
        Focus::FileTree => &FILE_TREE,
        Focus::Viewer if ctx.edit_mode => &EDIT_MODE,
        Focus::Viewer => &VIEWER,
        Focus::Output => &OUTPUT,
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

/// Generate help sections from binding definitions
/// Note: Wizard, Terminal, and Input bindings are shown in their own title bars, not here
pub fn help_sections() -> Vec<HelpSection> {
    vec![
        // HelpSection { title: "Global", bindings: &GLOBAL },
        HelpSection { title: "Worktrees", bindings: &WORKTREES },
        HelpSection { title: "Filetree (f)", bindings: &FILE_TREE },
        HelpSection { title: "Viewer", bindings: &VIEWER },
        HelpSection { title: "Edit Mode", bindings: &EDIT_MODE },
        HelpSection { title: "Convo", bindings: &OUTPUT },
    ]
}

/// Title + hints for prompt input (type mode).
/// Returns (short_label, full_title_with_hints, just_the_hints).
/// Callers use full title if it fits, otherwise short label in border + hints as inner row.
pub fn prompt_type_title() -> (String, String, String) {
    let esc = find_key_for_action(&INPUT, Action::ExitPromptMode).unwrap_or("Esc".into());
    let submit = find_key_for_action(&INPUT, Action::Submit).unwrap_or("Enter".into());
    let cancel = find_key_for_action(&GLOBAL, Action::CancelClaude).unwrap_or("⌃c".into());
    let (hprev, hnext) = find_key_pair(&INPUT, Action::HistoryPrev, Action::HistoryNext, "↑", "↓");
    let dw = find_key_for_action(&INPUT, Action::DeleteWord).unwrap_or("⌃w".into());
    let stt = find_key_for_action(&INPUT, Action::ToggleStt).unwrap_or("⌃s".into());
    let presets = find_key_for_action(&INPUT, Action::PresetPrompts).unwrap_or("⌥p".into());
    let hints = format!(
        "{}:exit | {}:submit | ⇧Enter:newline | {}:cancel agent | {}/{}:history | ⌥←/→:word | {}:del wrd | {}:speech | {}:presets",
        esc, submit, cancel, hprev, hnext, dw, stt, presets
    );
    let label = " PROMPT ".to_string();
    let full = format!(" PROMPT ({}) ", hints);
    (label, full, hints)
}

/// Title + hints for command mode — shows ALL global keybindings.
/// Returns (short_label, full_title_with_hints, just_the_hints).
pub fn prompt_command_title() -> (String, String, String) {
    let p = find_key_for_action(&GLOBAL, Action::EnterPromptMode).unwrap_or("p".into());
    let t = find_key_for_action(&GLOBAL, Action::ToggleTerminal).unwrap_or("T".into());
    let g = find_key_for_action(&GLOBAL, Action::OpenGitActions).unwrap_or("G".into());
    let h = find_key_for_action(&GLOBAL, Action::OpenHealth).unwrap_or("H".into());
    let help = find_key_for_action(&GLOBAL, Action::ToggleHelp).unwrap_or("?".into());
    let tab = find_key_for_action(&GLOBAL, Action::CycleFocusForward).unwrap_or("Tab".into());
    let stab = find_key_for_action(&GLOBAL, Action::CycleFocusBackward).unwrap_or("⇧Tab".into());
    let cancel = find_key_for_action(&GLOBAL, Action::CancelClaude).unwrap_or("⌃c".into());
    let quit = find_key_for_action(&GLOBAL, Action::Quit).unwrap_or("⌃q".into());
    let restart = find_key_for_action(&GLOBAL, Action::Restart).unwrap_or("⌃r".into());
    let debug = find_key_for_action(&GLOBAL, Action::DumpDebug).unwrap_or("⌃d".into());
    let hints = format!(
        "{}:PROMPT | {}:TERMINAL | {}:Git | {}:Health | {}:help | {}/{}:focus | {}:cancel agent | {}:quit | {}:restart | {}:debug",
        p, t, g, h, help, tab, stab, cancel, quit, restart, debug
    );
    let label = " COMMAND ".to_string();
    let full = format!(" COMMAND ({}) ", hints);
    (label, full, hints)
}

/// Title + hints for terminal type mode.
/// Returns (short_label, full_title, hints).
pub fn terminal_type_title() -> (String, String, String) {
    let esc = find_key_for_action(&TERMINAL, Action::Escape).unwrap_or("Esc".into());
    let hints = format!("{}:exit", esc);
    (" TERMINAL ".to_string(), format!(" TERMINAL ({}) ", hints), hints)
}

/// Title + hints for terminal command mode.
/// Returns (short_label, full_title, hints).
pub fn terminal_command_title() -> (String, String, String) {
    let t = find_key_for_action(&TERMINAL, Action::EnterTerminalType).unwrap_or("t".into());
    let p = find_key_for_action(&TERMINAL, Action::EnterPromptMode).unwrap_or("p".into());
    let esc = find_key_for_action(&TERMINAL, Action::Escape).unwrap_or("Esc".into());
    let (down, up) = find_key_pair(&TERMINAL, Action::NavDown, Action::NavUp, "j", "k");
    let (pdn, pup) = find_key_pair(&TERMINAL, Action::PageDown, Action::PageUp, "J", "K");
    let (top, bot) = find_key_pair(&TERMINAL, Action::GoToTop, Action::GoToBottom, "⌥↑", "⌥↓");
    let (rup, rdn) = find_key_pair(&TERMINAL, Action::ResizeUp, Action::ResizeDown, "+", "-");
    let hints = format!(
        "{}:type | {}:prompt | {}:close | {}/{}:scroll | {}/{}:page | {}/{}:top/bottom | {}/{}:resize",
        t, p, esc, down, up, pdn, pup, top, bot, rup, rdn
    );
    (" TERMINAL ".to_string(), format!(" TERMINAL ({}) ", hints), hints)
}

/// Title + hints for terminal scrolled mode.
/// Returns (short_label, full_title, hints).
pub fn terminal_scroll_title(scroll: usize) -> (String, String, String) {
    let (down, up) = find_key_pair(&TERMINAL, Action::NavDown, Action::NavUp, "j", "k");
    let (pdn, pup) = find_key_pair(&TERMINAL, Action::PageDown, Action::PageUp, "J", "K");
    let top = find_key_for_action(&TERMINAL, Action::GoToTop).unwrap_or("⌥↑".into());
    let bot = find_key_for_action(&TERMINAL, Action::GoToBottom).unwrap_or("⌥↓".into());
    let t = find_key_for_action(&TERMINAL, Action::EnterTerminalType).unwrap_or("t".into());
    let esc = find_key_for_action(&TERMINAL, Action::Escape).unwrap_or("Esc".into());
    let hints = format!(
        "{}/{}:scroll | {}/{}:page | {}:top | {}:bottom | {}:type | {}:close",
        down, up, pdn, pup, top, bot, t, esc
    );
    let label = format!(" TERMINAL [{}↑] ", scroll);
    let full = format!(" TERMINAL [{}↑] ({}) ", scroll, hints);
    (label, full, hints)
}

/// Generate wizard help text for "coming soon" tabs
pub fn wizard_coming_soon_help() -> String {
    let next = find_key_for_action(&WIZARD, Action::WizardNextTab).unwrap_or("]".to_string());
    let prev = find_key_for_action(&WIZARD, Action::WizardPrevTab).unwrap_or("[".to_string());
    format!("{}/{}:switch tabs  (Coming soon)", prev, next)
}

/// Generate wizard help text for selection step (worktree or session list selection)
pub fn wizard_select_help() -> String {
    let next = find_key_for_action(&WIZARD, Action::WizardNextTab).unwrap_or("]".to_string());
    let prev = find_key_for_action(&WIZARD, Action::WizardPrevTab).unwrap_or("[".to_string());
    format!("{}/{}:tabs  j/k:select  Enter:next  Esc:cancel", prev, next)
}

/// Generate wizard help text for details entry step
pub fn wizard_details_help() -> String {
    let next = find_key_for_action(&WIZARD, Action::WizardNextTab).unwrap_or("]".to_string());
    let prev = find_key_for_action(&WIZARD, Action::WizardPrevTab).unwrap_or("[".to_string());
    let field = find_key_for_action(&WIZARD, Action::WizardNextField).unwrap_or("Tab".to_string());
    format!("{}/{}:tabs  {}:field  Enter:next  Esc:back", prev, next, field)
}

/// Generate wizard help text for confirmation step
pub fn wizard_confirm_help() -> String {
    let next = find_key_for_action(&WIZARD, Action::WizardNextTab).unwrap_or("]".to_string());
    let prev = find_key_for_action(&WIZARD, Action::WizardPrevTab).unwrap_or("[".to_string());
    format!("{}/{}:tabs  Enter:create  Esc:back", prev, next)
}

// ─── Modal panel lookup functions ─────────────────────────────────────────────
// Each modal consumes ALL input. These resolve key → Action within that modal's
// context, applying any section-specific guards (e.g., git ops only when focused).

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
/// Git operations (r/m/f/l/P) only fire when actions section is focused.
/// View diff (d) only fires from the file list section.
pub fn lookup_git_actions_action(
    actions_focused: bool,
    modifiers: KeyModifiers,
    code: KeyCode,
) -> Option<Action> {
    for b in &GIT_ACTIONS {
        let skip = match b.action {
            Action::GitRebase | Action::GitMerge | Action::GitFetch
            | Action::GitPull | Action::GitPush if !actions_focused => true,
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

/// Resolve key → Action for the context menu overlay.
pub fn lookup_context_menu_action(modifiers: KeyModifiers, code: KeyCode) -> Option<Action> {
    for b in &CONTEXT_MENU { if b.matches(modifiers, code) { return Some(b.action); } }
    None
}

/// Resolve key → Action for the branch dialog overlay.
/// Filter chars (typing to search) stay raw in the handler.
pub fn lookup_branch_dialog_action(modifiers: KeyModifiers, code: KeyCode) -> Option<Action> {
    for b in &BRANCH_DIALOG { if b.matches(modifiers, code) { return Some(b.action); } }
    None
}

// ─── Modal hint generators ───────────────────────────────────────────────────
// Draw functions call these instead of hardcoding hint strings. Each function
// sources key labels from the binding arrays above via find_key_for_action().

/// Health panel footer for God Files tab
pub fn health_god_files_hints() -> String {
    let check = find_key_for_action(&HEALTH_GOD_FILES, Action::HealthToggleCheck).unwrap_or("Space".into());
    let all = find_key_for_action(&HEALTH_GOD_FILES, Action::HealthToggleAll).unwrap_or("a".into());
    let view = find_key_for_action(&HEALTH_GOD_FILES, Action::HealthViewChecked).unwrap_or("v".into());
    let scope = find_key_for_action(&HEALTH_GOD_FILES, Action::HealthScopeMode).unwrap_or("s".into());
    let modularize = find_key_for_action(&HEALTH_GOD_FILES, Action::HealthModularize).unwrap_or("Enter".into());
    let tab = find_key_for_action(&HEALTH_SHARED, Action::HealthSwitchTab).unwrap_or("Tab".into());
    let esc = find_key_for_action(&HEALTH_SHARED, Action::Escape).unwrap_or("Esc".into());
    format!(" {}:check  {}:all  {}:view  {}:scope  {}/m:modularize  {}:switch  {}:close ",
        check, all, view, scope, modularize, tab, esc)
}

/// Health panel footer for Documentation tab
pub fn health_docs_hints() -> String {
    let check = find_key_for_action(&HEALTH_DOCS, Action::HealthDocToggleCheck).unwrap_or("Space".into());
    let all = find_key_for_action(&HEALTH_DOCS, Action::HealthDocToggleNon100).unwrap_or("a".into());
    let view = find_key_for_action(&HEALTH_DOCS, Action::HealthViewChecked).unwrap_or("v".into());
    let spawn = find_key_for_action(&HEALTH_DOCS, Action::HealthDocSpawn).unwrap_or("Enter".into());
    let tab = find_key_for_action(&HEALTH_SHARED, Action::HealthSwitchTab).unwrap_or("Tab".into());
    let esc = find_key_for_action(&HEALTH_SHARED, Action::Escape).unwrap_or("Esc".into());
    format!(" {}:check  {}:non-100%  {}:view  {}:spawn  {}:switch  {}:close ",
        check, all, view, spawn, tab, esc)
}

/// Git Actions panel — action key+description pairs for the action list labels.
/// Returns (display_key, description) for each git action in display order.
pub fn git_actions_labels() -> Vec<(String, &'static str)> {
    [Action::GitRebase, Action::GitMerge, Action::GitFetch, Action::GitPull, Action::GitPush]
        .iter()
        .filter_map(|&a| {
            GIT_ACTIONS.iter().find(|b| b.action == a).map(|b| (b.primary.display(), b.description))
        })
        .collect()
}

/// Git Actions panel footer hints
pub fn git_actions_footer() -> String {
    let tab = find_key_for_action(&GIT_ACTIONS, Action::GitToggleFocus).unwrap_or("Tab".into());
    let enter = find_key_for_action(&GIT_ACTIONS, Action::Confirm).unwrap_or("Enter".into());
    let refresh = find_key_for_action(&GIT_ACTIONS, Action::GitRefresh).unwrap_or("R".into());
    let esc = find_key_for_action(&GIT_ACTIONS, Action::Escape).unwrap_or("Esc".into());
    format!(" {}:switch  {}:exec/view  {}:refresh  {} ", tab, enter, refresh, esc)
}

/// Projects panel browse-mode hint pairs: (key_display, label) for colored Span rendering.
/// Caller gets `has_project` to conditionally include Esc:close.
pub fn projects_browse_hint_pairs(has_project: bool) -> Vec<(String, &'static str)> {
    let mut v = vec![
        (find_key_for_action(&PROJECTS_BROWSE, Action::Confirm).unwrap_or("Enter".into()), "open"),
        (find_key_for_action(&PROJECTS_BROWSE, Action::ProjectsAdd).unwrap_or("a".into()), "add"),
        (find_key_for_action(&PROJECTS_BROWSE, Action::ProjectsDelete).unwrap_or("d".into()), "delete"),
        (find_key_for_action(&PROJECTS_BROWSE, Action::ProjectsRename).unwrap_or("n".into()), "name"),
        (find_key_for_action(&PROJECTS_BROWSE, Action::ProjectsInit).unwrap_or("i".into()), "init"),
    ];
    if has_project { v.push(("Esc".into(), "close")); }
    v.push((find_key_for_action(&PROJECTS_BROWSE, Action::Quit).unwrap_or("⌃Q".into()), "quit"));
    v
}

/// Picker title with keybinding hints for run command / preset prompt pickers.
/// `label` is the picker name (e.g., "Run Command" or "Preset Prompts").
pub fn picker_title(label: &str) -> String {
    let edit = find_key_for_action(&PICKER, Action::EditSelected).unwrap_or("e".into());
    let del = find_key_for_action(&PICKER, Action::DeleteSelected).unwrap_or("d".into());
    let add = find_key_for_action(&PICKER, Action::ProjectsAdd).unwrap_or("a".into());
    format!(" {} (1-9:select  {}:add  {}:edit  {}:del) ", label, add, edit, del)
}

/// Dialog footer hint pairs for run command / preset prompt dialogs.
/// Returns (key_display, label) for Tab/BackTab/CtrlS structural keys.
pub fn dialog_footer_hint_pairs() -> Vec<(String, &'static str)> {
    vec![
        ("Tab".into(), "next"),
        ("⇧Tab".into(), "back"),
        ("⌃s".into(), "scope"),
        ("Enter".into(), "save"),
        ("Esc".into(), "cancel"),
    ]
}

/// Find the display key for a given action in a binding list
pub fn find_key_for_action(bindings: &[Keybinding], action: Action) -> Option<String> {
    bindings.iter()
        .find(|b| b.action == action)
        .map(|b| b.primary.display())
}

/// Find a pair of keys for two related actions (e.g., NavDown/NavUp → "j"/"k")
pub fn find_key_pair(bindings: &[Keybinding], a: Action, b: Action, da: &str, db: &str) -> (String, String) {
    (
        find_key_for_action(bindings, a).unwrap_or_else(|| da.into()),
        find_key_for_action(bindings, b).unwrap_or_else(|| db.into()),
    )
}

/// macOS ⌥+letter produces unicode chars instead of setting the ALT modifier.
/// This maps those unicode chars back to the original letter so handlers can
/// match `⌥+letter` portably. Returns None if the char isn't an ⌥ mapping.
/// Based on macOS US keyboard layout.
#[inline]
#[allow(dead_code)]
pub fn macos_opt_key(ch: char) -> Option<char> {
    match ch {
        'å' => Some('a'), '∫' => Some('b'), 'ç' => Some('c'), '∂' => Some('d'),
        '´' => Some('e'), 'ƒ' => Some('f'), '©' => Some('g'), '˙' => Some('h'),
        'ˆ' => Some('i'), '∆' => Some('j'), '˚' => Some('k'), '¬' => Some('l'),
        'µ' => Some('m'), '˜' => Some('n'), 'ø' => Some('o'), 'π' => Some('p'),
        'œ' => Some('q'), '®' => Some('r'), 'ß' => Some('s'), '†' => Some('t'),
        '¨' => Some('u'), '√' => Some('v'), '∑' => Some('w'), '≈' => Some('x'),
        '¥' => Some('y'), 'Ω' => Some('z'),
        // ⌥+numbers on US keyboard layout
        '¡' => Some('1'), '™' => Some('2'), '£' => Some('3'), '¢' => Some('4'),
        '∞' => Some('5'), '§' => Some('6'), '¶' => Some('7'), '•' => Some('8'),
        'ª' => Some('9'), 'º' => Some('0'),
        _ => None,
    }
}
