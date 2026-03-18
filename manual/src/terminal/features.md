# Terminal Features

The embedded terminal goes beyond basic text I/O. It supports full ANSI colors,
accurate cursor positioning, dynamic resizing, and mouse-driven text selection
with clipboard integration.

---

## Full Color Support

Terminal output is rendered with complete ANSI color support via the
`ansi-to-tui` crate. This translates ANSI escape sequences into styled TUI
spans, meaning you see the same colors in AZUREAL's terminal that you would see
in a standalone terminal emulator:

- Standard 16 colors (bold/dim variants)
- 256-color palette
- 24-bit true color (RGB)
- Text attributes: bold, italic, underline, strikethrough, inverse

Programs like `git diff`, `ls --color`, `bat`, `delta`, and other color-aware
CLI tools render correctly without any special configuration beyond the
`TERM=xterm-256color` environment variable that AZUREAL sets automatically.

---

## Cursor Positioning

The terminal tracks cursor position using a `vt100` parser that interprets
cursor movement escape sequences in real time. This means programs that
reposition the cursor -- progress bars, interactive prompts, `top`-style
dashboards -- render accurately.

The cursor position is displayed as a blinking indicator in the terminal pane
when in type mode, showing exactly where the next character will be inserted.

---

## Dynamic Resizing

The terminal pane dynamically resizes to match its allocated dimensions in the
layout. Resizing happens in two scenarios:

1. **Manual resize** -- Pressing `+` or `-` in terminal command mode adjusts
   the pane height between 5 and 40 lines.
2. **Layout reflow** -- When the overall terminal window is resized, the
   terminal pane adjusts its width (and potentially height) to fit the new
   layout dimensions.

On each resize, the PTY is notified of the new dimensions so that programs
running inside it can reflow their output correctly. This means full-screen
programs like `vim`, `htop`, or `less` adapt to the available space.

---

## Text Selection

Text selection in the terminal uses mouse drag with automatic scrolling:

1. **Click and drag** to start a selection. The selected text is highlighted.
2. **Drag past the top or bottom edge** of the pane to auto-scroll through the
   terminal history while extending the selection.
3. **Release** to finalize the selection.

Selections are tracked in scrollback-adjusted absolute coordinates. This means
that if you scroll up into history, select text, and then the terminal scrolls
further, the selection remains anchored to the correct text.

### Clipboard Copy

With an active selection:

- **macOS**: `Cmd+C` copies the selected text to the system clipboard.
- **Linux / Windows**: `Ctrl+C` copies the selected text to the system
  clipboard.

Without an active selection, the same key combination sends the standard
interrupt signal (`SIGINT`) to the PTY, which is the expected behavior for
canceling a running command.

---

## Scrollback History

The terminal maintains a scrollback buffer that you can navigate with the mouse
wheel. Scrolling up moves through command history and output; scrolling down
returns toward the current output. The scrollback buffer preserves all output
from the shell session.

---

## Quick Reference

| Feature | Details |
|---------|---------|
| Color support | ANSI 16, 256, and 24-bit true color |
| Cursor tracking | vt100 parser for accurate positioning |
| Resize range | 5 to 40 lines (`+`/`-` keys) |
| Selection | Mouse drag with auto-scroll |
| Clipboard | `Cmd+C` / `Ctrl+C` with active selection |
| Scrollback | Mouse wheel navigation |
| Per-worktree | Independent shell per worktree |
| Environment | `TERM=xterm-256color` set automatically |
