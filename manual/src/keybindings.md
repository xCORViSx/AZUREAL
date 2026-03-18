# Keybindings & Input Modes

AZUREAL is a keyboard-driven application. Every action is reachable from the keyboard, and the keybinding system is designed around two principles: **vim-style modal input** and **a single source of truth**.

---

## Modal Input

The interface operates in one of four modes at any given time, each indicated by a colored border on the focused pane:

| Mode | Border Color | Purpose |
|------|-------------|---------|
| Command | Red | Keys trigger actions -- nothing is typed |
| Prompt | Yellow | Keys are typed as text for an agent prompt |
| Terminal | Azure | Keys are forwarded to the embedded PTY shell |
| Speech Recording | Magenta | Audio is being captured for transcription |

You always start in command mode. Pressing `p` enters prompt mode. Pressing `T` toggles the terminal (entering terminal mode when it gains focus). `Esc` returns to command mode from any other mode.

Mode determines what a keystroke does. The same physical key can trigger a global action in command mode, insert a character in prompt mode, or send input to a shell in terminal mode. This is covered in detail in [Vim-Style Modes](./keybindings/vim-modes.md).

---

## Centralized Keybinding System

All keybindings are defined once in the `keybindings` module. The function `lookup_action()` is the single entry point that resolves a key event into an action. Individual input handlers never define their own bindings -- they only handle keys that `lookup_action()` did not resolve.

This centralized design means:

- **No duplicate bindings.** A key combination maps to exactly one action in a given context.
- **Guard logic is consistent.** Globals never fire during text input, edit mode, or active filters.
- **Modal panels use per-modal lookup functions.** The health panel, git panel, and projects panel each have their own lookup function for panel-specific keys, but these are still defined in the keybindings module alongside everything else.
- **The help overlay reads from the same source.** What `?` displays is always in sync with what the keys actually do.

---

## Chapter Contents

- **[Vim-Style Modes](./keybindings/vim-modes.md)** -- The four input modes, how they interact, and how the border color tells you where you are.
- **[Global Keybindings](./keybindings/global.md)** -- The full table of keys available in command mode.
- **[Leader Sequences](./keybindings/leader-sequences.md)** -- Multi-key sequences starting with `W` for worktree operations.
- **[Platform Differences](./keybindings/platform-differences.md)** -- macOS vs. Windows/Linux modifier key mappings, symbol rendering, and the Option key workaround.
- **[Help Overlay](./keybindings/help-overlay.md)** -- The `?` overlay that shows all keybindings organized by section.
