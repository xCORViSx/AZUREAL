# Edit History

## 2026-02-02: Viewer Edit Mode Clipboard Operations

### Feature
Added copy, cut, paste, and selection support to the viewer's edit mode.

### Implementation
- Added `clipboard: String` field to App struct for storing copied/cut text
- Selection methods: `viewer_edit_start_selection()`, `viewer_edit_extend_selection()`, `viewer_edit_clear_selection()`
- Selection-aware movement: `viewer_edit_left_select(extend)`, etc. - extend selection with Shift+Arrow
- `get_selected_text()` extracts text from selection range (handles multi-line)
- `delete_selection_text()` removes selected text and returns it (used by cut and typing)
- Clipboard operations: `viewer_edit_copy()`, `viewer_edit_cut()`, `viewer_edit_paste()`
- `viewer_edit_select_all()` for Cmd+A
- Normalized selection in draw code so backwards selections render correctly

### Keybindings
- `⌘C` - Copy selection to clipboard
- `⌘X` - Cut selection to clipboard
- `⌘V` - Paste clipboard (replaces selection if any)
- `⌘A` - Select all
- `Shift+Arrow` - Extend selection
- Typing/Backspace/Delete with selection replaces it

### Files Changed
- `src/app/state/app.rs` - Added `clipboard` field
- `src/app/state/viewer_edit.rs` - All selection and clipboard methods
- `src/tui/input_viewer.rs` - Keybindings for clipboard operations
- `src/tui/draw_viewer.rs` - Normalize selection for rendering

---

## 2026-02-02: Fix .azureal Directory Eager Creation

### Problem
`.azureal/` directories were being created in every git repository azureal was run from, even when not needed. This was unintended behavior.

### Root Cause
- `ensure_config_dir()` was called on startup and created `.azureal/` in the current git root
- No separation between global config and project-specific data

### Solution
Separated global config from project-specific data with lazy directory creation:
- `config_dir()` → `~/.azureal/` (global config, home directory)
- `project_data_dir()` → `.azureal/` (project data in git root, only when needed)
- `ensure_project_data_dir()` → Creates `.azureal/` only when actually writing data

### Files Changed
- `src/config.rs` - Added `project_data_dir()` and `ensure_project_data_dir()`
- `src/app/state/ui.rs` - Use `ensure_project_data_dir()` for run_commands.json
- `src/app/state/load.rs` - Use `ensure_project_data_dir()` for debug output

---

## 2026-02-02: Show Hidden Files in FileTree

### Feature
Hidden files and directories (starting with `.`) are now shown in the FileTree pane with dimmed colors, sorted after non-hidden items. Children of hidden directories also inherit the dimmed styling.

### Implementation
- Added `is_hidden: bool` field to `FileTreeEntry` struct
- Modified `build_file_tree_recursive()` to include hidden files and mark them
- Added `parent_hidden` parameter to propagate hidden state to children
- Sort order: dirs first, then within each (dirs/files): non-hidden before hidden, then alphabetical
- Dimmed colors: gray (`rgb(100,100,100)`) for hidden files, muted cyan (`rgb(80,120,130)`) for hidden dirs
- Icons also dimmed for hidden items
- Still excludes `target/` and `node_modules/` (too noisy to include)

### Files Changed
- `src/app/types.rs` - Added `is_hidden` field to `FileTreeEntry`
- `src/app/state/helpers.rs` - Include hidden files in tree, propagate hidden state to children
- `src/tui/draw_file_tree.rs` - Dimmed styling for hidden items

---

## 2026-02-02: Centralized Keybindings Module

### Feature
Created a centralized `keybindings.rs` module where all keybindings are defined once and used by both input handlers and the help dialog automatically.

### Architecture
- `KeyCombo` struct with display methods for platform symbols (⌘, ⌃, ⌥, ⇧)
- `Action` enum for all possible keybinding actions (~45 actions)
- `Keybinding` struct with primary + alternatives (e.g., j/↓ for same action)
- Static binding arrays: GLOBAL, WORKTREES, FILE_TREE, VIEWER, EDIT_MODE, OUTPUT, INPUT, TERMINAL
- `lookup_action(focus, modifiers, code, ...)` for input handler dispatch
- `help_sections()` auto-generates help dialog content from same definitions

### Benefits
- Adding/changing a keybinding now automatically updates help
- No more duplicate definitions across files
- Single source of truth for all key mappings

### Files Changed
- `src/tui/keybindings.rs` - NEW: centralized keybinding definitions (~420 lines)
- `src/tui.rs` - Added `pub mod keybindings;`
- `src/tui/draw_dialogs.rs` - Help dialog now uses `keybindings::help_sections()`
- `src/tui/input_file_tree.rs` - Migrated to use `lookup_action()`
- `src/tui/input_output.rs` - Migrated to use `lookup_action()`
- `src/tui/input_worktrees.rs` - Migrated to use `lookup_action()`
- `src/tui/input_viewer.rs` - Migrated to use `lookup_action()`
- `src/tui/input_dialogs.rs` - Migrated to use `is_nav_down()/is_nav_up()` helpers

