# SUMMARY

Azural (Agent Zones: Unified Runtime for Autonomous LLMs) is a Rust TUI application that wraps Claude Code CLI to enable multi-agent development workflows. Each "Session" is a git worktree with its own Claude agent, allowing concurrent AI-assisted development across multiple feature branches.

**Stateless Architecture:** Azural stores NO persistent data. All state is derived at runtime from:
- Git repository info via `git rev-parse --show-toplevel`
- Git worktrees via `git worktree list` for active sessions
- Git branches via `git branch | grep azural/` for archived sessions
- Claude's session files in `~/.claude/projects/` for conversation history and `--resume` IDs

# FEATURES

### Multi-Session Claude Management

The core feature enabling multiple concurrent Claude Code CLI instances. Each session has its own:
- Git worktree for isolated file changes
- Claude session ID captured from init event for `--resume`
- Output stream parsed from `stream-json` format for clean display

**Architecture:**
- Each prompt spawns a new process: `claude -p "prompt" --verbose --output-format stream-json`
- First prompt: captures `session_id` from init event in stream-json output
- Follow-up prompts: add `--resume <session_id>` for conversation context
- Process exits after each response; new process for next prompt

**Critical: NO `--fork-session`**
Earlier we used `--fork-session` with `--resume`, but this creates a NEW session each time (losing conversation context and causing tool_use ID collisions). Removed in favor of simple `--resume` only.

**Why not use `--session-id`?**
`--session-id` requires a valid UUID format. Simpler to capture Claude's generated session ID from the init event.

**Why not keep process alive?**
Claude Code's interactive mode uses a full TUI that cannot be driven by simple stdin writes. The `--input-format stream-json` flag only works with `-p` mode which still exits after each response. Verified by testing - there's no headless interactive mode available.

Current approach (`-p --resume`) works reliably with ~100-200ms process spawn overhead per prompt.

Implementation: `src/claude.rs` spawns processes, `src/app/state.rs` tracks `claude_session_ids` HashMap for --resume.

### Git Worktree Isolation

Sessions are backed by git worktrees, providing true branch isolation. Each worktree:
- Has its own working directory
- Can have different uncommitted changes
- Operates on a separate branch from main

Implementation: `src/git.rs` handles worktree creation, deletion, and status queries.

### TUI Interface

A ratatui-based terminal interface with 4-pane layout:

```
┌──────────┬──────────┬─────────────────┬─────────────────┐
│ Sessions │ FileTree │     Viewer      │     Convo      │
│   (40)   │   (40)   │  (50% remain)   │  (50% remain)   │
├──────────┴──────────┴─────────────────┴─────────────────┤
│                    Input / Terminal                      │
├─────────────────────────────────────────────────────────┤
│                      Status Bar                          │
└─────────────────────────────────────────────────────────┘
```

**Panes:**
- **Sessions** (40 cols): Session list showing all worktrees and archived branches
- **FileTree** (40 cols): Directory tree for selected session's worktree (supports expand/collapse)
- **Viewer** (50% remaining): File content viewer or diff detail (dual-purpose)
- **Convo** (50% remaining): Claude conversation output with tool results
- **Input/Terminal**: Prompt input or embedded terminal (toggleable)
- **Status Bar**: Context-sensitive help and session info

**Viewer Dual Purpose:**
- When file selected in FileTree → shows syntax-highlighted file content with line numbers
- When diff selected in Convo → shows diff detail (future)

**Syntax Highlighting:**
- Uses syntect library with base16-ocean.dark theme
- Automatic language detection based on file extension
- Supports Rust, TOML, Markdown, JSON, YAML, and 150+ other languages

Other features:
- Vim-style modal editing
- Diff viewer with syntax highlighting
- Help overlay with keybindings
- Mouse scroll support (scroll panels based on cursor position)

Implementation: `src/tui/event_loop.rs` for event loop, `src/tui/run.rs` for rendering, `src/app/state/` for state management (split into 9 focused submodules).

---

## ⚠️ CRITICAL: CPU PERFORMANCE RULES ⚠️

**DO NOT REGRESS THESE OPTIMIZATIONS. CPU usage must stay <5% during scrolling.**

### 1. NEVER Create Expensive Objects in Render Path

