//! Shared key combo constants and alternative key arrays
//!
//! Platform-conditional key combos (`KEY_*`), modifier combos (`CMD_SHIFT`),
//! and alternative key arrays (`ALT_*`) used by binding arrays in sibling modules.

use super::super::types::KeyCombo;
use crossterm::event::{KeyCode, KeyModifiers};

// ── Alternative key arrays ───────────────────────────────────────────────────
// Enter/m alternative for health panel modularize action
pub(super) static ALT_CHAR_M: [KeyCombo; 1] = [KeyCombo {
    modifiers: KeyModifiers::NONE,
    code: KeyCode::Char('m'),
}];
// Enter/d alternative for git panel view-diff action
pub(super) static ALT_CHAR_D: [KeyCombo; 1] = [KeyCombo {
    modifiers: KeyModifiers::NONE,
    code: KeyCode::Char('d'),
}];
pub(super) static ALT_DOWN: [KeyCombo; 1] = [KeyCombo {
    modifiers: KeyModifiers::NONE,
    code: KeyCode::Down,
}];
pub(super) static ALT_UP: [KeyCombo; 1] = [KeyCombo {
    modifiers: KeyModifiers::NONE,
    code: KeyCode::Up,
}];
pub(super) static ALT_LEFT: [KeyCombo; 1] = [KeyCombo {
    modifiers: KeyModifiers::NONE,
    code: KeyCode::Left,
}];
pub(super) static ALT_RIGHT: [KeyCombo; 1] = [KeyCombo {
    modifiers: KeyModifiers::NONE,
    code: KeyCode::Right,
}];
// ⌃← alternative for ⌥← (word nav in prompt input)
pub(super) static ALT_CTRL_LEFT: [KeyCombo; 1] = [KeyCombo {
    modifiers: KeyModifiers::CONTROL,
    code: KeyCode::Left,
}];
pub(super) static ALT_CTRL_RIGHT: [KeyCombo; 1] = [KeyCombo {
    modifiers: KeyModifiers::CONTROL,
    code: KeyCode::Right,
}];
// ⌃Backspace alternative for ⌃w delete word (non-macOS)
pub(super) static ALT_DELETE_WORD: [KeyCombo; 1] = [KeyCombo {
    modifiers: KeyModifiers::CONTROL,
    code: KeyCode::Backspace,
}];
// PageUp/PageDown/Home/End alternatives for viewer scroll
pub(super) static ALT_PGDN: [KeyCombo; 1] = [KeyCombo {
    modifiers: KeyModifiers::NONE,
    code: KeyCode::PageDown,
}];
pub(super) static ALT_PGUP: [KeyCombo; 1] = [KeyCombo {
    modifiers: KeyModifiers::NONE,
    code: KeyCode::PageUp,
}];
pub(super) static ALT_HOME: [KeyCombo; 1] = [KeyCombo {
    modifiers: KeyModifiers::NONE,
    code: KeyCode::Home,
}];
pub(super) static ALT_END: [KeyCombo; 1] = [KeyCombo {
    modifiers: KeyModifiers::NONE,
    code: KeyCode::End,
}];
// Alt+M fallback for Ctrl+M (CycleModel) — without Kitty protocol, Ctrl+M is
// indistinguishable from Enter (both send 0x0D). On Linux/Windows, Alt+M sends
// ESC+'m', always unique. On macOS, ⌥m produces 'µ' (unicode) — added as a
// bare-char alternative (same pattern as ⌥p→π, ⌥r→®). WezTerm on macOS does
// NOT honor PushKeyboardEnhancementFlags despite claiming Kitty support via
// TERM_PROGRAM, so the macOS fallback is required.
#[cfg(not(target_os = "macos"))]
pub(super) static ALT_CYCLE_MODEL: [KeyCombo; 1] = [KeyCombo {
    modifiers: KeyModifiers::ALT,
    code: KeyCode::Char('m'),
}];
#[cfg(target_os = "macos")]
pub(super) static ALT_CYCLE_MODEL: [KeyCombo; 1] = [KeyCombo {
    modifiers: KeyModifiers::NONE,
    code: KeyCode::Char('µ'),
}];
// Alt+Enter and Ctrl+J fallbacks for Shift+Enter (InsertNewline).
// Shift+Enter works in most terminals even without Kitty protocol.
// NOTE: WezTerm on macOS intercepts Alt+Enter for fullscreen toggle. Users must add
// `{ key = "Enter", mods = "ALT", action = wezterm.action.DisableDefaultAssignment }`
// to their WezTerm config, or use Ctrl+J (0x0A, distinct from Enter's 0x0D).
// Alt+Enter first — shown in hints (works on most terminals).
// Ctrl+J second — silent fallback for WezTerm (which steals Alt+Enter for fullscreen).
pub(super) static ALT_INSERT_NEWLINE: [KeyCombo; 2] = [
    KeyCombo {
        modifiers: KeyModifiers::ALT,
        code: KeyCode::Enter,
    },
    KeyCombo {
        modifiers: KeyModifiers::CONTROL,
        code: KeyCode::Char('j'),
    },
];
// macOS ⌥p produces 'π' (unicode) instead of ALT+p — add as alternative
pub(super) static ALT_MACOS_P: [KeyCombo; 1] = [KeyCombo {
    modifiers: KeyModifiers::NONE,
    code: KeyCode::Char('π'),
}];
pub(super) static ALT_MACOS_T: [KeyCombo; 1] = [KeyCombo {
    modifiers: KeyModifiers::NONE,
    code: KeyCode::Char('†'),
}];
// Shift+[ → '{' — some terminals send (SHIFT, '{'), others (NONE, '{')
pub(super) static ALT_LBRACE: [KeyCombo; 1] = [KeyCombo {
    modifiers: KeyModifiers::NONE,
    code: KeyCode::Char('{'),
}];
pub(super) static ALT_RBRACE: [KeyCombo; 1] = [KeyCombo {
    modifiers: KeyModifiers::NONE,
    code: KeyCode::Char('}'),
}];
// Alt+J/K recursive expand/collapse: primary Alt+Right/Left, alts for ⌥j(∆)/⌥k(˚) on macOS
pub(super) static ALT_RECURSIVE_EXPAND: [KeyCombo; 2] = [
    KeyCombo {
        modifiers: KeyModifiers::ALT,
        code: KeyCode::Char('j'),
    },
    KeyCombo {
        modifiers: KeyModifiers::NONE,
        code: KeyCode::Char('∆'),
    }, // macOS ⌥j
];
pub(super) static ALT_RECURSIVE_COLLAPSE: [KeyCombo; 2] = [
    KeyCombo {
        modifiers: KeyModifiers::ALT,
        code: KeyCode::Char('k'),
    },
    KeyCombo {
        modifiers: KeyModifiers::NONE,
        code: KeyCode::Char('˚'),
    }, // macOS ⌥k
];

