//! Modal panel binding arrays
//!
//! Static keybinding arrays for modal overlays: health panel (shared + per-tab),
//! git actions panel, projects browser, picker, and branch dialog.

use super::keys::*;
use super::super::types::{Action, KeyCombo, Keybinding};
use crossterm::event::KeyCode;

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

/// Issues Panel — browse and filter GitHub issues
pub static ISSUES_BROWSE: [Keybinding; 10] = [
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
        KeyCombo::plain(KeyCode::Char('c')),
        "Create issue",
        Action::IssuesCreate,
    ),
    Keybinding::new(
        KeyCombo::plain(KeyCode::Enter),
        "Open in browser",
        Action::Confirm,
    ),
    Keybinding::new(KeyCombo::plain(KeyCode::Esc), "Close", Action::Escape),
    Keybinding::new(KeyCombo::ctrl(KeyCode::Char('q')), "Quit", Action::Quit),
];
