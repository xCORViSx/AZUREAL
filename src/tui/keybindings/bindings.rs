//! Static keybinding arrays
//!
//! Every context's binding table lives here as a `pub static` array.
//! Modal panels, non-modal contexts, and global shortcuts each get their own
//! array so lookup functions can iterate the right set.

use crossterm::event::{KeyCode, KeyModifiers};
use super::types::{KeyCombo, Keybinding, Action};

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
// Shift+[ → '{' — some terminals send (SHIFT, '{'), others (NONE, '{')
static ALT_LBRACE: [KeyCombo; 1] = [KeyCombo { modifiers: KeyModifiers::NONE, code: KeyCode::Char('{') }];
static ALT_RBRACE: [KeyCombo; 1] = [KeyCombo { modifiers: KeyModifiers::NONE, code: KeyCode::Char('}') }];

// Cmd+Shift modifier combo
const CMD_SHIFT: KeyModifiers = KeyModifiers::from_bits_truncate(
    KeyModifiers::SUPER.bits() | KeyModifiers::SHIFT.bits()
);

/// Global keybindings (always active, checked first)
pub static GLOBAL: [Keybinding; 17] = [
    Keybinding::new(KeyCombo::ctrl(KeyCode::Char('q')), "Quit azureal", Action::Quit),
    Keybinding::new(KeyCombo::ctrl(KeyCode::Char('d')), "Dump debug output", Action::DumpDebug),
    Keybinding::new(KeyCombo::ctrl(KeyCode::Char('c')), "Cancel agent", Action::CancelClaude),
    Keybinding::new(KeyCombo::cmd(KeyCode::Char('c')), "Copy selection", Action::CopySelection),
    Keybinding::new(KeyCombo::ctrl(KeyCode::Char('m')), "Cycle model", Action::CycleModel),
    Keybinding::new(KeyCombo::plain(KeyCode::Char('?')), "Toggle help", Action::ToggleHelp),
    Keybinding::new(KeyCombo::plain(KeyCode::Char('p')), "Enter prompt mode", Action::EnterPromptMode),
    Keybinding::new(KeyCombo::shift(KeyCode::Char('T')), "Toggle terminal", Action::ToggleTerminal),
    Keybinding::new(KeyCombo::shift(KeyCode::Char('G')), "Git actions", Action::OpenGitActions),
    Keybinding::new(KeyCombo::shift(KeyCode::Char('H')), "Worktree health", Action::OpenHealth),
    Keybinding::new(KeyCombo::shift(KeyCode::Char('M')), "Browse main", Action::BrowseMain),
    Keybinding::new(KeyCombo::shift(KeyCode::Char('P')), "Projects", Action::OpenProjects),
    Keybinding::new(KeyCombo::cmd(KeyCode::Char('r')), "Run command", Action::RunCommand),
    Keybinding::new(KeyCombo::plain(KeyCode::Char(']')), "Next worktree", Action::WorktreeTabNext).paired(),
    Keybinding::new(KeyCombo::plain(KeyCode::Char('[')), "Prev worktree", Action::WorktreeTabPrev),
    Keybinding::with_alt(KeyCombo::shift(KeyCode::Char('}')), &ALT_RBRACE, "Cycle focus forward", Action::CycleFocusForward),
    Keybinding::with_alt(KeyCombo::shift(KeyCode::Char('{')), &ALT_LBRACE, "Cycle focus backward", Action::CycleFocusBackward),
];

/// Worktree tab row bindings — actions available when tab row is focused
pub static WORKTREES: [Keybinding; 5] = [
    Keybinding::new(KeyCombo::plain(KeyCode::Char('a')), "Add worktree", Action::AddWorktree),
    Keybinding::new(KeyCombo::plain(KeyCode::Char('b')), "Browse branches", Action::BrowseBranches),
    Keybinding::with_alt(KeyCombo::alt(KeyCode::Char('r')), &ALT_MACOS_R, "Add run command", Action::AddRunCommand),
    Keybinding::new(KeyCombo::cmd(KeyCode::Char('a')), "Toggle archive", Action::ToggleArchiveWorktree),
    Keybinding::new(KeyCombo::cmd(KeyCode::Char('d')), "Delete worktree", Action::DeleteWorktree),
];

