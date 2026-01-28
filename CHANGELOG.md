# Changelog

All notable changes to Azural will be documented in this file.

## [Unreleased]

### Changed
- Replaced SQLite database (`azural.db`) with JSON config (`azural.json`) for minimal footprint
  - Session outputs now read exclusively from Claude's JSONL session files
  - One-time automatic migration from SQLite if old database exists
  - Human-readable JSON format for debugging and manual inspection

### Added
- Tool progress animation: Pulsating indicator (`◐`) while running, green (`●`) on success, red (`✗`) on failure
  - Visual feedback during tool execution matching Claude Code CLI style

### Fixed
- Tool error status now works when loading from session file (not just live streaming)
  - Errors detected by content patterns: "error:", "failed", "ENOENT", "permission denied", non-zero exit codes
  - Failed tools show red `✗` indicator instead of green `●`
- Pending tool status now tracked when loading from session file
  - Tools with `tool_use` but no `tool_result` yet show pulsating `◐` indicator
- Compaction summary no longer shows raw text blob as hooks
  - Summary message contains quoted `<system-reminder>` tags from conversation history
  - Compaction messages skipped during hook extraction (detected by summary format)
- UserPromptSubmit (UPS) hooks now appear directly after user prompts
  - UPS hooks extracted from assistant "thinking" blocks where Claude Code injects them
  - Hooks assigned timestamp = user_message_timestamp + 1ms for correct sort order
  - When events are sorted by timestamp, UPS hooks now appear immediately after their user message
  - UPS hooks from hooks.jsonl are skipped (duplicates of session file hooks with wrong timestamps)
  - Previously UPS hooks appeared after tool activity instead of after user prompts
- Command display: Slash commands (`/compact`, `/crt`, etc.) shown as prominent 3-line centered magenta banners
- Compacting indicator: "COMPACTING CONVERSATION" yellow banner when compaction starts
- Compacted indicator: "CONVERSATION COMPACTED" green banner when compaction completes
- Filtered out internal Claude messages: `<local-command-caveat>`, `<local-command-stdout>`, meta messages
- Rewound message deduplication: When user rewinds to edit a message, only the corrected version is shown
  - Detects by `parentUuid` - multiple user messages sharing the same parent, keeps only the most recent
- Debug dump (debug builds only): Auto-writes `.azural/debug-output.txt` on session load
  - Shows rendered output exactly as it appears in the TUI (with styling annotations)
  - Only enabled in debug builds (`cargo run`), not release builds
- Markdown rendering in Claude response output:
  - Headers (`#`, `##`, `###`) styled with block characters and colors
  - Bold (`**text**`) rendered without markers
  - Italic (`*text*`) rendered without markers
  - Inline code (`` `code` ``) with dark background
  - Code blocks (``` ```) with language label and box-drawn borders
  - Tables with `|` converted to box-drawing characters
  - Bullet and numbered lists properly indented
  - Blockquotes with vertical bar styling
- Hooks file watching - azural polls `<project>/.azural/hooks.jsonl` for entries from ALL hook types
  - File-based IPC workaround for Claude Code's stream-json limitation
  - Works with `~/.claude/scripts/log-hook.sh` helper script
  - All hooks (PreToolUse, PostToolUse, UserPromptSubmit, etc.) now display in output pane
- Live session output - azural continuously polls the Claude session file for changes
  - Output pane updates in real-time as you chat with Claude in another terminal
  - No need to switch sessions to see new messages
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
  - Tool calls shown as timeline nodes with tool name and primary parameter
  - Tool results with tool-specific formatting:
    - Read: first + last line with line count
    - Bash: last 2 lines (results usually at end)
    - Edit: complete diff output with actual file line numbers, red/green coloring for changed lines only, gray context for unchanged
    - Write: first comment/purpose line + line count
    - Grep: first 3 matches with overflow indicator
    - Glob: file count grouped by directory
    - Task: summary line from agent
    - WebFetch: page title + preview
    - WebSearch: first 3 numbered results
    - LSP: location + code context
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

### Changed
- Sessions now load scrolled to bottom (most recent messages visible)
  - Initial load, session switch, and 'o' key all scroll to bottom
  - Use 'g' to scroll to top if needed

### Fixed
- Output pane now loads conversation history on startup (was empty until switching sessions)
  - Added `load_session_output()` call after `app.load()` in startup sequence
- All hook types now display in output pane (UserPromptSubmit, PreToolUse, PostToolUse, etc.)
  - Parses `hook_progress` events from Claude Code's session data
  - Extracts hook output from echo commands in hook definitions
  - Parses hook output from system-reminder tags in user messages AND tool results
  - Previously only SessionStart hooks were visible
- Tool results now display in realtime during Claude's response (not just after completion)
  - EventParser now tracks tool calls by ID to match with tool_result blocks
  - Previously tool results only appeared after switching away from output pane and back
- Edit tool now shows actual diff with red/green highlighted backgrounds and real file line numbers
  - Extracts `old_string`/`new_string` from ToolCall input (not ToolResult which only has success message)
  - Reads file to find where edit occurred, displays actual line numbers (not relative 1,2,3...)
  - Only changed lines are highlighted - unchanged lines show in gray as context
  - Removed lines: white text on red background
  - Added lines: black text on green background
  - Diff displayed inline with the tool call for immediate visibility
- Write tool now shows line count + purpose line from ToolCall input (not empty result message)
  - Extracts `content` from ToolCall input to count lines and find first comment/purpose line
  - Displays inline with the tool call for immediate visibility
- Tool results now strip `<system-reminder>` blocks that Claude Code appends
  - Removes entire block (tags + content) so malware disclaimers don't appear in output
- Read tool now shows last non-empty line (skips trailing empty lines like `60→`)
- Init event no longer appears mid-conversation - only first Init shown
- Hook deduplication now consecutive-only (not global) - hooks appear throughout conversation
  - Previously: each unique (name, output) pair shown only once at first occurrence
  - Now: same hook can appear multiple times, only consecutive identical hooks deduplicated
  - Hooks now display next to their corresponding tool calls instead of clustering at beginning
- UserPromptSubmit hooks now extracted from session file via system-reminder tags
  - Added `extract_hooks_from_content` to `load_claude_session_events`
  - Parses hooks from user message content and tool result content
  - Extracts hooks from meta messages (`isMeta: true`) before skipping them for display
- Hook time-filtering now uses `now()` as upper bound instead of last event timestamp
  - Previously hooks after last session event were filtered out (5s buffer too small)
  - Now all hooks from session start to present are included
- Polling parser now properly captures ToolCall and ToolResult events for parallel tool calls
  - Fixed missing ToolResults when Claude makes multiple tool calls at once
  - Tool calls tracked by ID to match results to their corresponding calls
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
