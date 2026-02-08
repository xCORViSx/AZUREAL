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
- **Vim-Style Input** — Modal editing with command/prompt modes, multi-line via Shift+Enter
- **Embedded Terminal** — Full PTY-based shell per worktree with color support
- **Real-time Output** — Live-polls Claude's session file; output updates as Claude responds
- **Markdown Rendering** — Headers, bold, italic, code blocks, tables rendered with proper styling
- **Clickable Edit Links** — Click file paths in Convo to view diffs in the Viewer pane
- **Async Rendering** — Convo pane renders on a background thread; input is never blocked by markdown/syntax processing
- **Incremental Parsing** — Large session files parsed incrementally (only new lines since last read)
- **Mouse Support** — Click to focus panes, select sessions/files, position cursor; drag to select text; scroll by cursor position; Cmd+C to copy selection
- **Diff Viewer** — Syntax-highlighted git diffs per worktree
- **Creation Wizard** — Tabbed dialog for creating worktrees and sessions
- **Run Commands** — Save and execute shell commands/scripts per project
- **Hook Display** — All Claude Code hook types displayed inline in conversation
- **Token Usage Counter** — Color-coded context window percentage on Convo pane border (green/yellow/red) to predict compaction
- **TodoWrite Widget** — Sticky checkbox list at bottom of Convo pane showing Claude's task progress (✓/●/○)
- **AskUserQuestion Box** — Numbered options box for Claude's questions; respond with a number or custom text
- **Session Search/Filter** — Press `/` in Worktrees to filter sessions by name with live matching
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
| `r` | Run command (picker or execute) |
| `⌥r` | Add new run command |
| `R` | Rebase onto main |
| `d` | View diff |
| `/` | Search/filter sessions (in Worktrees) |
| `Space` | Context menu / Toggle expand |
| `?` | Help |
| `⌃c` | Cancel running Claude response |
| `⌃q` | Quit |
| `⌃r` | Restart |

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

**Rendering:** The convo pane uses a dedicated background thread for expensive rendering (markdown parsing, syntax highlighting, text wrapping). The main event loop sends non-blocking render requests via channels and polls for results. During typing, keystrokes get instant visual feedback via direct crossterm writes (~0.1ms) while the expensive `terminal.draw()` (~18ms) is deferred to quiet frames. This two-tier rendering ensures input is never blocked by screen updates.

## License

MIT