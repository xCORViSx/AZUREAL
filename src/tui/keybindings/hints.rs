//! UI hint generators
//!
//! Functions that produce display strings for title bars, footers, and help
//! overlays by reading key labels from the binding arrays. Draw functions call
//! these instead of hardcoding hint strings.

use super::bindings::*;
use super::types::{Action, HelpSection, Keybinding};
use crossterm::event::{KeyCode, KeyModifiers};

/// Platform-appropriate modifier prefix for Ctrl
fn plat_ctrl(key: &str) -> String {
    if cfg!(target_os = "macos") {
        format!("⌃{}", key)
    } else {
        format!("Ctrl+{}", key)
    }
}

/// Platform-appropriate modifier prefix for Alt
fn plat_alt(key: &str) -> String {
    if cfg!(target_os = "macos") {
        format!("⌥{}", key)
    } else {
        format!("Alt+{}", key)
    }
}

/// Platform-appropriate modifier prefix for Shift
fn plat_shift(key: &str) -> String {
    if cfg!(target_os = "macos") {
        format!("⇧{}", key)
    } else {
        format!("Shift+{}", key)
    }
}

/// Generate help sections from binding definitions
/// Note: Terminal and Input bindings are shown in their own title bars, not here
pub fn help_sections() -> Vec<HelpSection> {
    vec![
        HelpSection {
            title: "GLOBAL",
            bindings: &GLOBAL,
        },
        HelpSection {
            title: "WORKTREE (W)",
            bindings: &WORKTREES,
        },
        HelpSection {
            title: "Filetree",
            bindings: &FILE_TREE,
        },
        HelpSection {
            title: "Viewer",
            bindings: &VIEWER,
        },
        HelpSection {
            title: "Edit Mode",
            bindings: &EDIT_MODE,
        },
        HelpSection {
            title: "Session",
            bindings: &SESSION,
        },
    ]
}

/// Title + hints for prompt input (type mode).
/// Returns (short_label, full_title_with_hints, just_the_hints).
/// Callers use full title if it fits, otherwise short label in border + hints as inner row.
pub fn prompt_type_title(kbd_enhanced: bool, alt_enter_stolen: bool) -> (String, String, String) {
    let esc = find_key_for_action(&INPUT, Action::ExitPromptMode).unwrap_or("Esc".into());
    let submit = find_key_for_action(&INPUT, Action::Submit).unwrap_or("Enter".into());
    let cancel = find_key_for_action(&GLOBAL, Action::CancelClaude).unwrap_or(plat_ctrl("c"));
    let (hprev, hnext) = find_key_pair(&INPUT, Action::HistoryPrev, Action::HistoryNext, "↑", "↓");
    let dw = find_key_for_action(&INPUT, Action::DeleteWord).unwrap_or(plat_ctrl("w"));
    let stt = find_key_for_action(&INPUT, Action::ToggleStt).unwrap_or(plat_ctrl("s"));
    let presets = find_key_for_action(&INPUT, Action::PresetPrompts).unwrap_or(plat_alt("p"));
    // Without Kitty protocol, Shift+Enter is indistinguishable from Enter —
    // show the Alt+Enter fallback instead so users know what actually works.
    let newline_key = find_key_adaptive(&INPUT, Action::InsertNewline, kbd_enhanced, alt_enter_stolen)
        .unwrap_or_else(|| plat_shift("Enter"));
    let alt_arrows = if cfg!(target_os = "macos") {
        "⌥←/→:word".to_string()
    } else {
        "Alt+ ← / → :word".to_string()
    };
    let hints = format!(
        "{}:exit | {}:submit | {}:newline | {}:cancel agent | {}/{}:history | {} | {}:del wrd | {}:speech | {}:presets",
        esc, submit, newline_key, cancel, hprev, hnext, alt_arrows, dw, stt, presets
    );
    let label = " PROMPT ".to_string();
    let full = format!(" PROMPT ({}) ", hints);
    (label, full, hints)
}

/// Title + hints for command mode — essential mode switches + help shortcut.
/// Returns (short_label, full_title_with_hints, just_the_hints).
pub fn prompt_command_title() -> (String, String, String) {
    let p = find_key_for_action(&GLOBAL, Action::EnterPromptMode).unwrap_or("p".into());
    let t = find_key_for_action(&GLOBAL, Action::ToggleTerminal).unwrap_or("T".into());
    let cancel = find_key_for_action(&GLOBAL, Action::CancelClaude).unwrap_or(plat_ctrl("c"));
    let quit = find_key_for_action(&GLOBAL, Action::Quit).unwrap_or(plat_ctrl("q"));
    let help = find_key_for_action(&GLOBAL, Action::ToggleHelp).unwrap_or("?".into());
    let g = find_key_for_action(&GLOBAL, Action::OpenGitActions).unwrap_or("G".into());
    let h = find_key_for_action(&GLOBAL, Action::OpenHealth).unwrap_or("H".into());
    let main = find_key_for_action(&GLOBAL, Action::BrowseMain).unwrap_or("M".into());
    let run = find_key_for_action(&GLOBAL, Action::RunCommand).unwrap_or("r".into());
    let hints = format!(
        "{}:PROMPT | {}:TERMINAL | {}:Git | {}:Health | {}:main | {}:run | {}:cancel | {}:quit | {}:help",
        p, t, g, h, main, run, cancel, quit, help
    );
    let label = " COMMAND ".to_string();
    let full = format!(" COMMAND ({}) ", hints);
    (label, full, hints)
}

/// Title + hints for terminal type mode.
/// Returns (short_label, full_title, hints).
pub fn terminal_type_title() -> (String, String, String) {
    let esc = find_key_for_action(&TERMINAL, Action::Escape).unwrap_or("Esc".into());
    let word = if cfg!(target_os = "macos") {
        "⌥←/→:word"
    } else {
        "Alt+←/→:word"
    };
    let hints = format!("{}:exit | {}", esc, word);
    (
        " TERMINAL ".to_string(),
        format!(" TERMINAL ({}) ", hints),
        hints,
    )
}

