<p align="center">
  <img src="azureal_icon.png" alt="AZUREAL" width="180" />
</p>

<h1 align="center">AZUREAL</h1>
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

- **Multi-Worktree Sessions** — Run multiple Claude Code agents concurrently, each in its own git worktree; supports multiple simultaneous sessions per worktree via PID-keyed slots; main branch isolated from sidebar (browse read-only with `m`), archived worktrees show `◇` diamond, feature branches use status circles
- **3-Pane TUI** — Worktrees (15%), Viewer (50%), and Session (35%) panes with Tab cycling; proportional sizing maintained across all terminal sizes. FileTree and Session list available as toggle overlays (`f` and `s`)
- **Viewer Tabs** — Up to 12 tabs across 2 fixed-width rows (6 per row); `t` to tab current file, `⌥t` for tab dialog, `[`/`]` to navigate, `x` to close
- **File Browser** — Press `f` in Worktrees pane to toggle FileTree overlay; navigate with expand/collapse, open in syntax-highlighted Viewer; **Nerd Font icons** auto-detected at startup with language-brand colors for ~60 file types (emoji fallback when Nerd Font not detected); file actions: `a`dd, `d`elete, `r`ename (inline input), `c`opy/`m`ove (clipboard-style: grab → navigate → paste); `O` opens **Options overlay** to toggle visibility of `worktrees`, `.git`, `.claude`, `.azureal`, `.DS_Store` (all hidden by default, persisted to project azufig.toml)
- **Image Viewer** — Opening PNG, JPG, GIF, BMP, WebP, or ICO files from FileTree renders them inline in the Viewer pane using terminal graphics (Kitty on Ghostty/Kitty, Sixel on iTerm2, halfblock fallback everywhere else)
- **Vim-Style Input** — Modal editing with command/prompt modes, multi-line via Shift+Enter, word-boundary wrapping
- **Embedded Terminal** — Full PTY-based shell per worktree with color support
- **Real-time Output** — Kernel-level file watching (kqueue/inotify/ReadDirectoryChangesW via `notify`) for near-instant session updates and auto-refreshing file tree; graceful fallback to stat() polling
- **Markdown Rendering** — Headers, bold, italic, code blocks, tables rendered with proper styling
- **Clickable File Paths** — Edit/Read/Write tool file paths are underlined and clickable; Edit opens diff view, Read/Write opens plain file
- **Async Rendering** — Session pane renders on a background thread with backpressure + 50ms throttle; incremental renders send only new events (pre-scanned state from already-rendered events avoids mega-clones); single JSON parse per streaming event; input is never blocked by markdown/syntax processing
- **Incremental Parsing** — Large session files parsed incrementally (only new lines since last read)
- **Mouse Support** — Click to focus panes, select sessions/files, position cursor; drag to select text; scroll by cursor position; Cmd+C to copy selection. In file edit mode: click to position edit cursor (including on wrapped lines), drag to create selections
- **Diff Viewer** — Syntax-highlighted git diffs per worktree; inline diff viewing in the Git panel for changed files and commits
- **Add Worktree Dialog** — Unified dialog (`a` key) with "[+] Create new" row and branch list with `[N WT]` worktree count indicators; filter input doubles as new worktree name with git-safe character validation
- **Run Commands** — Save and execute shell commands/scripts globally or per-project; Prompt mode lets Claude generate commands from natural-language descriptions; `d` to delete with y/n confirmation; `⌃s` toggles global/project scope in add/edit dialog; picker shows G/P scope badge
- **Hook Display** — All Claude Code hook types displayed inline in conversation
- **Token Usage Counter** — Color-coded context window percentage on Session pane border (green/yellow/red) to predict compaction; at 95%+, a 20-second inactivity watcher detects likely auto-compaction and shows a banner so the user knows why the session appears frozen
- **Model Switcher** — Press `⌃m` to cycle between Claude models (sonnet/opus/haiku); Session pane bottom border shows color-coded `⌃m:model` indicator; selected model is always passed as `--model` to the CLI; works from command mode and prompt input
- **TodoWrite Widget** — Sticky checkbox list at bottom of Session pane showing Claude's task progress (✓/●/○); subagent subtasks shown indented under their parent item with ↳ prefix; caps at 20 visual lines with a proportional scrollbar and mouse wheel scrolling when content overflows
- **AskUserQuestion Box** — Numbered options box for Claude's questions; respond with a number or custom text
- **Session Search/Filter** — Press `/` in Worktrees to search across projects, worktrees, and sessions simultaneously; matches shown with parent hierarchy
- **Session Search** — Press `/` in Session pane to find text in current session with yellow match highlighting and `[N/M]` counter; `n`/`N` to cycle matches. In Session list: `/` filters by name, `//` searches across all session file contents
- **Speech-to-Text** — Press `⌃s` in prompt mode or file edit mode to dictate via microphone; transcribed locally with Whisper (Metal-accelerated), text inserted at cursor position
- **Projects Panel** — Persistent project registry (`~/.azureal/azufig.toml`); auto-registers git repos on startup; `P` to open panel for switching, adding, deleting, renaming, or initializing projects
- **Health Panel** — Press `Shift+H` from any pane to open a tabbed health-check modal titled `Health: <worktree>` (mirrors the Git panel naming). Panel border shows `Tab:tab` top-left and `s:scope` top-right — press `s` from any tab to enter scope mode (FileTree with green highlights, subdirs inherit accepted status, scope persists to `.azureal/azufig.toml` `[healthscope]`). **Auto-refreshes** when source files change (500ms debounce) — cursor, scroll, and checked state preserved across rescans. **God Files tab**: scans for source files >1000 LOC across ~60 language extensions with smart source-root detection; check files and modularize via simultaneous Claude sessions on the current worktree (with module style selector for Rust/Python dual-convention languages), or press `v` to open as Viewer tabs. **Documentation tab**: scans all source files for doc-comment coverage (`///` and `//!`), showing per-file coverage percentage with visual bars sorted worst-first; overall score color-coded (green/yellow/red); check files and spawn concurrent `[DH]` Claude sessions on the current worktree to add missing doc comments, or press `v` to open as Viewer tabs; `a` checks all non-100% files. Tab key switches between tabs. Both tabs support `J/K` page scroll and mouse wheel scrolling.
- **Rebase Support** — Rebase-before-merge ensures clean squash merges; rich squash commit messages preserve individual commit details as bullet points; manual rebase action (`r`) in Git panel; conflict detection with structured overlay and RCR resolution; **auto-rebase** toggle (`a`) keeps feature branches up-to-date automatically (sidebar `R` indicator: green=enabled, orange=RCR, blue=approval); **auto-resolve** configurable file list (`s`) with union merge strategy keeping both sides' changes (default: AGENTS.md, CHANGELOG.md, README.md, CLAUDE.md); dirty worktree guard, orphaned rebase cleanup on startup, stash recovery after RCR, config cleanup on worktree removal
- **Splash Screen** — Branded startup with 2x-scale AZUREAL block logo, half-block acronym, dim spring azure butterfly outline (app mascot), and "Loading project..." subtitle; minimum 3-second display while git/session I/O runs
- **Debug Build Indicator** — The CPU|PID status bar badge displays in azure when running a debug build and DarkGray for release builds, for quick build-profile identification
- **Terminal Title** — Shows `AZUREAL @ project : branch` in the OS terminal title bar; updates on session/project switch
- **Completion Notifications** — macOS notification with AZUREAL icon when any Claude instance finishes; shows `worktree:session_name` so you know which instance completed, even while in another app. Activity Monitor shows AZUREAL with branded icon. Notification permissions auto-enabled on first launch (zero setup)
- **Preset Prompts** — Press `⌥P` in prompt mode to open a picker with up to 10 saved prompt templates; quick-select with `1-9` and `0` from the picker, or directly with `⌥1`-`⌥9` and `⌥0` from prompt mode (skips picker); picker footer shows the ⌥+number shortcut hint; add, edit, or delete (`d` with y/n confirmation) presets from the picker; selected prompt populates the input box. Presets can be **global** (shared across all projects) or **project-local**; toggle scope with `⌃g` in the add/edit dialog
- **Git Panel** — `Shift+G` transforms the existing 3-pane layout into a git operations view: sidebar splits into Actions (top) + Changed Files (bottom), viewer shows file/commit diffs with text selection and copy support (`⌘A`/`⌘C`, `Shift+J/K` scroll), session pane becomes a branch-scoped commit log ("Commits" — feature branches show only their own commits), and a full-width git status box at the bottom displays keybinding hints and operation results. Context-aware actions change based on branch: **main branch** shows pull (`l`), commit (`c`), push (`P`); **feature branches** show squash-merge (`m`), rebase (`r`), commit (`c`), push (`P`); shows changed files with per-file color-coded `+N/-N` stats; navigate with `j/k`, `Enter` to view diff, `Tab` cycles focus through Actions → Files → Commits; **squash-merge** (`m`) rebases onto main first for clean linear merges — configurable auto-resolve files (default: AGENTS.md, CHANGELOG.md, README.md, CLAUDE.md) are union-merged during rebase keeping both sides' changes; press `s` to manage the auto-resolve list interactively; **commit** (`c`) generates a conventional commit message via `claude -p` (~3 sec), shown in an editable overlay — `Enter` commits, `⌘P` commits + pushes; **RCR** — non-auto-resolve rebase conflicts show a red overlay listing conflicted files; `y` enters RCR mode where Claude resolves on the feature branch, with approval dialog after completion
- **Debug Dump** — Press `⌃d` to save a debug snapshot of the current session state to `.azureal/debug-output-{name}.txt`
- **Loading Indicators** — Centered AZURE-bordered popups for all blocking operations: session loading, file opening, health scanning, project switching, health scope rescanning. Two-phase deferred draw ensures the UI never appears frozen during I/O
- **Minimal Footprint** — Two `azufig.toml` files consolidate all persistent state (global `~/.azureal/azufig.toml` + project-local `.azureal/azufig.toml`); scans git worktrees and `~/.claude/` at runtime

