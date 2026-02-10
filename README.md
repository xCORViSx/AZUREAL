<p align="center">
  <img src="azureal_icon.png" alt="Azureal" width="180" />
</p>

<h1 align="center">azureal</h1>
<p align="center">
  uh-zoo-ree-ull
</p>

<p align="center">
  <strong>Asynchronous Zoned Unified Runtime Environment for Agentic LLMs</strong>
</p>

<p align="center">
  A multi-session Claude Code manager with git worktree isolation
</p>

---

## Features

- **Multi-Worktree Sessions** — Run multiple Claude Code agents concurrently, each in its own git worktree
- **3-Pane TUI** — Worktrees (40), Viewer (remaining), and Convo (80) panes with Tab cycling; FileTree and Session list available as toggle overlays (`f` and `s`)
- **File Browser** — Press `f` in Worktrees pane to toggle FileTree overlay; navigate with expand/collapse, open in syntax-highlighted Viewer
- **Vim-Style Input** — Modal editing with command/prompt modes, multi-line via Shift+Enter, word-boundary wrapping
- **Embedded Terminal** — Full PTY-based shell per worktree with color support
- **Real-time Output** — Kernel-level file watching (kqueue/inotify/ReadDirectoryChangesW via `notify`) for near-instant session updates and auto-refreshing file tree; graceful fallback to stat() polling
- **Markdown Rendering** — Headers, bold, italic, code blocks, tables rendered with proper styling
- **Clickable File Paths** — Edit/Read/Write tool file paths are underlined and clickable; Edit opens diff view, Read/Write opens plain file
- **Async Rendering** — Convo pane renders on a background thread; input is never blocked by markdown/syntax processing
- **Incremental Parsing** — Large session files parsed incrementally (only new lines since last read)
- **Mouse Support** — Click to focus panes, select sessions/files, position cursor; drag to select text; scroll by cursor position; Cmd+C to copy selection. In file edit mode: click to position edit cursor (including on wrapped lines), drag to create selections
- **Diff Viewer** — Syntax-highlighted git diffs per worktree
- **Creation Wizard** — Tabbed dialog for creating worktrees and sessions
- **Run Commands** — Save and execute shell commands/scripts per project; Prompt mode lets Claude generate commands from natural-language descriptions
- **Hook Display** — All Claude Code hook types displayed inline in conversation
- **Token Usage Counter** — Color-coded context window percentage on Convo pane border (green/yellow/red) to predict compaction
- **TodoWrite Widget** — Sticky checkbox list at bottom of Convo pane showing Claude’s task progress (✓/●/○); subagent subtasks shown indented under their parent item with ↳ prefix
- **AskUserQuestion Box** — Numbered options box for Claude's questions; respond with a number or custom text
- **Session Search/Filter** — Press `/` in Worktrees to search across projects, worktrees, and sessions simultaneously; matches shown with parent hierarchy
- **Speech-to-Text** — Press `⌃s` in prompt mode to dictate via microphone; transcribed locally with Whisper (Metal-accelerated)
- **Projects Panel** — Persistent project registry (`~/.azureal/projects.txt`); auto-registers git repos on startup; `P` to open panel for switching, adding, deleting, renaming, or initializing projects
- **Rebase Support** — Interactive rebase with conflict detection and resolution
- **Terminal Title** — Shows `AZUREAL @ project : branch` in the OS terminal title bar; updates on session/project switch
- **Minimal Footprint** — Optional `~/.azureal/projects.txt` for project persistence; scans git worktrees and `~/.claude/` at runtime

## Requirements

- **Rust** (latest stable)
- **Claude Code CLI** (`npm install -g @anthropic-ai/claude-code`)
- **Git** with worktree support
- **Whisper model** (optional, for voice input): `mkdir -p ~/.azureal/models && curl -L -o ~/.azureal/models/ggml-base.en.bin https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.en.bin`

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
| `j/k` | Navigate / scroll line |
| `J/K` | Page scroll (viewport minus 2 overlap lines) |
| `⌥↑/⌥↓` | Jump to top/bottom of current list or pane |
| `Tab` | Cycle focus (Worktrees > Viewer > Convo > Input), closes overlays |
| `f` | Toggle FileTree overlay (in Worktrees pane) |
| `s` | Toggle Session list overlay (in Convo pane) |
| `n` | New worktree/session (creation wizard) |
| `r` | Run command (picker or execute) |
| `⌥r` | Add new run command |
| `P` | Projects panel |
| `R` | Rebase onto main |
| `d` | View diff |
| `/` | Search/filter sessions (in Worktrees) |
| `Space` | Context menu / Toggle expand |
| `?` | Help |
| `⌃c` | Cancel running Claude response |
| `⌃q` | Quit |
| `⌃r` | Restart |

**Input Modes:**

- Red border = Command mode (keys are commands; title bar shows all global bindings)
- Yellow border = Prompt mode (typing to Claude)
- Magenta border = Voice recording/transcribing (`⌃s` to toggle)
- Azure border = Terminal mode (typing shell commands)

## Architecture

Azureal is **mostly stateless** — all runtime state is derived from:

- Git worktrees via `git worktree list`
- Git branches via `git branch | grep azureal/`
- Claude's session files at `~/.claude/projects/<encoded-path>/*.jsonl`

No database. Minimal config: `~/.azureal/projects.txt` stores registered project paths; optional `.azureal/sessions.toml` stores custom session name mappings.

**Rendering:** The convo pane uses a dedicated background thread for expensive rendering (markdown parsing, syntax highlighting, text wrapping). The main event loop sends non-blocking render requests via channels and polls for results. During typing, keystrokes get instant visual feedback via direct crossterm writes (~0.1ms) while the expensive `terminal.draw()` (~18ms) is deferred to quiet frames. This two-tier rendering ensures input is never blocked by screen updates.

## License

MIT
