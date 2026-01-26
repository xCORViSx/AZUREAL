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
- **Real-time Output** — Stream Claude's responses with ANSI color support
- **Diff Viewer** — Syntax-highlighted diffs showing changes per session
- **Rebase Support** — Interactive rebase with conflict detection

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
| `j/k` | Navigate sessions |
| `J/K` | Navigate projects |
| `Tab` | Cycle focus (sessions → output → input) |
| `n` | New session |
| `b` | Browse branches |
| `d` | View diff |
| `r` | Rebase onto main |
| `Space` | Context menu |
| `?` | Help |
| `q` | Quit |

## License

MIT
