//! Core pane binding arrays
//!
//! Static keybinding arrays for the main TUI panes: global shortcuts,
//! worktree leader keys, file tree, viewer, edit mode, session, input, terminal.

use super::keys::*;
use super::super::types::{Action, KeyCombo, Keybinding};
use crossterm::event::{KeyCode, KeyModifiers};

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
    #[cfg(target_os = "macos")]
    Keybinding::with_alt_kitty(KEY_COPY, &ALT_COPY, "Copy selection", Action::CopySelection),
    #[cfg(not(target_os = "macos"))]
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
        KeyCombo::plain(KeyCode::Char('n')),
        "New worktree",
        Action::AddWorktree,
    ),
    Keybinding::new(
        KeyCombo::plain(KeyCode::Char('r')),
        "Rename worktree",
        Action::RenameWorktree,
    ),
    Keybinding::new(
        KeyCombo::plain(KeyCode::Char('a')),
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
    #[cfg(target_os = "macos")]
    Keybinding::with_alt_kitty(KEY_SELECT_ALL, &ALT_SELECT_ALL, "Select all", Action::SelectAll),
    #[cfg(not(target_os = "macos"))]
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
    #[cfg(target_os = "macos")]
    Keybinding::with_alt_kitty(KEY_SAVE, &ALT_SAVE, "Save file", Action::Save),
    #[cfg(not(target_os = "macos"))]
    Keybinding::new(KEY_SAVE, "Save file", Action::Save),
    #[cfg(target_os = "macos")]
    Keybinding::with_alt_kitty(KEY_UNDO, &ALT_UNDO, "Undo", Action::Undo).paired(),
    #[cfg(not(target_os = "macos"))]
    Keybinding::new(KEY_UNDO, "Undo", Action::Undo).paired(),
    #[cfg(target_os = "macos")]
    Keybinding::with_alt_kitty(KEY_REDO, &ALT_REDO, "Redo", Action::Redo),
    #[cfg(not(target_os = "macos"))]
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
    Keybinding::with_alt(
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