```rust
// ❌ WRONG - Creates SyntaxHighlighter on EVERY FRAME (loads entire syntect SyntaxSet)
fn render_edit_diff(...) {
    let highlighter = SyntaxHighlighter::new();  // CATASTROPHIC - 100ms+ per call
}

// ✅ CORRECT - Pass reference from App state
fn render_edit_diff(..., highlighter: &SyntaxHighlighter) {
    highlighter.highlight_file(...)  // Reuses pre-loaded syntax definitions
}
```

**Files:** `src/tui/render_events.rs` passes `&app.syntax_highlighter` to `render_edit_diff()`

### 2. CACHE Rendered Output

```rust
// ❌ WRONG - Re-renders ALL events on EVERY frame (O(n) per frame)
let all_lines = render_display_events(&app.display_events, ...);

// ✅ CORRECT - Cache rendered lines, only re-render when data changes
if app.rendered_lines_dirty || app.rendered_lines_width != width {
    app.rendered_lines_cache = render_display_events(...);
    app.rendered_lines_dirty = false;
}
let lines = app.rendered_lines_cache.iter().skip(scroll).take(height).cloned().collect();
```

**Files:** `src/tui/draw_output.rs` uses `app.rendered_lines_cache`; call `app.invalidate_render_cache()` when `display_events` changes

### 3. THROTTLE Animation and Scroll

```rust
// ❌ WRONG - Animation forces redraw every loop iteration
let has_pending = !app.pending_tool_calls.is_empty();
let mut needs_redraw = has_pending;  // CONSTANT REDRAWS when tools pending

// ✅ CORRECT - Throttle animation to 4fps
let animation_due = now.duration_since(last_animation) >= Duration::from_millis(250);
if animation_due && has_pending {
    app.animation_tick = app.animation_tick.wrapping_add(1);
    last_animation = now;
}
let mut needs_redraw = animation_due && has_pending;
```

**Throttle values in `src/tui/event_loop.rs`:**
- `min_draw_interval = 100ms` (10fps scroll)
- `min_animation_interval = 250ms` (4fps pulsating indicators)
- `min_poll_interval = 500ms` (session file polling)

### 4. SKIP Redraw When Nothing Changed

```rust
// ❌ WRONG - Always returns true, always redraws
pub fn scroll_output_up(&mut self, lines: usize) {
    self.output_scroll = self.output_scroll.saturating_sub(lines);
}

// ✅ CORRECT - Return whether position actually changed
pub fn scroll_output_up(&mut self, lines: usize) -> bool {
    let old = self.output_scroll;
    self.output_scroll = self.output_scroll.saturating_sub(lines);
    self.output_scroll != old  // false if already at top
}
```

**Files:** `src/app/state/scroll.rs` - all scroll functions return `bool`; `src/tui/event_loop.rs` uses return value

### 5. Event Loop Optimizations

- **Event batching:** Drain ALL pending events before redrawing (one redraw per batch)
- **Motion discard:** Mouse motion events discarded instantly (zero processing)
- **Conditional polling:** Terminal rx only polled when `app.terminal_mode == true`
- **Cached terminal size:** Only updated on resize events, not every frame

### 6. Pre-Format Expensive Data at Load Time

```rust
// ❌ WRONG - chrono::DateTime::from() on EVERY FRAME
fn draw_sidebar(...) {
    for file in files {
        let time_str = format_time(file.mtime);  // EXPENSIVE chrono call per-frame
    }
}

// ✅ CORRECT - Format once when loading, store String
pub fn list_claude_sessions(...) -> Vec<(String, PathBuf, String)> {
    sessions.into_iter()
        .map(|(id, path, mtime)| (id, path, format_time(mtime)))  // Format ONCE at load
        .collect()
}
```

**Rule:** Any data transformation (time formatting, string manipulation, parsing) must happen at load/update time, never in render functions.

**Files:** `src/config.rs::list_claude_sessions()` pre-formats time strings; `src/tui/draw_sidebar.rs` just displays them

### Performance Checklist for PRs

Before merging ANY change to render/event code:
- [ ] No `::new()` calls for expensive structs in render path
- [ ] No O(n) operations per frame (use caching)
- [ ] Animations throttled (not every frame)
- [ ] Scroll returns bool, caller checks before redraw
- [ ] Test: scroll aggressively, CPU must stay <5%

---

**Startup sequence** (`src/tui/run.rs::run`): `App::new()` → `app.load()` → `app.load_session_output()` → `event_loop::run_app()`. The `load_session_output()` call ensures the output pane shows conversation history immediately on startup.