---

## 2026-02-01: Inline Edit Diff for Last 20 Edits

### Feature
Re-added inline Edit diff display in Convo pane, but only for the last 20 Edit tool calls to avoid clutter. All Edit file paths remain clickable with underline styling.

### Implementation
- Count total Edit tool calls before rendering loop
- Track `edit_index` during rendering
- Show inline diff only when `edit_index >= total_edit_count - 20`
- Passed `show_inline_diff: bool` to `render_tool_call`
- Conditionally call `render_edit_diff()` only for last 20 Edits

### Files Changed
- `src/tui/render_events.rs` - Added edit counting, conditional inline diff rendering

---

## 2026-01-29: Bubble Navigation in Convo Pane

### Feature
Added n/p and N/P keybindings to jump between message bubbles in the Convo pane.

### Implementation
- `n/p` jumps to next/previous user prompt bubbles only
- `N/P` (Shift) includes assistant response bubbles too
- `render_display_events()` now returns bubble positions as `Vec<(usize, bool)>` where bool is `is_user`
- Added `message_bubble_positions` field to App state
- Added `jump_to_next_bubble()` and `jump_to_prev_bubble()` methods in scroll.rs

### Files Changed
- `src/app/state/app.rs` - Added `message_bubble_positions` field
- `src/app/state/scroll.rs` - Added bubble navigation methods
- `src/tui/render_events.rs` - Track bubble positions during rendering
- `src/tui/input_output.rs` - Added n/p/N/P keybindings
- `src/tui/draw_output.rs` - Store bubble positions in app state
- `src/tui/draw_dialogs.rs` - Updated help panel

---

## 2026-01-29: Plan Mode Display

### Feature
Display plan content from `~/.claude/plans/{slug}.md` when EnterPlanMode tool is called.

### Implementation
1. Added `DisplayEvent::Plan { name, content }` variant
2. Session parser extracts session slug from JSONL events
3. When EnterPlanMode tool call detected, loads matching plan file
4. Plan rendered with prominent full-width magenta border and header

### Rendering
- Full-width box with double-line border (╔═══╗)
- Magenta theme to stand out from other content
- Header: "📋 PLAN MODE: {name}"
- Content wrapped to fit width

### Files Changed
- `src/events/display.rs` - Added Plan variant
- `src/app/session_parser.rs` - Plan detection and loading
- `src/tui/render_events.rs` - Plan rendering

---

## 2026-01-29: Debug Dump Now Debug-Build Only

### Problem
Selecting a large session caused lag that persisted across all app interactions (file tree navigation, etc.).

### Root Cause
`dump_debug_output()` was called unconditionally on every session selection. This function:
1. Re-renders the ENTIRE session through `render_display_events()` (O(n) events)
2. Writes all rendered lines to disk (blocking I/O)

For a 15k event session, this blocked the main thread for seconds.

### Solution
Wrapped both the call and function with `#[cfg(debug_assertions)]` so it only runs in debug builds.

### Files Changed
- `src/app/state/load.rs` - Added `#[cfg(debug_assertions)]` to call site and function definition

---

## 2026-01-29: Animation Decoupled from Content Cache (CPU Fix)

### Problem
Loading a 15k line conversation caused 80% CPU on idle. Root cause: animation tick (every 250ms) was invalidating the entire content cache, forcing a full re-render of all 15k display events just to update one pulsating `◐` indicator.

### Solution
Decoupled animation from content caching:
1. Content rendering only happens when content actually changes (not animation tick)
2. Store `animation_line_indices: Vec<(usize, usize)>` - positions of pending indicators
3. Patch animation colors in viewport slice only (O(30 visible lines) not O(15k total))

### Files Changed
- `src/app/state/app.rs` - Removed `rendered_lines_tick`, added `animation_line_indices`
- `src/tui/render_events.rs` - Returns `(lines, animation_indices)` tuple
- `src/tui/draw_output.rs` - Patches animation colors in viewport slice
- `src/app/state/load.rs` - Updated debug dump call site

### Performance Impact
- Before: ~80% CPU idle (re-rendering 15k events × 4fps)
- After: Near-zero CPU idle (patching ~5 spans × 4fps)

---

## 2026-01-28: Fully Stateless Architecture

### Summary
Eliminated ALL persistent storage. Azureal now derives all state at runtime from git repository state and Claude's session files.

### Key Insight
All essential data was already stored elsewhere:
- **Project info** → `git rev-parse --show-toplevel` + main branch detection
- **Active sessions** → `git worktree list` (worktrees with `azureal/` branches)
- **Archived sessions** → `git branch | grep azureal/` (branches without worktrees)
- **Claude session ID** → discovered from Claude's `~/.claude/projects/` directory
- **Conversation history** → Claude's JSONL session files

### Files Deleted
- `src/store.rs` - JSON storage (no longer needed)
- `src/session.rs` - Session management layer (merged into app/mod.rs)

### Major Changes

