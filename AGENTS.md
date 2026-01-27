# SUMMARY

Azural (Agent Zones: Unified Runtime for Autonomous LLMs) is a Rust TUI application that wraps Claude Code CLI to enable multi-agent development workflows. Each "Session" is a git worktree with its own Claude agent, allowing concurrent AI-assisted development across multiple feature branches.

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
Claude Code's `-p` mode is "print and exit". The `--input-format stream-json` is for *chaining separate processes*, not keeping one alive. Verified by testing.

Implementation: `src/claude.rs` spawns processes, `src/app.rs` tracks `claude_session_ids` HashMap for --resume.

### Git Worktree Isolation

Sessions are backed by git worktrees, providing true branch isolation. Each worktree:
- Has its own working directory
- Can have different uncommitted changes
- Operates on a separate branch from main

Implementation: `src/git.rs` handles worktree creation, deletion, and status queries.

### TUI Interface

A ratatui-based terminal interface with:
- Session list panel (left side)
- Output display panel (main area)
- Input field with vim-style modal editing
- Diff viewer with syntax highlighting
- Help overlay with keybindings
- Mouse scroll support (scroll panels based on cursor position, Shift+drag for text selection)

**Performance Optimizations:**
- Event batching: All pending events drained before redrawing
- Scroll throttling: 20fps max for scroll redraws, immediate for key/Claude events
- Cached terminal size: Only updates on resize events
- Conditional polling: Terminal rx only polled when terminal mode active
- Motion discard: Mouse motion events discarded instantly (zero processing)

Implementation: `src/tui/event_loop.rs` for event loop, `src/tui/mod.rs` for rendering, `src/app.rs` for state management.

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

Implementation: `insert_mode: bool` in `App` struct, border color logic in `draw_input()` in `src/tui.rs`.

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
- `open_terminal()`, `close_terminal()`, `write_to_terminal()`, `poll_terminal()` in `src/app.rs`
- `draw_terminal()` in `src/tui.rs` syncs vt100 parser dimensions with viewport

### Stream-JSON Parsing

Claude output is received in `stream-json` format and parsed for clean display:
- User prompts shown as "You: <message>"
- Claude responses shown as "Claude: <text>"
- Tool usage shown as "[Using <name>...]"
- Completion info shown as "[Done: Xs, $X.XXXX]"
- Hook output shown as "[Hook: <name>] <output>"

**Hook Visibility Workaround:**
Claude Code's stream-json output only emits `hook_response` events (with output) for `SessionStart` hooks. For other hook types (UserPromptSubmit, PreToolUse, PostToolUse), the hooks execute but output isn't streamed.

**Solution:** File-based IPC system where hooks write to project's `.azural/hooks.jsonl` and azural polls for new entries:
1. Helper script `~/.claude/scripts/log-hook.sh` - logs hook output with timestamp to JSON file
2. All hooks in `~/.claude/settings.json` run `log-hook.sh HookName "$OUT" &` in background before echo
3. App polls `hooks.jsonl` every event loop iteration for new entries
4. Hooks are saved to database with `OutputType::Hook` for persistence across session switches
5. New entries create `DisplayEvent::Hook` and render in output pane

This enables ALL hook types to display in the output pane and persist when switching sessions.

Implementation: `poll_hooks_file()` in `src/app/mod.rs` (polls and saves to DB), `OutputType::Hook` in `src/models.rs`, `log-hook.sh` helper script

### Hooks Logging

All Claude Code hook events (that are emitted to stream-json) are captured to a JSON Lines file:
- File location: `<project>/.azural/hooks.jsonl` (project-level, falls back to `~/.azural/` if not in git repo)
- Format: One JSON object per line with timestamp, session_id, hook_name, output, and raw event
- CLI: `azural hooks` to view recent hooks

**CLI Usage:**
```bash
azural hooks              # Show last 20 hooks
azural hooks -l 50        # Show last 50 hooks
azural hooks --json       # Output raw JSON lines
azural hooks -n "submit"  # Filter by hook name
azural hooks --clear      # Clear the hooks log
```

Implementation: `log_hook_event()` in `src/app/util.rs`, `handle_hooks()` in `src/cmd/mod.rs`

### Conversation Persistence

