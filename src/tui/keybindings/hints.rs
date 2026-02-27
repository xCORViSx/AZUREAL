//! UI hint generators
//!
//! Functions that produce display strings for title bars, footers, and help
//! overlays by reading key labels from the binding arrays. Draw functions call
//! these instead of hardcoding hint strings.

use super::types::{Action, HelpSection, Keybinding};
use super::bindings::*;

/// Generate help sections from binding definitions
/// Note: Terminal and Input bindings are shown in their own title bars, not here
pub fn help_sections() -> Vec<HelpSection> {
    vec![
        HelpSection { title: "Worktrees", bindings: &WORKTREES },
        HelpSection { title: "Filetree (f)", bindings: &FILE_TREE },
        HelpSection { title: "Viewer", bindings: &VIEWER },
        HelpSection { title: "Edit Mode", bindings: &EDIT_MODE },
        HelpSection { title: "Session", bindings: &SESSION },
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
    let azureal = find_key_for_action(&GLOBAL, Action::OpenAzurealPanel).unwrap_or("⌃a".into());
    let hints = format!(
        "{}:PROMPT | {}:TERMINAL | {}:Git | {}:Health | {}:AZUREAL++ | {}:help | {}/{}:focus | {}:cancel agent | {}:quit | {}:restart",
        p, t, g, h, azureal, help, tab, stab, cancel, quit, restart
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

/// Health panel footer for God Files tab
pub fn health_god_files_hints() -> String {
    let check = find_key_for_action(&HEALTH_GOD_FILES, Action::HealthToggleCheck).unwrap_or("Space".into());
    let all = find_key_for_action(&HEALTH_GOD_FILES, Action::HealthToggleAll).unwrap_or("a".into());
    let view = find_key_for_action(&HEALTH_GOD_FILES, Action::HealthViewChecked).unwrap_or("v".into());
    let modularize = find_key_for_action(&HEALTH_GOD_FILES, Action::HealthModularize).unwrap_or("Enter".into());
    let tab = find_key_for_action(&HEALTH_SHARED, Action::HealthSwitchTab).unwrap_or("Tab".into());
    let esc = find_key_for_action(&HEALTH_SHARED, Action::Escape).unwrap_or("Esc".into());
    format!(" {}:check  {}:all  {}:view  {}/m:modularize  {}:switch  {}:close ",
        check, all, view, modularize, tab, esc)
}

/// Health panel footer for Documentation tab
pub fn health_docs_hints() -> String {
    let check = find_key_for_action(&HEALTH_DOCS, Action::HealthDocToggleCheck).unwrap_or("Space".into());
    let all = find_key_for_action(&HEALTH_DOCS, Action::HealthDocToggleNon100).unwrap_or("a".into());
    let view = find_key_for_action(&HEALTH_DOCS, Action::HealthViewChecked).unwrap_or("v".into());
    let complete = find_key_for_action(&HEALTH_DOCS, Action::HealthDocSpawn).unwrap_or("Enter".into());
    let tab = find_key_for_action(&HEALTH_SHARED, Action::HealthSwitchTab).unwrap_or("Tab".into());
    let esc = find_key_for_action(&HEALTH_SHARED, Action::Escape).unwrap_or("Esc".into());
    format!(" {}:check  {}:non-100%  {}:view  {}:complete  {}:switch  {}:close ",
        check, all, view, complete, tab, esc)
}

/// Git Actions panel — action key+description pairs for the action list labels.
/// Context-aware: main branch shows pull+commit+push, feature shows squash-merge+commit+push.
pub fn git_actions_labels(is_on_main: bool) -> Vec<(String, &'static str)> {
    let actions: &[Action] = if is_on_main {
        &[Action::GitPull, Action::GitCommit, Action::GitPush]
    } else {
        &[Action::GitSquashMerge, Action::GitRebase, Action::GitCommit, Action::GitPush]
    };
    actions.iter()
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
    let (prev, next) = find_key_pair(&GIT_ACTIONS, Action::GitPrevWorktree, Action::GitNextWorktree, "⇧←", "⇧→");
    format!("{}:cycle panes | {}:exec/view | {}:refresh | {}/{}:wt | {}:close", tab, enter, refresh, prev, next, esc)
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
