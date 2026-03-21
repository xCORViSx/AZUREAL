//! Static keybinding arrays
//!
//! Every context's binding table lives here as a `pub static` array.
//! Modal panels, non-modal contexts, and global shortcuts each get their own
//! array so lookup functions can iterate the right set.

use super::types::{Action, KeyCombo, Keybinding};
use crossterm::event::{KeyCode, KeyModifiers};

// Static alternative key arrays for dual-key bindings
// Enter/m alternative for health panel modularize action
static ALT_CHAR_M: [KeyCombo; 1] = [KeyCombo {
    modifiers: KeyModifiers::NONE,
    code: KeyCode::Char('m'),
}];
// Enter/d alternative for git panel view-diff action
static ALT_CHAR_D: [KeyCombo; 1] = [KeyCombo {
    modifiers: KeyModifiers::NONE,
    code: KeyCode::Char('d'),
}];
static ALT_DOWN: [KeyCombo; 1] = [KeyCombo {
    modifiers: KeyModifiers::NONE,
    code: KeyCode::Down,
}];
static ALT_UP: [KeyCombo; 1] = [KeyCombo {
    modifiers: KeyModifiers::NONE,
    code: KeyCode::Up,
}];
static ALT_LEFT: [KeyCombo; 1] = [KeyCombo {
    modifiers: KeyModifiers::NONE,
    code: KeyCode::Left,
}];
static ALT_RIGHT: [KeyCombo; 1] = [KeyCombo {
    modifiers: KeyModifiers::NONE,
    code: KeyCode::Right,
}];
// ⌃← alternative for ⌥← (word nav in prompt input)
static ALT_CTRL_LEFT: [KeyCombo; 1] = [KeyCombo {
    modifiers: KeyModifiers::CONTROL,
    code: KeyCode::Left,
}];
static ALT_CTRL_RIGHT: [KeyCombo; 1] = [KeyCombo {
    modifiers: KeyModifiers::CONTROL,
    code: KeyCode::Right,
}];
// ⌃Backspace alternative for ⌃w delete word (non-macOS)
static ALT_DELETE_WORD: [KeyCombo; 1] = [KeyCombo {
    modifiers: KeyModifiers::CONTROL,
    code: KeyCode::Backspace,
}];
// PageUp/PageDown/Home/End alternatives for viewer scroll
static ALT_PGDN: [KeyCombo; 1] = [KeyCombo {
    modifiers: KeyModifiers::NONE,
    code: KeyCode::PageDown,
}];
static ALT_PGUP: [KeyCombo; 1] = [KeyCombo {
    modifiers: KeyModifiers::NONE,
    code: KeyCode::PageUp,
}];
static ALT_HOME: [KeyCombo; 1] = [KeyCombo {
    modifiers: KeyModifiers::NONE,
    code: KeyCode::Home,
}];
static ALT_END: [KeyCombo; 1] = [KeyCombo {
    modifiers: KeyModifiers::NONE,
    code: KeyCode::End,
}];
// Alt+M fallback for Ctrl+M (CycleModel) — without Kitty protocol, Ctrl+M is
// indistinguishable from Enter (both send 0x0D). Alt+M sends ESC+'m', always unique.
// macOS terminals generally support Kitty protocol (iTerm2/Kitty/WezTerm/Ghostty),
// so no fallback is needed — and ⌥m produces 'µ' which should remain typeable.
#[cfg(not(target_os = "macos"))]
static ALT_CYCLE_MODEL: [KeyCombo; 1] = [KeyCombo {
    modifiers: KeyModifiers::ALT,
    code: KeyCode::Char('m'),
}];
#[cfg(target_os = "macos")]
static ALT_CYCLE_MODEL: [KeyCombo; 0] = [];
// Alt+Enter fallback for Shift+Enter (InsertNewline) — without Kitty protocol,
// Shift+Enter is indistinguishable from Enter. Alt+Enter sends ESC+CR, always unique.
static ALT_INSERT_NEWLINE: [KeyCombo; 1] = [KeyCombo {
    modifiers: KeyModifiers::ALT,
    code: KeyCode::Enter,
}];
// macOS ⌥p produces 'π' (unicode) instead of ALT+p — add as alternative
static ALT_MACOS_P: [KeyCombo; 1] = [KeyCombo {
    modifiers: KeyModifiers::NONE,
    code: KeyCode::Char('π'),
}];
static ALT_MACOS_T: [KeyCombo; 1] = [KeyCombo {
    modifiers: KeyModifiers::NONE,
    code: KeyCode::Char('†'),
}];
// Shift+[ → '{' — some terminals send (SHIFT, '{'), others (NONE, '{')
static ALT_LBRACE: [KeyCombo; 1] = [KeyCombo {
    modifiers: KeyModifiers::NONE,
    code: KeyCode::Char('{'),
}];
static ALT_RBRACE: [KeyCombo; 1] = [KeyCombo {
    modifiers: KeyModifiers::NONE,
    code: KeyCode::Char('}'),
}];
// Alt+J/K recursive expand/collapse: primary Alt+Right/Left, alts for ⌥j(∆)/⌥k(˚) on macOS
static ALT_RECURSIVE_EXPAND: [KeyCombo; 2] = [
    KeyCombo {
        modifiers: KeyModifiers::ALT,
        code: KeyCode::Char('j'),
    },
    KeyCombo {
        modifiers: KeyModifiers::NONE,
        code: KeyCode::Char('∆'),
    }, // macOS ⌥j
];
static ALT_RECURSIVE_COLLAPSE: [KeyCombo; 2] = [
    KeyCombo {
        modifiers: KeyModifiers::ALT,
        code: KeyCode::Char('k'),
    },
    KeyCombo {
        modifiers: KeyModifiers::NONE,
        code: KeyCode::Char('˚'),
    }, // macOS ⌥k
];

// Modifier combos
#[cfg(target_os = "macos")]
const CMD_SHIFT: KeyModifiers =
    KeyModifiers::from_bits_truncate(KeyModifiers::SUPER.bits() | KeyModifiers::SHIFT.bits());
#[allow(dead_code)] // Used on non-macOS targets
const CTRL_SHIFT: KeyModifiers =
    KeyModifiers::from_bits_truncate(KeyModifiers::CONTROL.bits() | KeyModifiers::SHIFT.bits());

// ── Platform-conditional key combos ──────────────────────────────────────────
// macOS: ⌘ bindings (Cmd key). Windows/Linux: Ctrl or Ctrl+Shift equivalents.
// Super (Win key) is intercepted by the OS on Windows — terminals never receive it.

#[cfg(target_os = "macos")]
const KEY_COPY: KeyCombo = KeyCombo::cmd(KeyCode::Char('c'));
#[cfg(not(target_os = "macos"))]
const KEY_COPY: KeyCombo = KeyCombo::ctrl(KeyCode::Char('c'));

#[cfg(target_os = "macos")]
const KEY_CANCEL: KeyCombo = KeyCombo::ctrl(KeyCode::Char('c'));
// Ctrl+Shift+C is Windows Terminal's copy — use Alt+c instead.
// Without Kitty keyboard protocol, Ctrl+Shift+C arrives as Ctrl+C anyway.
#[cfg(not(target_os = "macos"))]
const KEY_CANCEL: KeyCombo = KeyCombo::new(KeyModifiers::ALT, KeyCode::Char('c'));


#[cfg(target_os = "macos")]
const KEY_SELECT_ALL: KeyCombo = KeyCombo::cmd(KeyCode::Char('a'));
#[cfg(not(target_os = "macos"))]
const KEY_SELECT_ALL: KeyCombo = KeyCombo::ctrl(KeyCode::Char('a'));

#[cfg(target_os = "macos")]
const KEY_SAVE: KeyCombo = KeyCombo::cmd(KeyCode::Char('s'));
#[cfg(not(target_os = "macos"))]
const KEY_SAVE: KeyCombo = KeyCombo::ctrl(KeyCode::Char('s'));

#[cfg(target_os = "macos")]
const KEY_UNDO: KeyCombo = KeyCombo::cmd(KeyCode::Char('z'));
#[cfg(not(target_os = "macos"))]
const KEY_UNDO: KeyCombo = KeyCombo::ctrl(KeyCode::Char('z'));

#[cfg(target_os = "macos")]
const KEY_REDO: KeyCombo = KeyCombo::new(CMD_SHIFT, KeyCode::Char('Z'));
#[cfg(not(target_os = "macos"))]
const KEY_REDO: KeyCombo = KeyCombo::ctrl(KeyCode::Char('y'));

// STT in edit mode: ⌃s on macOS (no conflict with ⌘s Save), ⌃⇧S on non-macOS (⌃s is Save)
#[cfg(target_os = "macos")]
const KEY_EDIT_STT: KeyCombo = KeyCombo::ctrl(KeyCode::Char('s'));
// Ctrl+Shift+S not reliably delivered on Windows without Kitty protocol
#[cfg(not(target_os = "macos"))]
const KEY_EDIT_STT: KeyCombo = KeyCombo::new(KeyModifiers::ALT, KeyCode::Char('s'));