// ── Modifier combos ──────────────────────────────────────────────────────────

#[cfg(target_os = "macos")]
pub(super) const CMD_SHIFT: KeyModifiers =
    KeyModifiers::from_bits_truncate(KeyModifiers::SUPER.bits() | KeyModifiers::SHIFT.bits());
#[allow(dead_code)] // Used on non-macOS targets
pub(super) const CTRL_SHIFT: KeyModifiers =
    KeyModifiers::from_bits_truncate(KeyModifiers::CONTROL.bits() | KeyModifiers::SHIFT.bits());

// ── Platform-conditional key combos ──────────────────────────────────────────
// macOS: ⌘ bindings (Cmd key). Windows/Linux: Ctrl or Ctrl+Shift equivalents.
// Super (Win key) is intercepted by the OS on Windows — terminals never receive it.
//
// On macOS, ⌘ keys require Kitty keyboard protocol — without it, SUPER modifier
// is intercepted by the terminal (copy, quit, etc.) and never reaches the app.
// WezTerm on macOS ignores PushKeyboardEnhancementFlags entirely. Each ⌘ binding
// has an ⌥+letter fallback (same pattern as CycleModel's ⌥m→µ): the macOS Option
// key always produces a distinct unicode char regardless of protocol support.

#[cfg(target_os = "macos")]
pub(super) const KEY_COPY: KeyCombo = KeyCombo::cmd(KeyCode::Char('c'));
#[cfg(not(target_os = "macos"))]
pub(super) const KEY_COPY: KeyCombo = KeyCombo::ctrl(KeyCode::Char('c'));

