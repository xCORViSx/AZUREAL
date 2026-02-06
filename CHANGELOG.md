# Changelog

All notable changes to Azureal will be documented in this file.

## [Unreleased]

### Optimized
- Deferred initial render: large conversations (200+ events) only render the tail on initial load
  - User starts at bottom, sees recent messages instantly (no 10s+ wait)
  - Full render happens lazily when scrolling to top
- Edit diff render no longer reads files from disk (was O(file_size) per Edit event)
  - Eliminated `std::fs::read_to_string()` + substring search per Edit tool call
  - Uses relative line numbers instead — convo panel is a summary view
- Edit diff syntax highlighting reduced from 3→2 calls per event
  - Reuses base syntect parse, applies background colors via cheap span iteration
- Incremental JSONL parsing: seeks to last byte offset, parses only newly appended lines
  - Rebuilds tool_call context from existing DisplayEvents via `IncrementalParserState`
  - Falls back to full re-parse if file shrank or user-message rewrite detected
- Incremental rendering: appends only new display events to cached rendered lines
  - Skips full re-render when width unchanged and events only grew
  - Pre-scans existing events to establish state flags for correct continuation
- Fast path in `wrap_text()`: skips textwrap entirely when text fits in one line
- Reduced clones in render pipeline: borrow file_path, reference-compare hooks before cloning
- Removed redundant `.wrap(Wrap { trim: false })` from Convo Paragraph
  - Content is pre-wrapped by `wrap_text()`/`wrap_spans()` — ratatui was re-wrapping every viewport line char-by-char per frame
- Animation patching loop now skipped when no tools are pending (avoids pulse computation on every scroll frame)

### Changed
- Convo pane now extends full height (down to status bar), no longer shares height with Input/Terminal
  - Input/Terminal pane now spans only the first 3 panes (Sessions, FileTree, Viewer)
  - Gives Convo pane more vertical space for reading conversation history
  - Mouse scroll dispatch updated for asymmetric layout
- Terminal keybindings moved from help panel to terminal pane title bar
  - Command mode title: `(t:type | p:prompt | Esc:close | j/k:scroll | J/K:page | g/G:top/bottom | +/-:resize)`
  - Type mode title: `(Esc:exit)`
  - Scroll mode title: `[N↑] (j/k:scroll | ... | Esc:close)`
  - Help panel (`?`) no longer has a Terminal section
  - All title hints dynamically sourced from `TERMINAL` binding array (single source of truth)
- Input keybindings moved from help panel to prompt input pane title bar
  - Type mode title: `(Esc:exit | Enter:submit | ⌃c:cancel | ↑/↓:history | ⌥←/→:word | ⌃w:del wrd | ⌃u:clear)`
  - Command mode title: `(p:type | t:terminal)`
  - Help panel (`?`) no longer has an Input section
  - All title hints dynamically sourced from `INPUT` binding array (single source of truth)

### Fixed
- Prompt input keybindings now actually work: ⌥c (clear), ↑/↓ (history), word nav
  - INPUT binding array previously declared ⌃z/⌃x for word nav, which conflicted with clipboard cut/undo
  - Word nav now uses standard macOS ⌥←/⌥→ (and ⌃←/⌃→) matching the actual handler
  - Added missing handlers for ⌃u (clear input), ↑ (history prev), ↓ (history next)
  - Prompt history browses UserMessage entries from the session conversation
- Prompt input no longer crashes on multi-byte characters (e.g., `ç` from ⌥+c)
  - `input_cursor` was used as both char index and byte offset — `String::insert()`/`remove()` need byte offsets
  - Added `char_to_byte()` conversion; all String operations now use byte offsets derived from char index
  - Also fixed `input_right()` and `input_end()` comparing char index against `String::len()` (bytes)
- User prompts no longer appear twice in the Convo pane
  - `pending_user_message` dedup was limited to last 5 events; Claude's rapid output (hooks, tools, text) pushed the matching `UserMessage` beyond that window
  - Now scans backward to the most recent `UserMessage` regardless of distance from tail
- Session dropdown in Worktrees pane now shows custom names from `.azureal/sessions.toml` instead of truncated UUIDs
- `KeyCombo::display()` now preserves character case — previously uppercased all chars (e.g., `r` showed as `R`)
- `KeyCombo::display()` no longer shows `⇧` prefix for uppercase char keys (J, K, G, R show as-is)
- Quit simplified to `⌃q` (was `⌃⌥⌘c`), Restart to `⌃r` (was `⌃⌥⌘r`)
- `⌃c` now cancels Claude response only (was also quit)

