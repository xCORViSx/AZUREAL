# Vim-Style Modes

AZUREAL borrows the modal editing concept from Vim: a keystroke's meaning depends on which mode is active. There are four modes, each visually indicated by the border color of the focused pane.

---

## Command Mode (Red Border)

Command mode is the default. When the application launches, you are in command mode.

In this mode, every key press is interpreted as an action. Typing `j` scrolls down one line -- it does not insert the letter "j" anywhere. Typing `p` enters prompt mode. Typing `?` opens the help overlay.

No text input occurs in command mode. It is purely navigational.

**Enter command mode** by pressing `Esc` from any other mode.

---

## Prompt Mode (Yellow Border)

Prompt mode is for composing agent prompts. Pressing `p` from command mode enters prompt mode. The input box appears at the bottom of the session pane, and keystrokes are typed as text.

### Submitting a Prompt

Press `Enter` to submit the prompt to the active agent backend.

If the agent is already running when you press `Enter`, the behavior changes: the current agent run is cancelled and the prompt text is stored as a **staged prompt**. Once cancellation completes, the staged prompt is automatically sent to the agent. This lets you interrupt and redirect an agent without losing your next instruction.

### Multi-Line Input

Press `Shift+Enter` to insert a newline instead of submitting. The input box grows dynamically to accommodate multi-line prompts, expanding up to three-quarters of the terminal height. Once the prompt is submitted or cleared, the input box shrinks back to a single line.

### Returning to Command Mode

Press `Esc` to leave prompt mode and return to command mode. The input box content is preserved -- re-entering prompt mode with `p` restores whatever was typed.

---

## Terminal Mode (Azure Border)

Terminal mode is active when the embedded terminal pane has focus. In this mode, all keystrokes are forwarded directly to the PTY shell running inside the terminal.

Toggle the terminal with `T` (Shift+T) from command mode. When the terminal is shown and focused, you are in terminal mode. Press `Esc` to return to command mode without closing the terminal -- or press `T` again to hide it entirely.

The terminal runs a full shell per worktree. See [The Embedded Terminal](../terminal.md) for details on shell integration and terminal-specific features.

---

## Speech Recording Mode (Magenta Border)

Speech recording mode is active while audio is being captured for speech-to-text transcription. The border turns magenta to indicate that the microphone is live.

This mode is entered and exited via the speech-to-text keybinding (see [Speech-to-Text](../speech-to-text.md)). While recording, most other keybindings are suppressed to avoid accidental actions.

---

## How Modes Interact

Only one mode is active at a time. The active mode is always visible from the border color of the focused pane. The flow between modes follows a simple pattern:

```text
              p
Command ───────────> Prompt
  ^  ^                 │
  │  │     Esc          │
  │  │<────────────────┘
  │  │
  │  │     Esc
  │  │<────────────────┐
  │  │                 │
  │  T ───────────> Terminal
  │
  │        Esc
  └────────────────── Speech
```

`Esc` is the universal escape hatch. From any mode, pressing `Esc` returns to command mode. This means you never need to remember a mode-specific exit key -- `Esc` always works.

---

## Guard Logic

The centralized keybinding system includes guard logic that prevents global keybindings from firing when they would conflict with text input:

- **Prompt mode**: Letter keys type text. Global bindings like `j` (scroll) and `p` (enter prompt mode) are suppressed.
- **Terminal mode**: All keys are forwarded to the PTY. No global bindings fire.
- **Edit mode** (in the file viewer): Letter keys type text into the editor. Global bindings are suppressed.
- **Active filters**: When a search/filter input is active (e.g., in the session list), letter keys type into the filter field.

Only `Esc` and a small set of modifier-key combinations (like `Ctrl+Q` to quit) remain active across all modes.
