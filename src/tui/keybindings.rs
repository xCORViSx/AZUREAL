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
        self.modifiers == modifiers && self.code == code
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
    HalfPageDown,
    HalfPageUp,
    FullPageDown,
    FullPageUp,
    GoToTop,
    GoToBottom,

    // Worktrees
    SelectNextProject,
    SelectPrevProject,
    OpenContextMenu,
    NewWorktree,
    BrowseBranches,
    ViewDiff,
    RunCommand,
    AddRunCommand,
    RebaseOntoMain,
    ArchiveWorktree,
    StartResume,

    // FileTree
    ToggleDir,
    OpenFile,

    // Viewer
    EnterEditMode,
    JumpNextEdit,
    JumpPrevEdit,
    CloseViewer,

    // Viewer Edit Mode
    Save,
    Undo,
    Redo,

    // Output/Convo
    JumpNextBubble,
    JumpPrevBubble,
    JumpNextMessage,
    JumpPrevMessage,
    SwitchToOutput,

    // Input
    Submit,
    InsertNewline,
    ExitPromptMode,
    WordLeft,
    WordRight,
    DeleteWord,
    ClearInput,
    HistoryPrev,
    HistoryNext,

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
}

impl Keybinding {
    pub const fn new(primary: KeyCombo, description: &'static str, action: Action) -> Self {
        Self { primary, alternatives: &[], description, action }
    }

    pub const fn with_alt(
        primary: KeyCombo,
        alternatives: &'static [KeyCombo],
        description: &'static str,
        action: Action,
    ) -> Self {
        Self { primary, alternatives, description, action }
    }

    /// Check if any key combo matches
    #[inline]
    pub fn matches(&self, modifiers: KeyModifiers, code: KeyCode) -> bool {
        self.primary.matches(modifiers, code)
            || self.alternatives.iter().any(|k| k.matches(modifiers, code))
    }

    /// Display string combining primary and alternatives (e.g., "j/↓")
    pub fn display_keys(&self) -> String {
        if self.alternatives.is_empty() {
            self.primary.display()
        } else {
            let mut s = self.primary.display();
            for alt in self.alternatives {
                s.push('/');
                s.push_str(&alt.display());
            }
            s
        }
    }
}

/// Help section for UI display
pub struct HelpSection {
    pub title: &'static str,
    pub bindings: &'static [Keybinding],
}

// Static alternative key arrays for dual-key bindings
static ALT_DOWN: [KeyCombo; 1] = [KeyCombo { modifiers: KeyModifiers::NONE, code: KeyCode::Down }];
static ALT_UP: [KeyCombo; 1] = [KeyCombo { modifiers: KeyModifiers::NONE, code: KeyCode::Up }];
static ALT_LEFT: [KeyCombo; 1] = [KeyCombo { modifiers: KeyModifiers::NONE, code: KeyCode::Left }];
static ALT_RIGHT: [KeyCombo; 1] = [KeyCombo { modifiers: KeyModifiers::NONE, code: KeyCode::Right }];
// ⌃← alternative for ⌥← (word nav in prompt input)
static ALT_CTRL_LEFT: [KeyCombo; 1] = [KeyCombo { modifiers: KeyModifiers::CONTROL, code: KeyCode::Left }];
static ALT_CTRL_RIGHT: [KeyCombo; 1] = [KeyCombo { modifiers: KeyModifiers::CONTROL, code: KeyCode::Right }];
/// ⌃J is a universal fallback for Shift+Enter (terminals that lack Kitty protocol)
static CTRL_J: [KeyCombo; 1] = [KeyCombo { modifiers: KeyModifiers::CONTROL, code: KeyCode::Char('j') }];

// Ctrl+Alt+Cmd modifier combo (for quit/restart/debug)
const CTRL_ALT_CMD: KeyModifiers = KeyModifiers::from_bits_truncate(
    KeyModifiers::CONTROL.bits() | KeyModifiers::ALT.bits() | KeyModifiers::SUPER.bits()
);