1. **`src/app/mod.rs`** - Added stateless discovery
   - `load()` discovers project via git
   - `load_sessions()` discovers from worktrees + branches
   - `discover_claude_session_id()` scans Claude's project dir
   - Removed `store` field entirely

2. **`src/models.rs`** - Simplified Session
   - `Session { branch_name, worktree_path: Option<PathBuf>, claude_session_id, archived }`
   - `name()` method strips `azureal/` prefix
   - `status()` method derives status from runtime state

3. **`src/cmd/session.rs` and `src/cmd/project.rs`** - Stateless CLI
   - `discover_project()` and `discover_sessions()` functions
   - No store dependency

4. **`src/tui/` handlers** - Updated for new Session API
   - `session.worktree_path` now `Option<PathBuf>` (None if archived)
   - All handlers properly check for None before git operations

### Why
User asked "why do we even need the JSON?" - the answer was we don't. Everything can be derived. This eliminates all persistent state, making azureal truly stateless.

---

## 2026-01-28: Remove hooks.jsonl

### Summary
Removed hooks.jsonl logging since hooks are already embedded in Claude's session files via `system-reminder` tags. Fully stateless now - azureal writes NO files.

### Removed
- `log_hook_event()` in `src/app/util.rs`
- `handle_hooks()` in `src/cmd/mod.rs`
- `poll_hooks_file()` in `src/app/mod.rs`
- `load_hooks_with_timestamps()` in `src/app/mod.rs`
- `hooks_file_pos` field in App struct
- `Hooks` CLI command in `src/cli/mod.rs`
- Hook merging code in `load_session_output()`

### Why
Hooks are already extracted from Claude's JSONL session files via `extract_hooks_from_content()`. The hooks.jsonl was redundant storage that violated the stateless architecture.

---

## 2026-01-28: Upgrade to Claude Code 2.1.22

### Summary
Upgraded from 2.1.18 to 2.1.22. The "tool_use ids must be unique" bug has been fixed upstream.

### Verification
Tested the exact pattern that previously failed:
1. Simple prompt → create session ✅
2. Resume with single tool (Read) ✅
3. Resume with multiple tools (Glob + Read) ✅

All resume + tools combinations now work. No longer need to pin to 2.1.18.

---

## 2026-01-28: Improved Task/Subagent Display

### Problem
When Claude uses Task tool (subagents like Explore, Plan), only hooks showed in azureal's output pane. The actual subagent work was invisible because:
1. Task tool results were truncated to 1 line
2. Subagent type wasn't shown

### Fix
1. **Task tool results** now show up to 20 lines of subagent output instead of just 1
2. **Task tool calls** now show `[subagent_type] description` (e.g., `[Explore] Search codebase`)
3. **EnterPlanMode** shows `🔍 Planning...`
4. **ExitPlanMode** shows `📋 Plan complete`

### Files Changed
- `src/tui/util.rs` - `render_tool_result()` for Task, `extract_tool_param()` for Task/EnterPlanMode/ExitPlanMode

---

## 2026-01-26: Multi-Session Claude + Clean Output Display

### Summary
Implemented multi-agent concurrent processing and clean output display for Claude stream-json format.

### Changes

1. **Multi-session Claude support** (`src/app.rs`)
   - Changed `claude_receiver: Option<Receiver>` to `claude_receivers: HashMap<String, Receiver<ClaudeEvent>>`
   - Changed `running_session_id: Option<String>` to `running_sessions: HashSet<String>`
   - Each session can now have its own Claude process running concurrently

2. **Conversation persistence** (`src/claude.rs`, `src/app.rs`)
   - Added `claude_session_ids: HashMap<String, String>` to track Claude session IDs
   - Added `--resume <session-id>` flag support for conversation continuity
   - Added `SessionId(String)` event variant to capture init event session_id

3. **Stream-JSON output mode** (`src/claude.rs`)
   - Added `--verbose --output-format stream-json` flags
   - `--verbose` is required when using stream-json with `-p` mode

4. **Clean output display** (`src/app.rs`)
   - Added `parse_stream_json_for_display()` function
   - User prompts: "You: <message>"
   - Claude responses: "Claude: <text>"
   - Tool usage: "[Using <name>...]"
   - Completion: "[Done: Xs, $X.XXXX]"

5. **Prompt echo consistency** (`src/tui.rs`)
   - Changed prompt echo from "> " to "You: " for consistency with parsed output

### Why
User requested multi-agent processing (the core purpose of the app) and clean readable output instead of raw JSON.

---

## 2026-01-26: PTY-Based Interactive Sessions

### Summary
Replaced one-shot `-p` mode with PTY-based interactive sessions to properly distinguish SessionStart vs UserSubmitPrompt.

### Problem
Previous implementation spawned a new Claude process for each prompt using `-p` flag (one-shot mode). This conflated SessionStart and UserSubmitPrompt - every prompt was effectively a SessionStart. Used `--resume` for conversation context but process died after each response.

### Solution
Implemented PTY (pseudo-terminal) based architecture:

1. **start_session()** - First prompt spawns Claude interactively via PTY
   - No `-p` flag - Claude enters interactive mode
   - Process stays alive waiting for input
   - Initial prompt sent via PTY stdin

2. **send_prompt()** - Follow-up prompts write to existing PTY
   - Proper UserSubmitPrompt behavior
   - Claude receives input, processes, waits for more

3. **is_session_running()** - Checks if session has active PTY

4. **stop_session()** - Closes PTY, terminates Claude

### Changes

1. **src/claude.rs** - Complete rewrite
   - Added `ActiveSession` struct with PTY writer
   - Added `sessions: Arc<Mutex<HashMap<String, ActiveSession>>>`
   - `start_session()` spawns via `portable_pty`
   - `send_prompt()` writes to PTY stdin
   - Removed old `spawn()` method (kept `spawn_oneshot()` for legacy)

2. **src/tui.rs** - Updated input handler
   - Checks `claude_process.is_session_running()` first
   - If running: call `send_prompt()` (UserSubmitPrompt)
   - If not running: call `start_session()` (SessionStart)

### Why
Claude Code CLI has distinct SessionStart and UserSubmitPrompt events. Proper interactive mode via stream-json I/O enables true conversation flow where Claude stays alive and receives follow-up prompts without spawning new processes.

---

## 2026-01-26: Stream-JSON I/O (Corrected from PTY)

### Summary
Replaced PTY approach with proper stream-json input/output mode after verifying Claude Code's documented behavior.

### Investigation
User asked "how do u know youre doing it the way claude code does it?" - validated by:
1. `claude --help` shows `--output-format` only works with `--print`
2. `--input-format stream-json` allows "realtime streaming input" in `-p` mode
3. Third-party documentation confirmed NDJSON format for input

### Changes

1. **src/claude.rs** - Changed from PTY to stream-json I/O
   - Removed `portable-pty` usage
   - Uses `claude -p "" --input-format stream-json --output-format stream-json`
   - Stores `ChildStdin` handle instead of PTY writer
   - `send_prompt()` writes NDJSON: `{"type":"user","message":{"role":"user","content":"..."}}`
   - Requires `--verbose` flag with stream-json
   - Uses `-p "init"` as minimal initial prompt (empty string fails)

### Why
Claude Code's `--output-format stream-json` (which we need for structured output) only works with `-p` mode, not interactive mode. The `--input-format stream-json` flag was designed specifically for this use case - sending multiple prompts to a single `-p` process.

---

## 2026-01-26: Remove --fork-session (Fix for Tool Concurrency)

### Summary
Removed `--fork-session` flag which was causing tool_use ID collisions and loss of conversation context.

### Problem
Using `--fork-session` on every resume was creating a NEW session each time, causing:
1. Loss of conversation context (forking starts fresh)
2. Potential tool_use ID collisions when parallel tools ran