/// Title + hints for terminal command mode.
/// Returns (short_label, full_title, hints).
pub fn terminal_command_title() -> (String, String, String) {
    let t = find_key_for_action(&TERMINAL, Action::EnterTerminalType).unwrap_or("t".into());
    let p = find_key_for_action(&TERMINAL, Action::EnterPromptMode).unwrap_or("p".into());
    let esc = find_key_for_action(&TERMINAL, Action::Escape).unwrap_or("Esc".into());
    let (down, up) = find_key_pair(&TERMINAL, Action::NavDown, Action::NavUp, "j", "k");
    let (pdn, pup) = find_key_pair(&TERMINAL, Action::PageDown, Action::PageUp, "J", "K");
    let alt_up = if cfg!(target_os = "macos") {
        "⌥↑"
    } else {
        "Alt+↑"
    };
    let alt_dn = if cfg!(target_os = "macos") {
        "⌥↓"
    } else {
        "Alt+↓"
    };
    let (top, bot) = find_key_pair(
        &TERMINAL,
        Action::GoToTop,
        Action::GoToBottom,
        alt_up,
        alt_dn,
    );
    let (rup, rdn) = find_key_pair(&TERMINAL, Action::ResizeUp, Action::ResizeDown, "+", "-");
    let hints = format!(
        "{}:type | {}:PROMPT | {}:close | {}/{}:scroll | {}/{}:page | {}/{}:top/bottom | {}/{}:resize",
        t, p, esc, down, up, pdn, pup, top, bot, rup, rdn
    );
    (
        " TERMINAL ".to_string(),
        format!(" TERMINAL ({}) ", hints),
        hints,
    )
}

/// Title + hints for terminal scrolled mode.
/// Returns (short_label, full_title, hints).
pub fn terminal_scroll_title(scroll: usize) -> (String, String, String) {
    let (down, up) = find_key_pair(&TERMINAL, Action::NavDown, Action::NavUp, "j", "k");
    let (pdn, pup) = find_key_pair(&TERMINAL, Action::PageDown, Action::PageUp, "J", "K");
    let top = find_key_for_action(&TERMINAL, Action::GoToTop).unwrap_or(plat_alt("↑"));
    let bot = find_key_for_action(&TERMINAL, Action::GoToBottom).unwrap_or(plat_alt("↓"));
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
    let check =
        find_key_for_action(&HEALTH_GOD_FILES, Action::HealthToggleCheck).unwrap_or("Space".into());
    let all = find_key_for_action(&HEALTH_GOD_FILES, Action::HealthToggleAll).unwrap_or("a".into());
    let view =
        find_key_for_action(&HEALTH_GOD_FILES, Action::HealthViewChecked).unwrap_or("v".into());
    let modularize =
        find_key_for_action(&HEALTH_GOD_FILES, Action::HealthModularize).unwrap_or("Enter".into());
    let tab = find_key_for_action(&HEALTH_SHARED, Action::HealthSwitchTab).unwrap_or("Tab".into());
    let esc = find_key_for_action(&HEALTH_SHARED, Action::Escape).unwrap_or("Esc".into());
    format!(
        " {}:check  {}:all  {}:view  {}/m:modularize  {}:switch  {}:close ",
        check, all, view, modularize, tab, esc
    )
}

/// Health panel footer for Documentation tab
pub fn health_docs_hints() -> String {
    let check =
        find_key_for_action(&HEALTH_DOCS, Action::HealthDocToggleCheck).unwrap_or("Space".into());
    let all =
        find_key_for_action(&HEALTH_DOCS, Action::HealthDocToggleNon100).unwrap_or("a".into());
    let view = find_key_for_action(&HEALTH_DOCS, Action::HealthViewChecked).unwrap_or("v".into());
    let complete =
        find_key_for_action(&HEALTH_DOCS, Action::HealthDocSpawn).unwrap_or("Enter".into());
    let tab = find_key_for_action(&HEALTH_SHARED, Action::HealthSwitchTab).unwrap_or("Tab".into());
    let esc = find_key_for_action(&HEALTH_SHARED, Action::Escape).unwrap_or("Esc".into());
    format!(
        " {}:check  {}:non-100%  {}:view  {}:complete  {}:switch  {}:close ",
        check, all, view, complete, tab, esc
    )
}

/// Git Actions panel — action key+description pairs for the action list labels.
/// Context-aware: main branch shows pull+commit+push, feature shows squash-merge+commit+push.
pub fn git_actions_labels(is_on_main: bool) -> Vec<(String, &'static str)> {
    let actions: &[Action] = if is_on_main {
        &[
            Action::GitPull,
            Action::GitCommit,
            Action::GitPush,
            Action::GitStash,
            Action::GitStashPop,
        ]
    } else {
        &[
            Action::GitSquashMerge,
            Action::GitRebase,
            Action::GitCommit,
            Action::GitPush,
            Action::GitStash,
            Action::GitStashPop,
        ]
    };
    actions
        .iter()
        .filter_map(|&a| {
            GIT_ACTIONS
                .iter()
                .find(|b| b.action == a)
                .map(|b| (b.primary.display(), b.description))
        })
        .collect()
}