/// Global keybindings (always active, checked first).
pub static GLOBAL: [Keybinding; 18] = [
    Keybinding::new(
        KeyCombo::ctrl(KeyCode::Char('q')),
        "Quit azureal",
        Action::Quit,
    ),
    Keybinding::new(
        KeyCombo::ctrl(KeyCode::Char('d')),
        "Dump debug output",
        Action::DumpDebug,
    ),
    Keybinding::new(KEY_CANCEL, "Cancel agent", Action::CancelClaude),
    Keybinding::new(KEY_COPY, "Copy selection", Action::CopySelection),
    Keybinding::with_alt_kitty(
        KeyCombo::ctrl(KeyCode::Char('m')),
        &ALT_CYCLE_MODEL,
        "Cycle model",
        Action::CycleModel,
    ),
    Keybinding::new(
        KeyCombo::plain(KeyCode::Char('?')),
        "Help",
        Action::ToggleHelp,
    ),
    Keybinding::new(
        KeyCombo::plain(KeyCode::Char('p')),
        "Prompt mode",
        Action::EnterPromptMode,
    ),
    Keybinding::new(
        KeyCombo::shift(KeyCode::Char('T')),
        "Terminal",
        Action::ToggleTerminal,
    ),
    Keybinding::new(
        KeyCombo::shift(KeyCode::Char('G')),
        "GitView",
        Action::OpenGitActions,
    ),
    Keybinding::new(
        KeyCombo::shift(KeyCode::Char('H')),
        "Worktree health",
        Action::OpenHealth,
    ),
    Keybinding::new(
        KeyCombo::shift(KeyCode::Char('M')),
        "Browse main",
        Action::BrowseMain,
    ),
    Keybinding::new(
        KeyCombo::shift(KeyCode::Char('P')),
        "Projects",
        Action::OpenProjects,
    ),
    Keybinding::new(
        KeyCombo::plain(KeyCode::Char(']')),
        "Next worktree",
        Action::WorktreeTabNext,
    )
    .paired(),
    Keybinding::new(
        KeyCombo::plain(KeyCode::Char('[')),
        "Prev worktree",
        Action::WorktreeTabPrev,
    ),
    Keybinding::new(
        KeyCombo::plain(KeyCode::Tab),
        "Cycle focus forward",
        Action::CycleFocusForward,
    )
    .paired(),
    Keybinding::new(
        KeyCombo::plain(KeyCode::BackTab),
        "Cycle focus backward",
        Action::CycleFocusBackward,
    ),
    Keybinding::new(
        KeyCombo::plain(KeyCode::Char('r')),
        "Run command",
        Action::RunCommand,
    )
    .paired(),
    Keybinding::new(
        KeyCombo::shift(KeyCode::Char('R')),
        "Add run command",
        Action::AddRunCommand,
    ),
];

/// Worktree leader-key bindings (`W <key>`).
/// These fire only after the `Shift+W` leader prefix.
/// The action keys below are the THIRD keystroke in the sequence.
pub static WORKTREES: [Keybinding; 4] = [
    Keybinding::new(
        KeyCombo::plain(KeyCode::Char('a')),
        "Add worktree",
        Action::AddWorktree,
    ),
    Keybinding::new(
        KeyCombo::plain(KeyCode::Char('r')),
        "Rename worktree",
        Action::RenameWorktree,
    ),
    Keybinding::new(
        KeyCombo::plain(KeyCode::Char('x')),
        "Archive worktree",
        Action::ToggleArchiveWorktree,
    ),
    Keybinding::new(
        KeyCombo::plain(KeyCode::Char('d')),
        "Delete worktree",
        Action::DeleteWorktree,
    ),
];

/// FileTree bindings
pub static FILE_TREE: [Keybinding; 17] = [
    Keybinding::with_alt(
        KeyCombo::plain(KeyCode::Char('j')),
        &ALT_DOWN,
        "Navigate",
        Action::NavDown,
    )
    .paired(),
    Keybinding::with_alt(
        KeyCombo::plain(KeyCode::Char('k')),
        &ALT_UP,
        "Navigate",
        Action::NavUp,
    ),
    Keybinding::with_alt(
        KeyCombo::plain(KeyCode::Char('h')),
        &ALT_LEFT,
        "Collapse",
        Action::NavLeft,
    )
    .paired(),
    Keybinding::with_alt(
        KeyCombo::plain(KeyCode::Char('l')),
        &ALT_RIGHT,
        "Expand",
        Action::NavRight,
    ),
    Keybinding::with_alt(
        KeyCombo::alt(KeyCode::Right),
        &ALT_RECURSIVE_EXPAND,
        "Expand all",
        Action::RecursiveExpand,
    )
    .paired(),
    Keybinding::with_alt(
        KeyCombo::alt(KeyCode::Left),
        &ALT_RECURSIVE_COLLAPSE,
        "Collapse all",
        Action::RecursiveCollapse,
    ),
    Keybinding::new(
        KeyCombo::alt(KeyCode::Up),
        "First in folder",
        Action::GoToTop,
    )
    .paired(),
    Keybinding::new(
        KeyCombo::alt(KeyCode::Down),
        "Last in folder",
        Action::GoToBottom,
    ),
    Keybinding::new(
        KeyCombo::plain(KeyCode::Enter),
        "Open/toggle",
        Action::OpenFile,
    ),
    Keybinding::new(
        KeyCombo::plain(KeyCode::Char(' ')),
        "Toggle dir",
        Action::ToggleDir,
    ),
    Keybinding::new(
        KeyCombo::plain(KeyCode::Char('a')),
        "Add file/dir",
        Action::AddFile,
    ),
    Keybinding::new(
        KeyCombo::plain(KeyCode::Char('d')),
        "Delete",
        Action::DeleteFile,
    ),
    Keybinding::new(
        KeyCombo::plain(KeyCode::Char('r')),
        "Rename",
        Action::RenameFile,
    ),
    Keybinding::new(
        KeyCombo::plain(KeyCode::Char('c')),
        "Copy",
        Action::CopyFile,
    ),
    Keybinding::new(
        KeyCombo::plain(KeyCode::Char('m')),
        "Move",
        Action::MoveFile,
    ),
    Keybinding::new(
        KeyCombo::shift(KeyCode::Char('O')),
        "Options",
        Action::FileTreeOptions,
    ),
    Keybinding::new(KeyCombo::plain(KeyCode::Esc), "Back", Action::Escape),
];

/// Viewer bindings (read-only mode)
pub static VIEWER: [Keybinding; 14] = [
    Keybinding::with_alt(
        KeyCombo::plain(KeyCode::Char('j')),
        &ALT_DOWN,
        "Scroll line",
        Action::NavDown,
    )
    .paired(),
    Keybinding::with_alt(
        KeyCombo::plain(KeyCode::Char('k')),
        &ALT_UP,
        "Scroll line",
        Action::NavUp,
    ),
    Keybinding::with_alt(
        KeyCombo::shift(KeyCode::Char('J')),
        &ALT_PGDN,
        "Page down",
        Action::PageDown,
    )
    .paired(),
    Keybinding::with_alt(
        KeyCombo::shift(KeyCode::Char('K')),
        &ALT_PGUP,
        "Page up",
        Action::PageUp,
    ),
    Keybinding::with_alt(
        KeyCombo::alt(KeyCode::Up),
        &ALT_HOME,
        "Top",
        Action::GoToTop,
    )
    .paired(),
    Keybinding::with_alt(
        KeyCombo::alt(KeyCode::Down),
        &ALT_END,
        "Bottom",
        Action::GoToBottom,
    ),
    Keybinding::new(
        KeyCombo::alt(KeyCode::Right),
        "Next Edit",
        Action::JumpNextEdit,
    )
    .paired(),
    Keybinding::new(
        KeyCombo::alt(KeyCode::Left),
        "Prev Edit",
        Action::JumpPrevEdit,
    ),
    Keybinding::new(
        KeyCombo::plain(KeyCode::Char('e')),
        "Edit file",
        Action::EnterEditMode,
    ),
    Keybinding::new(
        KeyCombo::plain(KeyCode::Esc),
        "Close viewer",
        Action::Escape,
    ),
    Keybinding::new(KEY_SELECT_ALL, "Select all", Action::SelectAll),
    Keybinding::new(
        KeyCombo::plain(KeyCode::Char('t')),
        "Tab file",
        Action::ViewerTabCurrent,
    ),
    Keybinding::with_alt(
        KeyCombo::alt(KeyCode::Char('t')),
        &ALT_MACOS_T,
        "Tab dialog",
        Action::ViewerOpenTabDialog,
    ),
    Keybinding::new(
        KeyCombo::plain(KeyCode::Char('x')),
        "Close tab",
        Action::ViewerCloseTab,
    ),
];