## Requirements

- **Rust** (latest stable)
- **Claude Code CLI** (`npm install -g @anthropic-ai/claude-code`)
- **Git** with worktree support
- **Nerd Font** (recommended) — Any [Nerd Font](https://www.nerdfonts.com/) for file tree icons with language-brand colors. Auto-detected on startup; emoji fallback used automatically when Nerd Font not detected
- **Whisper model** (optional, for speech input): `mkdir -p ~/.azureal/speech && curl -L -o ~/.azureal/speech/ggml-base.en.bin https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.en.bin`

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
| `T` | Toggle terminal pane |
| `Esc` | Return to command mode |
| `j/k` | Navigate / scroll line |
| `J/K` | Page scroll (viewport minus 2 overlap lines) |
| `⌥↑/⌥↓` | Jump to top/bottom of current list or pane |
| `Tab` | Cycle focus forward (Worktrees > Viewer > Session > Input), closes overlays |
| `⇧Tab` | Cycle focus backward; lands on FileTree if overlay is open |
| `m` | Browse main branch read-only (in Worktrees pane); Esc to exit |
| `f` | Toggle FileTree overlay (in Worktrees pane) |
| `s` | Toggle Session list overlay (in Session pane) |
| `n` | New worktree/session (creation wizard) |
| `r` | Run command (picker or execute) |
| `⌥r` | Add new run command |
| `G` | Git panel — transforms panes to show actions, files, diff viewer, commit log |
| `H` | Health panel (God Files + Documentation tabs) |
| `P` | Projects panel |
| `/` | Search/filter sessions (Worktrees); Search text (Session); Filter/search sessions (Session list) |
| `?` | Help |
| `⌘a` | Archive/unarchive worktree (keeps branch) |
| `⌘d` | Delete worktree + branch (sibling guard: y=delete all, a=archive only) |
| `⌃d` | Debug dump (save session state snapshot) |
| `⌃c` | Cancel agent |
| `⌃m` | Cycle model (opus → sonnet → haiku) |
| `⌃q` | Quit |

**Input Modes:**

- Red border = Command mode (keys are commands; title bar shows all global bindings)
- Yellow border = Prompt mode (typing to Claude)
- Magenta border = Speech recording/transcribing (`⌃s` to toggle)
- Azure border = Terminal mode (typing shell commands)

## Architecture

Azureal is **mostly stateless** — all runtime state is derived from:

- Git worktrees via `git worktree list`
- Git branches via `git branch | grep {BRANCH_PREFIX}/`
- Claude's session files at `~/.claude/projects/<encoded-path>/*.jsonl`

No database. All persistent state consolidated into two `azufig.toml` files: `~/.azureal/azufig.toml` (global config, projects, shared runcmds/presets) and `.azureal/azufig.toml` (filetree options, session names, healthscope, local runcmds/presets).

**Rendering:** The session pane uses a dedicated background thread for expensive rendering (markdown parsing, syntax highlighting, text wrapping). The main event loop sends non-blocking render requests via channels and polls for results. During typing, keystrokes get instant visual feedback via direct crossterm writes (~0.1ms) while the expensive `terminal.draw()` (~18ms) is deferred to quiet frames. This two-tier rendering ensures input is never blocked by screen updates.

**Keybindings:** All keybindings are defined once in `src/tui/keybindings/` (5 submodules: types, bindings, lookup, hints, platform) with `lookup_action()` as the single resolver for main views and 7 per-modal lookup functions for modal panels. Guard logic lives inside lookup functions — never duplicated across input handlers. Draw functions source footer hints and labels from keybinding hint generators, not hardcoded strings. The module root re-exports everything so existing import paths work unchanged. Press `?` for the help overlay.

## License

MIT