/// Git Actions panel footer hints
pub fn git_actions_footer() -> String {
    let (tab_fwd, tab_back) = find_key_pair(
        &GIT_ACTIONS,
        Action::GitToggleFocus,
        Action::GitToggleFocusBack,
        "Tab",
        if cfg!(target_os = "macos") { "⇧Tab" } else { "Shift+Tab" },
    );
    let enter = find_key_for_action(&GIT_ACTIONS, Action::Confirm).unwrap_or("Enter".into());
    let refresh = find_key_for_action(&GIT_ACTIONS, Action::GitRefresh).unwrap_or("R".into());
    let esc = find_key_for_action(&GIT_ACTIONS, Action::Escape).unwrap_or("Esc".into());
    let (prev, next) = find_key_pair(
        &GIT_ACTIONS,
        Action::GitPrevWorktree,
        Action::GitNextWorktree,
        "[",
        "]",
    );
    let (pprev, pnext) = find_key_pair(
        &GIT_ACTIONS,
        Action::GitPrevPage,
        Action::GitNextPage,
        "{",
        "}",
    );
    format!("{}/{}:cycle | {}:exec/view | {}:refresh | {}/{}:wt | {}/{}:page | {}:close", tab_fwd, tab_back, enter, refresh, prev, next, pprev, pnext, esc)
}

/// Stage/discard hint string for the changed-files pane bottom border.
pub fn git_files_pane_footer() -> String {
    let stage = find_key_for_action(&GIT_ACTIONS, Action::GitToggleStage).unwrap_or("s".into());
    let discard =
        find_key_for_action(&GIT_ACTIONS, Action::GitDiscardFile).unwrap_or("x".into());
    format!(" {}:stage | {}:discard ", stage, discard)
}

/// Projects panel browse-mode hint pairs: (key_display, label) for colored Span rendering.
/// Caller gets `has_project` to conditionally include Esc:close.
pub fn projects_browse_hint_pairs(has_project: bool) -> Vec<(String, &'static str)> {
    let mut v = vec![
        (
            find_key_for_action(&PROJECTS_BROWSE, Action::Confirm).unwrap_or("Enter".into()),
            "open",
        ),
        (
            find_key_for_action(&PROJECTS_BROWSE, Action::ProjectsAdd).unwrap_or("a".into()),
            "add",
        ),
        (
            find_key_for_action(&PROJECTS_BROWSE, Action::ProjectsDelete).unwrap_or("d".into()),
            "delete",
        ),
        (
            find_key_for_action(&PROJECTS_BROWSE, Action::ProjectsRename).unwrap_or("n".into()),
            "name",
        ),
        (
            find_key_for_action(&PROJECTS_BROWSE, Action::ProjectsInit).unwrap_or("i".into()),
            "init",
        ),
    ];
    if has_project {
        v.push(("Esc".into(), "close"));
    }
    v.push((
        find_key_for_action(&PROJECTS_BROWSE, Action::Quit).unwrap_or("⌃Q".into()),
        "quit",
    ));
    v
}

/// Picker title with keybinding hints for run command / preset prompt pickers.
/// `label` is the picker name (e.g., "Run Command" or "Preset Prompts").
pub fn picker_title(label: &str) -> String {
    let edit = find_key_for_action(&PICKER, Action::EditSelected).unwrap_or("e".into());
    let del = find_key_for_action(&PICKER, Action::DeleteSelected).unwrap_or("d".into());
    let add = find_key_for_action(&PICKER, Action::ProjectsAdd).unwrap_or("a".into());
    format!(
        " {} (1-9:select  {}:add  {}:edit  {}:del) ",
        label, add, edit, del
    )
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
    bindings
        .iter()
        .find(|b| b.action == action)
        .map(|b| b.primary.display())
}

/// Like `find_key_for_action` but returns the first alt key when `kbd_enhanced`
/// is false (the primary key may be indistinguishable from Enter without Kitty).
pub fn find_key_adaptive(
    bindings: &[Keybinding],
    action: Action,
    kbd_enhanced: bool,
    alt_enter_stolen: bool,
) -> Option<String> {
    bindings
        .iter()
        .find(|b| b.action == action)
        .map(|b| {
            if !kbd_enhanced {
                for alt in b.alternatives {
                    // Skip Alt+Enter when WezTerm steals it for fullscreen
                    if alt_enter_stolen
                        && alt.modifiers == KeyModifiers::ALT
                        && alt.code == KeyCode::Enter
                    {
                        continue;
                    }
                    return alt.display();
                }
            }
            b.primary.display()
        })
}

