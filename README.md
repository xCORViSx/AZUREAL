<p align="center">
  <img src="azureal_icon.png" alt="Azureal" width="180" />
</p>

<h1 align="center">azureal</h1>

<p align="center">
  <strong>Agent-Zoned Unified Runtime Environment for Autonomous LLMs</strong>
</p>

<p align="center">
  A multi-session Claude Code manager with git worktree isolation
</p>

---

## Features

- **Multi-Worktree Sessions** — Run multiple Claude Code agents concurrently, each in its own git worktree
- **4-Pane TUI** — Worktrees, FileTree, Viewer, and Convo panes with Tab cycling
- **File Browser** — Navigate worktree files with expand/collapse, open in syntax-highlighted Viewer
- **Vim-Style Input** — Modal editing with command/prompt modes (red/yellow border)
- **Embedded Terminal** — Full PTY-based shell per worktree with color support
- **Real-time Output** — Live-polls Claude's session file; output updates as Claude responds
- **Markdown Rendering** — Headers, bold, italic, code blocks, tables rendered with proper styling
- **Clickable Edit Links** — Click file paths in Convo to view diffs in the Viewer pane
- **Incremental Parsing** — Large session files parsed incrementally (only new lines since last read)
- **Mouse Support** — Scroll panels by cursor position, Shift+drag for text selection, click file links
- **Diff Viewer** — Syntax-highlighted git diffs per worktree
- **Creation Wizard** — Tabbed dialog for creating worktrees and sessions
- **Run Commands** — Save and execute shell commands/scripts per project
- **Hook Display** — All Claude Code hook types displayed inline in conversation
- **Rebase Support** — Interactive rebase with conflict detection and resolution
- **Zero Footprint** — No database or config files; scans git worktrees and `~/.claude/` at runtime

## Requirements

- **Rust** (latest stable)
- **Claude Code CLI** (`npm install -g @anthropic-ai/claude-code`)
- **Git** with worktree support

## Installation

```bash
cargo install --path .
```

## Usage

```bash
# Launch the TUI
azureal tui

# Or just
azureal
```

### Keybindings

| Key | Action |
|-----|--------|
| `p` | Enter prompt mode (focus input) |
| `t` | Toggle terminal pane |
| `Esc` | Return to command mode |
| `j/k` | Navigate (worktrees, files, scroll) |
| `Tab` | Cycle focus (Worktrees > FileTree > Viewer > Convo > Input) |
| `n` | New worktree/session (creation wizard) |
| `r` | Run command |
| `d` | View diff |
| `Space` | Context menu / Toggle expand |
| `?` | Help |
| `Ctrl+X` | Cancel running Claude response |
| `Ctrl+c` | Quit |

**Input Modes:**

- Red border = Command mode (keys are commands)
- Yellow border = Prompt mode (typing to Claude)
- Cyan border = Terminal mode (typing shell commands)

## Architecture

Azureal is **mostly stateless** — all runtime state is derived from:

- Git worktrees via `git worktree list`
- Git branches via `git branch | grep azureal/`
- Claude's session files at `~/.claude/projects/<encoded-path>/*.jsonl`

No database. No config files required. Optional `.azureal/sessions.toml` stores custom session name mappings.

## License

MIT