// Cmd+Shift modifier combo
const CMD_SHIFT: KeyModifiers = KeyModifiers::from_bits_truncate(
    KeyModifiers::SUPER.bits() | KeyModifiers::SHIFT.bits()
);

/// Global keybindings (always active, checked first)
pub static GLOBAL: [Keybinding; 10] = [
    Keybinding::new(KeyCombo::ctrl(KeyCode::Char('q')), "Quit azureal", Action::Quit),
    Keybinding::new(KeyCombo::ctrl(KeyCode::Char('r')), "Restart azureal", Action::Restart),
    Keybinding::new(KeyCombo::new(CTRL_ALT_CMD, KeyCode::Char('d')), "Dump debug output", Action::DumpDebug),
    Keybinding::new(KeyCombo::ctrl(KeyCode::Char('c')), "Cancel Claude response", Action::CancelClaude),
    Keybinding::new(KeyCombo::cmd(KeyCode::Char('c')), "Copy selection", Action::CopySelection),
    Keybinding::new(KeyCombo::plain(KeyCode::Char('?')), "Toggle help", Action::ToggleHelp),
    Keybinding::new(KeyCombo::plain(KeyCode::Char('p')), "Enter prompt mode", Action::EnterPromptMode),
    Keybinding::new(KeyCombo::plain(KeyCode::Char('t')), "Toggle terminal", Action::ToggleTerminal),
    Keybinding::new(KeyCombo::plain(KeyCode::Tab), "Cycle focus forward", Action::CycleFocusForward),
    Keybinding::new(KeyCombo::shift(KeyCode::BackTab), "Cycle focus backward", Action::CycleFocusBackward),
];

/// Worktrees context bindings
pub static WORKTREES: [Keybinding; 15] = [
    Keybinding::with_alt(KeyCombo::plain(KeyCode::Char('j')), &ALT_DOWN, "Select worktree", Action::NavDown),
    Keybinding::with_alt(KeyCombo::plain(KeyCode::Char('k')), &ALT_UP, "Select worktree", Action::NavUp),
    Keybinding::with_alt(KeyCombo::plain(KeyCode::Char('l')), &ALT_RIGHT, "Expand files", Action::NavRight),
    Keybinding::with_alt(KeyCombo::plain(KeyCode::Char('h')), &ALT_LEFT, "Collapse files", Action::NavLeft),
    Keybinding::new(KeyCombo::shift(KeyCode::Char('J')), "Select project", Action::SelectNextProject),
    Keybinding::new(KeyCombo::shift(KeyCode::Char('K')), "Select project", Action::SelectPrevProject),
    Keybinding::new(KeyCombo::plain(KeyCode::Char(' ')), "Context menu", Action::OpenContextMenu),
    Keybinding::new(KeyCombo::plain(KeyCode::Enter), "Start/resume", Action::StartResume),
    Keybinding::new(KeyCombo::plain(KeyCode::Char('n')), "New...", Action::NewWorktree),
    Keybinding::new(KeyCombo::plain(KeyCode::Char('b')), "Browse branches", Action::BrowseBranches),
    Keybinding::new(KeyCombo::plain(KeyCode::Char('d')), "View diff", Action::ViewDiff),
    Keybinding::new(KeyCombo::plain(KeyCode::Char('r')), "Run command", Action::RunCommand),
    Keybinding::new(KeyCombo::alt(KeyCode::Char('r')), "Add run command", Action::AddRunCommand),
    Keybinding::new(KeyCombo::shift(KeyCode::Char('R')), "Rebase onto main", Action::RebaseOntoMain),
    Keybinding::new(KeyCombo::plain(KeyCode::Char('a')), "Archive worktree", Action::ArchiveWorktree),
];

