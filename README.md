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
- **File Browser** — Navigate, create, rename, copy, move, and delete files with Nerd Font icons and syntax-highlighted previews; markdown files render with styled headers, tables, code blocks, and lists
- **Image Viewer** — View PNG, JPG, GIF, and other image formats inline in the terminal
- **Embedded Terminal** — Full shell per worktree with color support
- **Projects Panel** — Switch between registered projects; auto-discovers git repos on startup

### Session

- **Markdown Rendering** — Syntax-highlighted code blocks, tables, headers, lists, and inline formatting
- **Clickable File Paths** — Click tool file paths to open files or view diffs directly in the viewer
- **Clickable Tables** — Click any table to expand it in a full-width popup
- **Todo Widget** — Live task progress from Claude's TodoWrite calls (checkboxes with subagent nesting)
- **Token Counter** — Color-coded context window usage on the session border to anticipate compaction
- **Model Switcher** — Cycle between Claude models with `⌃m` / `Ctrl+M`
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
- **Speech-to-Text** — Dictate with `⌃s` / `Ctrl+S`; transcribed locally via Whisper
- **Mouse Support** — Click, drag-select, scroll, and copy across all panes
- **Diff Viewer** — Syntax-highlighted inline diffs with `⌥←`/`⌥→` (`Alt+←`/`Alt+→`) to cycle through edits

### Extras

- **Run Commands** — Save and execute shell commands globally or per-project
- **Preset Prompts** — Quick-access prompt templates with `⌥P` / `Alt+P` or `⌥1`-`⌥9` / `Alt+1`-`Alt+9` shortcuts
- **Health Panel** — Scan for oversized files and missing documentation; spawn Claude to fix them
- **Completion Notifications** — macOS notifications when any Claude instance finishes
- **Worktree Safety** — Delete dialog warns about uncommitted changes and unmerged commits

### Performance

- **Non-blocking UI** — All expensive work (rendering, parsing, file I/O) runs on background threads
- **Fast Input & Session** — Prompt keystrokes render via `fast_draw_input()` (~0.1ms); session pane streams via `fast_draw_session()` using direct cursor positioning (~2-5ms for session column only vs ~87KB for full ratatui diff); both update simultaneously during typing+streaming with zero session pane freezing
- **Incremental Everything** — Session files parsed incrementally; renders send only new content
- **Minimal Footprint** — No database; two small TOML config files and runtime git/Claude discovery

## Requirements

- **Rust** (latest stable) — `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`
- **Claude Code CLI** — macOS/Linux: `curl -fsSL https://claude.ai/install.sh | bash` · Windows: `irm https://claude.ai/install.ps1 | iex`
- **Git** (2.15+, worktree support) — macOS: `xcode-select --install` · Linux: `sudo apt install git` · Windows: [git-scm.com](https://git-scm.com/downloads)
- **Nerd Font** (recommended) — Any [Nerd Font](https://www.nerdfonts.com/) for file tree icons; emoji fallback when not detected
- **LLVM/Clang + CMake** (build dependency) — Required by whisper-rs. macOS: included with Xcode CLT · Linux: `sudo apt install libclang-dev cmake` · Windows: `winget install LLVM.LLVM Kitware.CMake` then `[Environment]::SetEnvironmentVariable("LIBCLANG_PATH", "C:\Program Files\LLVM\bin", "User")` in PowerShell (restart terminal after)
- **Whisper model** (optional, for speech) — Create `~/.azureal/speech/` and download [ggml-small.en.bin](https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-small.en.bin) into it

### Platform Support

| Platform | Status |
|----------|--------|
| macOS | Primary — Metal GPU for Whisper, `.app` bundle icon |
| Linux | Supported — CPU Whisper, all features |
| Windows | Supported — ConPTY, `cmd.exe`/PowerShell shell, CPU Whisper |

## Installation

### Pre-built Binaries

Download the latest binary from [Releases](https://github.com/xCORViSx/AZUREAL/releases) and place it in your PATH:

**macOS / Linux:**

```bash
chmod +x azureal-*
sudo mv azureal-* /usr/local/bin/azureal
```

**Windows:** Place `azureal-windows-x64.exe` in a directory on your `PATH` (e.g., `C:\Users\<you>\bin\`).

### From Source

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
| `Tab/Shift+Tab` | Cycle pane focus |
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
| `⌘a` / `Ctrl+Shift+A` | Archive worktree |
| `⌘d` / `Ctrl+Shift+D` | Delete worktree |
| `⌘c` / `Ctrl+C` | Copy selection |
| `⌃c` / `Ctrl+Shift+C` | Cancel agent |
| `⌃m` / `Ctrl+M` | Cycle model |
| `⌃q` / `Ctrl+Q` | Quit |

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
