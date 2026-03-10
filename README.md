<p align="center">
  <img src="azureal_icon.png" alt="AZUREAL" width="180" />
</p>

<h1 align="center">AZUREAL</h1>

<p align="center">
  <strong>Asynchronous Zoned Unified Runtime Environment for Agentic LLMs</strong>
</p>

<p align="center">
  A multi-session Claude Code manager with git worktree isolation
</p>

---

## Features

### Workspace

- **Multi-Worktree Sessions** — Run multiple Claude agents concurrently, each in its own git worktree with independent sessions
- **3-Pane Layout** — File tree, viewer, and session panes with Tab cycling and proportional sizing
- **Viewer Tabs** — Tab up to 12 files for quick switching between references
- **File Browser** — Navigate, create, rename, copy, move, and delete files with Nerd Font icons and syntax-highlighted previews
- **Image Viewer** — View PNG, JPG, GIF, and other image formats inline in the terminal
- **Embedded Terminal** — Full shell per worktree with color support
- **Projects Panel** — Switch between registered projects; auto-discovers git repos on startup

### Session

- **Markdown Rendering** — Syntax-highlighted code blocks, tables, headers, lists, and inline formatting
- **Clickable File Paths** — Click tool file paths to open files or view diffs directly in the viewer
- **Clickable Tables** — Click any table to expand it in a full-width popup
- **Todo Widget** — Live task progress from Claude's TodoWrite calls (checkboxes with subagent nesting)
- **Token Counter** — Color-coded context window usage on the session border to anticipate compaction
- **Model Switcher** — Cycle between Claude models with `⌃m`
- **Session Search** — `/` to search text in the current session; `/` in the session list to filter or `//` to search across all sessions
- **AskUserQuestion** — Numbered options box for responding to Claude's questions

### Git

- **Git Panel** — Full git workflow in one view: changed files, diffs, commit log, and context-aware actions
- **Squash Merge** — One-key squash merge with auto-rebase onto main and rich commit messages
- **AI Commit Messages** — Claude generates conventional commit messages from your staged changes
- **Auto-Rebase** — Keep feature branches up-to-date automatically with configurable auto-resolve files
- **Conflict Resolution** — Structured conflict overlay with Claude-assisted resolution (RCR)

### Editor & Input

- **Vim-Style Modes** — Command mode (red border), prompt mode (yellow), terminal mode (azure)
- **Speech-to-Text** — Dictate with `⌃s`; transcribed locally via Whisper
- **Mouse Support** — Click, drag-select, scroll, and copy across all panes
- **Diff Viewer** — Syntax-highlighted inline diffs with `⌥←/⌥→` to cycle through edits

### Extras

- **Run Commands** — Save and execute shell commands globally or per-project
- **Preset Prompts** — Quick-access prompt templates with `⌥P` or `⌥1`-`⌥9` shortcuts
- **Health Panel** — Scan for oversized files and missing documentation; spawn Claude to fix them
- **Completion Notifications** — macOS notifications when any Claude instance finishes
- **Worktree Safety** — Delete dialog warns about uncommitted changes and unmerged commits

### Performance

- **Non-blocking UI** — All expensive work (rendering, parsing, file I/O) runs on background threads
- **Instant Input** — Keystrokes render in ~0.1ms regardless of session size or streaming activity
- **Incremental Everything** — Session files parsed incrementally; renders send only new content
- **Minimal Footprint** — No database; two small TOML config files and runtime git/Claude discovery

## Requirements

- **Rust** (latest stable)
- **Claude Code CLI** (`npm install -g @anthropic-ai/claude-code`)
- **Git** with worktree support
- **Nerd Font** (recommended) — Any [Nerd Font](https://www.nerdfonts.com/) for file tree icons; emoji fallback when not detected
- **Whisper model** (optional, for speech): `mkdir -p ~/.azureal/speech && curl -L -o ~/.azureal/speech/ggml-small.en.bin https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-small.en.bin`

## Installation

```bash
cargo install --path .
```

## Usage

```bash
azureal
```

### Keybindings

| Key | Action |
|-----|--------|
| `p` | Enter prompt mode |
| `T` | Toggle terminal |
| `Esc` | Return to command mode |
| `j/k` | Scroll line |
| `J/K` | Page scroll |
| `Tab/⇧Tab` | Cycle pane focus |
| `M` | Browse main branch |
| `f` | Toggle file tree |
| `s` | Toggle session list |
| `w` | New worktree |
| `R` | Run command |
| `G` | Git panel |
| `H` | Health panel |
| `P` | Projects panel |
| `/` | Search / filter |
| `?` | Help |
| `⌘a` | Archive worktree |
| `⌘d` | Delete worktree |
| `⌃c` | Cancel agent |
| `⌃m` | Cycle model |
| `⌃q` | Quit |

**Input modes** are indicated by the input box border color:

- **Red** — Command mode (keys trigger actions)
- **Yellow** — Prompt mode (typing to Claude)
- **Magenta** — Speech recording
- **Azure** — Terminal mode

## Architecture

Azureal is **mostly stateless** — runtime state is derived from git worktrees, branches, and Claude's session files at `~/.claude/projects/`. No database. Persistent config lives in two `azufig.toml` files (global + project-local).

All keybindings are defined once in a central module. Press `?` for the full help overlay.

## License

MIT