/// FileTree bindings
pub static FILE_TREE: [Keybinding; 7] = [
    Keybinding::with_alt(KeyCombo::plain(KeyCode::Char('j')), &ALT_DOWN, "Navigate", Action::NavDown),
    Keybinding::with_alt(KeyCombo::plain(KeyCode::Char('k')), &ALT_UP, "Navigate", Action::NavUp),
    Keybinding::with_alt(KeyCombo::plain(KeyCode::Char('h')), &ALT_LEFT, "Collapse", Action::NavLeft),
    Keybinding::with_alt(KeyCombo::plain(KeyCode::Char('l')), &ALT_RIGHT, "Expand", Action::NavRight),
    Keybinding::new(KeyCombo::plain(KeyCode::Enter), "Open/toggle", Action::OpenFile),
    Keybinding::new(KeyCombo::plain(KeyCode::Char(' ')), "Toggle dir", Action::ToggleDir),
    Keybinding::new(KeyCombo::plain(KeyCode::Esc), "Back to Worktrees", Action::Escape),
];

/// Viewer bindings (read-only mode)
pub static VIEWER: [Keybinding; 10] = [
    Keybinding::with_alt(KeyCombo::plain(KeyCode::Char('j')), &ALT_DOWN, "Scroll line", Action::NavDown),
    Keybinding::with_alt(KeyCombo::plain(KeyCode::Char('k')), &ALT_UP, "Scroll line", Action::NavUp),
    Keybinding::new(KeyCombo::shift(KeyCode::Char('J')), "Half page", Action::HalfPageDown),
    Keybinding::new(KeyCombo::shift(KeyCode::Char('K')), "Half page", Action::HalfPageUp),
    Keybinding::new(KeyCombo::plain(KeyCode::Char('g')), "Top", Action::GoToTop),
    Keybinding::new(KeyCombo::shift(KeyCode::Char('G')), "Bottom", Action::GoToBottom),
    Keybinding::new(KeyCombo::plain(KeyCode::Char('f')), "Next Edit", Action::JumpNextEdit),
    Keybinding::new(KeyCombo::plain(KeyCode::Char('b')), "Prev Edit", Action::JumpPrevEdit),
    Keybinding::new(KeyCombo::plain(KeyCode::Char('e')), "Edit file", Action::EnterEditMode),
    Keybinding::new(KeyCombo::plain(KeyCode::Esc), "Close viewer", Action::Escape),
];

/// Edit mode bindings
pub static EDIT_MODE: [Keybinding; 4] = [
    Keybinding::new(KeyCombo::cmd(KeyCode::Char('s')), "Save file", Action::Save),
    Keybinding::new(KeyCombo::cmd(KeyCode::Char('z')), "Undo", Action::Undo),
    Keybinding::new(KeyCombo::new(CMD_SHIFT, KeyCode::Char('Z')), "Redo", Action::Redo),
    Keybinding::new(KeyCombo::plain(KeyCode::Esc), "Exit edit mode", Action::Escape),
];

/// Convo/Output bindings
pub static OUTPUT: [Keybinding; 13] = [
    Keybinding::with_alt(KeyCombo::plain(KeyCode::Char('j')), &ALT_DOWN, "Scroll line", Action::NavDown),
    Keybinding::with_alt(KeyCombo::plain(KeyCode::Char('k')), &ALT_UP, "Scroll line", Action::NavUp),
    Keybinding::new(KeyCombo::plain(KeyCode::Down), "Next prompt", Action::JumpNextBubble),
    Keybinding::new(KeyCombo::plain(KeyCode::Up), "Prev prompt", Action::JumpPrevBubble),
    Keybinding::new(KeyCombo::shift(KeyCode::Down), "Next message", Action::JumpNextMessage),
    Keybinding::new(KeyCombo::shift(KeyCode::Up), "Prev message", Action::JumpPrevMessage),
    Keybinding::new(KeyCombo::shift(KeyCode::Char('J')), "Half page", Action::HalfPageDown),
    Keybinding::new(KeyCombo::shift(KeyCode::Char('K')), "Half page", Action::HalfPageUp),
    Keybinding::new(KeyCombo::plain(KeyCode::Char('f')), "Full page", Action::FullPageDown),
    Keybinding::new(KeyCombo::plain(KeyCode::Char('b')), "Full page", Action::FullPageUp),
    Keybinding::new(KeyCombo::plain(KeyCode::Char('g')), "Top", Action::GoToTop),
    Keybinding::new(KeyCombo::shift(KeyCode::Char('G')), "Bottom", Action::GoToBottom),
    Keybinding::new(KeyCombo::plain(KeyCode::Esc), "Back to Worktrees", Action::Escape),
];