### Investigation
Explored Claude Code source (`~/claude-code-main`) and Crystal wrapper (`~/crystal`):
- `--fork-session` should only be used when actually wanting to fork
- `--session-id` requires valid UUID format (can't use arbitrary strings)
- Crystal uses simple `--resume <captured_id>` pattern without forking

### Failed Attempt: Deterministic `--session-id`
Tried using `--session-id azureal-{session_id}` but Claude Code requires valid UUID format.
Error: "Invalid session ID. Must be a valid UUID."

### Final Solution
Reverted to original approach but WITHOUT `--fork-session`:
- First prompt: no --resume, Claude generates session ID
- Capture session ID from init event in stream-json output
- Follow-up prompts: `--resume <captured_id>` (NO --fork-session)

### Changes

1. **src/claude.rs** - Remove --fork-session, restore session ID capture
   - Signature: `spawn(working_dir, prompt, resume_session_id: Option<&str>)`
   - Only `--resume` on follow-ups, no `--fork-session`
   - Re-enabled session ID parsing from init event

2. **src/app.rs** - Back to HashMap
   - `claude_session_ids: HashMap<String, String>` (azureal session → Claude session)
   - `set_claude_session_id()` / `get_claude_session_id()` methods

3. **src/tui.rs** - Updated to use original pattern

### Why
The original session capture approach worked fine. The ONLY issue was `--fork-session` which was:
1. Creating new sessions on every resume (losing context)
2. Potentially causing tool_use ID collisions
Simply removing `--fork-session` fixes both issues.

---

## 2026-01-26: Claude Code Bug Confirmed (Upstream Issue)

### Summary
After extensive investigation, confirmed that "tool_use ids must be unique" error is a **known Claude Code bug**, not an azureal issue.

### Investigation
1. Removed `--fork-session` - didn't fix
2. Tried `--continue` instead of `--resume` - didn't fix
3. Tried PTY spawning (matching Crystal) - didn't fix
4. Compared with Crystal codebase extensively - same pattern, no visible difference
5. Found existing GitHub issues: #20508, #20527, #13124

### Root Cause
When Claude makes parallel tool calls during a `-p --resume` turn, duplicate `tool_use` IDs are generated during API request construction. Not in the stored session file.

### Pattern
| First Prompt | Resume Prompt | Result |
|--------------|---------------|--------|
| No tools | No tools | ✅ Works |
| No tools | With tools | ❌ FAILS |
| With tools | No tools | ✅ Works |
| With tools | With tools | ❌ FAILS |

### Status
Bug report created at `.project/claude-code-bug-report.md`. Awaiting upstream fix.

---

## 2026-01-26: Rollback to 2.1.17 Fixes Bug

### Discovery
GitHub issue #20508 reported 2.1.18 was last working version. Rolled back to test.

### Solution
```bash
ln -sf ~/.local/share/claude/versions/2.1.17 ~/.local/bin/claude
```

### Verification
- 2.1.17: "hello" → resume with "read README.md" → ✅ SUCCESS
- Bug introduced in 2.1.19

### Note
No built-in way to disable auto-updates. If Claude auto-updates, manually re-symlink to 2.1.17.

---

## 2026-01-28: TUI Module Modularization

### Summary
Split `src/tui/util.rs` (1236 lines) into focused modules for better organization and maintainability.

### New Module Structure

```
src/tui/
├── colorize.rs (183 lines)    - Output colorization, strip_ansi, MessageType
├── markdown.rs (120 lines)    - Markdown parsing (bold, italic, code, tables)
├── render_events.rs (428 lines) - DisplayEvent → Lines rendering
├── render_tools.rs (307 lines)  - Tool parameter extraction, result rendering
└── util.rs (49 lines)          - Small utilities, re-exports
```

### Files Changed

1. **Created `src/tui/colorize.rs`**
   - `ORANGE` color constant
   - `strip_ansi()` - remove ANSI escape codes
   - `MessageType` enum
   - `detect_message_type()` - identify user/assistant lines
   - `colorize_output()` - legacy fallback colorization

2. **Created `src/tui/markdown.rs`**
   - `parse_markdown_spans()` - inline bold/italic/code
   - `parse_table_row()` - table rendering with box drawing
   - `is_table_separator()` - detect markdown table separators

3. **Created `src/tui/render_tools.rs`**
   - `extract_tool_param()` - get primary param for display
   - `truncate_line()` - truncate with ellipsis
   - `render_tool_result()` - tool-specific result formatting

4. **Created `src/tui/render_events.rs`**
   - `render_display_events()` - main event rendering function
   - `render_edit_diff()` - inline diff display for Edit tool
   - `render_write_preview()` - Write tool preview

5. **Simplified `src/tui/util.rs`**
   - Reduced from 1236 lines to 49 lines
   - Keeps only: `truncate()`, `is_scrolled_to_bottom()`, `calculate_cursor_position()`
   - Re-exports commonly used items from submodules

6. **Updated `src/tui/mod.rs`**
   - Added module declarations for new files
   - Updated documentation header

### Also Fixed
- Added `FileTree` and `Viewer` to Focus enum match arms in:
  - `app/mod.rs` (focus_next, focus_prev)
  - `tui/draw_status.rs` (help text)
  - `tui/event_loop.rs` (input handling)

### Why
User requested modularization of files over 500 lines. The 1236-line util.rs was the priority since it contained multiple distinct responsibilities.

---

## 2026-01-28: 4-Pane TUI Layout Restructure

### Summary
Restructured TUI from 3-pane to 4-pane layout, adding FileTree and Viewer panels for file browsing and content viewing.

### New Layout
```
┌──────────┬──────────┬─────────────────┬─────────────────┐
│ Sessions │ FileTree │     Viewer      │     Output      │
│   (40)   │   (40)   │  (50% remain)   │  (50% remain)   │
├──────────┴──────────┴─────────────────┴─────────────────┤
│                    Input / Terminal                      │
└─────────────────────────────────────────────────────────┘
```

### New Files Created
1. **`src/tui/draw_file_tree.rs`** - FileTree panel rendering
   - Shows directory tree for selected session's worktree
   - Directory expand/collapse with ▼/▶ indicators
   - File icons based on extension (🦀 for .rs, ⚙ for .toml, etc.)
   - Selection highlighting with blue background

2. **`src/tui/draw_viewer.rs`** - Viewer panel rendering
   - Empty state: "Select file or diff"
   - File mode: Line numbers + file content
   - Diff mode: Color-coded diff lines (prepared for future)

3. **`src/tui/input_file_tree.rs`** - FileTree input handling
   - j/k: Navigate up/down
   - Enter: Open file in Viewer / Expand directory
   - h/l: Collapse/Expand directory
   - Space: Toggle directory expand

4. **`src/tui/input_viewer.rs`** - Viewer input handling
   - j/k: Scroll content
   - Ctrl+d/u: Half-page scroll
   - Ctrl+f/b: Full-page scroll
   - g/G: Jump to top/bottom
   - Esc: Clear viewer, return to FileTree

### Files Modified
1. **`src/app/types.rs`** - Added types
   - `ViewerMode` enum (Empty, File, Diff)
   - `FileTreeEntry` struct (path, name, is_dir, depth)

2. **`src/app/state.rs`** - Added state and methods
   - New fields: file_tree_entries, file_tree_selected, file_tree_scroll, file_tree_expanded, viewer_content, viewer_path, viewer_scroll, viewer_mode
   - Methods: load_file_tree(), toggle_file_tree_dir(), file_tree_next/prev(), load_file_into_viewer(), scroll_viewer_down/up(), clear_viewer()
   - Updated focus_next/prev for 4-pane cycle
   - Added build_file_tree() helper

3. **`src/tui/run.rs`** - Updated layout
   - Changed from 2 panes to 4 panes
   - Widths: Sessions(40), FileTree(40), Viewer(50%), Output(50%)

4. **`src/tui/event_loop.rs`** - Updated input routing
   - Added handlers for FileTree and Viewer focus states
   - Updated apply_scroll_cached for 4-pane mouse scroll zones

5. **`src/tui/draw_status.rs`** - Updated help text
   - FileTree: "j/k:navigate Enter:open h/l:collapse/expand Space:toggle Tab:switch"
   - Viewer: "j/k:scroll Ctrl+d/u:half-page g/G:top/bottom Esc:close Tab:switch"

6. **`src/tui.rs`** - Registered new modules

7. **`src/app.rs`** - Exported ViewerMode

### Why
User requested file viewer pane as Phase 2 feature. The 4-pane layout enables browsing session files and viewing content alongside Claude output.

---

## 2026-01-28: Message Bubble Containment + Width Constraints

### Summary
Constrained ALL content outside bubbles to `bubble_width + 10` max width. Tool commands show full content; tool results show summarized output.

### Changes
1. **Bubble containment** (`src/tui/render_events.rs`)
   - User messages wrap to `bubble_width - 4`
   - Claude text wraps to `bubble_width - 2`
   - Code blocks, headers, bullets, quotes truncate within bubble

2. **Width constraints for non-bubble content** (`src/tui/render_events.rs`)
   - Tool command lines: param_display constrained to `bubble_width + 10`
   - Hooks: output line constrained to `bubble_width + 10`
   - Edit diffs: each line constrained to `bubble_width + 10`
   - Write previews: purpose line constrained to `bubble_width + 10`

3. **Tool commands** (`src/tui/render_tools.rs`)
   - `extract_tool_param()` returns FULL command (no truncation with "...")
   - Bash commands, file paths, patterns all shown fully
   - Truncation happens at display time via `truncate_line()` which cuts without "..."

4. **Tool results** (`src/tui/render_tools.rs`)
   - Restored summarized output per tool type:
     - Read: first + last line with line count
     - Bash: last 2 non-empty lines
     - Grep: first 3 matches + overflow count
     - Glob: file count
     - Task: first 5 lines + overflow count
   - All constrained to max_width parameter

### Why
User clarified: full COMMANDS on tool lines (no "..." truncation), but results can be summarized. All content must stay within `bubble_width + 10` to prevent extending to pane edge.

---

## 2026-01-28: FileTree/Viewer Fixes + Syntax Highlighting

### Summary
Fixed FileTree cursor reset on expand/collapse, improved pane display, and added syntax highlighting to the Viewer.

### Issues Fixed

1. **FileTree cursor reset** - Cursor jumped to top after every expand/collapse
   - Root cause: `toggle_file_tree_dir()` called `load_file_tree()` which reset selection to `Some(0)`
   - Fix: Remember selected path before rebuild, restore selection to same path after rebuild

2. **Pane display issues** - Lines not showing properly
   - Root cause: `Wrap { trim: false }` caused lines to wrap and mess up scroll calculations
   - Fix: Removed `Wrap`, added explicit line truncation with `truncate_str()` and `truncate_line_spans()`

3. **Added syntax highlighting** - Files now display with proper code highlighting
   - Added `SyntaxHighlighter` struct to `src/syntax.rs` using syntect
   - Base16-ocean.dark theme with 150+ language support
   - Auto-detects language from file extension

### Files Modified

1. **`src/syntax.rs`** - Added `SyntaxHighlighter`
   - `highlight_file(content, filename)` returns Vec of styled spans per line
   - Reuses syntect SyntaxSet and Theme from DiffHighlighter

2. **`src/app/state.rs`**
   - Added `syntax_highlighter: SyntaxHighlighter` field
   - Updated `toggle_file_tree_dir()` to preserve selection path

3. **`src/tui/draw_viewer.rs`**
   - Changed `app: &App` to `app: &mut App` for scroll state updates
   - Removed `Wrap { trim: false }` that caused display issues
   - Added syntax highlighting for File mode using `app.syntax_highlighter`
   - Added `truncate_str()` and `truncate_line_spans()` helpers
   - Updates `app.viewer_scroll` during render to clamp to valid range

4. **`src/tui/draw_file_tree.rs`**
   - Changed `app: &App` to `app: &mut App` for scroll state updates
   - Added auto-scroll logic to keep selection visible in viewport
   - Updates `app.file_tree_scroll` during render

---

## 2026-02-01: Clickable Edit File Paths

### Summary
Made Edit tool file paths clickable hyperlinks in the Convo pane. Click to open the full file in Viewer with the edit region highlighted using conflict markers.

### Changes

1. **Removed inline diff preview** - Edit tool calls no longer show the full diff inline in the convo. Instead, file paths are underlined and clickable.

2. **Clickable links tracking** - New `clickable_paths` field in App state tracks positions of clickable file paths:
   - `Vec<(line_idx, start_col, end_col, file_path, old_string, new_string)>`

3. **Underlined file paths** - Edit tool file paths rendered with underline style to indicate clickability

4. **Click handling** - When clicking on a file path, opens the file in Viewer with conflict markers showing the edit:
   ```
   <<<<<<< OLD (removed)
   original content here
   =======
   new content here
   >>>>>>> NEW (added)
   ```

5. **Keyboard navigation preserved** - `e`/`E` keys still cycle through Edit diffs (shows just the diff, not full file)

### Files Changed
- `src/app/state/app.rs` - Added `clickable_paths` field
- `src/app/state/ui.rs` - Added `load_file_with_edit_diff()` method
- `src/tui/render_events.rs` - Track clickable positions, add underline style, skip inline diff
- `src/tui/draw_output.rs` - Store clickable_paths from render_display_events
- `src/tui/event_loop.rs` - Handle clicks on file paths
- `src/tui/draw_dialogs.rs` - Updated help panel with e/E keybindings

### Why
User requested clickable Edit file paths instead of inline diffs to save space in the conversation view. The full file with edit context is more useful than just the diff preview.

---

## 2026-02-01: Edit Diff Viewer with Syntax Highlighting

### Summary
Fixed the clickable Edit file paths to open files with proper syntax highlighting and diff display. Now shows:
- Full file with syntax highlighting (same as opening from file tree)
- Deleted lines (old_string) displayed above the edit with red background and "-" line number
- Added lines (new_string) highlighted with green background
- Auto-scroll to the edit position with 3 lines of context above

### Changes

1. **ViewerMode::File with diff overlay** - Instead of using ViewerMode::Diff (plain text), now uses ViewerMode::File with a separate `viewer_edit_diff` overlay

2. **New App state fields:**
   - `viewer_edit_diff: Option<(String, String)>` - stores (old_string, new_string) for overlay
   - `viewer_edit_diff_line: Option<usize>` - line number where edit starts (for scrolling)

3. **Diff rendering in draw_viewer.rs:**
   - Inserts deleted (old) lines above the edit position with red background (`Color::Rgb(60, 20, 20)`)
   - Highlights added (new) lines with green background (`Color::Rgb(20, 60, 20)`)
   - Shows "-" as line number for deleted lines, normal line numbers for added lines

4. **Auto-scroll to edit position** - Viewer scrolls to show the edit with 3 lines of context above

5. **Clear diff overlay** - When loading files from file tree or clearing viewer, the diff overlay is cleared

### Files Changed
- `src/app/state/app.rs` - Added `viewer_edit_diff` and `viewer_edit_diff_line` fields
- `src/app/state/ui.rs` - Updated `load_file_with_edit_diff()` to use File mode with overlay
- `src/app/state/file_browser.rs` - Clear diff overlay when loading from file tree
- `src/tui/draw_viewer.rs` - Added diff overlay rendering in File mode

---

## 2026-02-02: Improved Edit Diff Line Finding

### Problem
Edit diff views didn't always jump to the edited lines when opened. The search logic using `content.find(new_string)` was too simplistic and often failed to locate edits.

### Solution
Implemented multi-strategy search in `find_edit_line()`:
1. **Full new_string match** - Most accurate when edit is applied
2. **Full old_string match** - Works for edit history before application
3. **Significant lines from new_string** - Skips trivial lines (`{`, `}`, whitespace), searches lines with >3 chars
4. **Significant lines from old_string** - Same for old content
5. **Identifier search** - Finds function/variable names (≥6 chars), sorted by length for uniqueness

Each strategy is tried in order until a match is found, with fallback to line 0.

### Files Changed
- `src/app/state/ui.rs` - Extracted `find_edit_line()` helper with 6-strategy search algorithm

---

## 2026-02-02: Prompt History Navigation

### Feature
Added Up/Down arrow keys in inprompt mode to scroll through previous prompts from the conversation.

### Implementation
- Pulls last 50 user messages from `display_events` (conversation history)
- `prompt_history_idx: Option<usize>` - current position when browsing (None = new input)
- `prompt_history_temp: Option<String>` - saves current input when starting to browse
- `get_conversation_history()` - extracts UserMessage content from display_events
- `prompt_history_prev()` - Up arrow, navigate to older prompts
- `prompt_history_next()` - Down arrow, navigate to newer prompts or back to current input

### Behavior
- Up arrow: loads previous prompt from conversation (most recent first)
- Down arrow while browsing: moves to newer entries
- Down arrow at newest: restores original input that was being typed
- History reflects actual conversation, persists across session switches

### Files Changed
- `src/app/state/app.rs` - Added history navigation state fields
- `src/app/input.rs` - Added history methods using display_events
- `src/tui/input_terminal.rs` - Added Up/Down keybindings in Claude prompt mode

---

## 2026-02-02: Rename inprompt to prompt mode + global 'p' key

### Changes
1. Renamed `insert_mode` to `prompt_mode` throughout codebase
2. Renamed "INPROMPT" display text to "PROMPT"
3. Changed 'i' key to 'p' for entering prompt mode
4. Made 'p' key work globally from any state except viewer edit mode

### Behavior
- `p` key now enters prompt mode from anywhere (Worktrees, FileTree, Viewer, Convo, Input command mode)
- Does not work when in viewer edit mode (to allow typing 'p')
- Status bar help text updated to show `p:prompt` instead of `i:inprompt`

### Files Changed
- `src/app/state/app.rs` - Renamed `insert_mode` to `prompt_mode`
- `src/app/terminal.rs` - Renamed `insert_mode` to `prompt_mode`
- `src/tui/event_loop.rs` - Changed 'i' to 'p', added `!app.viewer_edit_mode` guard
- `src/tui/input_terminal.rs` - Changed 'i' to 'p' in command mode handlers
- `src/tui/input_worktrees.rs` - Removed 'i' handler (now handled globally)
- `src/tui/draw_input.rs` - Updated text "INPROMPT" → "PROMPT", "i:inprompt" → "p:prompt"
- `src/tui/draw_terminal.rs` - Renamed `insert_mode` to `prompt_mode`
- `src/tui/draw_status.rs` - Updated help text `i:inprompt` → `p:prompt`

---

## 2026-02-02: Convo Pane Message Counter

### Change
Changed the Convo pane title from showing line count `[line/total_lines]` to message count `[msg/total_msgs]`.

### Implementation
- Uses `message_bubble_positions` (already tracked) to count total messages
- Finds current message by looking for the last bubble position at or before `scroll + 3` lines
- Shows `[current_msg/total_msgs]` format (e.g., `[5/12]`)

### Files Changed
- `src/tui/draw_output.rs` - Updated title generation to use message count instead of line count

---

## 2026-02-02: Fix global p and ? keybindings

### Problem
`p` (prompt) and `?` (help) weren't triggering from all panes as expected.

### Root Causes
1. `?` key generates `KeyCode::Char('?')` with SHIFT modifier on US keyboards, but handler only checked `KeyModifiers::NONE`
2. `input_worktrees.rs` had `?` mapped to open context menu, conflicting with global help
3. Missing `!app.viewer_edit_mode` guard on `?` handler

### Fixes
1. Changed `?` handler to accept `KeyModifiers::NONE | KeyModifiers::SHIFT`
2. Removed `?` from worktrees context menu trigger (now just Space)
3. Added `!app.viewer_edit_mode` guard to `?` handler
4. Updated status bar help text to show `p:prompt` in more panes

### Files Changed
- `src/tui/event_loop.rs` - Fixed `?` modifier check, added viewer_edit_mode guard
- `src/tui/input_worktrees.rs` - Removed `?` from context menu trigger
- `src/tui/draw_status.rs` - Added `p:prompt` to Output, FileTree, Viewer help text

---

## 2026-02-02: Fix Compacting Indicator Timing

### Problem
Compacting indicator showed "Compacting context..." AFTER compaction completed, not during. User reported: "rn it doesnt say anything while compacting, then after compacted it says 'Compacting context...'"

### Root Cause
The detection logic was backwards:
1. `Compacting` event was triggered by "This session is being continued..." message - but this is the compaction SUMMARY that appears AFTER compaction
2. Both `Compacting` and `Compacted` were emitting after compaction was done

### Solution
1. **Removed `DisplayEvent::Compacting` variant** - no longer needed
2. **Changed "is_compaction_summary" to emit `Compacted`** - the summary message now correctly signals compaction is DONE
3. **Updated `render_command()` to show "⏳ Compacting context..." for `/compact` command** - the START of manual compaction

### Flow Now
- **Manual `/compact`**: Command event renders "⏳ Compacting context..." (START), then Compacted event renders "✓ Context compacted" (END)
- **Auto-compact**: Only shows "✓ Context compacted" when summary appears (no way to detect auto-compact start)

### Files Changed
- `src/events/display.rs` - Removed `Compacting` variant
- `src/app/session_parser.rs` - Parse `compact_boundary` system event for clean compaction detection; removed duplicate detection from user messages
- `src/tui/render_events.rs` - Removed `Compacting` handler; updated `render_command()` to show compacting indicator for "compact" command

### Session File Analysis
Discovered `system` events with `subtype: "compact_boundary"` which cleanly signal compaction completion:
```json
{"type":"system","subtype":"compact_boundary","content":"Conversation compacted","compactMetadata":{"trigger":"auto","preTokens":168173}}
```
This is more reliable than parsing user message text patterns.