### Added
- Run command system: save, pick, edit, delete, and execute shell commands from Worktrees pane
  - `r` opens picker (or executes directly if only 1 command saved)
  - `⌥r` opens dialog to add a new run command
  - `R` now performs rebase onto main (moved from `r`)
  - Picker: `j/k` navigate, `1-9` quick-select, `e` edit, `x` delete, `a` add
  - Commands persisted in `.azureal/run_commands.json`, loaded on startup
- Unified "New..." dialog with tabs for creating different resources
  - `n` from Worktrees pane opens tabbed dialog
  - Tab 1: Project (placeholder)
  - Tab 2: Branch (placeholder)
  - Tab 3: Worktree - existing worktree creation functionality
  - Tab 4: Session - create new Claude session with optional custom name
    - Custom names stored in `.azureal/sessions.toml`
    - Leave name blank to use Claude's auto-generated UUID
    - Select target worktree for the session
  - `←`/`→` to switch tabs (except during text input)
- Clipboard operations for both Viewer edit mode and Prompt input (system clipboard)
  - `⌘C` - Copy to system clipboard
  - `⌘X` - Cut to system clipboard
  - `⌘V` - Paste from system clipboard
  - `⌘A` - Select all
  - `Shift+Arrow` - Extend selection
  - Selection highlighted with blue background
  - Typing/backspace/delete replaces selection
  - Works with external apps (copy from browser, paste in azureal, etc.)
- Hidden files/directories now shown in FileTree (previously filtered out)
  - Sorted after non-hidden items within each category (dirs/files)
  - Displayed in dimmed colors: gray for files, muted cyan for directories
  - Children of hidden directories inherit dimmed styling
  - Still excludes `target/` and `node_modules/` (too noisy)

### Fixed
- `.azureal/` directory no longer created eagerly on startup
  - Global config uses `~/.azureal/` (created only when needed)
  - Project data uses `.azureal/` in git root (created only when writing data)
  - Prevents unwanted `.azureal/` directories appearing in every git repo you run azureal from
- Centralized keybindings module (`src/tui/keybindings.rs`)
  - All keybindings defined once, used by both input handlers and help dialog
  - `Action` enum for all possible actions
  - `Keybinding` struct with primary + alternatives (e.g., j/↓ for same action)
  - `lookup_action()` for input handler dispatch
  - `help_sections()` auto-generates help dialog content
  - Adding/changing a keybinding now updates help automatically

### Changed
- Keybinding updates for terminal and prompt navigation:
  - `Esc` now closes terminal (was `t`)
  - `t` enters terminal type mode (was `i`)
  - `p` in terminal command mode closes terminal and enters Claude prompt
  - `p` is now global: closes help panel and enters prompt from anywhere
  - Terminal title shows context-aware hints: `t:type | p:prompt | Esc:close` in command mode, `Esc:exit` in type mode
  - Prompt title shows `⌃C:cancel response` in type mode

### Optimized
- Session file polling now uses lightweight file size check + dirty flag pattern
  - `check_session_file()` only reads file metadata (no parsing)
  - `poll_session_file()` defers parsing until idle via dirty flag
  - `refresh_session_events()` is a lightweight path that skips terminal/file tree reload

### Added
- Run commands feature: save and execute shell commands/scripts
  - `r` - Execute run command (picker if multiple, direct if one)
  - `⌥r` - Add new run command (name + command fields)
  - Picker dialog supports `j/k` nav, `1-9` quick select, `e` edit, `x` delete
  - Commands persisted to `.azureal/run_commands.json`
- Debug output now triggered manually via `⌃⌥⌘D` (Ctrl+Opt+Cmd+D)
  - Saves session parsing diagnostics to `.azureal/debug-output.txt`
  - Removed `--out`/`-D` flag and `cargo rd` alias
- Viewer tabs: `t` to tab current file, `T` for tab dialog, `[`/`]` to switch
- Clickable file paths for Read, Write, and Edit tools in Convo pane
- Clickable file paths for Read, Write, and Edit tools in Convo pane
  - File paths are underlined and can be clicked to open in Viewer
  - Edit tool clicks show file with diff overlay highlighting changes
  - Read/Write tool clicks open file without diff overlay