/// Input mode bindings — keys that work in Claude prompt type mode
/// Word nav uses standard macOS shortcuts (⌥← / ⌥→), not ⌃z/⌃x which conflict with clipboard
/// Newline: ⇧Enter (Kitty protocol terminals) or ⌃J (universal fallback)
pub static INPUT: [Keybinding; 9] = [
    Keybinding::new(KeyCombo::plain(KeyCode::Enter), "Submit prompt", Action::Submit),
    Keybinding::with_alt(KeyCombo::shift(KeyCode::Enter), &CTRL_J, "Insert newline", Action::InsertNewline),
    Keybinding::new(KeyCombo::plain(KeyCode::Esc), "Exit to COMMAND", Action::ExitPromptMode),
    Keybinding::with_alt(KeyCombo::alt(KeyCode::Left), &ALT_CTRL_LEFT, "Word left", Action::WordLeft),
    Keybinding::with_alt(KeyCombo::alt(KeyCode::Right), &ALT_CTRL_RIGHT, "Word right", Action::WordRight),
    Keybinding::new(KeyCombo::ctrl(KeyCode::Char('w')), "Delete word", Action::DeleteWord),
    Keybinding::new(KeyCombo::ctrl(KeyCode::Char('u')), "Clear input", Action::ClearInput),
    Keybinding::new(KeyCombo::plain(KeyCode::Up), "History prev", Action::HistoryPrev),
    Keybinding::new(KeyCombo::plain(KeyCode::Down), "History next", Action::HistoryNext),
];

/// Terminal bindings (command mode) — ALL terminal keybindings live here
/// so title bar hints can source from them dynamically
pub static TERMINAL: [Keybinding; 11] = [
    Keybinding::new(KeyCombo::plain(KeyCode::Char('t')), "Enter type mode", Action::EnterTerminalType),
    Keybinding::new(KeyCombo::plain(KeyCode::Char('p')), "Close & prompt", Action::EnterPromptMode),
    Keybinding::new(KeyCombo::plain(KeyCode::Esc), "Close terminal", Action::Escape),
    Keybinding::with_alt(KeyCombo::plain(KeyCode::Char('j')), &ALT_DOWN, "Scroll line", Action::NavDown),
    Keybinding::with_alt(KeyCombo::plain(KeyCode::Char('k')), &ALT_UP, "Scroll line", Action::NavUp),
    Keybinding::new(KeyCombo::shift(KeyCode::Char('J')), "Scroll page", Action::HalfPageDown),
    Keybinding::new(KeyCombo::shift(KeyCode::Char('K')), "Scroll page", Action::HalfPageUp),
    Keybinding::new(KeyCombo::plain(KeyCode::Char('g')), "Scroll to top", Action::GoToTop),
    Keybinding::new(KeyCombo::shift(KeyCode::Char('G')), "Scroll to bottom", Action::GoToBottom),
    Keybinding::new(KeyCombo::plain(KeyCode::Char('+')), "Resize up", Action::ResizeUp),
    Keybinding::new(KeyCombo::plain(KeyCode::Char('-')), "Resize down", Action::ResizeDown),
];