/// Find a pair of keys for two related actions (e.g., NavDown/NavUp → "j"/"k")
pub fn find_key_pair(
    bindings: &[Keybinding],
    a: Action,
    b: Action,
    da: &str,
    db: &str,
) -> (String, String) {
    (
        find_key_for_action(bindings, a).unwrap_or_else(|| da.into()),
        find_key_for_action(bindings, b).unwrap_or_else(|| db.into()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    // ══════════════════════════════════════════════════════════════════
    //  help_sections
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn help_sections_returns_six_sections() {
        let sections = help_sections();
        assert_eq!(sections.len(), 6);
    }

    #[test]
    fn help_sections_first_is_global() {
        let sections = help_sections();
        assert_eq!(sections[0].title, "GLOBAL");
    }

    #[test]
    fn help_sections_second_is_worktree() {
        let sections = help_sections();
        assert_eq!(sections[1].title, "WORKTREE (W)");
    }

    #[test]
    fn help_sections_third_is_filetree() {
        let sections = help_sections();
        assert_eq!(sections[2].title, "Filetree");
    }

    #[test]
    fn help_sections_fourth_is_viewer() {
        let sections = help_sections();
        assert_eq!(sections[3].title, "Viewer");
    }

    #[test]
    fn help_sections_fifth_is_edit_mode() {
        let sections = help_sections();
        assert_eq!(sections[4].title, "Edit Mode");
    }

    #[test]
    fn help_sections_sixth_is_session() {
        let sections = help_sections();
        assert_eq!(sections[5].title, "Session");
    }

    #[test]
    fn help_sections_all_have_nonempty_bindings() {
        for section in help_sections() {
            assert!(
                !section.bindings.is_empty(),
                "section '{}' has no bindings",
                section.title
            );
        }
    }

    #[test]
    fn help_sections_global_binding_count() {
        let sections = help_sections();
        assert_eq!(sections[0].bindings.len(), GLOBAL.len());
    }

    #[test]
    fn help_sections_worktree_binding_count() {
        let sections = help_sections();
        assert_eq!(sections[1].bindings.len(), WORKTREES.len());
    }

    #[test]
    fn help_sections_filetree_binding_count() {
        let sections = help_sections();
        assert_eq!(sections[2].bindings.len(), FILE_TREE.len());
    }

    #[test]
    fn help_sections_viewer_binding_count() {
        let sections = help_sections();
        assert_eq!(sections[3].bindings.len(), VIEWER.len());
    }

    #[test]
    fn help_sections_edit_mode_binding_count() {
        let sections = help_sections();
        assert_eq!(sections[4].bindings.len(), EDIT_MODE.len());
    }

    #[test]
    fn help_sections_session_binding_count() {
        let sections = help_sections();
        assert_eq!(sections[5].bindings.len(), SESSION.len());
    }

    // ══════════════════════════════════════════════════════════════════
    //  find_key_for_action
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn find_key_quit_in_global() {
        let key = find_key_for_action(&GLOBAL, Action::Quit);
        assert!(key.is_some());
        assert_eq!(key.unwrap(), plat_ctrl("q"));
    }

    #[test]
    fn find_key_cancel_claude_in_global() {
        let key = find_key_for_action(&GLOBAL, Action::CancelClaude);
        #[cfg(target_os = "macos")]
        assert_eq!(key.unwrap(), "⌃c");
        #[cfg(not(target_os = "macos"))]
        assert_eq!(key.unwrap(), "Alt+c");
    }

    #[test]
    fn find_key_toggle_help_in_global() {
        let key = find_key_for_action(&GLOBAL, Action::ToggleHelp);
        assert_eq!(key.unwrap(), "?");
    }

    #[test]
    fn find_key_enter_prompt_in_global() {
        let key = find_key_for_action(&GLOBAL, Action::EnterPromptMode);
        assert_eq!(key.unwrap(), "p");
    }

    #[test]
    fn find_key_cycle_focus_forward_in_global() {
        let key = find_key_for_action(&GLOBAL, Action::CycleFocusForward);
        assert_eq!(key.unwrap(), "Tab");
    }

    #[test]
    fn find_key_cycle_focus_backward_in_global() {
        let key = find_key_for_action(&GLOBAL, Action::CycleFocusBackward);
        if cfg!(target_os = "macos") {
            assert_eq!(key.unwrap(), "⇧Tab");
        } else {
            assert_eq!(key.unwrap(), "Shift+Tab");
        }
    }

    #[test]
    fn find_key_nonexistent_action_returns_none() {
        // Action::Save does not appear in GLOBAL
        assert!(find_key_for_action(&GLOBAL, Action::Save).is_none());
    }

    #[test]
    fn find_key_submit_in_input() {
        let key = find_key_for_action(&INPUT, Action::Submit);
        assert_eq!(key.unwrap(), "Enter");
    }

    #[test]
    fn find_key_exit_prompt_in_input() {
        let key = find_key_for_action(&INPUT, Action::ExitPromptMode);
        assert_eq!(key.unwrap(), "Esc");
    }

    #[test]
    fn find_key_delete_word_in_input() {
        let key = find_key_for_action(&INPUT, Action::DeleteWord);
        assert_eq!(key.unwrap(), plat_ctrl("w"));
    }

    #[test]
    fn find_key_toggle_stt_in_input() {
        let key = find_key_for_action(&INPUT, Action::ToggleStt);
        assert_eq!(key.unwrap(), plat_ctrl("s"));
    }

    #[test]
    fn find_key_escape_in_terminal() {
        let key = find_key_for_action(&TERMINAL, Action::Escape);
        assert_eq!(key.unwrap(), "Esc");
    }

    #[test]
    fn find_key_enter_terminal_type() {
        let key = find_key_for_action(&TERMINAL, Action::EnterTerminalType);
        assert_eq!(key.unwrap(), "t");
    }

    #[test]
    fn find_key_in_empty_slice() {
        let empty: [Keybinding; 0] = [];
        assert!(find_key_for_action(&empty, Action::Quit).is_none());
    }

    // ══════════════════════════════════════════════════════════════════
    //  find_key_pair
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn find_key_pair_nav_down_up_in_filetree() {
        let (down, up) = find_key_pair(&FILE_TREE, Action::NavDown, Action::NavUp, "j", "k");
        assert_eq!(down, "j");
        assert_eq!(up, "k");
    }

    #[test]
    fn find_key_pair_fallback_when_not_found() {
        let empty: [Keybinding; 0] = [];
        let (a, b) = find_key_pair(
            &empty,
            Action::NavDown,
            Action::NavUp,
            "fallback_a",
            "fallback_b",
        );
        assert_eq!(a, "fallback_a");
        assert_eq!(b, "fallback_b");
    }

    #[test]
    fn find_key_pair_history_in_input() {
        let (prev, next) =
            find_key_pair(&INPUT, Action::HistoryPrev, Action::HistoryNext, "↑", "↓");
        assert_eq!(prev, "↑");
        assert_eq!(next, "↓");
    }

    #[test]
    fn find_key_pair_resize_in_terminal() {
        let (up, down) = find_key_pair(&TERMINAL, Action::ResizeUp, Action::ResizeDown, "+", "-");
        assert_eq!(up, "+");
        assert_eq!(down, "-");
    }

    #[test]
    fn find_key_pair_page_in_terminal() {
        let (pdn, pup) = find_key_pair(&TERMINAL, Action::PageDown, Action::PageUp, "J", "K");
        assert_eq!(pdn, "J");
        assert_eq!(pup, "K");
    }

    #[test]
    fn find_key_pair_worktree_tabs_in_global() {
        let (prev, next) = find_key_pair(
            &GLOBAL,
            Action::WorktreeTabPrev,
            Action::WorktreeTabNext,
            "[",
            "]",
        );
        assert_eq!(prev, "[");
        assert_eq!(next, "]");
    }

    // ══════════════════════════════════════════════════════════════════
    //  find_key_adaptive
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn find_key_adaptive_returns_primary_when_enhanced() {
        let key = find_key_adaptive(&GLOBAL, Action::CycleModel, true, false);
        assert!(key.is_some());
        let k = key.unwrap();
        // With Kitty: should show Ctrl+M (or ⌃m on macOS)
        assert!(
            k.contains('m') || k.contains('M'),
            "expected Ctrl+M variant, got: {}",
            k
        );
    }

    #[test]
    #[cfg(not(target_os = "macos"))]
    fn find_key_adaptive_returns_alt_when_not_enhanced() {
        let key = find_key_adaptive(&GLOBAL, Action::CycleModel, false, false);
        assert!(key.is_some());
        let k = key.unwrap();
        // Without Kitty: should show Alt+M
        assert!(
            k.contains("Alt"),
            "expected Alt+M variant, got: {}",
            k
        );
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn find_key_adaptive_returns_opt_m_on_macos() {
        // macOS has ⌥m → µ fallback — without Kitty, shows ⌥m (not raw µ)
        let key = find_key_adaptive(&GLOBAL, Action::CycleModel, false, false);
        assert!(key.is_some());
        let k = key.unwrap();
        assert!(
            k.contains("⌥m"),
            "expected ⌥m (macOS option+m fallback), got: {}",
            k
        );
    }

    #[test]
    fn find_key_adaptive_returns_primary_when_no_alts() {
        // Action::Quit has no alternative keys — should return primary regardless
        let with = find_key_adaptive(&GLOBAL, Action::Quit, true, false);
        let without = find_key_adaptive(&GLOBAL, Action::Quit, false, false);
        assert_eq!(with, without);
    }

    // ══════════════════════════════════════════════════════════════════
    //  prompt_type_title
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn prompt_type_title_short_label() {
        let (label, _, _) = prompt_type_title(true, false);
        assert_eq!(label, " PROMPT ");
    }

    #[test]
    fn prompt_type_title_full_starts_with_prompt() {
        let (_, full, _) = prompt_type_title(true, false);
        assert!(full.starts_with(" PROMPT ("));
    }

    #[test]
    fn prompt_type_title_full_ends_with_paren() {
        let (_, full, _) = prompt_type_title(true, false);
        assert!(full.ends_with(") "));
    }

    #[test]
    fn prompt_type_title_hints_contains_exit() {
        let (_, _, hints) = prompt_type_title(true, false);
        assert!(
            hints.contains("exit"),
            "hints should mention exit: {}",
            hints
        );
    }

    #[test]
    fn prompt_type_title_hints_contains_submit() {
        let (_, _, hints) = prompt_type_title(true, false);
        assert!(
            hints.contains("submit"),
            "hints should mention submit: {}",
            hints
        );
    }

    #[test]
    fn prompt_type_title_hints_contains_cancel() {
        let (_, _, hints) = prompt_type_title(true, false);
        assert!(
            hints.contains("cancel"),
            "hints should mention cancel: {}",
            hints
        );
    }

    #[test]
    fn prompt_type_title_hints_contains_history() {
        let (_, _, hints) = prompt_type_title(true, false);
        assert!(
            hints.contains("history"),
            "hints should mention history: {}",
            hints
        );
    }

    #[test]
    fn prompt_type_title_hints_contains_del_wrd() {
        let (_, _, hints) = prompt_type_title(true, false);
        assert!(
            hints.contains("del wrd"),
            "hints should mention del wrd: {}",
            hints
        );
    }

    #[test]
    fn prompt_type_title_hints_contains_speech() {
        let (_, _, hints) = prompt_type_title(true, false);
        assert!(
            hints.contains("speech"),
            "hints should mention speech: {}",
            hints
        );
    }

    #[test]
    fn prompt_type_title_hints_contains_presets() {
        let (_, _, hints) = prompt_type_title(true, false);
        assert!(
            hints.contains("presets"),
            "hints should mention presets: {}",
            hints
        );
    }

    #[test]
    fn prompt_type_title_no_kitty_shows_alt_enter_fallback() {
        let (_, _, hints) = prompt_type_title(false, false);
        assert!(
            hints.contains("Alt+Enter") || hints.contains("⌥Enter"),
            "without Kitty, newline hint should show Alt+Enter fallback: {}",
            hints
        );
    }

    #[test]
    fn prompt_type_title_wezterm_shows_ctrl_j_fallback() {
        let (_, _, hints) = prompt_type_title(false, true);
        assert!(
            hints.contains("⌃j") || hints.contains("Ctrl+J"),
            "WezTerm (alt_enter_stolen) should show Ctrl+J fallback: {}",
            hints
        );
        assert!(
            !hints.contains("Alt+Enter") && !hints.contains("⌥Enter"),
            "WezTerm should NOT show Alt+Enter: {}",
            hints
        );
    }

    #[test]
    fn prompt_type_title_kitty_shows_shift_enter() {
        let (_, _, hints) = prompt_type_title(true, false);
        assert!(
            hints.contains("Shift+Enter") || hints.contains("⇧Enter"),
            "with Kitty, newline hint should show Shift+Enter: {}",
            hints
        );
    }

    // ══════════════════════════════════════════════════════════════════
    //  prompt_command_title
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn prompt_command_title_short_label() {
        let (label, _, _) = prompt_command_title();
        assert_eq!(label, " COMMAND ");
    }

    #[test]
    fn prompt_command_title_full_starts_with_command() {
        let (_, full, _) = prompt_command_title();
        assert!(full.starts_with(" COMMAND ("));
    }

    #[test]
    fn prompt_command_title_hints_contains_prompt() {
        let (_, _, hints) = prompt_command_title();
        assert!(
            hints.contains("PROMPT"),
            "hints should mention PROMPT: {}",
            hints
        );
    }

    #[test]
    fn prompt_command_title_hints_contains_terminal() {
        let (_, _, hints) = prompt_command_title();
        assert!(
            hints.contains("TERMINAL"),
            "hints should mention TERMINAL: {}",
            hints
        );
    }

    #[test]
    fn prompt_command_title_hints_contains_git() {
        let (_, _, hints) = prompt_command_title();
        assert!(hints.contains("Git"), "hints should mention Git: {}", hints);
    }

    #[test]
    fn prompt_command_title_hints_contains_health() {
        let (_, _, hints) = prompt_command_title();
        assert!(
            hints.contains("Health"),
            "hints should mention Health: {}",
            hints
        );
    }

    #[test]
    fn prompt_command_title_hints_contains_run() {
        let (_, _, hints) = prompt_command_title();
        assert!(hints.contains("run"), "hints should mention run: {}", hints);
    }

    #[test]
    fn prompt_command_title_hints_contains_cancel() {
        let (_, _, hints) = prompt_command_title();
        assert!(
            hints.contains("cancel"),
            "hints should mention cancel: {}",
            hints
        );
    }

    #[test]
    fn prompt_command_title_hints_contains_quit() {
        let (_, _, hints) = prompt_command_title();
        assert!(
            hints.contains("quit"),
            "hints should mention quit: {}",
            hints
        );
    }

    #[test]
    fn prompt_command_title_hints_contains_help() {
        let (_, _, hints) = prompt_command_title();
        assert!(
            hints.contains("help"),
            "hints should mention help: {}",
            hints
        );
    }

    // ══════════════════════════════════════════════════════════════════
    //  terminal_type_title
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn terminal_type_title_short_label() {
        let (label, _, _) = terminal_type_title();
        assert_eq!(label, " TERMINAL ");
    }

    #[test]
    fn terminal_type_title_hints_contains_exit() {
        let (_, _, hints) = terminal_type_title();
        assert!(
            hints.contains("exit"),
            "hints should mention exit: {}",
            hints
        );
    }

    #[test]
    fn terminal_type_title_full_contains_terminal() {
        let (_, full, _) = terminal_type_title();
        assert!(full.contains("TERMINAL"));
    }

    // ══════════════════════════════════════════════════════════════════
    //  terminal_command_title
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn terminal_command_title_short_label() {
        let (label, _, _) = terminal_command_title();
        assert_eq!(label, " TERMINAL ");
    }

    #[test]
    fn terminal_command_title_hints_contains_type() {
        let (_, _, hints) = terminal_command_title();
        assert!(
            hints.contains("type"),
            "hints should mention type: {}",
            hints
        );
    }

    #[test]
    fn terminal_command_title_hints_contains_prompt() {
        let (_, _, hints) = terminal_command_title();
        assert!(
            hints.contains("PROMPT"),
            "hints should mention PROMPT: {}",
            hints
        );
    }

    #[test]
    fn terminal_command_title_hints_contains_close() {
        let (_, _, hints) = terminal_command_title();
        assert!(
            hints.contains("close"),
            "hints should mention close: {}",
            hints
        );
    }

    #[test]
    fn terminal_command_title_hints_contains_scroll() {
        let (_, _, hints) = terminal_command_title();
        assert!(
            hints.contains("scroll"),
            "hints should mention scroll: {}",
            hints
        );
    }

    #[test]
    fn terminal_command_title_hints_contains_resize() {
        let (_, _, hints) = terminal_command_title();
        assert!(
            hints.contains("resize"),
            "hints should mention resize: {}",
            hints
        );
    }

    // ══════════════════════════════════════════════════════════════════
    //  terminal_scroll_title
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn terminal_scroll_title_label_includes_count() {
        let (label, _, _) = terminal_scroll_title(42);
        assert!(
            label.contains("42"),
            "label should include scroll count: {}",
            label
        );
    }

    #[test]
    fn terminal_scroll_title_full_includes_count() {
        let (_, full, _) = terminal_scroll_title(100);
        assert!(
            full.contains("100"),
            "full title should include scroll count: {}",
            full
        );
    }

    #[test]
    fn terminal_scroll_title_zero_scroll() {
        let (label, _, _) = terminal_scroll_title(0);
        assert!(label.contains("0"), "label should include zero: {}", label);
    }

    #[test]
    fn terminal_scroll_title_hints_contains_scroll() {
        let (_, _, hints) = terminal_scroll_title(5);
        assert!(
            hints.contains("scroll"),
            "hints should mention scroll: {}",
            hints
        );
    }

    #[test]
    fn terminal_scroll_title_hints_contains_page() {
        let (_, _, hints) = terminal_scroll_title(5);
        assert!(
            hints.contains("page"),
            "hints should mention page: {}",
            hints
        );
    }

    #[test]
    fn terminal_scroll_title_hints_contains_top() {
        let (_, _, hints) = terminal_scroll_title(5);
        assert!(hints.contains("top"), "hints should mention top: {}", hints);
    }

    #[test]
    fn terminal_scroll_title_hints_contains_bottom() {
        let (_, _, hints) = terminal_scroll_title(5);
        assert!(
            hints.contains("bottom"),
            "hints should mention bottom: {}",
            hints
        );
    }

    // ══════════════════════════════════════════════════════════════════
    //  health_god_files_hints
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn health_god_files_hints_contains_check() {
        let h = health_god_files_hints();
        assert!(h.contains("check"), "hints should mention check: {}", h);
    }

    #[test]
    fn health_god_files_hints_contains_all() {
        let h = health_god_files_hints();
        assert!(h.contains("all"), "hints should mention all: {}", h);
    }

    #[test]
    fn health_god_files_hints_contains_view() {
        let h = health_god_files_hints();
        assert!(h.contains("view"), "hints should mention view: {}", h);
    }

    #[test]
    fn health_god_files_hints_contains_modularize() {
        let h = health_god_files_hints();
        assert!(
            h.contains("modularize"),
            "hints should mention modularize: {}",
            h
        );
    }

    #[test]
    fn health_god_files_hints_contains_switch() {
        let h = health_god_files_hints();
        assert!(h.contains("switch"), "hints should mention switch: {}", h);
    }

    #[test]
    fn health_god_files_hints_contains_close() {
        let h = health_god_files_hints();
        assert!(h.contains("close"), "hints should mention close: {}", h);
    }

    // ══════════════════════════════════════════════════════════════════
    //  health_docs_hints
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn health_docs_hints_contains_check() {
        let h = health_docs_hints();
        assert!(h.contains("check"), "hints should mention check: {}", h);
    }

    #[test]
    fn health_docs_hints_contains_non_100() {
        let h = health_docs_hints();
        assert!(
            h.contains("non-100%"),
            "hints should mention non-100%: {}",
            h
        );
    }

    #[test]
    fn health_docs_hints_contains_view() {
        let h = health_docs_hints();
        assert!(h.contains("view"), "hints should mention view: {}", h);
    }

    #[test]
    fn health_docs_hints_contains_complete() {
        let h = health_docs_hints();
        assert!(
            h.contains("complete"),
            "hints should mention complete: {}",
            h
        );
    }

    #[test]
    fn health_docs_hints_contains_switch() {
        let h = health_docs_hints();
        assert!(h.contains("switch"), "hints should mention switch: {}", h);
    }

    #[test]
    fn health_docs_hints_contains_close() {
        let h = health_docs_hints();
        assert!(h.contains("close"), "hints should mention close: {}", h);
    }

    // ══════════════════════════════════════════════════════════════════
    //  git_actions_labels
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn git_labels_main_has_pull_commit_push() {
        let labels = git_actions_labels(true);
        let descs: Vec<&str> = labels.iter().map(|(_, d)| *d).collect();
        assert!(
            descs.contains(&"Pull"),
            "main should show Pull: {:?}",
            descs
        );
        assert!(
            descs.contains(&"Commit"),
            "main should show Commit: {:?}",
            descs
        );
        assert!(
            descs.contains(&"Push to remote"),
            "main should show Push to remote: {:?}",
            descs
        );
    }

    #[test]
    fn git_labels_main_count() {
        let labels = git_actions_labels(true);
        assert_eq!(labels.len(), 5);
    }

    #[test]
    fn git_labels_feature_has_squash_rebase_commit_push() {
        let labels = git_actions_labels(false);
        let descs: Vec<&str> = labels.iter().map(|(_, d)| *d).collect();
        assert!(
            descs.contains(&"Squash merge to main"),
            "feature should show Squash: {:?}",
            descs
        );
        assert!(
            descs.contains(&"Rebase onto main"),
            "feature should show Rebase: {:?}",
            descs
        );
        assert!(
            descs.contains(&"Commit"),
            "feature should show Commit: {:?}",
            descs
        );
        assert!(
            descs.contains(&"Push to remote"),
            "feature should show Push: {:?}",
            descs
        );
    }

    #[test]
    fn git_labels_feature_count() {
        let labels = git_actions_labels(false);
        assert_eq!(labels.len(), 6);
    }

    #[test]
    fn git_labels_main_no_squash() {
        let labels = git_actions_labels(true);
        let descs: Vec<&str> = labels.iter().map(|(_, d)| *d).collect();
        assert!(!descs.contains(&"Squash merge to main"));
    }

    #[test]
    fn git_labels_feature_no_pull() {
        let labels = git_actions_labels(false);
        let descs: Vec<&str> = labels.iter().map(|(_, d)| *d).collect();
        assert!(!descs.contains(&"Pull"));
    }

    #[test]
    fn git_labels_keys_are_nonempty() {
        for (key, _) in git_actions_labels(true) {
            assert!(!key.is_empty(), "key display should not be empty");
        }
        for (key, _) in git_actions_labels(false) {
            assert!(!key.is_empty(), "key display should not be empty");
        }
    }

    // ══════════════════════════════════════════════════════════════════
    //  git_actions_footer
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn git_footer_contains_cycle_panes() {
        let f = git_actions_footer();
        assert!(f.contains("cycle"), "footer should mention cycle: {}", f);
    }

    #[test]
    fn git_footer_contains_exec_view() {
        let f = git_actions_footer();
        assert!(
            f.contains("exec/view"),
            "footer should mention exec/view: {}",
            f
        );
    }

    #[test]
    fn git_footer_contains_refresh() {
        let f = git_actions_footer();
        assert!(
            f.contains("refresh"),
            "footer should mention refresh: {}",
            f
        );
    }

    #[test]
    fn git_footer_contains_close() {
        let f = git_actions_footer();
        assert!(f.contains("close"), "footer should mention close: {}", f);
    }

    #[test]
    fn git_footer_contains_wt() {
        let f = git_actions_footer();
        assert!(
            f.contains("wt"),
            "footer should mention wt (worktree): {}",
            f
        );
    }

    #[test]
    fn git_footer_contains_page() {
        let f = git_actions_footer();
        assert!(f.contains("page"), "footer should mention page: {}", f);
    }

    // ══════════════════════════════════════════════════════════════════
    //  projects_browse_hint_pairs
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn projects_hints_with_project_includes_close() {
        let pairs = projects_browse_hint_pairs(true);
        let labels: Vec<&str> = pairs.iter().map(|(_, l)| *l).collect();
        assert!(
            labels.contains(&"close"),
            "should include close: {:?}",
            labels
        );
    }

    #[test]
    fn projects_hints_without_project_no_close() {
        let pairs = projects_browse_hint_pairs(false);
        let labels: Vec<&str> = pairs.iter().map(|(_, l)| *l).collect();
        assert!(
            !labels.contains(&"close"),
            "should not include close: {:?}",
            labels
        );
    }

    #[test]
    fn projects_hints_always_has_open() {
        for has_project in [true, false] {
            let pairs = projects_browse_hint_pairs(has_project);
            let labels: Vec<&str> = pairs.iter().map(|(_, l)| *l).collect();
            assert!(
                labels.contains(&"open"),
                "should include open: {:?}",
                labels
            );
        }
    }

    #[test]
    fn projects_hints_always_has_add() {
        for has_project in [true, false] {
            let pairs = projects_browse_hint_pairs(has_project);
            let labels: Vec<&str> = pairs.iter().map(|(_, l)| *l).collect();
            assert!(labels.contains(&"add"), "should include add: {:?}", labels);
        }
    }

    #[test]
    fn projects_hints_always_has_delete() {
        for has_project in [true, false] {
            let pairs = projects_browse_hint_pairs(has_project);
            let labels: Vec<&str> = pairs.iter().map(|(_, l)| *l).collect();
            assert!(
                labels.contains(&"delete"),
                "should include delete: {:?}",
                labels
            );
        }
    }

    #[test]
    fn projects_hints_always_has_name() {
        for has_project in [true, false] {
            let pairs = projects_browse_hint_pairs(has_project);
            let labels: Vec<&str> = pairs.iter().map(|(_, l)| *l).collect();
            assert!(
                labels.contains(&"name"),
                "should include name: {:?}",
                labels
            );
        }
    }

    #[test]
    fn projects_hints_always_has_init() {
        for has_project in [true, false] {
            let pairs = projects_browse_hint_pairs(has_project);
            let labels: Vec<&str> = pairs.iter().map(|(_, l)| *l).collect();
            assert!(
                labels.contains(&"init"),
                "should include init: {:?}",
                labels
            );
        }
    }

    #[test]
    fn projects_hints_always_has_quit() {
        for has_project in [true, false] {
            let pairs = projects_browse_hint_pairs(has_project);
            let labels: Vec<&str> = pairs.iter().map(|(_, l)| *l).collect();
            assert!(
                labels.contains(&"quit"),
                "should include quit: {:?}",
                labels
            );
        }
    }

    #[test]
    fn projects_hints_with_project_count() {
        let pairs = projects_browse_hint_pairs(true);
        // open, add, delete, name, init, close, quit = 7
        assert_eq!(pairs.len(), 7);
    }

    #[test]
    fn projects_hints_without_project_count() {
        let pairs = projects_browse_hint_pairs(false);
        // open, add, delete, name, init, quit = 6 (no close)
        assert_eq!(pairs.len(), 6);
    }

    #[test]
    fn projects_hints_keys_are_nonempty() {
        for (key, _) in projects_browse_hint_pairs(true) {
            assert!(!key.is_empty());
        }
    }

    // ══════════════════════════════════════════════════════════════════
    //  picker_title
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn picker_title_contains_label() {
        let t = picker_title("Run Command");
        assert!(
            t.contains("Run Command"),
            "title should contain label: {}",
            t
        );
    }

    #[test]
    fn picker_title_contains_select_hint() {
        let t = picker_title("Test");
        assert!(
            t.contains("1-9:select"),
            "title should contain select hint: {}",
            t
        );
    }

    #[test]
    fn picker_title_contains_add() {
        let t = picker_title("Test");
        assert!(t.contains("add"), "title should contain add: {}", t);
    }

    #[test]
    fn picker_title_contains_edit() {
        let t = picker_title("Test");
        assert!(t.contains("edit"), "title should contain edit: {}", t);
    }

    #[test]
    fn picker_title_contains_del() {
        let t = picker_title("Test");
        assert!(t.contains("del"), "title should contain del: {}", t);
    }

    #[test]
    fn picker_title_preset_prompts() {
        let t = picker_title("Preset Prompts");
        assert!(t.contains("Preset Prompts"));
    }

    // ══════════════════════════════════════════════════════════════════
    //  dialog_footer_hint_pairs
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn dialog_footer_has_five_pairs() {
        assert_eq!(dialog_footer_hint_pairs().len(), 5);
    }

    #[test]
    fn dialog_footer_first_is_tab_next() {
        let pairs = dialog_footer_hint_pairs();
        assert_eq!(pairs[0], ("Tab".into(), "next"));
    }

    #[test]
    fn dialog_footer_second_is_shift_tab_back() {
        let pairs = dialog_footer_hint_pairs();
        assert_eq!(pairs[1], ("⇧Tab".into(), "back"));
    }

    #[test]
    fn dialog_footer_third_is_ctrl_s_scope() {
        let pairs = dialog_footer_hint_pairs();
        assert_eq!(pairs[2], ("⌃s".into(), "scope"));
    }

    #[test]
    fn dialog_footer_fourth_is_enter_save() {
        let pairs = dialog_footer_hint_pairs();
        assert_eq!(pairs[3], ("Enter".into(), "save"));
    }

    #[test]
    fn dialog_footer_fifth_is_esc_cancel() {
        let pairs = dialog_footer_hint_pairs();
        assert_eq!(pairs[4], ("Esc".into(), "cancel"));
    }

    #[test]
    fn dialog_footer_keys_are_nonempty() {
        for (key, _) in dialog_footer_hint_pairs() {
            assert!(!key.is_empty());
        }
    }

    #[test]
    fn dialog_footer_labels_are_nonempty() {
        for (_, label) in dialog_footer_hint_pairs() {
            assert!(!label.is_empty());
        }
    }
}