/// Edit mode bindings
pub static EDIT_MODE: [Keybinding; 5] = [
    Keybinding::new(KEY_SAVE, "Save file", Action::Save),
    Keybinding::new(KEY_UNDO, "Undo", Action::Undo).paired(),
    Keybinding::new(KEY_REDO, "Redo", Action::Redo),
    Keybinding::new(KEY_EDIT_STT, "Speech input", Action::ToggleStt),
    Keybinding::new(
        KeyCombo::plain(KeyCode::Esc),
        "Exit edit mode",
        Action::Escape,
    ),
];

/// Convo/Output bindings
pub static SESSION: [Keybinding; 14] = [
    Keybinding::new(
        KeyCombo::plain(KeyCode::Char('a')),
        "Add session",
        Action::NewSession,
    ),
    Keybinding::new(
        KeyCombo::plain(KeyCode::Char('s')),
        "Session list",
        Action::ToggleSessionList,
    ),
    Keybinding::new(
        KeyCombo::plain(KeyCode::Char('/')),
        "Search",
        Action::SearchSession,
    ),
    Keybinding::new(
        KeyCombo::plain(KeyCode::Char('j')),
        "Scroll line",
        Action::NavDown,
    )
    .paired(),
    Keybinding::new(
        KeyCombo::plain(KeyCode::Char('k')),
        "Scroll line",
        Action::NavUp,
    ),
    Keybinding::new(
        KeyCombo::plain(KeyCode::Down),
        "Next message",
        Action::JumpNextBubble,
    )
    .paired(),
    Keybinding::new(
        KeyCombo::plain(KeyCode::Up),
        "Prev message",
        Action::JumpPrevBubble,
    ),
    Keybinding::new(
        KeyCombo::shift(KeyCode::Down),
        "Next prompt",
        Action::JumpNextMessage,
    )
    .paired(),
    Keybinding::new(
        KeyCombo::shift(KeyCode::Up),
        "Prev prompt",
        Action::JumpPrevMessage,
    ),
    Keybinding::new(
        KeyCombo::shift(KeyCode::Char('J')),
        "Page down",
        Action::PageDown,
    )
    .paired(),
    Keybinding::new(
        KeyCombo::shift(KeyCode::Char('K')),
        "Page up",
        Action::PageUp,
    ),
    Keybinding::new(KeyCombo::alt(KeyCode::Up), "Top", Action::GoToTop).paired(),
    Keybinding::new(KeyCombo::alt(KeyCode::Down), "Bottom", Action::GoToBottom),
    Keybinding::new(
        KeyCombo::plain(KeyCode::Esc),
        "Back to Worktrees",
        Action::Escape,
    ),
];

/// Input mode bindings — keys that work in Claude prompt type mode
/// Word nav uses standard macOS shortcuts (⌥← / ⌥→), not ⌃z/⌃x which conflict with clipboard
/// Newline: ⇧Enter (Kitty keyboard protocol makes this distinguishable from bare Enter)
pub static INPUT: [Keybinding; 10] = [
    Keybinding::new(
        KeyCombo::plain(KeyCode::Enter),
        "Submit prompt",
        Action::Submit,
    ),
    Keybinding::with_alt_kitty(
        KeyCombo::shift(KeyCode::Enter),
        &ALT_INSERT_NEWLINE,
        "Insert newline",
        Action::InsertNewline,
    ),
    Keybinding::new(
        KeyCombo::plain(KeyCode::Esc),
        "Exit to COMMAND",
        Action::ExitPromptMode,
    ),
    Keybinding::with_alt(
        KeyCombo::alt(KeyCode::Left),
        &ALT_CTRL_LEFT,
        "Word left",
        Action::WordLeft,
    )
    .paired(),
    Keybinding::with_alt(
        KeyCombo::alt(KeyCode::Right),
        &ALT_CTRL_RIGHT,
        "Word right",
        Action::WordRight,
    ),
    Keybinding::with_alt(
        KeyCombo::ctrl(KeyCode::Char('w')),
        &ALT_DELETE_WORD,
        "Delete word",
        Action::DeleteWord,
    ),
    Keybinding::new(
        KeyCombo::plain(KeyCode::Up),
        "History prev",
        Action::HistoryPrev,
    )
    .paired(),
    Keybinding::new(
        KeyCombo::plain(KeyCode::Down),
        "History next",
        Action::HistoryNext,
    ),
    Keybinding::new(
        KeyCombo::ctrl(KeyCode::Char('s')),
        "Speech input",
        Action::ToggleStt,
    ),
    Keybinding::with_alt(
        KeyCombo::alt(KeyCode::Char('p')),
        &ALT_MACOS_P,
        "Preset prompts",
        Action::PresetPrompts,
    ),
];

/// Terminal bindings (command mode) — ALL terminal keybindings live here
/// so title bar hints can source from them dynamically
pub static TERMINAL: [Keybinding; 11] = [
    Keybinding::new(
        KeyCombo::plain(KeyCode::Char('t')),
        "Enter type mode",
        Action::EnterTerminalType,
    ),
    Keybinding::new(
        KeyCombo::plain(KeyCode::Char('p')),
        "Close & prompt",
        Action::EnterPromptMode,
    ),
    Keybinding::new(
        KeyCombo::plain(KeyCode::Esc),
        "Close terminal",
        Action::Escape,
    ),
    Keybinding::with_alt(
        KeyCombo::plain(KeyCode::Char('j')),
        &ALT_DOWN,
        "Scroll line",
        Action::NavDown,
    )
    .paired(),
    Keybinding::with_alt(
        KeyCombo::plain(KeyCode::Char('k')),
        &ALT_UP,
        "Scroll line",
        Action::NavUp,
    ),
    Keybinding::new(
        KeyCombo::shift(KeyCode::Char('J')),
        "Scroll page",
        Action::PageDown,
    )
    .paired(),
    Keybinding::new(
        KeyCombo::shift(KeyCode::Char('K')),
        "Scroll page",
        Action::PageUp,
    ),
    Keybinding::new(KeyCombo::alt(KeyCode::Up), "Scroll to top", Action::GoToTop).paired(),
    Keybinding::new(
        KeyCombo::alt(KeyCode::Down),
        "Scroll to bottom",
        Action::GoToBottom,
    ),
    Keybinding::new(
        KeyCombo::plain(KeyCode::Char('+')),
        "Resize up",
        Action::ResizeUp,
    )
    .paired(),
    Keybinding::new(
        KeyCombo::plain(KeyCode::Char('-')),
        "Resize down",
        Action::ResizeDown,
    ),
];

// ─── Modal panel binding arrays ───────────────────────────────────────────────

/// Health Panel — bindings shared across both tabs (Tab, nav, Esc)
pub static HEALTH_SHARED: [Keybinding; 9] = [
    Keybinding::new(
        KeyCombo::plain(KeyCode::Tab),
        "Switch tab",
        Action::HealthSwitchTab,
    ),
    Keybinding::with_alt(
        KeyCombo::plain(KeyCode::Char('j')),
        &ALT_DOWN,
        "Navigate",
        Action::NavDown,
    )
    .paired(),
    Keybinding::with_alt(
        KeyCombo::plain(KeyCode::Char('k')),
        &ALT_UP,
        "Navigate",
        Action::NavUp,
    ),
    Keybinding::with_alt(
        KeyCombo::shift(KeyCode::Char('J')),
        &ALT_PGDN,
        "Page down",
        Action::PageDown,
    )
    .paired(),
    Keybinding::with_alt(
        KeyCombo::shift(KeyCode::Char('K')),
        &ALT_PGUP,
        "Page up",
        Action::PageUp,
    ),
    Keybinding::new(KeyCombo::alt(KeyCode::Up), "Jump to top", Action::GoToTop).paired(),
    Keybinding::new(
        KeyCombo::alt(KeyCode::Down),
        "Jump to bottom",
        Action::GoToBottom,
    ),
    Keybinding::new(
        KeyCombo::plain(KeyCode::Char('s')),
        "Scope mode",
        Action::HealthScopeMode,
    ),
    Keybinding::new(KeyCombo::plain(KeyCode::Esc), "Close", Action::Escape),
];

/// Health Panel — God Files tab actions (Space/a/v/Enter/m)
pub static HEALTH_GOD_FILES: [Keybinding; 4] = [
    Keybinding::new(
        KeyCombo::plain(KeyCode::Char(' ')),
        "Toggle check",
        Action::HealthToggleCheck,
    ),
    Keybinding::new(
        KeyCombo::plain(KeyCode::Char('a')),
        "Toggle all",
        Action::HealthToggleAll,
    ),
    Keybinding::new(
        KeyCombo::plain(KeyCode::Char('v')),
        "View checked",
        Action::HealthViewChecked,
    ),
    Keybinding::with_alt(
        KeyCombo::plain(KeyCode::Enter),
        &ALT_CHAR_M,
        "Modularize",
        Action::HealthModularize,
    ),
];