### Vim-Style Input Mode

The input box uses vim-style modal editing:
- **Command mode** (red border): Keys are commands, not text input
- **Inprompt mode** (yellow border): Keys are typed as Claude prompts

**Rationale:** Allows single-letter commands like 't' for terminal toggle without conflicting with text input. The red border in command mode provides immediate visual feedback that typing will execute commands, preventing accidental command execution.

Key mappings:
- `i` (from anywhere): Enter inprompt mode and focus input
- `t` (command mode): Toggle terminal pane
- `Escape` (in inprompt mode): Return to command mode
- `Enter` (in inprompt mode): Submit prompt

Implementation: `insert_mode: bool` in `App` struct, border color logic in `draw_input()` in `src/tui/draw_input.rs`.

### Terminal Pane

A PTY-based embedded terminal that acts as a portal to the user's actual shell:
- **Cyan border**: Terminal mode active
- Full shell emulation via `portable-pty` - runs in session's worktree
- Color support via `ansi-to-tui` conversion of ANSI escape sequences
- Proper cursor positioning via `vt100` terminal state parser
- Dynamic resizing to match pane dimensions
- Resizable height (5-40 lines)

Key mappings:
- `t` (command mode): Toggle terminal on/off
- `+/-` (command mode): Increase/decrease terminal height
- All keystrokes in terminal insert mode forward directly to PTY

Implementation:
- `terminal_pty`, `terminal_writer`, `terminal_rx`, `terminal_parser` in `App` struct
- `open_terminal()`, `close_terminal()`, `write_to_terminal()`, `poll_terminal()` in `src/app/terminal.rs`
- `draw_terminal()` in `src/tui/draw_terminal.rs` syncs vt100 parser dimensions with viewport

### Stream-JSON Parsing

Claude output is received in `stream-json` format and parsed for clean display:
- User prompts shown as "You: <message>"
- Claude responses shown as "Claude: <text>"
- Tool calls shown as timeline nodes with tool name and primary parameter
- Tool results shown with tool-specific formatting (see below)
- Completion info shown as "[Done: Xs, $X.XXXX]"
- Hook output shown as "[Hook: <name>] <output>"
- Slash commands (`/compact`, `/crt`, etc.) shown as 3-line magenta banners
- Context compaction shown as "COMPACTING CONVERSATION" 3-line yellow banner

**Tool Status Indicators:**
| Indicator | Color | Meaning |
|-----------|-------|---------|
| ● | Green | Tool completed successfully |
| ◐ | Pulsating | Tool in progress (waiting for result) |
| ✗ | Red | Tool failed (error detected in result) |

Error detection checks for: "error:", "failed", "ENOENT", "permission denied", "No such file", "command failed", non-zero exit codes.

**Tool Result Display Formats:**
| Tool | Format | Description |
|------|--------|-------------|
| Read | First + last line | Shows file boundaries with line count |
| Bash | Last 2 lines | Shows command results (usually at end) |
| Edit | Full diff | Actual file line numbers, changed lines (red/green bg), unchanged in gray |
| Write | Purpose line | Line count + first comment (from input content) |
| Grep | First 3 matches | Preview of search results |
| Glob | Directory summary | File count grouped by directory |
| Task | Summary line | First line of agent response |
| WebFetch | Title + preview | Page title and first content line |
| WebSearch | First 3 results | Numbered search results |
| LSP | Result + context | Location and code context |

**Command Detection:**
User messages containing `<command-name>/xxx</command-name>` tags are parsed as slash commands and displayed prominently with centered 3-line banners in magenta.

**Compacting Detection:**
- "COMPACTING CONVERSATION" (yellow) - shown when user message starts with "This session is being continued from a previous conversation"
- "CONVERSATION COMPACTED" (green) - shown when `<local-command-stdout>` contains "Compacted"

**Filtered Messages:**
- Meta messages (`isMeta: true`) are hidden - internal Claude instructions
- `<local-command-caveat>` messages are hidden - tells Claude to ignore local command output
- `<local-command-stdout>` content is hidden - raw output from local commands like `/memory`, `/status`
  - Exception: "Compacted" triggers the CONVERSATION COMPACTED banner before being filtered
- Rewound/edited user messages - when user rewinds to edit a message, only the corrected version is shown
  - Detection: Multiple user messages sharing the same `parentUuid` - keep only the most recent by timestamp

