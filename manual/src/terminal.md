# The Embedded Terminal

AZUREAL includes a PTY-based embedded terminal that acts as a portal directly
into your shell. Rather than switching to a separate terminal application to run
commands, you can toggle a terminal pane inline, type commands, see full-color
output, and return to your normal workflow -- all without leaving the TUI.

---

## How It Works

The terminal is built on `portable-pty`, a cross-platform pseudo-terminal
library. When you open the terminal, AZUREAL spawns your detected shell inside a
real PTY, meaning programs that expect a terminal (colored output, interactive
prompts, curses-based tools) work correctly. The terminal pane renders the PTY
output using `ansi-to-tui` for full ANSI color support and `vt100` for cursor
positioning.

Each worktree gets its own terminal shell instance. When you switch worktrees,
the terminal switches to that worktree's shell (spawning one if it does not
already exist), with the working directory set to the worktree's root. This
keeps every branch's terminal activity isolated.

---

## Two Modes

The terminal operates in two distinct input modes:

- **Terminal command mode** -- The terminal is visible and you can interact with
  AZUREAL's keybindings normally. Global keys like `G`, `H`, `M`, `P`, and
  bracket navigation all work. Press `t` to drop into type mode.
- **Terminal type mode** -- All keystrokes are forwarded directly to the PTY.
  AZUREAL keybindings are suspended. Press `Esc` to return to command mode.

This separation means you never accidentally send a keystroke to the wrong
target. See [Terminal Modes](./terminal/modes.md) for the full breakdown.

---

## Visual Indicators

When the terminal pane is active (focused), its border turns **azure** to
clearly indicate where input is going. The border returns to its default color
when you move focus elsewhere.

The terminal pane sits at the bottom of the layout. Its height is adjustable
with `+` and `-` in terminal command mode, ranging from 5 to 40 lines.

---

## Chapter Contents

- **[Terminal Modes](./terminal/modes.md)** -- The two input modes, their
  keybindings, and how to switch between them.
- **[Shell Integration](./terminal/shell.md)** -- How AZUREAL detects and
  launches your shell across platforms.
- **[Terminal Features](./terminal/features.md)** -- Color rendering, cursor
  positioning, resizing, mouse interaction, and clipboard support.
