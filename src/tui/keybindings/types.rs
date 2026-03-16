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
        Self {
            modifiers: KeyModifiers::NONE,
            code,
        }
    }

    pub const fn shift(code: KeyCode) -> Self {
        Self {
            modifiers: KeyModifiers::SHIFT,
            code,
        }
    }

    pub const fn ctrl(code: KeyCode) -> Self {
        Self {
            modifiers: KeyModifiers::CONTROL,
            code,
        }
    }

    pub const fn alt(code: KeyCode) -> Self {
        Self {
            modifiers: KeyModifiers::ALT,
            code,
        }
    }

    #[cfg(target_os = "macos")]
    pub const fn cmd(code: KeyCode) -> Self {
        Self {
            modifiers: KeyModifiers::SUPER,
            code,
        }
    }

    /// Check if key event matches this combo
    #[inline]
    pub fn matches(&self, modifiers: KeyModifiers, code: KeyCode) -> bool {
        if self.modifiers == modifiers && self.code == code {
            return true;
        }
        // Shifted-symbol bindings: characters like ?, !, @, #, etc. are
        // produced by pressing Shift+<key>. On macOS, crossterm typically
        // delivers these as (NONE, Char('?')). On Windows, crossterm
        // delivers (SHIFT, Char('?')). When a binding uses plain(Char('?')),
        // also accept SHIFT modifier if the char matches — the SHIFT is
        // implicit in producing the character itself.
        if self.modifiers == KeyModifiers::NONE
            && modifiers == KeyModifiers::SHIFT
            && self.code == code
        {
            if let KeyCode::Char(c) = code {
                // Only for non-alpha chars — Shift+letter has separate handling below
                if !c.is_ascii_alphabetic() {
                    return true;
                }
            }
        }
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
        // Shift+letter bindings: crossterm delivers uppercase chars
        // inconsistently depending on terminal + Kitty flags. Generalized
        // for pure SHIFT and combined modifiers (e.g. CTRL+SHIFT+C).
        if self.modifiers.contains(KeyModifiers::SHIFT) {
            if let KeyCode::Char(c) = self.code {
                if c.is_ascii_uppercase() {
                    let other_mods = self.modifiers.difference(KeyModifiers::SHIFT);
                    if let KeyCode::Char(pressed) = code {
                        // (other_mods | SHIFT, any case) — SHIFT explicitly flagged
                        if modifiers.contains(KeyModifiers::SHIFT)
                            && modifiers.difference(KeyModifiers::SHIFT) == other_mods
                            && pressed.to_ascii_uppercase() == c
                        {
                            return true;
                        }
                        // (other_mods, uppercase) — legacy terminals omit SHIFT flag
                        if modifiers == other_mods && pressed == c {
                            return true;
                        }
                    }
                }
            }
        }
        false
    }

    /// Platform-appropriate display string
    /// macOS: ⌃⌥⇧⌘ symbols. Windows/Linux: Ctrl+Alt+Shift+ text labels.
    pub fn display(&self) -> String {
        let mut s = String::new();

        #[cfg(target_os = "macos")]
        {
            if self.modifiers.contains(KeyModifiers::CONTROL) {
                s.push('⌃');
            }
            if self.modifiers.contains(KeyModifiers::ALT) {
                s.push('⌥');
            }
            // Show ⇧ for non-char keys always, and for char keys when
            // combined with other modifiers (e.g. ⌃⇧C vs just G)
            if self.modifiers.contains(KeyModifiers::SHIFT) {
                let has_other_mods = self.modifiers != KeyModifiers::SHIFT;
                if !matches!(self.code, KeyCode::Char(_)) || has_other_mods {
                    s.push('⇧');
                }
            }
            if self.modifiers.contains(KeyModifiers::SUPER) {
                s.push('⌘');
            }
        }

        #[cfg(not(target_os = "macos"))]
        {
            if self.modifiers.contains(KeyModifiers::CONTROL) {
                s.push_str("Ctrl+");
            }
            if self.modifiers.contains(KeyModifiers::ALT) {
                s.push_str("Alt+");
            }
            if self.modifiers.contains(KeyModifiers::SHIFT) {
                let has_other_mods = self.modifiers != KeyModifiers::SHIFT;
                if !matches!(self.code, KeyCode::Char(_)) || has_other_mods {
                    s.push_str("Shift+");
                }
            }
        }

        match self.code {
            KeyCode::Char(' ') => s.push_str("Space"),
            KeyCode::Char(c) => s.push(c),
            KeyCode::Enter => s.push_str("Enter"),
            KeyCode::Esc => s.push_str("Esc"),
            KeyCode::Tab => s.push_str("Tab"),
            KeyCode::BackTab => s.push_str("S-Tab"),
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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
    RunCommand,
    AddRunCommand,
    ToggleArchiveWorktree,
    DeleteWorktree,
    OpenHealth,
    OpenGitActions,
    OpenProjects,

    // FileTree
    ToggleDir,
    RecursiveExpand,
    RecursiveCollapse,
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
    GitToggleFocusBack,
    GitSquashMerge,
    GitRebase,
    GitPull,
    GitViewDiff,
    GitRefresh,
    GitCommit,
    GitPush,
    GitAutoRebase,
    GitAutoResolveSettings,
    GitToggleStage,
    GitStageAll,
    GitDiscardFile,
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
        Self {
            primary,
            alternatives: &[],
            description,
            action,
            pair_with_next: false,
        }
    }

    pub const fn with_alt(
        primary: KeyCombo,
        alternatives: &'static [KeyCombo],
        description: &'static str,
        action: Action,
    ) -> Self {
        Self {
            primary,
            alternatives,
            description,
            action,
            pair_with_next: false,
        }
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
                if !c.is_ascii() && alt.modifiers == KeyModifiers::NONE {
                    continue;
                }
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

#[cfg(test)]
mod tests {
    use super::*;

    // ══════════════════════════════════════════════════════════════════
    //  KeyCombo constructors
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn keycombo_new_sets_modifiers_and_code() {
        let kc = KeyCombo::new(KeyModifiers::CONTROL, KeyCode::Char('c'));
        assert_eq!(kc.modifiers, KeyModifiers::CONTROL);
        assert_eq!(kc.code, KeyCode::Char('c'));
    }

    #[test]
    fn keycombo_plain_has_no_modifiers() {
        let kc = KeyCombo::plain(KeyCode::Char('q'));
        assert_eq!(kc.modifiers, KeyModifiers::NONE);
        assert_eq!(kc.code, KeyCode::Char('q'));
    }

    #[test]
    fn keycombo_shift_sets_shift() {
        let kc = KeyCombo::shift(KeyCode::Char('G'));
        assert_eq!(kc.modifiers, KeyModifiers::SHIFT);
        assert_eq!(kc.code, KeyCode::Char('G'));
    }

    #[test]
    fn keycombo_ctrl_sets_control() {
        let kc = KeyCombo::ctrl(KeyCode::Char('s'));
        assert_eq!(kc.modifiers, KeyModifiers::CONTROL);
        assert_eq!(kc.code, KeyCode::Char('s'));
    }

    #[test]
    fn keycombo_alt_sets_alt() {
        let kc = KeyCombo::alt(KeyCode::Char('x'));
        assert_eq!(kc.modifiers, KeyModifiers::ALT);
        assert_eq!(kc.code, KeyCode::Char('x'));
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn keycombo_cmd_sets_super() {
        let kc = KeyCombo::cmd(KeyCode::Char('o'));
        assert_eq!(kc.modifiers, KeyModifiers::SUPER);
        assert_eq!(kc.code, KeyCode::Char('o'));
    }

    #[test]
    fn keycombo_new_combined_modifiers() {
        let mods = KeyModifiers::CONTROL | KeyModifiers::SHIFT;
        let kc = KeyCombo::new(mods, KeyCode::Char('Z'));
        assert!(kc.modifiers.contains(KeyModifiers::CONTROL));
        assert!(kc.modifiers.contains(KeyModifiers::SHIFT));
    }

    #[test]
    fn keycombo_plain_enter() {
        let kc = KeyCombo::plain(KeyCode::Enter);
        assert_eq!(kc.modifiers, KeyModifiers::NONE);
        assert_eq!(kc.code, KeyCode::Enter);
    }

    #[test]
    fn keycombo_plain_esc() {
        let kc = KeyCombo::plain(KeyCode::Esc);
        assert_eq!(kc.code, KeyCode::Esc);
    }

    #[test]
    fn keycombo_ctrl_arrow() {
        let kc = KeyCombo::ctrl(KeyCode::Up);
        assert_eq!(kc.modifiers, KeyModifiers::CONTROL);
        assert_eq!(kc.code, KeyCode::Up);
    }

    // ══════════════════════════════════════════════════════════════════
    //  KeyCombo::matches
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn matches_exact_plain_char() {
        let kc = KeyCombo::plain(KeyCode::Char('j'));
        assert!(kc.matches(KeyModifiers::NONE, KeyCode::Char('j')));
    }

    #[test]
    fn matches_exact_ctrl_char() {
        let kc = KeyCombo::ctrl(KeyCode::Char('c'));
        assert!(kc.matches(KeyModifiers::CONTROL, KeyCode::Char('c')));
    }

    #[test]
    fn no_match_wrong_modifier() {
        let kc = KeyCombo::ctrl(KeyCode::Char('c'));
        assert!(!kc.matches(KeyModifiers::ALT, KeyCode::Char('c')));
    }

    #[test]
    fn no_match_wrong_code() {
        let kc = KeyCombo::ctrl(KeyCode::Char('c'));
        assert!(!kc.matches(KeyModifiers::CONTROL, KeyCode::Char('x')));
    }

    #[test]
    fn no_match_both_wrong() {
        let kc = KeyCombo::ctrl(KeyCode::Char('c'));
        assert!(!kc.matches(KeyModifiers::ALT, KeyCode::Char('x')));
    }

    #[test]
    fn matches_shift_uppercase_with_shift_modifier() {
        // Shift+G binding matches (SHIFT, Char('G'))
        let kc = KeyCombo::shift(KeyCode::Char('G'));
        assert!(kc.matches(KeyModifiers::SHIFT, KeyCode::Char('G')));
    }

    #[test]
    fn matches_shift_uppercase_with_none_modifier() {
        // Shift+G binding also matches (NONE, Char('G')) — legacy terminal behavior
        let kc = KeyCombo::shift(KeyCode::Char('G'));
        assert!(kc.matches(KeyModifiers::NONE, KeyCode::Char('G')));
    }

    #[test]
    fn matches_shift_uppercase_with_shift_lowercase() {
        // Shift+G binding matches (SHIFT, Char('g')) — Kitty DISAMBIGUATE mode
        let kc = KeyCombo::shift(KeyCode::Char('G'));
        assert!(kc.matches(KeyModifiers::SHIFT, KeyCode::Char('g')));
    }

    #[test]
    fn no_match_shift_uppercase_plain_lowercase() {
        // (NONE, Char('g')) should NOT match Shift+G — that's just a plain 'g'
        let kc = KeyCombo::shift(KeyCode::Char('G'));
        assert!(!kc.matches(KeyModifiers::NONE, KeyCode::Char('g')));
    }

    #[test]
    fn matches_enter_key() {
        let kc = KeyCombo::plain(KeyCode::Enter);
        assert!(kc.matches(KeyModifiers::NONE, KeyCode::Enter));
    }

    #[test]
    fn no_match_enter_vs_esc() {
        let kc = KeyCombo::plain(KeyCode::Enter);
        assert!(!kc.matches(KeyModifiers::NONE, KeyCode::Esc));
    }

    #[test]
    fn matches_f_key() {
        let kc = KeyCombo::plain(KeyCode::F(1));
        assert!(kc.matches(KeyModifiers::NONE, KeyCode::F(1)));
    }

    #[test]
    fn no_match_different_f_key() {
        let kc = KeyCombo::plain(KeyCode::F(1));
        assert!(!kc.matches(KeyModifiers::NONE, KeyCode::F(2)));
    }

    #[test]
    fn matches_tab_plain() {
        let kc = KeyCombo::plain(KeyCode::Tab);
        assert!(kc.matches(KeyModifiers::NONE, KeyCode::Tab));
    }

    #[test]
    fn no_match_extra_modifier_on_plain() {
        let kc = KeyCombo::plain(KeyCode::Char('q'));
        assert!(!kc.matches(KeyModifiers::CONTROL, KeyCode::Char('q')));
    }

    // ══════════════════════════════════════════════════════════════════
    //  KeyCombo::display
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn display_plain_char() {
        let kc = KeyCombo::plain(KeyCode::Char('j'));
        assert_eq!(kc.display(), "j");
    }

    #[test]
    fn display_uppercase_char() {
        let kc = KeyCombo::shift(KeyCode::Char('G'));
        // Shift suppressed for char keys — uppercase implies shift
        assert_eq!(kc.display(), "G");
    }

    #[test]
    fn display_ctrl_char() {
        let kc = KeyCombo::ctrl(KeyCode::Char('c'));
        if cfg!(target_os = "macos") {
            assert_eq!(kc.display(), "⌃c");
        } else {
            assert_eq!(kc.display(), "Ctrl+c");
        }
    }

    #[test]
    fn display_alt_char() {
        let kc = KeyCombo::alt(KeyCode::Char('x'));
        if cfg!(target_os = "macos") {
            assert_eq!(kc.display(), "⌥x");
        } else {
            assert_eq!(kc.display(), "Alt+x");
        }
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn display_cmd_char() {
        let kc = KeyCombo::cmd(KeyCode::Char('o'));
        assert_eq!(kc.display(), "⌘o");
    }

    #[test]
    fn display_ctrl_alt() {
        let kc = KeyCombo::new(
            KeyModifiers::CONTROL | KeyModifiers::ALT,
            KeyCode::Char('d'),
        );
        if cfg!(target_os = "macos") {
            assert_eq!(kc.display(), "⌃⌥d");
        } else {
            assert_eq!(kc.display(), "Ctrl+Alt+d");
        }
    }

    #[test]
    fn display_enter() {
        let kc = KeyCombo::plain(KeyCode::Enter);
        assert_eq!(kc.display(), "Enter");
    }

    #[test]
    fn display_esc() {
        let kc = KeyCombo::plain(KeyCode::Esc);
        assert_eq!(kc.display(), "Esc");
    }

    #[test]
    fn display_tab() {
        let kc = KeyCombo::plain(KeyCode::Tab);
        assert_eq!(kc.display(), "Tab");
    }

    #[test]
    fn display_backtab() {
        // plain(BackTab) — BackTab already implies Shift+Tab
        let kc = KeyCombo::plain(KeyCode::BackTab);
        assert_eq!(kc.display(), "S-Tab");
        // shift(BackTab) adds explicit shift prefix
        let kc2 = KeyCombo::shift(KeyCode::BackTab);
        if cfg!(target_os = "macos") {
            assert_eq!(kc2.display(), "⇧S-Tab");
        } else {
            assert_eq!(kc2.display(), "Shift+S-Tab");
        }
    }

    #[test]
    fn display_backspace() {
        let kc = KeyCombo::plain(KeyCode::Backspace);
        assert_eq!(kc.display(), "⌫");
    }

    #[test]
    fn display_delete() {
        let kc = KeyCombo::plain(KeyCode::Delete);
        assert_eq!(kc.display(), "⌦");
    }

    #[test]
    fn display_arrows() {
        assert_eq!(KeyCombo::plain(KeyCode::Up).display(), "↑");
        assert_eq!(KeyCombo::plain(KeyCode::Down).display(), "↓");
        assert_eq!(KeyCombo::plain(KeyCode::Left).display(), "←");
        assert_eq!(KeyCombo::plain(KeyCode::Right).display(), "→");
    }

    #[test]
    fn display_home_end() {
        assert_eq!(KeyCombo::plain(KeyCode::Home).display(), "Home");
        assert_eq!(KeyCombo::plain(KeyCode::End).display(), "End");
    }

    #[test]
    fn display_pageup_pagedown() {
        assert_eq!(KeyCombo::plain(KeyCode::PageUp).display(), "PgUp");
        assert_eq!(KeyCombo::plain(KeyCode::PageDown).display(), "PgDn");
    }

    #[test]
    fn display_space() {
        let kc = KeyCombo::plain(KeyCode::Char(' '));
        assert_eq!(kc.display(), "Space");
    }

    #[test]
    fn display_shift_arrow_shows_shift() {
        let kc = KeyCombo::shift(KeyCode::Up);
        if cfg!(target_os = "macos") {
            assert_eq!(kc.display(), "⇧↑");
        } else {
            assert_eq!(kc.display(), "Shift+↑");
        }
    }

    #[test]
    fn display_ctrl_shift_enter() {
        let kc = KeyCombo::new(KeyModifiers::CONTROL | KeyModifiers::SHIFT, KeyCode::Enter);
        if cfg!(target_os = "macos") {
            assert_eq!(kc.display(), "⌃⇧Enter");
        } else {
            assert_eq!(kc.display(), "Ctrl+Shift+Enter");
        }
    }

    // ══════════════════════════════════════════════════════════════════
    //  Keybinding constructors
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn keybinding_new_fields() {
        let kb = Keybinding::new(KeyCombo::plain(KeyCode::Char('q')), "Quit", Action::Quit);
        assert_eq!(kb.primary.code, KeyCode::Char('q'));
        assert_eq!(kb.description, "Quit");
        assert_eq!(kb.action, Action::Quit);
        assert!(kb.alternatives.is_empty());
        assert!(!kb.pair_with_next);
    }

    #[test]
    fn keybinding_with_alt_stores_alternatives() {
        static ALTS: [KeyCombo; 1] = [KeyCombo {
            modifiers: KeyModifiers::NONE,
            code: KeyCode::Down,
        }];
        let kb = Keybinding::with_alt(
            KeyCombo::plain(KeyCode::Char('j')),
            &ALTS,
            "Down",
            Action::NavDown,
        );
        assert_eq!(kb.alternatives.len(), 1);
        assert_eq!(kb.alternatives[0].code, KeyCode::Down);
    }

    #[test]
    fn keybinding_paired_sets_flag() {
        let kb =
            Keybinding::new(KeyCombo::plain(KeyCode::Char('j')), "Down", Action::NavDown).paired();
        assert!(kb.pair_with_next);
    }

    #[test]
    fn keybinding_new_not_paired_by_default() {
        let kb = Keybinding::new(KeyCombo::plain(KeyCode::Char('j')), "Down", Action::NavDown);
        assert!(!kb.pair_with_next);
    }

    // ══════════════════════════════════════════════════════════════════
    //  Keybinding::matches
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn keybinding_matches_primary() {
        let kb = Keybinding::new(KeyCombo::plain(KeyCode::Char('q')), "Quit", Action::Quit);
        assert!(kb.matches(KeyModifiers::NONE, KeyCode::Char('q')));
    }

    #[test]
    fn keybinding_matches_alternative() {
        static ALTS: [KeyCombo; 1] = [KeyCombo {
            modifiers: KeyModifiers::NONE,
            code: KeyCode::Down,
        }];
        let kb = Keybinding::with_alt(
            KeyCombo::plain(KeyCode::Char('j')),
            &ALTS,
            "Down",
            Action::NavDown,
        );
        assert!(kb.matches(KeyModifiers::NONE, KeyCode::Down));
    }

    #[test]
    fn keybinding_no_match() {
        let kb = Keybinding::new(KeyCombo::plain(KeyCode::Char('q')), "Quit", Action::Quit);
        assert!(!kb.matches(KeyModifiers::NONE, KeyCode::Char('x')));
    }

    #[test]
    fn keybinding_no_match_wrong_modifier() {
        let kb = Keybinding::new(KeyCombo::plain(KeyCode::Char('q')), "Quit", Action::Quit);
        assert!(!kb.matches(KeyModifiers::CONTROL, KeyCode::Char('q')));
    }

    // ══════════════════════════════════════════════════════════════════
    //  Keybinding::display_keys
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn display_keys_no_alternatives() {
        let kb = Keybinding::new(KeyCombo::plain(KeyCode::Char('q')), "Quit", Action::Quit);
        assert_eq!(kb.display_keys(), "q");
    }

    #[test]
    fn display_keys_with_ascii_alternative() {
        static ALTS: [KeyCombo; 1] = [KeyCombo {
            modifiers: KeyModifiers::NONE,
            code: KeyCode::Down,
        }];
        let kb = Keybinding::with_alt(
            KeyCombo::plain(KeyCode::Char('j')),
            &ALTS,
            "Down",
            Action::NavDown,
        );
        assert_eq!(kb.display_keys(), "j/↓");
    }

    #[test]
    fn display_keys_skips_macos_unicode_alt() {
        static ALTS: [KeyCombo; 1] = [KeyCombo {
            modifiers: KeyModifiers::NONE,
            code: KeyCode::Char('®'),
        }];
        let kb = Keybinding::with_alt(
            KeyCombo::alt(KeyCode::Char('r')),
            &ALTS,
            "Test",
            Action::Quit,
        );
        if cfg!(target_os = "macos") {
            assert_eq!(kb.display_keys(), "⌥r");
        } else {
            assert_eq!(kb.display_keys(), "Alt+r");
        }
    }

    #[test]
    fn display_keys_keeps_non_ascii_with_modifier() {
        static ALTS: [KeyCombo; 1] = [KeyCombo {
            modifiers: KeyModifiers::CONTROL,
            code: KeyCode::Char('®'),
        }];
        let kb = Keybinding::with_alt(
            KeyCombo::alt(KeyCode::Char('r')),
            &ALTS,
            "Test",
            Action::Quit,
        );
        if cfg!(target_os = "macos") {
            assert_eq!(kb.display_keys(), "⌥r/⌃®");
        } else {
            assert_eq!(kb.display_keys(), "Alt+r/Ctrl+®");
        }
    }

    #[test]
    fn display_keys_multiple_alternatives() {
        static ALTS: [KeyCombo; 2] = [
            KeyCombo {
                modifiers: KeyModifiers::NONE,
                code: KeyCode::Down,
            },
            KeyCombo {
                modifiers: KeyModifiers::CONTROL,
                code: KeyCode::Char('n'),
            },
        ];
        let kb = Keybinding::with_alt(
            KeyCombo::plain(KeyCode::Char('j')),
            &ALTS,
            "Down",
            Action::NavDown,
        );
        if cfg!(target_os = "macos") {
            assert_eq!(kb.display_keys(), "j/↓/⌃n");
        } else {
            assert_eq!(kb.display_keys(), "j/↓/Ctrl+n");
        }
    }

    // ══════════════════════════════════════════════════════════════════
    //  Action enum coverage
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn action_quit_eq() {
        assert_eq!(Action::Quit, Action::Quit);
    }

    #[test]
    fn action_different_not_eq() {
        assert_ne!(Action::Quit, Action::Save);
    }

    #[test]
    fn action_debug_format() {
        let s = format!("{:?}", Action::ToggleHelp);
        assert_eq!(s, "ToggleHelp");
    }

    #[test]
    fn action_clone() {
        let a = Action::NavDown;
        let b = a;
        assert_eq!(a, b);
    }

    #[test]
    fn action_copy() {
        let a = Action::CycleFocusForward;
        let b = a;
        assert_eq!(a, b);
    }

    // ══════════════════════════════════════════════════════════════════
    //  KeyCombo equality
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn keycombo_eq() {
        let a = KeyCombo::ctrl(KeyCode::Char('c'));
        let b = KeyCombo::ctrl(KeyCode::Char('c'));
        assert_eq!(a, b);
    }

    #[test]
    fn keycombo_ne_different_mod() {
        let a = KeyCombo::ctrl(KeyCode::Char('c'));
        let b = KeyCombo::alt(KeyCode::Char('c'));
        assert_ne!(a, b);
    }

    #[test]
    fn keycombo_ne_different_code() {
        let a = KeyCombo::ctrl(KeyCode::Char('c'));
        let b = KeyCombo::ctrl(KeyCode::Char('v'));
        assert_ne!(a, b);
    }

    #[test]
    fn keycombo_clone() {
        let a = KeyCombo::plain(KeyCode::Char('x'));
        let b = a;
        assert_eq!(a, b);
    }

    #[test]
    fn keycombo_debug_format() {
        let kc = KeyCombo::plain(KeyCode::Char('a'));
        let s = format!("{:?}", kc);
        assert!(s.contains("KeyCombo"));
    }

    // ══════════════════════════════════════════════════════════════════
    //  HelpSection
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn help_section_fields() {
        static BINDINGS: [Keybinding; 0] = [];
        let hs = HelpSection {
            title: "Test",
            bindings: &BINDINGS,
        };
        assert_eq!(hs.title, "Test");
        assert!(hs.bindings.is_empty());
    }

    // ══════════════════════════════════════════════════════════════════
    //  Additional edge cases
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn matches_shift_non_alpha_not_special_cased() {
        // Shift+Enter is not an alpha key, so only exact match works
        let kc = KeyCombo::shift(KeyCode::Enter);
        assert!(kc.matches(KeyModifiers::SHIFT, KeyCode::Enter));
        assert!(!kc.matches(KeyModifiers::NONE, KeyCode::Enter));
    }

    #[test]
    fn display_f_key() {
        let kc = KeyCombo::plain(KeyCode::F(5));
        let s = kc.display();
        assert!(s.contains("5"));
    }

    #[test]
    fn display_ctrl_space() {
        let kc = KeyCombo::ctrl(KeyCode::Char(' '));
        if cfg!(target_os = "macos") {
            assert_eq!(kc.display(), "⌃Space");
        } else {
            assert_eq!(kc.display(), "Ctrl+Space");
        }
    }

    #[test]
    fn keybinding_matches_shift_alt_through_alternatives() {
        static ALTS: [KeyCombo; 1] = [KeyCombo {
            modifiers: KeyModifiers::SHIFT,
            code: KeyCode::Char('G'),
        }];
        let kb = Keybinding::with_alt(
            KeyCombo::plain(KeyCode::Char('g')),
            &ALTS,
            "Go to bottom",
            Action::GoToBottom,
        );
        // Primary match
        assert!(kb.matches(KeyModifiers::NONE, KeyCode::Char('g')));
        // Alt match (SHIFT + G)
        assert!(kb.matches(KeyModifiers::SHIFT, KeyCode::Char('G')));
        // Alt match (NONE + G, legacy terminal)
        assert!(kb.matches(KeyModifiers::NONE, KeyCode::Char('G')));
    }

    #[test]
    fn keybinding_with_alt_description() {
        static ALTS: [KeyCombo; 1] = [KeyCombo {
            modifiers: KeyModifiers::NONE,
            code: KeyCode::Down,
        }];
        let kb = Keybinding::with_alt(
            KeyCombo::plain(KeyCode::Char('j')),
            &ALTS,
            "Navigate down",
            Action::NavDown,
        );
        assert_eq!(kb.description, "Navigate down");
    }

    #[test]
    fn keybinding_action_stored_correctly() {
        let kb = Keybinding::new(KeyCombo::ctrl(KeyCode::Char('s')), "Save", Action::Save);
        assert_eq!(kb.action, Action::Save);
    }

    #[test]
    fn keybinding_paired_preserves_other_fields() {
        let kb = Keybinding::new(KeyCombo::plain(KeyCode::Char('k')), "Up", Action::NavUp).paired();
        assert_eq!(kb.primary.code, KeyCode::Char('k'));
        assert_eq!(kb.description, "Up");
        assert_eq!(kb.action, Action::NavUp);
        assert!(kb.pair_with_next);
    }

    #[test]
    fn display_keys_ctrl_binding() {
        let kb = Keybinding::new(KeyCombo::ctrl(KeyCode::Char('z')), "Undo", Action::Undo);
        if cfg!(target_os = "macos") {
            assert_eq!(kb.display_keys(), "⌃z");
        } else {
            assert_eq!(kb.display_keys(), "Ctrl+z");
        }
    }

    #[test]
    fn display_keys_special_key_binding() {
        let kb = Keybinding::new(KeyCombo::plain(KeyCode::Esc), "Escape", Action::Escape);
        assert_eq!(kb.display_keys(), "Esc");
    }
}