**Debug Output (debug builds only):**
On debug builds (`cargo run`), azural automatically dumps rendered output to `.azural/debug-output.txt` whenever session output is loaded. Contains exactly what appears in the TUI output pane with style annotations (colors, bold, italic) for debugging rendering issues.

**Markdown Rendering:**
Claude responses are parsed for markdown syntax and rendered with proper styling:
- `# H1`, `## H2`, `### H3` headers → styled with block chars (█, ▓, ▒) and colors, prefix removed
- `**bold**` → bold text without markers
- `*italic*` → italic text without markers
- `` `inline code` `` → yellow text on dark background
- ``` code blocks ``` → box-drawn borders with language label
- `| table | rows |` → box-drawing characters (│, ├, ┼, ┤)
- `- bullet` and `1. numbered` lists → indented with cyan bullets
- `> blockquotes` → gray vertical bar with italic text

Implementation: `parse_markdown_spans()`, `parse_table_row()`, `is_table_separator()` in `src/tui/markdown.rs`

**Hook Visibility - Multiple Extraction Methods:**
Claude Code hooks are captured from multiple sources in the session file:

1. **hook_progress events** (type: "progress", data.type: "hook_progress")
   - PreToolUse, PostToolUse hooks
   - Hook output extracted from `command` field's echo statements
   - Patterns: `echo 'message'` or `OUT='message'; ...; echo "$OUT"`

2. **system-reminder tags** in assistant "thinking" blocks
   - UserPromptSubmit hooks appear here (Claude Code injects them into context)
   - Claude sees the injected system-reminder and it appears in thinking output
   - Format: `<system-reminder>HookName hook success: output</system-reminder>`
   - Extracted via `extract_hooks_from_content()` in `load_claude_session_events()`

3. **system-reminder tags** in user messages and tool results
   - Various hooks that appear in user message content or tool result content
   - Same extraction pattern as thinking blocks

4. **hook_response events** (SessionStart only)
   - Only emitted for SessionStart hooks in stream-json

5. **UserPromptSubmit hook positioning**
   - Claude Code doesn't execute shell commands for UserPromptSubmit hooks (only injects output into context)
   - System-reminder with hook content appears in assistant thinking blocks (not tool_results)
   - Azural extracts UPS hooks from thinking blocks and assigns them timestamp = user_message_timestamp + 1ms
   - When events are sorted by timestamp, UPS hooks naturally appear right after their user message
   - UPS hooks from hooks.jsonl are skipped (duplicates with wrong timestamps)
   - UPS hooks display as dim gray lines: `› UserPromptSubmit: <output>`

6. **Compaction summary handling**
   - When loading a continued session, the summary message ("This session is being continued...") contains quoted `<system-reminder>` references from conversation history
   - These quoted references should NOT be treated as real hooks
   - Azural skips hook extraction for the compaction summary and its immediately following tool results
   - Flag `in_compaction_summary` tracks this state and resets only when a real user prompt is encountered

**Hook Deduplication:**
- Consecutive-only deduplication (not global)
- Same hook can appear multiple times throughout conversation
- Only back-to-back identical hooks are filtered
- Hooks display next to their corresponding tool calls

**Supported hook types:** SessionStart, UserPromptSubmit, Stop, PreToolUse, PostToolUse, SubagentStop, PreCompact

Implementation: `extract_hooks_from_content()` in `src/app/session_parser.rs`, `parse_progress_event()` in `src/events/parser.rs`

### Conversation Persistence

Each session maintains conversation history across prompts using Claude's `--resume` flag:
- Session ID captured from init event in stream-json output
- Subsequent prompts use `--resume <session_id>` (without `--fork-session`)
- History preserved in Claude Code's session storage until session is destroyed

**Stateless Data Discovery:**
Azural reads all data at runtime without persisting anything:
- **Project**: Discovered via `git rev-parse --show-toplevel`, main branch detected from git
- **Sessions**: Discovered from `git worktree list` (active) + `git branch | grep azural/` (archived)
- **Conversation**: Read from Claude's session files at `~/.claude/projects/<encoded-path>/<session-id>.jsonl`
- **Auto-discovery**: Azural scans Claude's project directory to find/link session files by worktree path
- **Live polling**: Session file is continuously polled for changes; output updates in real-time
- **Hooks**: Extracted from `system-reminder` tags embedded in Claude's session files (no separate storage)