/// Health Panel — Documentation tab actions.
/// Space checks, `a` toggles all non-100%, `v` views in Viewer, Enter spawns [DH] sessions.
pub static HEALTH_DOCS: [Keybinding; 4] = [
    Keybinding::new(
        KeyCombo::plain(KeyCode::Char(' ')),
        "Toggle check",
        Action::HealthDocToggleCheck,
    ),
    Keybinding::new(
        KeyCombo::plain(KeyCode::Char('a')),
        "Check non-100%",
        Action::HealthDocToggleNon100,
    ),
    Keybinding::new(
        KeyCombo::plain(KeyCode::Char('v')),
        "View checked",
        Action::HealthViewChecked,
    ),
    Keybinding::new(
        KeyCombo::plain(KeyCode::Enter),
        "Complete checked",
        Action::HealthDocSpawn,
    ),
];

/// Git Actions Panel — all keys for the git modal overlay.
/// Actions are context-aware: main branch shows pull+commit+push,
/// feature branches show squash-merge+commit+push. Guards in
/// lookup_git_actions_action() enforce this based on is_on_main + actions_focused.
pub static GIT_ACTIONS: [Keybinding; 27] = [
    Keybinding::new(KeyCombo::plain(KeyCode::Esc), "Close", Action::Escape),
    Keybinding::new(
        KeyCombo::plain(KeyCode::Tab),
        "Cycle fwd",
        Action::GitToggleFocus,
    )
    .paired(),
    Keybinding::new(
        KeyCombo::plain(KeyCode::BackTab),
        "Cycle back",
        Action::GitToggleFocusBack,
    ),
    Keybinding::with_alt(
        KeyCombo::plain(KeyCode::Char('j')),
        &ALT_DOWN,
        "Navigate",
        Action::NavDown,
    )
    .paired(),
    Keybinding::with_alt(
        KeyCombo::plain(KeyCode::Char('k')),
        &ALT_UP,
        "Navigate",
        Action::NavUp,
    ),
    Keybinding::new(KeyCombo::alt(KeyCode::Up), "Jump to top", Action::GoToTop).paired(),
    Keybinding::new(
        KeyCombo::alt(KeyCode::Down),
        "Jump to bottom",
        Action::GoToBottom,
    ),
    Keybinding::new(
        KeyCombo::plain(KeyCode::Char('m')),
        "Squash merge to main",
        Action::GitSquashMerge,
    ),
    Keybinding::new(
        KeyCombo::plain(KeyCode::Char('r')),
        "Refresh",
        Action::GitRefresh,
    ),
    Keybinding::new(KeyCombo::plain(KeyCode::Char('l')), "Pull", Action::GitPull),
    Keybinding::new(
        KeyCombo::plain(KeyCode::Char('c')),
        "Commit",
        Action::GitCommit,
    ),
    Keybinding::new(
        KeyCombo::shift(KeyCode::Char('P')),
        "Push to remote",
        Action::GitPush,
    ),
    Keybinding::with_alt(
        KeyCombo::plain(KeyCode::Enter),
        &ALT_CHAR_D,
        "Exec/view diff",
        Action::Confirm,
    ),
    Keybinding::new(
        KeyCombo::shift(KeyCode::Char('R')),
        "Rebase onto main",
        Action::GitRebase,
    ),
    Keybinding::new(
        KeyCombo::plain(KeyCode::Char('d')),
        "View diff",
        Action::GitViewDiff,
    ),
    Keybinding::new(
        KeyCombo::plain(KeyCode::Char('a')),
        "Auto-rebase",
        Action::GitAutoRebase,
    ),
    Keybinding::new(
        KeyCombo::plain(KeyCode::Char('s')),
        "Auto-resolve files",
        Action::GitAutoResolveSettings,
    ),
    Keybinding::new(
        KeyCombo::plain(KeyCode::Char('s')),
        "Stage/unstage",
        Action::GitToggleStage,
    ),
    Keybinding::new(
        KeyCombo::shift(KeyCode::Char('S')),
        "Stage/unstage all",
        Action::GitStageAll,
    ),
    Keybinding::new(
        KeyCombo::plain(KeyCode::Char('x')),
        "Discard changes",
        Action::GitDiscardFile,
    ),
    Keybinding::new(
        KeyCombo::plain(KeyCode::Char('[')),
        "Prev worktree",
        Action::GitPrevWorktree,
    )
    .paired(),
    Keybinding::new(
        KeyCombo::plain(KeyCode::Char(']')),
        "Next worktree",
        Action::GitNextWorktree,
    ),
    Keybinding::with_alt(
        KeyCombo::shift(KeyCode::Char('{')),
        &ALT_LBRACE,
        "Prev page",
        Action::GitPrevPage,
    )
    .paired(),
    Keybinding::with_alt(
        KeyCombo::shift(KeyCode::Char('}')),
        &ALT_RBRACE,
        "Next page",
        Action::GitNextPage,
    ),
    Keybinding::new(
        KeyCombo::plain(KeyCode::Char('z')),
        "Stash changes",
        Action::GitStash,
    ),
    Keybinding::new(
        KeyCombo::shift(KeyCode::Char('Z')),
        "Stash pop",
        Action::GitStashPop,
    ),
    Keybinding::new(
        KeyCombo::shift(KeyCode::Char('M')),
        "Browse main",
        Action::BrowseMain,
    ),
];

/// Projects Panel — browse mode bindings (text input modes stay raw)
pub static PROJECTS_BROWSE: [Keybinding; 9] = [
    Keybinding::new(KeyCombo::ctrl(KeyCode::Char('q')), "Quit", Action::Quit),
    Keybinding::with_alt(
        KeyCombo::plain(KeyCode::Char('j')),
        &ALT_DOWN,
        "Navigate",
        Action::NavDown,
    )
    .paired(),
    Keybinding::with_alt(
        KeyCombo::plain(KeyCode::Char('k')),
        &ALT_UP,
        "Navigate",
        Action::NavUp,
    ),
    Keybinding::new(
        KeyCombo::plain(KeyCode::Enter),
        "Open project",
        Action::Confirm,
    ),
    Keybinding::new(
        KeyCombo::plain(KeyCode::Char('a')),
        "Add project",
        Action::ProjectsAdd,
    ),
    Keybinding::new(
        KeyCombo::plain(KeyCode::Char('d')),
        "Delete",
        Action::ProjectsDelete,
    ),
    Keybinding::new(
        KeyCombo::plain(KeyCode::Char('n')),
        "Rename",
        Action::ProjectsRename,
    ),
    Keybinding::new(
        KeyCombo::plain(KeyCode::Char('i')),
        "Init git repo",
        Action::ProjectsInit,
    ),
    Keybinding::new(KeyCombo::plain(KeyCode::Esc), "Close", Action::Escape),
];

/// Picker — shared bindings for run command + preset prompt pickers.
/// Number quick-select (1-9/0) stays raw in handlers — not rebindable.
pub static PICKER: [Keybinding; 7] = [
    Keybinding::with_alt(
        KeyCombo::plain(KeyCode::Char('j')),
        &ALT_DOWN,
        "Navigate",
        Action::NavDown,
    )
    .paired(),
    Keybinding::with_alt(
        KeyCombo::plain(KeyCode::Char('k')),
        &ALT_UP,
        "Navigate",
        Action::NavUp,
    ),
    Keybinding::new(KeyCombo::plain(KeyCode::Enter), "Select", Action::Confirm),
    Keybinding::new(
        KeyCombo::plain(KeyCode::Char('e')),
        "Edit",
        Action::EditSelected,
    ),
    Keybinding::new(
        KeyCombo::plain(KeyCode::Char('d')),
        "Delete",
        Action::DeleteSelected,
    ),
    Keybinding::new(
        KeyCombo::plain(KeyCode::Char('a')),
        "Add new",
        Action::ProjectsAdd,
    ),
    Keybinding::new(KeyCombo::plain(KeyCode::Esc), "Close", Action::Escape),
];