/// FileTree bindings
pub static FILE_TREE: [Keybinding; 16] = [
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
    Keybinding::new(KeyCombo::shift(KeyCode::Char('O')), "Options", Action::FileTreeOptions),
    Keybinding::new(KeyCombo::plain(KeyCode::Esc), "Back to Worktrees", Action::Escape),
];

/// Viewer bindings (read-only mode)
pub static VIEWER: [Keybinding; 14] = [
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
pub static SESSION: [Keybinding; 14] = [
    Keybinding::new(KeyCombo::plain(KeyCode::Char('a')), "Add session", Action::NewSession),
    Keybinding::new(KeyCombo::plain(KeyCode::Char('s')), "Session list", Action::ToggleSessionList),
    Keybinding::new(KeyCombo::plain(KeyCode::Char('/')), "Search", Action::SearchSession),
    Keybinding::new(KeyCombo::plain(KeyCode::Char('j')), "Scroll line", Action::NavDown).paired(),
    Keybinding::new(KeyCombo::plain(KeyCode::Char('k')), "Scroll line", Action::NavUp),
    Keybinding::new(KeyCombo::plain(KeyCode::Down), "Next message", Action::JumpNextBubble).paired(),
    Keybinding::new(KeyCombo::plain(KeyCode::Up), "Prev message", Action::JumpPrevBubble),
    Keybinding::new(KeyCombo::shift(KeyCode::Down), "Next prompt", Action::JumpNextMessage).paired(),
    Keybinding::new(KeyCombo::shift(KeyCode::Up), "Prev prompt", Action::JumpPrevMessage),
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

// ─── Modal panel binding arrays ───────────────────────────────────────────────

/// Health Panel — bindings shared across both tabs (Tab, nav, Esc)
pub static HEALTH_SHARED: [Keybinding; 9] = [
    Keybinding::new(KeyCombo::plain(KeyCode::Tab), "Switch tab", Action::HealthSwitchTab),
    Keybinding::with_alt(KeyCombo::plain(KeyCode::Char('j')), &ALT_DOWN, "Navigate", Action::NavDown).paired(),
    Keybinding::with_alt(KeyCombo::plain(KeyCode::Char('k')), &ALT_UP, "Navigate", Action::NavUp),
    Keybinding::with_alt(KeyCombo::shift(KeyCode::Char('J')), &ALT_PGDN, "Page down", Action::PageDown).paired(),
    Keybinding::with_alt(KeyCombo::shift(KeyCode::Char('K')), &ALT_PGUP, "Page up", Action::PageUp),
    Keybinding::new(KeyCombo::alt(KeyCode::Up), "Jump to top", Action::GoToTop).paired(),
    Keybinding::new(KeyCombo::alt(KeyCode::Down), "Jump to bottom", Action::GoToBottom),
    Keybinding::new(KeyCombo::plain(KeyCode::Char('s')), "Scope mode", Action::HealthScopeMode),
    Keybinding::new(KeyCombo::plain(KeyCode::Esc), "Close", Action::Escape),
];

/// Health Panel — God Files tab actions (Space/a/v/Enter/m)
pub static HEALTH_GOD_FILES: [Keybinding; 4] = [
    Keybinding::new(KeyCombo::plain(KeyCode::Char(' ')), "Toggle check", Action::HealthToggleCheck),
    Keybinding::new(KeyCombo::plain(KeyCode::Char('a')), "Toggle all", Action::HealthToggleAll),
    Keybinding::new(KeyCombo::plain(KeyCode::Char('v')), "View checked", Action::HealthViewChecked),
    Keybinding::with_alt(KeyCombo::plain(KeyCode::Enter), &ALT_CHAR_M, "Modularize", Action::HealthModularize),
];

/// Health Panel — Documentation tab actions.
/// Space checks, `a` toggles all non-100%, `v` views in Viewer, Enter spawns [DH] sessions.
pub static HEALTH_DOCS: [Keybinding; 4] = [
    Keybinding::new(KeyCombo::plain(KeyCode::Char(' ')), "Toggle check", Action::HealthDocToggleCheck),
    Keybinding::new(KeyCombo::plain(KeyCode::Char('a')), "Check non-100%", Action::HealthDocToggleNon100),
    Keybinding::new(KeyCombo::plain(KeyCode::Char('v')), "View checked", Action::HealthViewChecked),
    Keybinding::new(KeyCombo::plain(KeyCode::Enter), "Complete checked", Action::HealthDocSpawn),
];

/// Git Actions Panel — all keys for the git modal overlay.
/// Actions are context-aware: main branch shows pull+commit+push,
/// feature branches show squash-merge+commit+push. Guards in
/// lookup_git_actions_action() enforce this based on is_on_main + actions_focused.
pub static GIT_ACTIONS: [Keybinding; 20] = [
    Keybinding::new(KeyCombo::plain(KeyCode::Esc), "Close", Action::Escape),
    Keybinding::new(KeyCombo::plain(KeyCode::Tab), "Switch focus", Action::GitToggleFocus),
    Keybinding::with_alt(KeyCombo::plain(KeyCode::Char('j')), &ALT_DOWN, "Navigate", Action::NavDown).paired(),
    Keybinding::with_alt(KeyCombo::plain(KeyCode::Char('k')), &ALT_UP, "Navigate", Action::NavUp),
    Keybinding::new(KeyCombo::alt(KeyCode::Up), "Jump to top", Action::GoToTop).paired(),
    Keybinding::new(KeyCombo::alt(KeyCode::Down), "Jump to bottom", Action::GoToBottom),
    Keybinding::new(KeyCombo::plain(KeyCode::Char('m')), "Squash merge to main", Action::GitSquashMerge),
    Keybinding::new(KeyCombo::plain(KeyCode::Char('r')), "Rebase onto main", Action::GitRebase),
    Keybinding::new(KeyCombo::plain(KeyCode::Char('l')), "Pull", Action::GitPull),
    Keybinding::new(KeyCombo::plain(KeyCode::Char('c')), "Commit", Action::GitCommit),
    Keybinding::new(KeyCombo::shift(KeyCode::Char('P')), "Push to remote", Action::GitPush),
    Keybinding::with_alt(KeyCombo::plain(KeyCode::Enter), &ALT_CHAR_D, "Exec/view diff", Action::Confirm),
    Keybinding::new(KeyCombo::shift(KeyCode::Char('R')), "Refresh", Action::GitRefresh),
    Keybinding::new(KeyCombo::plain(KeyCode::Char('d')), "View diff", Action::GitViewDiff),
    Keybinding::new(KeyCombo::plain(KeyCode::Char('a')), "Auto-rebase", Action::GitAutoRebase),
    Keybinding::new(KeyCombo::plain(KeyCode::Char('s')), "Auto-resolve files", Action::GitAutoResolveSettings),
    Keybinding::new(KeyCombo::plain(KeyCode::Char('[')), "Prev worktree", Action::GitPrevWorktree).paired(),
    Keybinding::new(KeyCombo::plain(KeyCode::Char(']')), "Next worktree", Action::GitNextWorktree),
    Keybinding::with_alt(KeyCombo::shift(KeyCode::Char('{')), &ALT_LBRACE, "Prev page", Action::GitPrevPage).paired(),
    Keybinding::with_alt(KeyCombo::shift(KeyCode::Char('}')), &ALT_RBRACE, "Next page", Action::GitNextPage),
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

/// Branch Dialog — nav + select (filter chars stay raw)
pub static BRANCH_DIALOG: [Keybinding; 4] = [
    Keybinding::with_alt(KeyCombo::plain(KeyCode::Char('j')), &ALT_DOWN, "Navigate", Action::NavDown).paired(),
    Keybinding::with_alt(KeyCombo::plain(KeyCode::Char('k')), &ALT_UP, "Navigate", Action::NavUp),
    Keybinding::new(KeyCombo::plain(KeyCode::Enter), "Select", Action::Confirm),
    Keybinding::new(KeyCombo::plain(KeyCode::Esc), "Close", Action::Escape),
];