Implementation: `find_latest_claude_session()`, `list_claude_sessions()` in `src/config.rs`, `load_sessions()` in `src/app/state.rs`

**Fixed Bug: tool_use ID Collision**
Previously when using `-p --resume` with parallel tool calls, Claude Code 2.1.19 would return "tool_use ids must be unique" error (GitHub issues #20508, #20527, #13124).

**Status:** Fixed in Claude Code 2.1.22. All resume + tools combinations now work correctly.

### Rebase Support

Sessions can be rebased onto main with conflict detection:
- View rebase status
- Navigate conflicts
- Resolve and continue

Implementation: `src/git.rs` rebase functions, `RebaseStatus` in `src/models.rs`

### Session Creation Wizard

Multi-step wizard for creating new sessions:
- Branch selection
- Worktree name configuration
- Initial prompt option

Implementation: `src/wizard.rs`

# MANIFEST

```
azural/
├── .azural/                # Project-level azural data (gitignored)
│   └── config.toml         # Optional project config
├── .project/               # Project management files
│   ├── edits/              # Edit history
│   │   └── edits.md        # Current edit log
│   └── fix.md              # Bug queue
├── refs/                   # Reference files
├── src/
│   ├── app.rs              # Module root (re-exports only)
│   ├── app/                # Application state module
│   │   ├── state.rs        # State module root (re-exports only)
│   │   ├── state/          # State submodules
│   │   │   ├── app.rs      # App struct definition + new()
│   │   │   ├── load.rs     # Session loading and discovery
│   │   │   ├── sessions.rs # Session navigation and CRUD
│   │   │   ├── output.rs   # Output processing
│   │   │   ├── scroll.rs   # Scroll operations
│   │   │   ├── claude.rs   # Claude session handling
│   │   │   ├── file_browser.rs # File tree and viewer
│   │   │   ├── ui.rs       # Focus, dialogs, menus, wizard
│   │   │   └── helpers.rs  # Utility functions
│   │   ├── session_parser.rs # Claude session file parsing
│   │   ├── terminal.rs     # PTY terminal management
│   │   ├── types.rs        # Enums (Focus, ViewMode, dialogs)
│   │   ├── input.rs        # Input handling methods
│   │   └── util.rs         # ANSI stripping, JSON parsing
│   ├── tui.rs              # Module root (re-exports only)
│   ├── tui/                # Terminal UI module
│   │   ├── run.rs          # TUI entry point and 4-pane layout
│   │   ├── event_loop.rs   # Event handling loop
│   │   ├── util.rs         # Display utilities (re-exports)
│   │   ├── colorize.rs     # Output colorization
│   │   ├── markdown.rs     # Markdown parsing
│   │   ├── render_events.rs # DisplayEvent rendering
│   │   ├── render_tools.rs # Tool result rendering
│   │   ├── draw_sidebar.rs # Sessions pane rendering
│   │   ├── draw_file_tree.rs # FileTree pane rendering
│   │   ├── draw_viewer.rs  # Viewer pane rendering
│   │   ├── draw_output.rs  # Convo pane rendering
│   │   ├── draw_*.rs       # Other rendering functions
│   │   ├── input_file_tree.rs # FileTree navigation
│   │   ├── input_viewer.rs # Viewer scroll handling
│   │   └── input_*.rs      # Other input handlers
│   ├── events.rs           # Module root (re-exports only)
│   ├── events/             # Stream-JSON events module
│   │   ├── types.rs        # Raw Claude Code event types
│   │   ├── display.rs      # DisplayEvent enum
│   │   └── parser.rs       # EventParser + tests
│   ├── git.rs              # Module root (re-exports only)
│   ├── git/                # Git operations module
│   │   ├── core.rs         # Git struct, repo detection, diffs
│   │   ├── branch.rs       # Branch management
│   │   ├── rebase.rs       # Rebase operations
│   │   └── worktree.rs     # Worktree create/delete/list
│   ├── cmd/                # CLI command handlers
│   │   ├── mod.rs          # Main command routing
│   │   ├── session.rs      # Session list/show commands
│   │   └── project.rs      # Project info command
│   ├── claude.rs           # Claude CLI process management
│   ├── cli/mod.rs          # CLI argument parsing
│   ├── config.rs           # Configuration paths, Claude session discovery
│   ├── main.rs             # Entry point
│   ├── models.rs           # Domain models (Session, Project, etc.)
│   ├── syntax.rs           # Syntax highlighting for diffs
│   └── wizard.rs           # Session creation wizard
├── worktrees/              # Git worktrees for sessions
├── AGENTS.md               # This file
├── CHANGELOG.md            # Version history
├── Cargo.toml              # Rust dependencies
├── PTY_FEATURE.md          # PTY implementation notes
├── README.md               # User-facing documentation
└── WORKTREES.md            # Worktree documentation
```

# ROADMAP

## Phase 1: Core Functionality (Current)
- [x] TUI with session/output/input panels
- [x] Git worktree creation and management
- [x] Claude CLI spawning with `-p` mode
- [x] Multi-session concurrent agents
- [x] Stream-JSON parsing for clean output
- [x] Conversation persistence via --resume
- [x] Diff viewing with syntax highlighting
- [x] Rebase support
- [x] Vim-style modal input (command/insert modes)
- [x] Embedded terminal pane for shell commands

## Phase 2: Enhanced UX
- [x] File viewer pane (4-pane layout: Sessions, FileTree, Viewer, Convo)
- [ ] Token estimate counter on input
- [ ] Auto-rebase hooks when main is ahead
- [ ] PTY mode for full Claude interactivity
- [ ] Session templates
- [ ] Per-project configuration
- [ ] Theme customization
- [ ] Input history persistence
- [ ] Search/filter sessions

## Phase 3: Advanced Features
- [ ] Session export/reporting
- [ ] Cross-session context sharing
- [ ] Agent orchestration (one agent spawns tasks for others)
- [ ] Custom tool definitions per session

# TESTING REQUIREMENTS

## Domain-Specific Guidelines

This is a TUI + CLI wrapper application with stateless architecture. Testing focuses on:

1. **Process Management**: Verify Claude processes spawn, communicate, and terminate correctly
2. **State Discovery**: Ensure app correctly discovers sessions from git worktrees and branches
3. **Event Parsing**: Validate stream-json parsing handles all event types
4. **Concurrent Operations**: Test multiple sessions running Claude simultaneously
5. **Error Recovery**: Verify graceful handling of Claude exits and git errors

## Test Categories

- Unit tests for parsing functions (`parse_stream_json_for_display`, event parsing)
- Integration tests for git operations (worktree create/delete/list)
- Integration tests for session discovery from git state
- E2E tests for TUI event handling (would require mock terminal)

# REFERENCES

(None fetched yet)

---

## **CONFLICTS**

(None)

# USE

## Installation

```bash
cargo install --path .
```

## Running

```bash
# Launch the TUI
azural tui

# Or simply
azural
```

## Keybindings

### Global (Command Mode)
| Key | Action |
|-----|--------|
| `i` | Enter inprompt mode (focus input) |
| `t` | Toggle terminal pane |
| `j/k` | Navigate (sessions, files, scroll) |
| `J/K` | Navigate projects |
| `Tab` | Cycle focus (Sessions → FileTree → Viewer → Convo → Input) |
| `Shift+Tab` | Cycle focus reverse |
| `n` | New session |
| `d` | View diff |
| `Space` | Context menu (Sessions) / Toggle expand (FileTree) |
| `?` | Help |
| `Ctrl+c` | Quit |

### FileTree Pane
| Key | Action |
|-----|--------|
| `j/k` | Navigate up/down |
| `Enter` | Open file in Viewer / Expand directory |
| `h/l` | Collapse/Expand directory |
| `Space` | Toggle directory expand |

### Viewer Pane
| Key | Action |
|-----|--------|
| `j/k` | Scroll up/down |
| `Ctrl+d/u` | Half-page scroll |
| `Ctrl+f/b` | Full-page scroll |
| `g/G` | Jump to top/bottom |
| `Esc` | Clear viewer, return to FileTree |
| `q` | Return to FileTree (keep content) |

### Insert Mode (Input Focused)
| Key | Action |
|-----|--------|
| `Escape` | Return to command mode |
| `Enter` | Submit prompt / execute command (terminal) |

### Terminal Mode
| Key | Action |
|-----|--------|
| `t` | Close terminal (command mode) |
| `+/-` | Resize terminal height (command mode) |
| `Enter` | Execute shell command (insert mode) |
