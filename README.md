<p align="center">
  <img src="azural_icon.png" alt="Azural" width="180" />
</p>

<h1 align="center">Azural</h1>

<p align="center">
  <strong>Agent Zones: Unified Runtime for Autonomous LLMs</strong>
</p>

<p align="center">
  A multi-session Claude Code manager with git worktree isolation
</p>

---

## Features

- **Session Management** — Create, switch, and manage multiple Claude Code sessions
- **Git Worktree Isolation** — Each session runs in its own worktree for clean separation
- **TUI Interface** — Terminal UI for navigating sessions, viewing output, and diffs
- **Vim-Style Input** — Modal editing with command/insert modes (red/yellow border)
- **Embedded Terminal** — Full PTY-based shell terminal with color support in the worktree
- **Real-time Output** — Stream Claude's responses with ANSI color support
- **Markdown Rendering** — Headers, bold, italic, code blocks, tables rendered with proper styling
- **Mouse Scroll** — Scroll panels based on cursor position (independent of focus)
- **Diff Viewer** — Syntax-highlighted diffs showing changes per session
- **Rebase Support** — Interactive rebase with conflict detection

## Requirements

- **Claude Code ≤ 2.1.18** — Version 2.1.19 has a bug breaking `-p --resume` with tool calls ([#20508](https://github.com/anthropics/claude-code/issues/20508)). Install 2.1.18 if needed:
  ```bash
  npm install -g @anthropic-ai/claude-code@2.1.18
  ```

## Hook Display

Azural automatically displays Claude Code hook output in the output pane:
- **SessionStart** hooks appear via `hook_response` events
- **PreToolUse/PostToolUse** hooks appear via `hook_progress` events
- **UserPromptSubmit** hooks appear from system-reminder tags in the session file

## Installation

```bash
cargo install --path .
```

## Usage

```bash
# Launch the TUI
azural tui

# Or just
azural
```

### Keybindings

| Key | Action |
|-----|--------|
| `i` | Enter inprompt mode (start typing) |
| `t` | Toggle terminal pane |
| `Esc` | Return to command mode |
| `j/k` | Navigate sessions |
| `J/K` | Navigate projects |
| `Tab` | Cycle focus (sessions → output → input) |
| `n` | New session |
| `b` | Browse branches |
| `d` | View diff |
| `r` | Rebase onto main |
| `Space` | Context menu |
| `?` | Help |
| `Ctrl+c` | Quit |

**Input Modes:**
- Red border = Command mode (keys are commands)
- Yellow border = Inprompt mode (typing to Claude)
- Cyan border = Terminal mode (typing shell commands)

## License

MIT


non-intrusive : only the binary added to PATH ; no config or database files outside repo azural is working with

No file footprint - no database or config files / directly scans git worktrees and ~/.claude directory for session data