// macOS ⌥c → ç (copy fallback for terminals without Kitty/SUPER support)
#[cfg(target_os = "macos")]
pub(super) static ALT_COPY: [KeyCombo; 1] = [KeyCombo {
    modifiers: KeyModifiers::NONE,
    code: KeyCode::Char('ç'),
}];

#[cfg(target_os = "macos")]
pub(super) const KEY_CANCEL: KeyCombo = KeyCombo::ctrl(KeyCode::Char('c'));
// Ctrl+Shift+C is Windows Terminal's copy — use Alt+c instead.
// Without Kitty keyboard protocol, Ctrl+Shift+C arrives as Ctrl+C anyway.
#[cfg(not(target_os = "macos"))]
pub(super) const KEY_CANCEL: KeyCombo = KeyCombo::new(KeyModifiers::ALT, KeyCode::Char('c'));


#[cfg(target_os = "macos")]
pub(super) const KEY_SELECT_ALL: KeyCombo = KeyCombo::cmd(KeyCode::Char('a'));
#[cfg(not(target_os = "macos"))]
pub(super) const KEY_SELECT_ALL: KeyCombo = KeyCombo::ctrl(KeyCode::Char('a'));

// macOS ⌥a → å (select-all fallback)
#[cfg(target_os = "macos")]
pub(super) static ALT_SELECT_ALL: [KeyCombo; 1] = [KeyCombo {
    modifiers: KeyModifiers::NONE,
    code: KeyCode::Char('å'),
}];

#[cfg(target_os = "macos")]
pub(super) const KEY_SAVE: KeyCombo = KeyCombo::cmd(KeyCode::Char('s'));
#[cfg(not(target_os = "macos"))]
pub(super) const KEY_SAVE: KeyCombo = KeyCombo::ctrl(KeyCode::Char('s'));

// macOS ⌥s → ß (save fallback — no conflict with STT's ⌃s, different modifier)
#[cfg(target_os = "macos")]
pub(super) static ALT_SAVE: [KeyCombo; 1] = [KeyCombo {
    modifiers: KeyModifiers::NONE,
    code: KeyCode::Char('ß'),
}];

#[cfg(target_os = "macos")]
pub(super) const KEY_UNDO: KeyCombo = KeyCombo::cmd(KeyCode::Char('z'));
#[cfg(not(target_os = "macos"))]
pub(super) const KEY_UNDO: KeyCombo = KeyCombo::ctrl(KeyCode::Char('z'));

// macOS ⌥z → Ω (undo fallback)
#[cfg(target_os = "macos")]
pub(super) static ALT_UNDO: [KeyCombo; 1] = [KeyCombo {
    modifiers: KeyModifiers::NONE,
    code: KeyCode::Char('Ω'),
}];

#[cfg(target_os = "macos")]
pub(super) const KEY_REDO: KeyCombo = KeyCombo::new(CMD_SHIFT, KeyCode::Char('Z'));
#[cfg(not(target_os = "macos"))]
pub(super) const KEY_REDO: KeyCombo = KeyCombo::ctrl(KeyCode::Char('y'));

// macOS redo fallback: ⌃y (same as Win/Linux — sends 0x19, always distinct)
#[cfg(target_os = "macos")]
pub(super) static ALT_REDO: [KeyCombo; 1] = [KeyCombo {
    modifiers: KeyModifiers::CONTROL,
    code: KeyCode::Char('y'),
}];

// STT in edit mode: ⌃s on macOS (no conflict with ⌘s Save), ⌃⇧S on non-macOS (⌃s is Save)
#[cfg(target_os = "macos")]
pub(super) const KEY_EDIT_STT: KeyCombo = KeyCombo::ctrl(KeyCode::Char('s'));
// Ctrl+Shift+S not reliably delivered on Windows without Kitty protocol
#[cfg(not(target_os = "macos"))]
pub(super) const KEY_EDIT_STT: KeyCombo = KeyCombo::new(KeyModifiers::ALT, KeyCode::Char('s'));
