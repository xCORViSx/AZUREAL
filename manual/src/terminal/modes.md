# Terminal Modes

The embedded terminal uses two input modes to cleanly separate AZUREAL
navigation from shell interaction. At any given moment, the terminal is in
exactly one of these modes.

---

## Terminal Command Mode

Terminal command mode is the default state when the terminal pane is open. You
can see the terminal output and interact with AZUREAL normally.

### Entering Command Mode

| From | Key | Effect |
|------|-----|--------|
| Any pane (terminal closed) | `T` (Shift+T) | Toggle terminal open in command mode |
| Terminal type mode | `Esc` | Exit type mode, enter command mode |

### Available Keys in Command Mode

All global keybindings remain active:

| Key | Action |
|-----|--------|
| `G` | Open Git panel |
| `H` | Open Health panel |
| `M` | Browse main branch |
| `P` | Open Projects panel |
| `T` | Toggle terminal (close) |
| `[` / `]` | Switch worktrees |
| `r` | Open run commands |
| `t` | Enter terminal type mode |
| `p` | Close terminal / refocus prompt |
| `Esc` | Close terminal |
| `+` | Increase terminal height |
| `-` | Decrease terminal height |

The `p` key provides a quick way to dismiss the terminal and return focus to the
prompt. It works from terminal command mode and also from any mode where the
prompt is tabbed away.

### Resizing

The terminal pane height is adjustable in command mode:

- **`+`** increases height by one line.
- **`-`** decreases height by one line.
- Height is clamped between **5 lines** (minimum) and **40 lines** (maximum).

The resize takes effect immediately and the terminal content reflows to match
the new dimensions.

---

## Terminal Type Mode

Type mode forwards all keystrokes directly to the PTY. AZUREAL keybindings are
completely suspended -- everything you type goes to the shell.

### Entering Type Mode

| From | Action | Effect |
|------|--------|--------|
| Terminal command mode | Press `t` | Enter type mode |
| Any mode | Click inside terminal pane | Enter type mode, reposition cursor |

Clicking inside the terminal pane is a shortcut that both enters type mode and
moves the cursor to the clicked position within the shell's input line.

### Available Keys in Type Mode

| Key | Action |
|-----|--------|
| `Esc` | Exit type mode (return to command mode) |
| `Alt+Left` or `Ctrl+Left` | Word navigation backward |
| `Alt+Right` or `Ctrl+Right` | Word navigation forward |
| All other keys | Forwarded to PTY |

Word navigation sends readline-compatible escape sequences to the PTY:
`\x1bb` for backward and `\x1bf` for forward. This works in bash, zsh, and
other readline-based shells.

### Enter Key Behavior

The Enter key sends `\r` (carriage return) to the PTY, not `\n` (line feed).
This matches standard terminal behavior and ensures proper command execution
across all shells.

---

## Mouse Interaction

Mouse input works in both modes:

| Action | Effect |
|--------|--------|
| Click inside terminal | Enter type mode, reposition cursor |
| Mouse drag | Select text with auto-scroll |
| Mouse wheel | Scroll through terminal history |
| `Cmd+C` / `Ctrl+C` (with selection) | Copy selected text to clipboard |

Text selection operates in scrollback-adjusted absolute coordinates, meaning
selections remain accurate even when scrolled through history. Dragging past the
top or bottom edge of the terminal pane triggers automatic scrolling.

When `Cmd+C` or `Ctrl+C` is pressed with an active selection, the selected text
is copied to the system clipboard. Without a selection, the standard interrupt
signal is sent to the PTY instead.

---

## Quick Reference

```text
T (Shift+T)       Toggle terminal open/closed
t                  Enter type mode (keystrokes go to shell)
Esc                Exit type mode / close terminal
p                  Close terminal / refocus prompt
+/-                Resize terminal height (5-40 lines)
Click              Enter type mode + reposition cursor
Drag               Select text
Cmd+C / Ctrl+C     Copy selection (or interrupt if no selection)
```