Each session maintains conversation history across prompts using Claude's `--resume` flag:
- Session ID captured from init event in stream-json output
- Subsequent prompts use `--resume <session_id>` (without `--fork-session`)
- History preserved in Claude Code's session storage until session is destroyed

**Data Storage Architecture:**
Azural reads conversation data from Claude's session files with auto-discovery:
- **Primary**: Claude's session files at `~/.claude/projects/<encoded-path>/<session-id>.jsonl`
- **Auto-discovery**: If no `claude_session_id` is set, azural scans Claude's project directory and links the most recent session file
- **Hooks**: Read from project's `.azural/hooks.jsonl`, merged by timestamp with conversation events
- **Fallback**: Database `session_outputs` table when no Claude session files exist
- **azural.db**: Stores session metadata; outputs saved as fallback

Implementation: `find_latest_claude_session()`, `list_claude_sessions()` in `src/config.rs`

Implementation: `load_session_output()`, `load_claude_session_events()`, `load_hooks_with_timestamps()` in `src/app/mod.rs`, `claude_session_file()` in `src/config.rs`

**Known Bug: tool_use ID Collision (Fixed by Rollback)**
When using `-p --resume` and Claude makes parallel tool calls, the API returns "tool_use ids must be unique" error. This is a known Claude Code bug (GitHub issues #20508, #20527, #13124) **introduced in 2.1.19**.

**Workaround:** Use Claude Code ≤ 2.1.18:
```bash
npm install -g @anthropic-ai/claude-code@2.1.18
```

Pattern on 2.1.19 (broken):
- Simple → Tools resume: ❌ Fails
- Tools → Tools resume: ❌ Fails

Pattern on 2.1.17/2.1.18 (works):
- All combinations: ✅ Works

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
│   ├── azural.db           # SQLite database for sessions/outputs
│   ├── hooks.jsonl         # Hook events log
│   └── config.toml         # Optional project config
├── .project/               # Project management files
│   ├── edits/              # Edit history
│   │   └── edits.md        # Current edit log
│   └── fix.md              # Bug queue
├── refs/                   # Reference files
├── src/
│   ├── app/                # Application state module
│   │   ├── mod.rs          # App struct, state, core methods
│   │   ├── terminal.rs     # PTY terminal management
│   │   ├── types.rs        # Enums (Focus, ViewMode, dialogs)
│   │   ├── input.rs        # Input handling methods
│   │   └── util.rs         # ANSI stripping, JSON parsing, hooks logging
│   ├── tui/                # Terminal UI module
│   │   ├── mod.rs          # Main layout and entry
│   │   ├── event_loop.rs   # Event handling loop
│   │   ├── util.rs         # Display utilities
│   │   ├── draw_*.rs       # Rendering functions
│   │   └── input_*.rs      # Mode-specific input handlers
│   ├── cmd/                # CLI command handlers
│   │   └── mod.rs          # Session, project, hooks commands
│   ├── claude.rs           # Claude CLI process management
│   ├── config.rs           # Configuration loading/saving
│   ├── db.rs               # SQLite database operations
│   ├── events.rs           # Stream-JSON event types
│   ├── git.rs              # Git and worktree operations
│   ├── main.rs             # Entry point and CLI
│   ├── models.rs           # Domain models (Session, Project, etc.)
│   ├── session.rs          # Session management layer
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
- [ ] File viewer/editor pane (third column)
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

This is a TUI + CLI wrapper application. Testing focuses on:

1. **Process Management**: Verify Claude processes spawn, communicate, and terminate correctly
2. **State Consistency**: Ensure app state matches database state and git worktree state
3. **Event Parsing**: Validate stream-json parsing handles all event types
4. **Concurrent Operations**: Test multiple sessions running Claude simultaneously
5. **Error Recovery**: Verify graceful handling of Claude exits, git errors, DB failures

## Test Categories

- Unit tests for parsing functions (`parse_stream_json_for_display`, event parsing)
- Integration tests for git operations (worktree create/delete/list)
- Integration tests for database CRUD operations
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
| `j/k` | Navigate sessions |
| `J/K` | Navigate projects |
| `Tab` | Cycle focus |
| `n` | New session |
| `d` | View diff |
| `Space` | Context menu |
| `?` | Help |
| `Ctrl+c` | Quit |

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
