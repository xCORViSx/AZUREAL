# Changelog

All notable changes to Azural will be documented in this file.

## [Unreleased]

### Added
- Hooks file watching - azural polls `<project>/.azural/hooks.jsonl` for entries from ALL hook types
  - File-based IPC workaround for Claude Code's stream-json limitation
  - Works with `~/.claude/scripts/log-hook.sh` helper script
  - All hooks (PreToolUse, PostToolUse, UserPromptSubmit, etc.) now display in output pane
- PTY-based embedded terminal pane - press `t` to toggle a full shell terminal
  - Acts as a portal to the user's actual terminal within Azural
  - Full color support with ANSI escape sequences via `ansi-to-tui`
  - Proper cursor positioning and terminal emulation via `vt100` parser
  - Dynamic resizing to match pane dimensions
- Multi-session concurrent Claude agents - each session can run its own Claude process
- Conversation continuity via `--resume <session_id>` flag
- Clean output display parsing stream-json format:
  - User prompts shown as "You: <message>"
  - Claude responses shown as "Claude: <text>"
  - Tool usage shown as "[Using <tool> | <param>]" with parameter preview
  - Completion info shown as "[Done: Xs, $X.XXXX]"
- Mouse scroll support - scroll panels based on cursor position (independent of keyboard focus, Shift+drag for text selection)
- iMessage-style output formatting:
  - User messages: right-aligned cyan
  - Claude messages: left-aligned orange
  - Two blank lines between transitions
  - Timeline-style tool use display with parameter preview
### Changed
- Conversation data now read from Claude's session files with auto-discovery
  - Auto-discovers Claude session files by scanning `~/.claude/projects/<encoded-path>/`
  - Links most recent session file to azural session automatically
  - Hooks loaded from `<project>/.azural/hooks.jsonl` and merged by timestamp
  - Fallback to database when no Claude session files exist

### Optimized
- Event loop CPU usage reduced from 60-90% to <20% during mouse interaction:
  - Event batching: all pending events drained before redrawing
  - Scroll throttling: 20fps max for scroll, immediate for key events
  - Cached terminal size: only updates on resize events
  - Mouse motion events discarded instantly (zero processing)
  - Conditional terminal polling: only when terminal mode active

### Changed
- Storage moved from system-level (`~/.azural/`) to project-level (`.azural/` in git root)
  - Database, hooks.jsonl, and config are now per-project
  - Eliminates cross-project hook pollution
  - Falls back to `~/.azural/` if not in a git repository
- Updated all dependencies to latest versions:
  - ratatui: 0.29 → 0.30
  - crossterm: 0.28 → 0.29
  - ansi-to-tui: 7 → 8
  - portable-pty: 0.8 → 0.9
  - vt100: 0.15 → 0.16
- Prompt echo format changed from "> " to "You: " for consistency

### Fixed
- Hooks now persist across session switches - saved to database with OutputType::Hook
- Hook logging runs in background (`&`) to ensure execution even if Claude Code terminates early
- Hook output no longer truncated to 50 characters - full first line now displays
- Event parser now captures ALL content blocks from assistant messages (was only capturing first)
- Fixed raw JSON appearing in output when tool_result content contained "Hook" text - now parses JSON before checking text patterns
- Fixed empty UserMessage boxes appearing for tool_result events with no text content
- Improved hook event parsing to only show hooks with actual output (skips hook_started events)
- Identified Claude Code 2.1.19 bug breaking `-p --resume` with tool calls
- Documented workaround: use Claude Code ≤ 2.1.17
- Resolved hook visibility limitation via file-based IPC workaround (all hooks now display)
