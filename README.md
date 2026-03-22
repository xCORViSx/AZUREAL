<p align="center">
  <img src="AZUREAL_icon.png" alt="AZUREAL" width="180" />
</p>

<h1 align="center">AZUREAL</h1>

<p align="center">
  <strong>Asynchronous Zoned Unified Runtime Environment for Agentic LLMs</strong>
</p>

<p align="center">
  A multi-session AI agent manager with git worktree isolation
</p>

<p align="center">
  <a href="https://xcorvisx.github.io/AZUREAL/"><strong>📖 Manual</strong></a>
</p>

---

## Features

### Workspace

- **Multi-Worktree Sessions** — Run multiple AI agents (Claude Code or OpenAI Codex) concurrently, each in its own git worktree with independent sessions
- **3-Pane Layout** — File tree, viewer, and session panes with Tab cycling and proportional sizing
- **Viewer Tabs** — Tab up to 12 files for quick switching between references
- **File Browser** — Navigate, create, rename, copy, move, and delete files with Nerd Font icons and syntax-highlighted previews; markdown files render with styled headers, tables, code blocks, and lists
- **Image Viewer** — View PNG, JPG, GIF, and other image formats inline in the terminal
- **Embedded Terminal** — Full shell per worktree with color support, click-to-position cursor, Alt/Option+Arrow word navigation, mouse drag selection with auto-scroll, and mouse wheel scrolling
- **Projects Panel** — Switch between projects in parallel; Claude processes continue running in background with activity status icons per project

### Session

- **Markdown Rendering** — Syntax-highlighted code blocks, tables, headers, lists, and inline formatting
- **Clickable File Paths** — Click tool file paths or assistant markdown file links to open files or view diffs directly in the viewer
- **Clickable Tables** — Click any table to expand it in a full-width popup
- **Todo Widget** — Live task progress from Claude's TodoWrite calls (checkboxes with subagent nesting)
- **Context Meter** — Color-coded session store usage on the session border (chars / 400k compaction threshold)
- **Model Switcher** — Cycle between backend models with `⌃m` / `Ctrl+M` (fallback: `⌥m` on macOS, `Alt+M` on Linux) (Claude: opus/sonnet/haiku; Codex: gpt-5.4/gpt-5.3-codex/gpt-5.2-codex/gpt-5.2/gpt-5.1-codex-max/gpt-5.1-codex-mini); only shows models whose backend CLI is installed; restores each session's last-used model on switch
- **Session Search** — `/` to search text in the current session; `/` in the session list to filter or `//` to search across all sessions
- **Session Rename** — `r` in the session list to rename the selected session (persisted in SQLite store)
- **AskUserQuestion** — Numbered options box for responding to Claude's questions

### Git

- **Git Panel** — Full git workflow in one view: changed files, diffs, commit log, and context-aware actions
- **File Staging** — Stage/unstage individual files (`s`) or all at once (`S`); unstaged files shown with strikethrough
- **Discard Changes** — Revert individual files (`x`) with inline confirmation prompt
- **Squash Merge** — One-key squash merge with auto-rebase onto main and rich commit messages
- **AI Commit Messages** — Generates conventional commit messages using the currently selected model, with automatic cross-backend fallback
- **Stash** — Quick stash (`z`) and stash pop (`Shift+Z`) from the git panel
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
- **Debug Dump** — Capture an obfuscated snapshot of internal state for troubleshooting with `⌃d` / `Ctrl+D`
- **Completion Notifications** — Cross-platform notifications with branded Azureal icon when any agent instance finishes
- **Worktree Safety** — Delete dialog warns about uncommitted changes and unmerged commits

### Performance

- **Non-blocking UI** — All expensive work (rendering, parsing, file I/O) runs on background threads
- **Fast Input** (macOS) — Prompt keystrokes render via `fast_draw_input()` (~0.1ms) bypassing ratatui's full diff for instant typing feedback. Disabled on Windows where direct VT writes conflict with the console input parser
- **Smart Redraw** — Animation ticks only trigger redraws when spinners are visible on screen; git sidebar stats are cached and recomputed only at mutation points
- **Incremental Everything** — Session files parsed incrementally; renders send only new content
- **Minimal Footprint** — Single-file SQLite session store (`.azs`), two small TOML config files, and runtime git/Claude discovery

## Recommended Terminals

For the best experience, use a modern terminal emulator with true-color and mouse support:

