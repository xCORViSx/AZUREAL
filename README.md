<p align="center">
  <img src="azureal_icon.png" alt="AZUREAL" width="180" />
</p>

<h1 align="center">AZUREAL</h1>

<p align="center">
  <strong>Asynchronous Zoned Unified Runtime Environment for Agentic LLMs</strong>
</p>

<p align="center">
  A multi-session AI agent manager with git worktree isolation
</p>

---

## Features

### Workspace

- **Multi-Worktree Sessions** — Run multiple AI agents (Claude Code or OpenAI Codex) concurrently, each in its own git worktree with independent sessions
- **3-Pane Layout** — File tree, viewer, and session panes with Tab cycling and proportional sizing
- **Viewer Tabs** — Tab up to 12 files for quick switching between references
- **File Browser** — Navigate, create, rename, copy, move, and delete files with Nerd Font icons and syntax-highlighted previews; markdown files render with styled headers, tables, code blocks, and lists
- **Image Viewer** — View PNG, JPG, GIF, and other image formats inline in the terminal
- **Embedded Terminal** — Full shell per worktree with color support
- **Projects Panel** — Switch between projects in parallel; Claude processes continue running in background with activity status icons per project

### Session

- **Markdown Rendering** — Syntax-highlighted code blocks, tables, headers, lists, and inline formatting
- **Clickable File Paths** — Click tool file paths or assistant markdown file links to open files or view diffs directly in the viewer
- **Clickable Tables** — Click any table to expand it in a full-width popup
- **Todo Widget** — Live task progress from Claude's TodoWrite calls (checkboxes with subagent nesting)
- **Context Meter** — Color-coded session store usage on the session border (chars / 400k compaction threshold)
- **Model Switcher** — Cycle between backend models with `⌃m` / `Ctrl+M` (Claude: opus/sonnet/haiku; Codex: gpt-5.4/gpt-5.3-codex/gpt-5.2-codex/gpt-5.2/gpt-5.1-codex-max/gpt-5.1-codex-mini); restores each session's last-used model on switch
- **Session Search** — `/` to search text in the current session; `/` in the session list to filter or `//` to search across all sessions
- **Session Rename** — `r` in the session list to rename the selected session (persisted in SQLite store)
- **AskUserQuestion** — Numbered options box for responding to Claude's questions

### Git

- **Git Panel** — Full git workflow in one view: changed files, diffs, commit log, and context-aware actions
- **File Staging** — Stage/unstage individual files (`s`) or all at once (`S`); unstaged files shown with strikethrough
- **Discard Changes** — Revert individual files (`x`) with inline confirmation prompt
- **Squash Merge** — One-key squash merge with auto-rebase onto main and rich commit messages
- **AI Commit Messages** — Generates conventional commit messages using the currently selected model, with automatic cross-backend fallback
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
- **Fast Input** (macOS) — Prompt keystrokes render via `fast_draw_input()` (~0.1ms) bypassing ratatui's full diff for instant typing feedback. Disabled on Windows where direct VT writes conflict with the console input parser
- **Incremental Everything** — Session files parsed incrementally; renders send only new content
- **Minimal Footprint** — Single-file SQLite session store (`.azs`), two small TOML config files, and runtime git/Claude discovery

## Requirements

- **Rust** (latest stable) — `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`
- **Claude Code CLI** (for Claude backend) — macOS/Linux: `curl -fsSL https://claude.ai/install.sh | bash` · Windows: `irm https://claude.ai/install.ps1 | iex`
- **Codex CLI** (for Codex backend, optional) — `npm install -g @openai/codex`
- **Git** (2.15+, worktree support) — macOS: `xcode-select --install` · Linux: `sudo apt install git` · Windows: [git-scm.com](https://git-scm.com/downloads)
- **Nerd Font** (recommended) — Any [Nerd Font](https://www.nerdfonts.com/) with at least regular, bold, and italic variants installed, so AZUREAL can show file tree icons and the full range of text styling differences; emoji fallback when not detected
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

Download the latest binary for your platform from [Releases](https://github.com/xCORViSx/AZUREAL/releases) and run it. The binary is **self-installing** — it detects first run, copies itself to your PATH, and you're done.

| Platform | Install location |
|----------|-----------------|
| macOS/Linux | `/usr/local/bin/azureal` (or `~/.local/bin/` if not writable) |
| Windows | `%USERPROFILE%\.azureal\bin\azureal.exe` |

After install, run `azureal` from any terminal.

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
| `[/]` | Switch worktree tab |
| `M` | Browse main branch |
| `f` | Toggle file tree |
| `s` | Toggle session list |
| `/` | Search / filter |
| `?` | Help |
| `⌘c` / `Ctrl+C` | Copy selection |
| `⌃c` / `Alt+C` | Cancel agent |
| `⌃m` / `Ctrl+M` | Cycle model |
| `⌃q` / `Ctrl+Q` | Quit |

**Worktree actions** use a `w ␣` leader sequence (press `w` then a second key):

| Key | Action |
|-----|--------|
| `w ␣ w` | New worktree |
| `w ␣ G` | Git panel |
| `w ␣ H` | Health panel |
| `w ␣ P` | Projects panel |
| `w ␣ R` | Run command |
| `w ␣ x` | Archive worktree |
| `w ␣ d` | Delete worktree |

**Input modes** are indicated by the input box border color:

- **Red** — Command mode (keys trigger actions)
- **Yellow** — Prompt mode (typing to Claude)
- **Magenta** — Speech recording
- **Azure** — Terminal mode

## Architecture

Azureal is **mostly stateless** — runtime state is derived from git worktrees and branches. Persistent config lives in two `azufig.toml` files (global + project-local). Backend selection (`claude` or `codex`) is derived from the active model (`gpt-*` → Codex, everything else → Claude). Sessions are stored in a single SQLite database (`.azureal/sessions.azs`) — portable, self-contained, and transferable between machines by copying one file. Agent JSONL session files are temporary — parsed during live streaming, ingested into the store on exit, then deleted.

All keybindings are defined once in a central module. Press `?` for the full help overlay.

## License

MIT