/// Branch Dialog — nav + select (filter chars stay raw)
pub static BRANCH_DIALOG: [Keybinding; 4] = [
    Keybinding::with_alt(
        KeyCombo::plain(KeyCode::Char('j')),
        &ALT_DOWN,
        "Navigate",
        Action::NavDown,
    )
    .paired(),
    Keybinding::with_alt(
        KeyCombo::plain(KeyCode::Char('k')),
        &ALT_UP,
        "Navigate",
        Action::NavUp,
    ),
    Keybinding::new(KeyCombo::plain(KeyCode::Enter), "Select", Action::Confirm),
    Keybinding::new(KeyCombo::plain(KeyCode::Esc), "Close", Action::Escape),
];

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    /// Collect all (modifier_bits, code_debug) pairs from a binding array to detect dupes.
    /// Includes primary + all alternatives.
    fn all_combos(bindings: &[Keybinding]) -> Vec<(u8, String)> {
        let mut combos = Vec::new();
        for b in bindings {
            combos.push((b.primary.modifiers.bits(), format!("{:?}", b.primary.code)));
            for alt in b.alternatives {
                combos.push((alt.modifiers.bits(), format!("{:?}", alt.code)));
            }
        }
        combos
    }

    /// Assert no duplicate PRIMARY key combos in a single array.
    /// (Alternatives across different bindings may intentionally overlap with primaries.)
    fn assert_no_duplicate_primaries(name: &str, bindings: &[Keybinding]) {
        let mut seen = HashSet::new();
        for b in bindings {
            let key = (b.primary.modifiers.bits(), format!("{:?}", b.primary.code));
            assert!(
                seen.insert(key.clone()),
                "duplicate primary key in {}: {:?}",
                name,
                key
            );
        }
    }

    // ══════════════════════════════════════════════════════════════════
    //  Array lengths
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn global_length() {
        assert_eq!(GLOBAL.len(), 18);
    }

    #[test]
    fn worktrees_length() {
        assert_eq!(WORKTREES.len(), 4);
    }

    #[test]
    fn file_tree_length() {
        assert_eq!(FILE_TREE.len(), 17);
    }

    #[test]
    fn viewer_length() {
        assert_eq!(VIEWER.len(), 14);
    }

    #[test]
    fn edit_mode_length() {
        assert_eq!(EDIT_MODE.len(), 5);
    }

    #[test]
    fn session_length() {
        assert_eq!(SESSION.len(), 14);
    }

    #[test]
    fn input_length() {
        assert_eq!(INPUT.len(), 10);
    }

    #[test]
    fn terminal_length() {
        assert_eq!(TERMINAL.len(), 11);
    }

    #[test]
    fn health_shared_length() {
        assert_eq!(HEALTH_SHARED.len(), 9);
    }

    #[test]
    fn health_god_files_length() {
        assert_eq!(HEALTH_GOD_FILES.len(), 4);
    }

    #[test]
    fn health_docs_length() {
        assert_eq!(HEALTH_DOCS.len(), 4);
    }

    #[test]
    fn git_actions_length() {
        assert_eq!(GIT_ACTIONS.len(), 27);
    }

    #[test]
    fn projects_browse_length() {
        assert_eq!(PROJECTS_BROWSE.len(), 9);
    }

    #[test]
    fn picker_length() {
        assert_eq!(PICKER.len(), 7);
    }

    #[test]
    fn branch_dialog_length() {
        assert_eq!(BRANCH_DIALOG.len(), 4);
    }

    // ══════════════════════════════════════════════════════════════════
    //  Non-empty arrays
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn global_nonempty() {
        assert!(!GLOBAL.is_empty());
    }

    #[test]
    fn worktrees_nonempty() {
        assert!(!WORKTREES.is_empty());
    }

    #[test]
    fn file_tree_nonempty() {
        assert!(!FILE_TREE.is_empty());
    }

    #[test]
    fn viewer_nonempty() {
        assert!(!VIEWER.is_empty());
    }

    #[test]
    fn edit_mode_nonempty() {
        assert!(!EDIT_MODE.is_empty());
    }

    #[test]
    fn session_nonempty() {
        assert!(!SESSION.is_empty());
    }

    #[test]
    fn input_nonempty() {
        assert!(!INPUT.is_empty());
    }

    #[test]
    fn terminal_nonempty() {
        assert!(!TERMINAL.is_empty());
    }

    #[test]
    fn health_shared_nonempty() {
        assert!(!HEALTH_SHARED.is_empty());
    }

    #[test]
    fn health_god_files_nonempty() {
        assert!(!HEALTH_GOD_FILES.is_empty());
    }

    #[test]
    fn health_docs_nonempty() {
        assert!(!HEALTH_DOCS.is_empty());
    }

    #[test]
    fn git_actions_nonempty() {
        assert!(!GIT_ACTIONS.is_empty());
    }

    #[test]
    fn projects_browse_nonempty() {
        assert!(!PROJECTS_BROWSE.is_empty());
    }

    #[test]
    fn picker_nonempty() {
        assert!(!PICKER.is_empty());
    }

    #[test]
    fn branch_dialog_nonempty() {
        assert!(!BRANCH_DIALOG.is_empty());
    }

    // ══════════════════════════════════════════════════════════════════
    //  No duplicate primary keys within any single array
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn global_no_duplicate_primaries() {
        assert_no_duplicate_primaries("GLOBAL", &GLOBAL);
    }

    #[test]
    fn worktrees_no_duplicate_primaries() {
        assert_no_duplicate_primaries("WORKTREES", &WORKTREES);
    }

    #[test]
    fn file_tree_no_duplicate_primaries() {
        assert_no_duplicate_primaries("FILE_TREE", &FILE_TREE);
    }

    #[test]
    fn viewer_no_duplicate_primaries() {
        assert_no_duplicate_primaries("VIEWER", &VIEWER);
    }

    #[test]
    fn edit_mode_no_duplicate_primaries() {
        assert_no_duplicate_primaries("EDIT_MODE", &EDIT_MODE);
    }

    #[test]
    fn session_no_duplicate_primaries() {
        assert_no_duplicate_primaries("SESSION", &SESSION);
    }

    #[test]
    fn input_no_duplicate_primaries() {
        assert_no_duplicate_primaries("INPUT", &INPUT);
    }

    #[test]
    fn terminal_no_duplicate_primaries() {
        assert_no_duplicate_primaries("TERMINAL", &TERMINAL);
    }

    #[test]
    fn health_shared_no_duplicate_primaries() {
        assert_no_duplicate_primaries("HEALTH_SHARED", &HEALTH_SHARED);
    }

    #[test]
    fn health_god_files_no_duplicate_primaries() {
        assert_no_duplicate_primaries("HEALTH_GOD_FILES", &HEALTH_GOD_FILES);
    }

    #[test]
    fn health_docs_no_duplicate_primaries() {
        assert_no_duplicate_primaries("HEALTH_DOCS", &HEALTH_DOCS);
    }

    #[test]
    fn git_actions_no_duplicate_primaries() {
        // GIT_ACTIONS has intentional pane-specific key overloads (e.g. 's' = auto-resolve in
        // actions pane, toggle-stage in files pane). Check (key, action) uniqueness instead.
        let mut seen = HashSet::new();
        for b in &GIT_ACTIONS {
            let key = (
                b.primary.modifiers.bits(),
                format!("{:?}", b.primary.code),
                b.action,
            );
            assert!(
                seen.insert(key.clone()),
                "duplicate primary+action in GIT_ACTIONS: {:?}",
                key
            );
        }
    }

    #[test]
    fn projects_browse_no_duplicate_primaries() {
        assert_no_duplicate_primaries("PROJECTS_BROWSE", &PROJECTS_BROWSE);
    }

    #[test]
    fn picker_no_duplicate_primaries() {
        assert_no_duplicate_primaries("PICKER", &PICKER);
    }

    #[test]
    fn branch_dialog_no_duplicate_primaries() {
        assert_no_duplicate_primaries("BRANCH_DIALOG", &BRANCH_DIALOG);
    }

    // ══════════════════════════════════════════════════════════════════
    //  All bindings have non-empty descriptions
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn global_all_descriptions_nonempty() {
        for b in &GLOBAL {
            assert!(
                !b.description.is_empty(),
                "action {:?} has empty desc",
                b.action
            );
        }
    }

    #[test]
    fn worktrees_all_descriptions_nonempty() {
        for b in &WORKTREES {
            assert!(
                !b.description.is_empty(),
                "action {:?} has empty desc",
                b.action
            );
        }
    }

    #[test]
    fn file_tree_all_descriptions_nonempty() {
        for b in &FILE_TREE {
            assert!(
                !b.description.is_empty(),
                "action {:?} has empty desc",
                b.action
            );
        }
    }

    #[test]
    fn viewer_all_descriptions_nonempty() {
        for b in &VIEWER {
            assert!(
                !b.description.is_empty(),
                "action {:?} has empty desc",
                b.action
            );
        }
    }

    #[test]
    fn edit_mode_all_descriptions_nonempty() {
        for b in &EDIT_MODE {
            assert!(
                !b.description.is_empty(),
                "action {:?} has empty desc",
                b.action
            );
        }
    }

    #[test]
    fn session_all_descriptions_nonempty() {
        for b in &SESSION {
            assert!(
                !b.description.is_empty(),
                "action {:?} has empty desc",
                b.action
            );
        }
    }

    #[test]
    fn input_all_descriptions_nonempty() {
        for b in &INPUT {
            assert!(
                !b.description.is_empty(),
                "action {:?} has empty desc",
                b.action
            );
        }
    }

    #[test]
    fn terminal_all_descriptions_nonempty() {
        for b in &TERMINAL {
            assert!(
                !b.description.is_empty(),
                "action {:?} has empty desc",
                b.action
            );
        }
    }

    #[test]
    fn health_shared_all_descriptions_nonempty() {
        for b in &HEALTH_SHARED {
            assert!(
                !b.description.is_empty(),
                "action {:?} has empty desc",
                b.action
            );
        }
    }

    #[test]
    fn health_god_files_all_descriptions_nonempty() {
        for b in &HEALTH_GOD_FILES {
            assert!(
                !b.description.is_empty(),
                "action {:?} has empty desc",
                b.action
            );
        }
    }

    #[test]
    fn health_docs_all_descriptions_nonempty() {
        for b in &HEALTH_DOCS {
            assert!(
                !b.description.is_empty(),
                "action {:?} has empty desc",
                b.action
            );
        }
    }

    #[test]
    fn git_actions_all_descriptions_nonempty() {
        for b in &GIT_ACTIONS {
            assert!(
                !b.description.is_empty(),
                "action {:?} has empty desc",
                b.action
            );
        }
    }

    #[test]
    fn projects_browse_all_descriptions_nonempty() {
        for b in &PROJECTS_BROWSE {
            assert!(
                !b.description.is_empty(),
                "action {:?} has empty desc",
                b.action
            );
        }
    }

    #[test]
    fn picker_all_descriptions_nonempty() {
        for b in &PICKER {
            assert!(
                !b.description.is_empty(),
                "action {:?} has empty desc",
                b.action
            );
        }
    }

    #[test]
    fn branch_dialog_all_descriptions_nonempty() {
        for b in &BRANCH_DIALOG {
            assert!(
                !b.description.is_empty(),
                "action {:?} has empty desc",
                b.action
            );
        }
    }

    // ══════════════════════════════════════════════════════════════════
    //  Specific binding verification — GLOBAL
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn global_first_is_quit() {
        assert_eq!(GLOBAL[0].action, Action::Quit);
        assert_eq!(GLOBAL[0].primary.modifiers, KeyModifiers::CONTROL);
        assert_eq!(GLOBAL[0].primary.code, KeyCode::Char('q'));
    }

    #[test]
    fn global_has_dump_debug() {
        assert!(GLOBAL.iter().any(|b| b.action == Action::DumpDebug));
    }

    #[test]
    fn global_has_cancel_claude() {
        assert!(GLOBAL.iter().any(|b| b.action == Action::CancelClaude));
    }

    #[test]
    fn global_has_copy_selection() {
        let b = GLOBAL
            .iter()
            .find(|b| b.action == Action::CopySelection)
            .unwrap();
        #[cfg(target_os = "macos")]
        assert_eq!(b.primary.modifiers, KeyModifiers::SUPER);
        #[cfg(not(target_os = "macos"))]
        assert_eq!(b.primary.modifiers, KeyModifiers::CONTROL);
    }

    #[test]
    fn global_has_cycle_model() {
        assert!(GLOBAL.iter().any(|b| b.action == Action::CycleModel));
    }

    #[test]
    fn global_has_toggle_help() {
        let b = GLOBAL
            .iter()
            .find(|b| b.action == Action::ToggleHelp)
            .unwrap();
        assert_eq!(b.primary.code, KeyCode::Char('?'));
    }

    #[test]
    fn global_worktree_tab_next_paired() {
        let b = GLOBAL
            .iter()
            .find(|b| b.action == Action::WorktreeTabNext)
            .unwrap();
        assert!(b.pair_with_next);
    }

    #[test]
    fn global_worktree_tab_prev_not_paired() {
        let b = GLOBAL
            .iter()
            .find(|b| b.action == Action::WorktreeTabPrev)
            .unwrap();
        assert!(!b.pair_with_next);
    }

    // ══════════════════════════════════════════════════════════════════
    //  Specific binding verification — FILE_TREE
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn filetree_j_has_down_arrow_alt() {
        let b = FILE_TREE
            .iter()
            .find(|b| b.action == Action::NavDown)
            .unwrap();
        assert!(b.alternatives.iter().any(|a| a.code == KeyCode::Down));
    }

    #[test]
    fn filetree_nav_down_is_paired() {
        let b = FILE_TREE
            .iter()
            .find(|b| b.action == Action::NavDown)
            .unwrap();
        assert!(b.pair_with_next);
    }

    #[test]
    fn filetree_nav_up_not_paired() {
        let b = FILE_TREE
            .iter()
            .find(|b| b.action == Action::NavUp)
            .unwrap();
        assert!(!b.pair_with_next);
    }

    #[test]
    fn filetree_has_all_file_operations() {
        let actions: Vec<Action> = FILE_TREE.iter().map(|b| b.action).collect();
        assert!(actions.contains(&Action::AddFile));
        assert!(actions.contains(&Action::DeleteFile));
        assert!(actions.contains(&Action::RenameFile));
        assert!(actions.contains(&Action::CopyFile));
        assert!(actions.contains(&Action::MoveFile));
    }

    #[test]
    fn filetree_has_options_overlay() {
        let b = FILE_TREE
            .iter()
            .find(|b| b.action == Action::FileTreeOptions)
            .unwrap();
        assert_eq!(b.primary.modifiers, KeyModifiers::SHIFT);
        assert_eq!(b.primary.code, KeyCode::Char('O'));
    }

    // ══════════════════════════════════════════════════════════════════
    //  Specific binding verification — VIEWER
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn viewer_has_enter_edit_mode() {
        let b = VIEWER
            .iter()
            .find(|b| b.action == Action::EnterEditMode)
            .unwrap();
        assert_eq!(b.primary.code, KeyCode::Char('e'));
    }

    #[test]
    fn viewer_has_page_down_with_pgdn_alt() {
        let b = VIEWER
            .iter()
            .find(|b| b.action == Action::PageDown)
            .unwrap();
        assert!(b.alternatives.iter().any(|a| a.code == KeyCode::PageDown));
    }

    #[test]
    fn viewer_has_page_up_with_pgup_alt() {
        let b = VIEWER.iter().find(|b| b.action == Action::PageUp).unwrap();
        assert!(b.alternatives.iter().any(|a| a.code == KeyCode::PageUp));
    }

    #[test]
    fn viewer_has_go_to_top_with_home_alt() {
        let b = VIEWER.iter().find(|b| b.action == Action::GoToTop).unwrap();
        assert!(b.alternatives.iter().any(|a| a.code == KeyCode::Home));
    }

    #[test]
    fn viewer_has_go_to_bottom_with_end_alt() {
        let b = VIEWER
            .iter()
            .find(|b| b.action == Action::GoToBottom)
            .unwrap();
        assert!(b.alternatives.iter().any(|a| a.code == KeyCode::End));
    }

    #[test]
    fn viewer_has_select_all() {
        let b = VIEWER
            .iter()
            .find(|b| b.action == Action::SelectAll)
            .unwrap();
        #[cfg(target_os = "macos")]
        assert_eq!(b.primary.modifiers, KeyModifiers::SUPER);
        #[cfg(not(target_os = "macos"))]
        assert_eq!(b.primary.modifiers, KeyModifiers::CONTROL);
    }

    #[test]
    fn viewer_has_tab_current() {
        assert!(VIEWER.iter().any(|b| b.action == Action::ViewerTabCurrent));
    }

    #[test]
    fn viewer_has_tab_dialog_with_macos_alt() {
        let b = VIEWER
            .iter()
            .find(|b| b.action == Action::ViewerOpenTabDialog)
            .unwrap();
        assert!(!b.alternatives.is_empty());
    }

    #[test]
    fn viewer_has_close_tab() {
        assert!(VIEWER.iter().any(|b| b.action == Action::ViewerCloseTab));
    }

    // ══════════════════════════════════════════════════════════════════
    //  Specific binding verification — EDIT_MODE
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn edit_mode_has_save() {
        let b = EDIT_MODE.iter().find(|b| b.action == Action::Save).unwrap();
        #[cfg(target_os = "macos")]
        assert_eq!(b.primary.modifiers, KeyModifiers::SUPER);
        #[cfg(not(target_os = "macos"))]
        assert_eq!(b.primary.modifiers, KeyModifiers::CONTROL);
        assert_eq!(b.primary.code, KeyCode::Char('s'));
    }

    #[test]
    fn edit_mode_has_undo() {
        let b = EDIT_MODE.iter().find(|b| b.action == Action::Undo).unwrap();
        #[cfg(target_os = "macos")]
        assert_eq!(b.primary.modifiers, KeyModifiers::SUPER);
        #[cfg(not(target_os = "macos"))]
        assert_eq!(b.primary.modifiers, KeyModifiers::CONTROL);
        assert_eq!(b.primary.code, KeyCode::Char('z'));
    }

    #[test]
    fn edit_mode_undo_is_paired() {
        let b = EDIT_MODE.iter().find(|b| b.action == Action::Undo).unwrap();
        assert!(b.pair_with_next);
    }

    #[test]
    fn edit_mode_has_redo() {
        assert!(EDIT_MODE.iter().any(|b| b.action == Action::Redo));
    }

    #[test]
    fn edit_mode_has_stt() {
        let b = EDIT_MODE
            .iter()
            .find(|b| b.action == Action::ToggleStt)
            .unwrap();
        #[cfg(target_os = "macos")]
        assert_eq!(b.primary.modifiers, KeyModifiers::CONTROL);
        #[cfg(not(target_os = "macos"))]
        assert_eq!(b.primary.modifiers, KeyModifiers::ALT);
    }

    #[test]
    fn edit_mode_has_escape() {
        assert!(EDIT_MODE.iter().any(|b| b.action == Action::Escape));
    }

    // ══════════════════════════════════════════════════════════════════
    //  Specific binding verification — SESSION
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn session_has_new_session() {
        assert!(SESSION.iter().any(|b| b.action == Action::NewSession));
    }

    #[test]
    fn session_has_session_list() {
        assert!(SESSION
            .iter()
            .any(|b| b.action == Action::ToggleSessionList));
    }

    #[test]
    fn session_has_search() {
        let b = SESSION
            .iter()
            .find(|b| b.action == Action::SearchSession)
            .unwrap();
        assert_eq!(b.primary.code, KeyCode::Char('/'));
    }

    #[test]
    fn session_has_jump_next_bubble() {
        assert!(SESSION.iter().any(|b| b.action == Action::JumpNextBubble));
    }

    #[test]
    fn session_has_jump_prev_bubble() {
        assert!(SESSION.iter().any(|b| b.action == Action::JumpPrevBubble));
    }

    #[test]
    fn session_has_jump_next_message() {
        assert!(SESSION.iter().any(|b| b.action == Action::JumpNextMessage));
    }

    #[test]
    fn session_has_jump_prev_message() {
        assert!(SESSION.iter().any(|b| b.action == Action::JumpPrevMessage));
    }

    // ══════════════════════════════════════════════════════════════════
    //  Specific binding verification — INPUT
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn input_has_submit() {
        let b = INPUT.iter().find(|b| b.action == Action::Submit).unwrap();
        assert_eq!(b.primary.code, KeyCode::Enter);
    }

    #[test]
    fn input_has_insert_newline() {
        let b = INPUT
            .iter()
            .find(|b| b.action == Action::InsertNewline)
            .unwrap();
        assert_eq!(b.primary.modifiers, KeyModifiers::SHIFT);
        assert_eq!(b.primary.code, KeyCode::Enter);
    }

    #[test]
    fn input_word_left_has_ctrl_left_alt() {
        let b = INPUT.iter().find(|b| b.action == Action::WordLeft).unwrap();
        assert!(b
            .alternatives
            .iter()
            .any(|a| a.modifiers == KeyModifiers::CONTROL && a.code == KeyCode::Left));
    }

    #[test]
    fn input_word_right_has_ctrl_right_alt() {
        let b = INPUT
            .iter()
            .find(|b| b.action == Action::WordRight)
            .unwrap();
        assert!(b
            .alternatives
            .iter()
            .any(|a| a.modifiers == KeyModifiers::CONTROL && a.code == KeyCode::Right));
    }

    #[test]
    fn input_delete_word_has_ctrl_backspace_alt() {
        let b = INPUT
            .iter()
            .find(|b| b.action == Action::DeleteWord)
            .unwrap();
        assert!(b
            .alternatives
            .iter()
            .any(|a| a.modifiers == KeyModifiers::CONTROL && a.code == KeyCode::Backspace));
    }

    #[test]
    fn input_has_preset_prompts_with_macos_alt() {
        let b = INPUT
            .iter()
            .find(|b| b.action == Action::PresetPrompts)
            .unwrap();
        assert!(!b.alternatives.is_empty());
    }

    // ══════════════════════════════════════════════════════════════════
    //  Specific binding verification — TERMINAL
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn terminal_has_enter_type_mode() {
        let b = TERMINAL
            .iter()
            .find(|b| b.action == Action::EnterTerminalType)
            .unwrap();
        assert_eq!(b.primary.code, KeyCode::Char('t'));
    }

    #[test]
    fn terminal_has_enter_prompt() {
        let b = TERMINAL
            .iter()
            .find(|b| b.action == Action::EnterPromptMode)
            .unwrap();
        assert_eq!(b.primary.code, KeyCode::Char('p'));
    }

    #[test]
    fn terminal_has_resize_up_down() {
        assert!(TERMINAL.iter().any(|b| b.action == Action::ResizeUp));
        assert!(TERMINAL.iter().any(|b| b.action == Action::ResizeDown));
    }

    #[test]
    fn terminal_resize_up_is_paired() {
        let b = TERMINAL
            .iter()
            .find(|b| b.action == Action::ResizeUp)
            .unwrap();
        assert!(b.pair_with_next);
    }

    // ══════════════════════════════════════════════════════════════════
    //  Specific binding verification — GIT_ACTIONS
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn git_has_squash_merge() {
        let b = GIT_ACTIONS
            .iter()
            .find(|b| b.action == Action::GitSquashMerge)
            .unwrap();
        assert_eq!(b.primary.code, KeyCode::Char('m'));
    }

    #[test]
    fn git_has_rebase() {
        let b = GIT_ACTIONS
            .iter()
            .find(|b| b.action == Action::GitRebase)
            .unwrap();
        assert_eq!(b.primary.modifiers, KeyModifiers::SHIFT);
        assert_eq!(b.primary.code, KeyCode::Char('R'));
    }

    #[test]
    fn git_has_pull() {
        let b = GIT_ACTIONS
            .iter()
            .find(|b| b.action == Action::GitPull)
            .unwrap();
        assert_eq!(b.primary.code, KeyCode::Char('l'));
    }

    #[test]
    fn git_has_commit() {
        let b = GIT_ACTIONS
            .iter()
            .find(|b| b.action == Action::GitCommit)
            .unwrap();
        assert_eq!(b.primary.code, KeyCode::Char('c'));
    }

    #[test]
    fn git_has_push() {
        let b = GIT_ACTIONS
            .iter()
            .find(|b| b.action == Action::GitPush)
            .unwrap();
        assert_eq!(b.primary.modifiers, KeyModifiers::SHIFT);
    }

    #[test]
    fn git_confirm_enter_with_d_alt() {
        let b = GIT_ACTIONS
            .iter()
            .find(|b| b.action == Action::Confirm)
            .unwrap();
        assert_eq!(b.primary.code, KeyCode::Enter);
        assert!(b.alternatives.iter().any(|a| a.code == KeyCode::Char('d')));
    }

    #[test]
    fn git_has_refresh() {
        let b = GIT_ACTIONS
            .iter()
            .find(|b| b.action == Action::GitRefresh)
            .unwrap();
        assert_eq!(b.primary.modifiers, KeyModifiers::NONE);
        assert_eq!(b.primary.code, KeyCode::Char('r'));
    }

    #[test]
    fn git_has_prev_next_worktree() {
        assert!(GIT_ACTIONS
            .iter()
            .any(|b| b.action == Action::GitPrevWorktree));
        assert!(GIT_ACTIONS
            .iter()
            .any(|b| b.action == Action::GitNextWorktree));
    }

    #[test]
    fn git_prev_page_has_lbrace_alt() {
        let b = GIT_ACTIONS
            .iter()
            .find(|b| b.action == Action::GitPrevPage)
            .unwrap();
        assert!(b.alternatives.iter().any(|a| a.code == KeyCode::Char('{')));
    }

    #[test]
    fn git_next_page_has_rbrace_alt() {
        let b = GIT_ACTIONS
            .iter()
            .find(|b| b.action == Action::GitNextPage)
            .unwrap();
        assert!(b.alternatives.iter().any(|a| a.code == KeyCode::Char('}')));
    }

    // ══════════════════════════════════════════════════════════════════
    //  Specific binding verification — HEALTH
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn health_shared_has_switch_tab() {
        let b = HEALTH_SHARED
            .iter()
            .find(|b| b.action == Action::HealthSwitchTab)
            .unwrap();
        assert_eq!(b.primary.code, KeyCode::Tab);
    }

    #[test]
    fn health_shared_has_scope_mode() {
        let b = HEALTH_SHARED
            .iter()
            .find(|b| b.action == Action::HealthScopeMode)
            .unwrap();
        assert_eq!(b.primary.code, KeyCode::Char('s'));
    }

    #[test]
    fn health_god_files_modularize_has_m_alt() {
        let b = HEALTH_GOD_FILES
            .iter()
            .find(|b| b.action == Action::HealthModularize)
            .unwrap();
        assert!(b.alternatives.iter().any(|a| a.code == KeyCode::Char('m')));
    }

    // ══════════════════════════════════════════════════════════════════
    //  Specific binding verification — PROJECTS_BROWSE
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn projects_has_quit() {
        assert!(PROJECTS_BROWSE.iter().any(|b| b.action == Action::Quit));
    }

    #[test]
    fn projects_has_add() {
        assert!(PROJECTS_BROWSE
            .iter()
            .any(|b| b.action == Action::ProjectsAdd));
    }

    #[test]
    fn projects_has_delete() {
        assert!(PROJECTS_BROWSE
            .iter()
            .any(|b| b.action == Action::ProjectsDelete));
    }

    #[test]
    fn projects_has_rename() {
        assert!(PROJECTS_BROWSE
            .iter()
            .any(|b| b.action == Action::ProjectsRename));
    }

    #[test]
    fn projects_has_init() {
        assert!(PROJECTS_BROWSE
            .iter()
            .any(|b| b.action == Action::ProjectsInit));
    }

    // ══════════════════════════════════════════════════════════════════
    //  Specific binding verification — PICKER
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn picker_has_edit_selected() {
        assert!(PICKER.iter().any(|b| b.action == Action::EditSelected));
    }

    #[test]
    fn picker_has_delete_selected() {
        assert!(PICKER.iter().any(|b| b.action == Action::DeleteSelected));
    }

    #[test]
    fn picker_has_add_new() {
        assert!(PICKER.iter().any(|b| b.action == Action::ProjectsAdd));
    }

    // ══════════════════════════════════════════════════════════════════
    //  Specific binding verification — BRANCH_DIALOG
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn branch_has_nav_down_with_arrow_alt() {
        let b = BRANCH_DIALOG
            .iter()
            .find(|b| b.action == Action::NavDown)
            .unwrap();
        assert!(b.alternatives.iter().any(|a| a.code == KeyCode::Down));
    }

    #[test]
    fn branch_has_nav_up_with_arrow_alt() {
        let b = BRANCH_DIALOG
            .iter()
            .find(|b| b.action == Action::NavUp)
            .unwrap();
        assert!(b.alternatives.iter().any(|a| a.code == KeyCode::Up));
    }

    #[test]
    fn branch_has_confirm() {
        assert!(BRANCH_DIALOG.iter().any(|b| b.action == Action::Confirm));
    }

    #[test]
    fn branch_has_escape() {
        assert!(BRANCH_DIALOG.iter().any(|b| b.action == Action::Escape));
    }

    // ══════════════════════════════════════════════════════════════════
    //  Static alt arrays
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn alt_down_is_down_arrow() {
        assert_eq!(ALT_DOWN[0].code, KeyCode::Down);
        assert_eq!(ALT_DOWN[0].modifiers, KeyModifiers::NONE);
    }

    #[test]
    fn alt_up_is_up_arrow() {
        assert_eq!(ALT_UP[0].code, KeyCode::Up);
        assert_eq!(ALT_UP[0].modifiers, KeyModifiers::NONE);
    }

    #[test]
    fn alt_left_is_left_arrow() {
        assert_eq!(ALT_LEFT[0].code, KeyCode::Left);
    }

    #[test]
    fn alt_right_is_right_arrow() {
        assert_eq!(ALT_RIGHT[0].code, KeyCode::Right);
    }

    #[test]
    fn alt_ctrl_left_is_ctrl_left() {
        assert_eq!(ALT_CTRL_LEFT[0].modifiers, KeyModifiers::CONTROL);
        assert_eq!(ALT_CTRL_LEFT[0].code, KeyCode::Left);
    }

    #[test]
    fn alt_ctrl_right_is_ctrl_right() {
        assert_eq!(ALT_CTRL_RIGHT[0].modifiers, KeyModifiers::CONTROL);
        assert_eq!(ALT_CTRL_RIGHT[0].code, KeyCode::Right);
    }

    #[test]
    fn alt_delete_word_is_ctrl_backspace() {
        assert_eq!(ALT_DELETE_WORD[0].modifiers, KeyModifiers::CONTROL);
        assert_eq!(ALT_DELETE_WORD[0].code, KeyCode::Backspace);
    }

    #[test]
    fn alt_pgdn_is_pagedown() {
        assert_eq!(ALT_PGDN[0].code, KeyCode::PageDown);
    }

    #[test]
    fn alt_pgup_is_pageup() {
        assert_eq!(ALT_PGUP[0].code, KeyCode::PageUp);
    }

    #[test]
    fn alt_home_is_home() {
        assert_eq!(ALT_HOME[0].code, KeyCode::Home);
    }

    #[test]
    fn alt_end_is_end() {
        assert_eq!(ALT_END[0].code, KeyCode::End);
    }

    #[test]
    fn alt_macos_p_is_pi() {
        assert_eq!(ALT_MACOS_P[0].code, KeyCode::Char('π'));
    }

    #[test]
    fn alt_macos_t_is_dagger() {
        assert_eq!(ALT_MACOS_T[0].code, KeyCode::Char('†'));
    }

    #[test]
    fn alt_char_m_is_m() {
        assert_eq!(ALT_CHAR_M[0].code, KeyCode::Char('m'));
    }

    #[test]
    fn alt_char_d_is_d() {
        assert_eq!(ALT_CHAR_D[0].code, KeyCode::Char('d'));
    }

    #[test]
    fn alt_lbrace_is_lbrace() {
        assert_eq!(ALT_LBRACE[0].code, KeyCode::Char('{'));
    }

    #[test]
    fn alt_rbrace_is_rbrace() {
        assert_eq!(ALT_RBRACE[0].code, KeyCode::Char('}'));
    }

    // ══════════════════════════════════════════════════════════════════
    //  CMD_SHIFT constant (macOS only)
    // ══════════════════════════════════════════════════════════════════

    #[test]
    #[cfg(target_os = "macos")]
    fn cmd_shift_contains_super() {
        assert!(CMD_SHIFT.contains(KeyModifiers::SUPER));
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn cmd_shift_contains_shift() {
        assert!(CMD_SHIFT.contains(KeyModifiers::SHIFT));
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn cmd_shift_does_not_contain_control() {
        assert!(!CMD_SHIFT.contains(KeyModifiers::CONTROL));
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn cmd_shift_does_not_contain_alt() {
        assert!(!CMD_SHIFT.contains(KeyModifiers::ALT));
    }

    // ══════════════════════════════════════════════════════════════════
    //  display_keys for bindings with alternatives (integration)
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn filetree_nav_down_display_keys() {
        let b = FILE_TREE
            .iter()
            .find(|b| b.action == Action::NavDown)
            .unwrap();
        assert_eq!(b.display_keys(), "j/↓");
    }

    #[test]
    fn viewer_page_down_display_keys() {
        let b = VIEWER
            .iter()
            .find(|b| b.action == Action::PageDown)
            .unwrap();
        let dk = b.display_keys();
        assert!(dk.contains("J"), "display_keys should contain J: {}", dk);
        assert!(
            dk.contains("PgDn"),
            "display_keys should contain PgDn: {}",
            dk
        );
    }

    #[test]
    fn input_preset_prompts_display_hides_macos_pi() {
        let b = INPUT
            .iter()
            .find(|b| b.action == Action::PresetPrompts)
            .unwrap();
        let dk = b.display_keys();
        // π (macos alt) should be hidden
        assert!(
            !dk.contains('π'),
            "display_keys should not contain π: {}",
            dk
        );
    }

    // ══════════════════════════════════════════════════════════════════
    //  Matching integration tests
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn global_quit_matches_ctrl_q() {
        assert!(GLOBAL[0].matches(KeyModifiers::CONTROL, KeyCode::Char('q')));
    }

    #[test]
    fn global_quit_does_not_match_plain_q() {
        assert!(!GLOBAL[0].matches(KeyModifiers::NONE, KeyCode::Char('q')));
    }

    #[test]
    fn filetree_nav_down_matches_arrow() {
        let b = FILE_TREE
            .iter()
            .find(|b| b.action == Action::NavDown)
            .unwrap();
        assert!(b.matches(KeyModifiers::NONE, KeyCode::Down));
    }

    #[test]
    fn total_binding_count_across_all_arrays() {
        let total = GLOBAL.len()
            + WORKTREES.len()
            + FILE_TREE.len()
            + VIEWER.len()
            + EDIT_MODE.len()
            + SESSION.len()
            + INPUT.len()
            + TERMINAL.len()
            + HEALTH_SHARED.len()
            + HEALTH_GOD_FILES.len()
            + HEALTH_DOCS.len()
            + GIT_ACTIONS.len()
            + PROJECTS_BROWSE.len()
            + PICKER.len()
            + BRANCH_DIALOG.len();
        // Sanity: we have a non-trivial number of bindings
        assert!(
            total > 100,
            "total bindings should exceed 100, got {}",
            total
        );
    }

    #[test]
    fn all_arrays_produce_valid_combos() {
        // Verify all_combos helper doesn't panic on any array
        let arrays: &[&[Keybinding]] = &[
            &GLOBAL,
            &WORKTREES,
            &FILE_TREE,
            &VIEWER,
            &EDIT_MODE,
            &SESSION,
            &INPUT,
            &TERMINAL,
            &HEALTH_SHARED,
            &HEALTH_GOD_FILES,
            &HEALTH_DOCS,
            &GIT_ACTIONS,
            &PROJECTS_BROWSE,
            &PICKER,
            &BRANCH_DIALOG,
        ];
        for arr in arrays {
            let combos = all_combos(arr);
            // WORKTREES is now empty — skip the non-empty assertion for empty arrays
            if !arr.is_empty() {
                assert!(!combos.is_empty());
            }
        }
    }
}