/// Wizard/New dialog bindings
pub static WIZARD: [Keybinding; 3] = [
    Keybinding::new(KeyCombo::plain(KeyCode::Char(']')), "Next tab", Action::WizardNextTab),
    Keybinding::new(KeyCombo::plain(KeyCode::Char('[')), "Prev tab", Action::WizardPrevTab),
    Keybinding::new(KeyCombo::plain(KeyCode::Tab), "Next field", Action::WizardNextField),
];

/// Find matching action for current context
pub fn lookup_action(
    focus: Focus,
    modifiers: KeyModifiers,
    code: KeyCode,
    is_prompt_mode: bool,
    is_edit_mode: bool,
    is_terminal_mode: bool,
) -> Option<Action> {
    // Global bindings checked first (some are context-sensitive)
    for binding in &GLOBAL {
        let skip = match binding.action {
            Action::EnterPromptMode | Action::ToggleTerminal | Action::ToggleHelp
                if is_prompt_mode || is_edit_mode => true,
            Action::CancelClaude if is_prompt_mode => true,
            _ => false,
        };
        if !skip && binding.matches(modifiers, code) {
            return Some(binding.action);
        }
    }

    // Context-specific bindings
    let context_bindings: &[Keybinding] = match focus {
        Focus::Worktrees => &WORKTREES,
        Focus::FileTree => &FILE_TREE,
        Focus::Viewer if is_edit_mode => &EDIT_MODE,
        Focus::Viewer => &VIEWER,
        Focus::Output => &OUTPUT,
        Focus::Input if is_terminal_mode => &TERMINAL,
        Focus::Input if is_prompt_mode => &INPUT,
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
        HelpSection { title: "Global", bindings: &GLOBAL },
        HelpSection { title: "Worktrees", bindings: &WORKTREES },
        HelpSection { title: "Filetree", bindings: &FILE_TREE },
        HelpSection { title: "Viewer", bindings: &VIEWER },
        HelpSection { title: "Edit Mode", bindings: &EDIT_MODE },
        HelpSection { title: "Convo", bindings: &OUTPUT },
    ]
}

/// Generate title hints for prompt input (type mode) — shows ALL input keybindings
pub fn prompt_type_title() -> String {
    let esc = find_key_for_action(&INPUT, Action::ExitPromptMode).unwrap_or("Esc".into());
    let submit = find_key_for_action(&INPUT, Action::Submit).unwrap_or("Enter".into());
    let cancel = find_key_for_action(&GLOBAL, Action::CancelClaude).unwrap_or("⌃c".into());
    let (hprev, hnext) = find_key_pair(&INPUT, Action::HistoryPrev, Action::HistoryNext, "↑", "↓");
    let dw = find_key_for_action(&INPUT, Action::DeleteWord).unwrap_or("⌃w".into());
    let cl = find_key_for_action(&INPUT, Action::ClearInput).unwrap_or("⌃u".into());
    format!(
        " PROMPT ({}:exit | {}:submit | ⇧Enter/⌃j:newline | {}:cancel | {}/{}:history | ⌥←/→:word | {}:del wrd | {}:clear) ",
        esc, submit, cancel, hprev, hnext, dw, cl
    )
}

/// Generate title hints for prompt input (command mode)
pub fn prompt_command_title() -> String {
    let prompt = find_key_for_action(&GLOBAL, Action::EnterPromptMode).unwrap_or("p".into());
    let terminal = find_key_for_action(&GLOBAL, Action::ToggleTerminal).unwrap_or("t".into());
    format!(" PROMPT ({}:type | {}:terminal) ", prompt, terminal)
}

/// Generate title hints for terminal (type mode) — all keys forward to PTY except Esc
pub fn terminal_type_title() -> String {
    let esc = find_key_for_action(&TERMINAL, Action::Escape).unwrap_or("Esc".into());
    format!(" TERMINAL ({}:exit) ", esc)
}