- 4-pane TUI layout: Sessions (40 cols), FileTree (40 cols), Viewer (50%), Convo (50%)
  - FileTree shows directory structure for selected session's worktree
  - Viewer displays file content with syntax highlighting and line numbers
  - Viewer dual-purpose: file preview from FileTree OR diff detail from Convo
  - Tab cycles through all 4 panes plus Input
- FileTree navigation with j/k, Enter to open, Space/l to expand, h to collapse
- Viewer scroll with j/k, Ctrl+d/u, Ctrl+f/b, g/G
- Per-session terminal persistence: each session maintains its own PTY shell session

### Changed
- Message bubble containment: all content constrained to bubble + 10 max width
  - User/Claude message text wraps within bubble width
  - Tool calls, results, hooks, diffs constrained to bubble + 10
- Tool command lines show full parameters (no "..." truncation on commands)

### Fixed
- **Critical performance fix**: `SyntaxHighlighter::new()` was being called inside `render_edit_diff` on EVERY render frame, loading the entire syntect SyntaxSet each time. Now reuses single instance from App state.
- **Render caching**: Convo pane now caches rendered lines instead of re-rendering all events on every frame. Cache invalidated only when display_events actually change. Eliminates O(n) rendering on scroll/navigation.
- **Scroll optimization**: Scroll functions now return whether position changed; skip redraw when at boundaries (no wasted frames when already at top/bottom)
- **Animation throttling**: Pulsating tool indicators now update at 4fps instead of every frame; scroll throttled to 10fps (was 20fps)
- Session file polling throttled from 100ms to 500ms to reduce parsing overhead on large sessions
- Removed debug dump on every redraw (was causing disk I/O on every frame in debug builds)
- Tool results show summarized output constrained to width:
  - Read: first + last line with line count
  - Bash: last 2 non-empty lines
  - Grep: first 3 matches
  - Glob: file count
  - Task: first 5 lines
- Modularized large source files using file-based module roots:
  - Module root files (`app.rs`, `git.rs`, `events.rs`, `tui.rs`) now contain only mod declarations and re-exports
  - Created `app/state.rs` for App struct and core methods (extracted from app.rs)
  - Created `app/session_parser.rs` for Claude session file parsing
  - Created `git/core.rs` for Git struct and core operations
  - Created `events/types.rs`, `events/display.rs`, `events/parser.rs` (split from events.rs)
  - Created `tui/run.rs` for TUI entry point and main layout
  - Split `tui/util.rs` into `colorize.rs`, `markdown.rs`, `render_events.rs`, `render_tools.rs`
- Replaced SQLite database (`azureal.db`) with JSON config (`azureal.json`) for minimal footprint
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
- Debug dump (debug builds only): Auto-writes `.azureal/debug-output.txt` on session load
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
- Hooks file watching - azureal polls `<project>/.azureal/hooks.jsonl` for entries from ALL hook types
  - File-based IPC workaround for Claude Code's stream-json limitation
  - Works with `~/.claude/scripts/log-hook.sh` helper script
  - All hooks (PreToolUse, PostToolUse, UserPromptSubmit, etc.) now display in output pane
- Live session output - azureal continuously polls the Claude session file for changes
  - Output pane updates in real-time as you chat with Claude in another terminal
  - No need to switch sessions to see new messages
- PTY-based embedded terminal pane - press `t` to toggle a full shell terminal
  - Acts as a portal to the user's actual terminal within Azureal
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
  - Links most recent session file to azureal session automatically
  - Hooks loaded from `<project>/.azureal/hooks.jsonl` and merged by timestamp
  - Fallback to database when no Claude session files exist

### Optimized
- Event loop CPU usage reduced from 60-90% to <20% during mouse interaction:
  - Event batching: all pending events drained before redrawing
  - Scroll throttling: 20fps max for scroll, immediate for key events
  - Cached terminal size: only updates on resize events
  - Mouse motion events discarded instantly (zero processing)
  - Conditional terminal polling: only when terminal mode active

### Changed
- Storage moved from system-level (`~/.azureal/`) to project-level (`.azureal/` in git root)
  - Database, hooks.jsonl, and config are now per-project
  - Eliminates cross-project hook pollution
  - Falls back to `~/.azureal/` if not in a git repository
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