| Platform | Recommended | Also tested |
|----------|-------------|-------------|
| macOS | [Kitty](https://sw.kovidgoyal.net/kitty/) | Ghostty, Alacritty, WezTerm, Terminal.app |
| Linux | [Kitty](https://sw.kovidgoyal.net/kitty/) | Ghostty, Alacritty, WezTerm, Konsole |
| Windows | [Windows Terminal](https://aka.ms/terminal) | — |

**Kitty** and **Windows Terminal** deliver the best overall experience — not just for input protocol support (Kitty keyboard protocol on macOS/Linux, full ConPTY on Windows), but also for **rendering fidelity**. Both terminals produce the cleanest interpretation of AZUREAL's box-drawing characters, Unicode glyphs, and styled borders, resulting in pixel-perfect pane separators, tab bars, and dialog frames. Other listed terminals work well — the main differences are that terminals without the Kitty keyboard protocol cannot distinguish certain key combinations (e.g., `Ctrl+M` vs `Enter`), so AZUREAL automatically provides `Alt+` fallback bindings where needed, and some terminals may show minor visual artifacts in complex border intersections or half-block character rendering.

A [Nerd Font](https://www.nerdfonts.com/) is recommended for file tree icons and full text styling. AZUREAL falls back to emoji icons when a Nerd Font is not detected.

### Recommended Color Schemes

AZUREAL's AZURE (`#3399FF`) accent and dark-background design pairs well with cool-toned terminal themes. These two schemes have been tested extensively:

**Windows Terminal — Winter is Coming (Dark Blue)**

Add to your `settings.json` under `"schemes"`:

```json
{
    "background": "#011627",
    "black": "#011627",
    "blue": "#2472C8",
    "brightBlack": "#666666",
    "brightBlue": "#3B8EEA",
    "brightCyan": "#29B8DB",
    "brightGreen": "#23D18B",
    "brightPurple": "#D670D6",
    "brightRed": "#F14C4C",
    "brightWhite": "#E5E5E5",
    "brightYellow": "#F5F543",
    "cursorColor": "#FFFFFF",
    "cyan": "#11A8CD",
    "foreground": "#CCCCCC",
    "green": "#0DBC79",
    "name": "Winter is Coming (Dark Blue)",
    "purple": "#BC3FBC",
    "red": "#CD3131",
    "selectionBackground": "#FFFFFF",
    "white": "#E5E5E5",
    "yellow": "#E5E510"
}
```

Then set `"colorScheme": "Winter is Coming (Dark Blue)"` in your profile.

**Kitty — Copland OS**

Copland OS is a built-in Kitty theme. Apply it with:

```bash
kitty +kitten themes Copland OS
```

Or add `include themes/Copland_OS.conf` to your `kitty.conf`.

## Requirements

- **Rust** (latest stable) — `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`
- **Claude Code CLI** (for Claude backend) — macOS/Linux: `curl -fsSL https://claude.ai/install.sh | bash` · Windows: `irm https://claude.ai/install.ps1 | iex`
- **Codex CLI** (for Codex backend, optional) — `npm install -g @openai/codex`
- **Git** (2.15+, worktree support) — macOS: `xcode-select --install` · Linux: `sudo apt install git` · Windows: [git-scm.com](https://git-scm.com/downloads)
- **Nerd Font** (recommended) — Any [Nerd Font](https://www.nerdfonts.com/) with at least regular, bold, and italic variants installed, so AZUREAL can show file tree icons and the full range of text styling differences; emoji fallback when not detected
- **LLVM/Clang + CMake + Ninja** (build dependency) — Required by whisper-rs. macOS: included with Xcode CLT · Linux: `sudo apt install libclang-dev cmake` · Windows: `winget install LLVM.LLVM Kitware.CMake Ninja-build.Ninja` then `[Environment]::SetEnvironmentVariable("LIBCLANG_PATH", "C:\Program Files\LLVM\bin", "User")` and `[Environment]::SetEnvironmentVariable("CMAKE_GENERATOR", "Ninja", "User")` in PowerShell (restart terminal after). Ninja is required on Windows because the default VS generator's MSBuild strips CUDA include paths
- **NVIDIA CUDA Toolkit** (Windows build dependency, for GPU-accelerated Whisper) — `winget install Nvidia.CUDA` (restart terminal after install to pick up `CUDA_PATH`)
- **Whisper model** (optional, for speech) — Create `~/.azureal/speech/` and download [ggml-small.en.bin](https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-small.en.bin) into it

### Platform Support

| Platform | Status |
|----------|--------|
| macOS | Primary — Metal GPU for Whisper, `.app` bundle icon |
| Linux | Supported — CPU Whisper, all features |
| Windows | Supported — ConPTY, `cmd.exe`/PowerShell shell, CUDA GPU Whisper, branded console icon (terminal tab + taskbar) |

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
| `G` | GitView panel |
| `H` | Health panel |
| `M` | Browse main branch |
| `P` | Projects panel |
| `r` | Run command |
| `R` | Add run command |
| `[/]` | Switch worktree tab |
| `f` | Toggle file tree |
| `s` | Toggle session list |
| `/` | Search / filter |
| `?` | Help |
| `⌘c` / `Ctrl+C` | Copy selection (fallback: `⌥c` macOS) |
| `⌃c` / `Alt+C` | Cancel agent |
| `⌃m` / `Ctrl+M` | Cycle model (fallback: `⌥m` macOS, `Alt+M` Linux) |
| `⌃q` / `Ctrl+Q` | Quit |

**Worktree mutations** work directly when the worktrees panel is focused, or via `W` (Shift+W) leader sequence from any focus:

| Key | Action |
|-----|--------|
| `Wn` | New worktree |
| `Wr` | Rename worktree |
| `Wa` | Archive worktree |
| `Wd` | Delete worktree |

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