/// Generate title hints for terminal (command mode) — shows ALL keybindings so help panel can omit them
pub fn terminal_command_title() -> String {
    let t = find_key_for_action(&TERMINAL, Action::EnterTerminalType).unwrap_or("t".into());
    let p = find_key_for_action(&TERMINAL, Action::EnterPromptMode).unwrap_or("p".into());
    let esc = find_key_for_action(&TERMINAL, Action::Escape).unwrap_or("Esc".into());
    let (down, up) = find_key_pair(&TERMINAL, Action::NavDown, Action::NavUp, "j", "k");
    let (pdn, pup) = find_key_pair(&TERMINAL, Action::HalfPageDown, Action::HalfPageUp, "J", "K");
    let (top, bot) = find_key_pair(&TERMINAL, Action::GoToTop, Action::GoToBottom, "g", "G");
    let (rup, rdn) = find_key_pair(&TERMINAL, Action::ResizeUp, Action::ResizeDown, "+", "-");
    format!(
        " TERMINAL ({}:type | {}:prompt | {}:close | {}/{}:scroll | {}/{}:page | {}/{}:top/bottom | {}/{}:resize) ",
        t, p, esc, down, up, pdn, pup, top, bot, rup, rdn
    )
}

/// Generate title hints for terminal (scrolled) — shows scroll position + relevant keys
pub fn terminal_scroll_title(scroll: usize) -> String {
    let (down, up) = find_key_pair(&TERMINAL, Action::NavDown, Action::NavUp, "j", "k");
    let (pdn, pup) = find_key_pair(&TERMINAL, Action::HalfPageDown, Action::HalfPageUp, "J", "K");
    let top = find_key_for_action(&TERMINAL, Action::GoToTop).unwrap_or("g".into());
    let bot = find_key_for_action(&TERMINAL, Action::GoToBottom).unwrap_or("G".into());
    let t = find_key_for_action(&TERMINAL, Action::EnterTerminalType).unwrap_or("t".into());
    let esc = find_key_for_action(&TERMINAL, Action::Escape).unwrap_or("Esc".into());
    format!(
        " TERMINAL [{}↑] ({}/{}:scroll | {}/{}:page | {}:top | {}:bottom | {}:type | {}:close) ",
        scroll, down, up, pdn, pup, top, bot, t, esc
    )
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

/// Find the display key for a given action in a binding list
fn find_key_for_action(bindings: &[Keybinding], action: Action) -> Option<String> {
    bindings.iter()
        .find(|b| b.action == action)
        .map(|b| b.primary.display())
}

/// Find a pair of keys for two related actions (e.g., NavDown/NavUp → "j"/"k")
fn find_key_pair(bindings: &[Keybinding], a: Action, b: Action, da: &str, db: &str) -> (String, String) {
    (
        find_key_for_action(bindings, a).unwrap_or_else(|| da.into()),
        find_key_for_action(bindings, b).unwrap_or_else(|| db.into()),
    )
}

/// Quick matcher for common navigation (hot path optimization)
#[inline]
pub fn is_nav_down(modifiers: KeyModifiers, code: KeyCode) -> bool {
    modifiers == KeyModifiers::NONE && (code == KeyCode::Char('j') || code == KeyCode::Down)
}

#[inline]
pub fn is_nav_up(modifiers: KeyModifiers, code: KeyCode) -> bool {
    modifiers == KeyModifiers::NONE && (code == KeyCode::Char('k') || code == KeyCode::Up)
}

#[inline]
pub fn is_nav_left(modifiers: KeyModifiers, code: KeyCode) -> bool {
    modifiers == KeyModifiers::NONE && (code == KeyCode::Char('h') || code == KeyCode::Left)
}

#[inline]
pub fn is_nav_right(modifiers: KeyModifiers, code: KeyCode) -> bool {
    modifiers == KeyModifiers::NONE && (code == KeyCode::Char('l') || code == KeyCode::Right)
}
