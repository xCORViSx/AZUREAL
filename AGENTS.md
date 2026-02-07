# SUMMARY

Azureal (Agent-Zoned Unified Runtime Environment for Autonomous LLMs) is a Rust TUI application that wraps Claude Code CLI to enable multi-agent development workflows. Each **worktree** is a git worktree with its own Claude **session**, allowing concurrent AI-assisted development across multiple feature branches.

**Terminology:**
- **Worktree**: A git worktree with its own working directory and branch (displayed in left panel)
- **Session**: A Claude Code conversation (stored in `~/.claude/projects/`, displayed in Convo pane)

**Mostly Stateless Architecture:** All runtime state is derived from:
- Git repository info via `git rev-parse --show-toplevel`
- Git worktrees via `git worktree list` for active worktrees
- Git branches via `git branch | grep azureal/` for archived worktrees
- Claude's session files in `~/.claude/projects/` for conversation history and `--resume` IDs

**Optional State:** `.azureal/sessions.toml` stores custom session name → UUID mappings (only created when user provides custom names)

# FEATURES

### Multi-Worktree Claude Management

The core feature enabling multiple concurrent Claude Code CLI instances. Each worktree has its own:
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

Each worktree provides true branch isolation:
- Has its own working directory
- Can have different uncommitted changes
- Operates on a separate branch from main

Implementation: `src/git.rs` handles worktree creation, deletion, and status queries.

### TUI Interface

A ratatui-based terminal interface with 4-pane layout:

```
┌───────────┬──────────┬─────────────┬─────────────────┐
│ Worktrees │ FileTree │   Viewer    │                 │
│   (40)    │   (40)   │ (50% rem)   │      Convo      │
├───────────┴──────────┴─────────────┤  (50% rem,      │
│     Input / Terminal               │   full height)  │
├────────────────────────────────────┴────────────────┤
│                    Status Bar                        │
└─────────────────────────────────────────────────────┘
```

**Panes:**
- **Worktrees** (40 cols): Worktree list showing all active and archived worktrees
- **FileTree** (40 cols): Directory tree for selected worktree (supports expand/collapse)
- **Viewer** (50% remaining): File content viewer or diff detail (dual-purpose)
- **Convo** (50% remaining, full height): Claude conversation output with tool results — extends past input pane down to status bar
- **Input/Terminal**: Prompt input or embedded terminal (spans first 3 panes width only)
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

Implementation: `src/tui/event_loop.rs` for event loop, `src/tui/run.rs` for rendering, `src/tui/render_thread.rs` for background convo rendering, `src/app/state/` for state management (split into 9 focused submodules).

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

**Files:** `src/tui/draw_output.rs` uses `app.rendered_lines_cache`; render cache is updated asynchronously by the background `RenderThread` — call `app.invalidate_render_cache()` when `display_events` changes to trigger a new render request

**Diff caching:** Same pattern for diff view - `app.diff_lines_cache` stores colorized diff output. Set `app.diff_lines_dirty = true` when `diff_text` changes. `src/tui/draw_output.rs` checks dirty flag before re-highlighting.

### 3. DECOUPLE Animation from Content Cache

**Critical: Animation must NOT invalidate the content cache.** The pulsating indicator only changes color - the content (markdown, tool calls) is unchanged.

```rust
// ❌ WRONG - Animation tick invalidates entire cache, re-renders 15k events every 250ms
let animation_changed = !app.pending_tool_calls.is_empty() && app.rendered_lines_tick != app.animation_tick;
if app.rendered_lines_dirty || animation_changed {
    app.rendered_lines_cache = render_display_events(...);  // EXPENSIVE: parses ALL events
}

// ✅ CORRECT - Cache content independently, patch animation colors in viewport only
if app.rendered_lines_dirty || app.rendered_lines_width != inner_width {
    let (lines, anim_indices) = render_display_events(...);  // Only when content changes
    app.rendered_lines_cache = lines;
    app.animation_line_indices = anim_indices;  // Track which lines need animation
}

// Patch animation colors in viewport slice (O(viewport) not O(all))
let pulse_color = pulse_colors[(app.animation_tick / 2) as usize % 4];
for &(line_idx, span_idx) in &app.animation_line_indices {
    if line_idx >= scroll && line_idx < scroll + viewport_height {
        if let Some(span) = lines[line_idx - scroll].spans.get_mut(span_idx) {
            span.style = span.style.fg(pulse_color);
        }
    }
}
```

**Files:** `src/tui/draw_output.rs` patches colors in viewport; `src/tui/render_events.rs` returns `animation_line_indices`

**Animation guard:** The animation patching loop is skipped entirely when `animation_line_indices` is empty (no pending tools). This avoids pulse_color computation and viewport iteration on every scroll frame when nothing is animating.

**Throttle values in `src/tui/event_loop.rs`:**
- `min_draw_interval = 33ms` (~30fps — ALL draws throttled uniformly. `terminal.draw()` costs ~18ms, so this guarantees at least one event-only loop iteration between draws for keystroke pickup)
- `min_animation_interval = 250ms` (4fps pulsating indicators - viewport color patch only)
- `min_poll_interval = 500ms` (session file polling)
- `poll_ms = 16ms` when busy (render in-flight / Claude streaming), `100ms` when idle

### 9. NEVER Use `.wrap()` on Pre-Wrapped Content

```rust
// ❌ WRONG - ratatui re-wraps every viewport line char-by-char during render()
let para = Paragraph::new(pre_wrapped_lines).wrap(Wrap { trim: false });

// ✅ CORRECT - content is already wrapped by wrap_text()/wrap_spans(), no re-wrapping needed
let para = Paragraph::new(pre_wrapped_lines);
```

Convo pane content is pre-wrapped to `inner_width` by `wrap_text()` and `wrap_spans()` in `render_events.rs`. Adding `.wrap()` causes ratatui's `Paragraph::render()` to iterate every character of every span to compute line breaks that already exist — pure redundant O(viewport_chars) work per frame.

**Files:** `src/tui/draw_output.rs` renders Paragraph without `.wrap()`. If you add a new Paragraph that displays pre-wrapped content, do NOT add `.wrap()`.

### 8. Session File Polling (Deferred Parse + Incremental)

Three-phase polling pipeline for session files:

1. **Cheap metadata check** (`check_session_file()`): stat() the file, compare size. O(1).
2. **Incremental parse** (`refresh_session_events()`): Seek to `session_file_parse_offset`, parse only new JSONL lines appended since last read. Rebuilds tool_call context from existing DisplayEvents via `IncrementalParserState`. Falls back to full re-parse if file shrank or user-message rewrite detected (parentUuid dedup).
3. **Incremental render** (`draw_output()`): If `rendered_events_count < display_events.len()` and width unchanged, renders only newly appended events and appends to `rendered_lines_cache`. Falls back to full re-render on width change or event count decrease (session switch).

```rust
// Phase 1: O(1) metadata check (every 500ms)
pub fn check_session_file(&mut self) {
    if metadata.len() != self.session_file_size {
        self.session_file_dirty = true;
    }
}

// Phase 2: Incremental parse (only new bytes since last offset)
fn refresh_session_events(&mut self) {
    let parsed = parse_session_file_incremental(
        &path, self.session_file_parse_offset,
        &self.display_events, &self.pending_tool_calls, &self.failed_tool_calls,
    );
    // Merges new events with existing, updates parse offset
}

// Phase 3: Incremental render (only new events → append to cache)
// In draw_output():
if event_count > app.rendered_events_count && width unchanged {
    render_display_events_incremental(events, start_idx, ..., existing_cache);
}
```

**Files:**
- `src/app/session_parser.rs` - `parse_session_file_incremental()`, `IncrementalParserState`
- `src/app/state/load.rs` - `check_session_file()`, `poll_session_file()`, `refresh_session_events()`
- `src/tui/render_events.rs` - `render_display_events_incremental()`, `render_display_events_from()`
- `src/tui/draw_output.rs` - incremental render path selection

**App state for incremental tracking:**
- `session_file_parse_offset: u64` — byte offset after last successful parse
- `rendered_events_count: usize` — how many events were rendered into current cache
- `rendered_events_start: usize` — start index for deferred render (>0 means early events skipped)

**Fallback triggers (reverts to full re-parse/re-render):**
- File shrank (shouldn't happen with append-only JSONL)
- User-message rewrite detected (parentUuid dedup → events reference earlier indices)
- Terminal width changed (need to re-wrap all text)
- Session switched (event count drops to 0)

### 10. Deferred Initial Render for Large Conversations

For conversations with 200+ events, only the last 200 events are rendered on initial load. The user starts at the bottom (`output_scroll = usize::MAX`) so they see recent messages instantly. Full render happens lazily when user scrolls to the top.

```rust
// On initial full render with many events, skip early ones:
let deferred_start = if event_count > DEFERRED_RENDER_TAIL {
    event_count.saturating_sub(DEFERRED_RENDER_TAIL)
} else {
    0
};
render_display_events(&events[deferred_start..], ...);
app.rendered_events_start = deferred_start;

// When user scrolls to top and there are unrendered early events:
if app.rendered_events_start > 0 && app.output_scroll == 0 {
    // Expand to full render
    app.rendered_events_start = 0;
    app.rendered_events_count = 0;
    app.rendered_lines_dirty = true;
}
```

**Files:** `src/tui/draw_output.rs` (DEFERRED_RENDER_TAIL const, deferred render logic)

### 11. NEVER Do File I/O in Render Path

```rust
// ❌ WRONG - Reads file from disk for EVERY Edit event during render
fn render_edit_diff(...) {
    let start_line = std::fs::read_to_string(path).ok().and_then(|content| {
        content.find(new_str).map(|pos| ...)  // O(file_size) per Edit event
    });
}

// ✅ CORRECT - Use relative line numbers, no disk I/O
fn render_edit_diff(...) {
    let start_line = 1usize;  // Relative numbering, no file read
}
```

For a session with 200 Edit events, the wrong approach reads the file 200 times × O(file_size) per read. The correct approach uses relative line numbers (1-based) since the convo panel is a summary view.

**Also:** Syntax highlighting calls per Edit event reduced from 3→2 by reusing the base syntect parse and applying background colors via cheap span iteration instead of re-parsing.

**Files:** `src/tui/render_tools.rs` (`render_edit_diff()`)

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
- **Universal draw throttle (33ms / ~30fps):** `terminal.draw()` costs ~18ms (measured). ALL draws — including key-triggered ones — are throttled to 33ms minimum interval. This guarantees at least one event-only loop iteration between draws, giving crossterm a window to pick up keystrokes typed during the previous draw's 18ms blocking window. The old approach of bypassing throttle for key events caused missed keystrokes during Claude streaming.
- **Pre-draw event drain:** Right before `terminal.draw()`, drain any key events that arrived since the top-of-loop drain (~0-5ms gap). Catches keys that would otherwise be delayed by the 18ms draw.
- **Adaptive poll timeout:** 16ms when busy (render in-flight or Claude streaming via receivers), 100ms when idle. Ensures quick pickup of completed renders and key events without burning CPU when nothing is happening.
- **Viewport cache:** Convo pane caches the cloned viewport slice (`output_viewport_cache`). Only rebuilds when scroll position, content, or animation tick changes. On typing-only frames, serves from cache instead of re-cloning from the full `rendered_lines_cache`.
- **Background render thread:** Expensive convo rendering (markdown parsing, syntax highlighting, text wrapping via `render_display_events`) runs on a dedicated background thread (`RenderThread`). The event loop sends render requests via `submit_render_request()` (non-blocking channel send) and polls for results via `poll_render_result()` (non-blocking channel recv). Input is NEVER blocked by rendering — the main thread only does cheap draw operations. Sequence numbers ensure stale results are discarded (latest-wins). The render thread drains to the latest request when multiple are queued, and uses zero CPU when idle (blocks on `mpsc::recv`). `draw_output()` has a width-mismatch fallback that re-renders if the terminal width changed since the request was submitted (rare, only on resize).

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

### 7. CACHE Sidebar Items (Avoid Per-Frame Rebuild)

```rust
// ❌ WRONG - Rebuilds ALL sidebar ListItems on EVERY FRAME
fn draw_sidebar(f: &mut Frame, app: &App, area: Rect) {
    let mut items: Vec<ListItem> = Vec::new();
    for session in &app.sessions { ... }  // O(sessions) per frame
}

// ✅ CORRECT - Cache sidebar items, only rebuild when state changes
if app.sidebar_dirty || app.sidebar_focus_cached != is_focused {
    app.sidebar_cache = build_sidebar_items(app);
    app.sidebar_dirty = false;
    app.sidebar_focus_cached = is_focused;
}
let sidebar = List::new(app.sidebar_cache.clone());  // Cheap clone of cached items
```

**Files:**
- `src/tui/draw_sidebar.rs` uses `app.sidebar_cache`
- Call `app.invalidate_sidebar()` when sessions, selection, or expansion changes:
  - `src/app/state/sessions.rs` - selection, expansion, file navigation
  - `src/app/state/claude.rs` - running_sessions changes
  - `src/app/state/load.rs` - sessions list changes

### Performance Checklist for PRs

Before merging ANY change to render/event code:
- [ ] No `::new()` calls for expensive structs in render path
- [ ] No O(n) operations per frame (use caching for expensive computations)
- [ ] Animations throttled (not every frame)
- [ ] Scroll returns bool, caller checks before redraw
- [ ] Sidebar and file tree items cached (invalidated only on state change)
- [ ] Test: scroll aggressively, CPU must stay <5%

---

### Background Render Thread (Convo Pane)

The convo pane's expensive rendering pipeline (markdown parsing, syntax highlighting, text wrapping) runs on a dedicated background thread. This ensures the main event loop is never blocked by rendering, eliminating input freezing and character dropping during convo updates.

**Architecture:**
- `RenderThread` owns its own `SyntaxHighlighter` (no cross-thread sharing needed)
- Requests carry cloned data (`Vec<DisplayEvent>`, `HashSet<String>`, etc.) so threads work independently
- Communication via `mpsc` channels: `submit_tx` (main → render), `result_rx` (render → main)
- Sequence numbers (`u64`) ensure stale results are discarded — only the latest result is applied
- Render thread drains to the latest request when multiple are queued (skips intermediate states)
- Zero CPU when idle (blocks on `mpsc::recv`)

**Event loop integration:**
```rust
// Non-blocking send — never blocks the event loop
submit_render_request(&app);  // Sends cloned state to render thread

// Non-blocking poll — checks if a completed render is available
if let Some(result) = poll_render_result(&mut app) {
    if result.seq > app.render_seq_applied {
        app.rendered_lines_cache = result.lines;
        app.render_seq_applied = result.seq;
    }
}
```

**App state fields:**
- `render_thread: RenderThread` — handle to the background thread
- `render_seq_applied: u64` — sequence number of the last applied render result
- `render_in_flight: bool` — whether a render request is currently being processed

**Evolution:** Previous iterations moved rendering outside `terminal.draw()` (commit 8834050) and split render/draw into separate loop iterations (commit 5192228), but those were still synchronous. The background thread makes rendering fully asynchronous.

**Files:** `src/tui/render_thread.rs` (RenderThread struct, request/result types), `src/tui/draw_output.rs` (`submit_render_request()`, `poll_render_result()`), `src/tui/event_loop.rs` (submit/poll integration), `src/app/state/app.rs` (render_thread fields)

**Startup sequence** (`src/tui/run.rs::run`): `App::new()` → `app.load()` → `app.load_session_output()` → `event_loop::run_app()`. The `load_session_output()` call ensures the output pane shows conversation history immediately on startup.

### Vim-Style Input Mode

The input box uses vim-style modal editing:
- **Command mode** (red border): Keys are commands, not text input
- **Prompt mode** (yellow border): Keys are typed as Claude prompts

**Rationale:** Allows single-letter commands like 't' for terminal toggle without conflicting with text input. The red border in command mode provides immediate visual feedback that typing will execute commands, preventing accidental command execution.

Key mappings:
- `p` (global, except edit mode): Enter prompt mode and focus input (closes terminal/help if open)
- `t` (command mode): Open terminal pane
- `Escape` (in prompt mode): Return to command mode
- `Enter` (in prompt mode): Submit prompt

Multi-line input is supported via Shift+Enter. The Kitty keyboard protocol is enabled on startup via `PushKeyboardEnhancementFlags` (DISAMBIGUATE + REPORT_EVENT_TYPES). We intentionally omit `REPORT_ALL_KEYS_AS_ESCAPE_CODES` because it causes Shift+letter to arrive as `(SHIFT, Char('1'))` instead of `(NONE, Char('!'))`, breaking secondary character input. With DISAMBIGUATE alone, Shift+Enter sends `CSI 13;2u` → `(SHIFT, Enter)`, which is sufficient. An `(ALT, Enter)` arm is kept as a safety net for Kitty-macOS edge cases. Release/Repeat events are dropped (only Press processed). The input field dynamically grows in height (up to 3/4 of terminal height) with proper cursor positioning for newlines and word-wrapping. When content exceeds the visible area, the view scrolls to keep the cursor visible.

Implementation: `prompt_mode: bool` in `App` struct, border color logic in `draw_input()` in `src/tui/draw_input.rs`.

### Terminal Pane

A PTY-based embedded terminal that acts as a portal to the user's actual shell:
- **Cyan border**: Terminal mode active
- Full shell emulation via `portable-pty` - runs in session's worktree
- Color support via `ansi-to-tui` conversion of ANSI escape sequences
- Proper cursor positioning via `vt100` terminal state parser
- Dynamic resizing to match pane dimensions
- Resizable height (5-40 lines)

Key mappings:
- `t` (command mode): Open terminal / Enter type mode (when in terminal)
- `Esc` (terminal command mode): Close terminal
- `p` (terminal command mode): Close terminal and enter Claude prompt
- `+/-` (terminal command mode): Increase/decrease terminal height
- `Esc` (terminal type mode): Exit type mode
- All keystrokes in terminal type mode forward directly to PTY

Implementation:
- `terminal_pty`, `terminal_writer`, `terminal_rx`, `terminal_parser` in `App` struct
- `open_terminal()`, `close_terminal()`, `write_to_terminal()`, `poll_terminal()` in `src/app/terminal.rs`
- `draw_terminal()` in `src/tui/draw_terminal.rs` syncs vt100 parser dimensions with viewport

### Centralized Keybindings

All keybindings are defined once in `src/tui/keybindings.rs` and used by both input handlers and the help dialog:

**Architecture:**
- `Action` enum: All possible keybinding actions (NavDown, NavUp, EnterEditMode, Save, etc.)
- `KeyCombo`: Key + modifier combination with display helpers
- `Keybinding`: Primary key, alternatives (j/↓), description, and action
- Static arrays per context: `GLOBAL`, `WORKTREES`, `FILE_TREE`, `VIEWER`, `EDIT_MODE`, `OUTPUT`, `INPUT`, `TERMINAL`
- Terminal and Input bindings shown in title bar only (not in help panel) via title functions
- `prompt_type_title()`, `prompt_command_title()` for Input pane
- `terminal_command_title()`, `terminal_type_title()`, `terminal_scroll_title()` for Terminal pane

**Usage pattern:**
```rust
// Input handlers use lookup_action() for centralized matching
let action = lookup_action(Focus::FileTree, key.modifiers, key.code, false, false, false);
match action {
    Some(Action::NavDown) => app.file_tree_next(),
    Some(Action::NavUp) => app.file_tree_prev(),
    // ...
}
```

**Benefits:**
- Single source of truth for all keybindings
- Help dialog automatically reflects actual keybindings via `help_sections()`
- Adding/changing keybindings only requires one code change
- Dual-key bindings (j/↓) handled via `alternatives` field

**macOS ⌥+letter gotcha:** On macOS, `Option+letter` produces Unicode characters (e.g., `⌥c` → `ç`), so crossterm sees `KeyCode::Char('ç')` with `KeyModifiers::NONE` — NOT `ALT + 'c'`. Never use `⌥+letter` combos for keybindings in text input modes. Use `⌃+letter` (Ctrl) instead — those send real control codes. `⌥+arrow` keys work fine since arrows don't produce Unicode.

**input_cursor is a CHAR INDEX, not a byte offset.** `String::insert()` and `String::remove()` take byte offsets. Use `char_to_byte(char_idx)` to convert before calling them. Comparing `input_cursor` against `String::len()` (bytes) is wrong — use `.chars().count()` instead. See `src/app/input.rs`.

Implementation: `src/tui/keybindings.rs` (definitions), `src/tui/draw_dialogs.rs::draw_help_overlay()` (uses `keybindings::help_sections()`), `src/tui/input_file_tree.rs` and `src/tui/input_viewer.rs` (use `lookup_action()`)

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
| Read | Clickable link + first/last line | File path underlined and clickable; shows file boundaries with line count |
| Bash | Last 2 lines | Shows command results (usually at end) |
| Edit | Clickable link + inline diff (last 20) | File path underlined; click to view in Viewer with diff overlay. Last 20 Edit calls also show inline red/green diff preview |
| Write | Clickable link + purpose line | File path underlined and clickable; shows line count + first comment |
| Grep | First 3 matches | Preview of search results |
| Glob | Directory summary | File count grouped by directory |
| Task | Summary line | First line of agent response |
| WebFetch | Title + preview | Page title and first content line |
| WebSearch | First 3 results | Numbered search results |
| LSP | Result + context | Location and code context |

**Command Detection:**
User messages containing `<command-name>/xxx</command-name>` tags are parsed as slash commands and displayed prominently with centered 3-line banners in magenta.

**Compacting Detection:**
- "⏳ Compacting context..." (yellow) - shown when `/compact` command is detected (START of manual compaction)
- "✓ Context compacted" (green) - shown when `system` event with `subtype: "compact_boundary"` is received (compaction complete)

Note: For auto-compaction, there's no visible "starting" event - we only see the `compact_boundary` after it completes.

**Filtered Messages:**
- Meta messages (`isMeta: true`) are hidden - internal Claude instructions
- `<local-command-caveat>` messages are hidden - tells Claude to ignore local command output
- `<local-command-stdout>` content is hidden - raw output from local commands like `/memory`, `/status`
  - Exception: "Compacted" triggers the CONVERSATION COMPACTED banner before being filtered
- Rewound/edited user messages - when user rewinds to edit a message, only the corrected version is shown
  - Detection: Multiple user messages sharing the same `parentUuid` - keep only the most recent by timestamp

**Debug Output (debug builds only):**
On debug builds (`cargo run`), azureal automatically dumps rendered output to `.azureal/debug-output.txt` whenever session output is loaded. Contains exactly what appears in the TUI output pane with style annotations (colors, bold, italic) for debugging rendering issues.

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
   - **Fallback**: If command format doesn't match, shows `[hookName]` to ensure visibility

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
   - Azureal extracts UPS hooks from thinking blocks and assigns them timestamp = user_message_timestamp + 1ms
   - When events are sorted by timestamp, UPS hooks naturally appear right after their user message
   - UPS hooks from hooks.jsonl are skipped (duplicates with wrong timestamps)
   - UPS hooks display as dim gray lines: `› UserPromptSubmit: <output>`

6. **Compaction summary handling**
   - When loading a continued session, the summary message ("This session is being continued...") contains quoted `<system-reminder>` references from conversation history
   - These quoted references should NOT be treated as real hooks
   - Azureal skips hook extraction for the compaction summary and its immediately following tool results
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
Azureal reads all data at runtime without persisting anything:
- **Project**: Discovered via `git rev-parse --show-toplevel`, main branch detected from git
- **Sessions**: Discovered from `git worktree list` (active) + `git branch | grep azureal/` (archived)
- **Conversation**: Read from Claude's session files at `~/.claude/projects/<encoded-path>/<session-id>.jsonl`
- **Auto-discovery**: Azureal scans Claude's project directory to find/link session files by worktree path
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

### Run Commands

User-defined shell commands that can be saved and executed from the Worktrees pane. Commands are stored per-project in `.azureal/run_commands.json` and executed in the embedded terminal.

**Keybindings (from Worktrees pane):**
- `r` — Open picker (if multiple saved commands) or execute directly (if only 1)
- `⌥r` — Open dialog to create a new run command

**Picker overlay:**
- `j/k` / `↑/↓` — Navigate selection
- `1-9` — Quick-select by number
- `Enter` — Execute selected command
- `e` — Edit selected command
- `x` — Delete selected command
- `a` — Add new command

**Dialog overlay:**
- `Tab` — Toggle between Name and Command fields
- `Enter` — Save
- `Esc` — Cancel

**Storage:** `.azureal/run_commands.json` — JSON array of `{name, command}` objects, loaded on startup.

Implementation: Types in `src/app/types.rs` (RunCommand, RunCommandDialog, RunCommandPicker), state methods in `src/app/state/ui.rs`, input handling in `src/tui/input_dialogs.rs`, rendering in `src/tui/draw_dialogs.rs`

### Creation Wizard

Unified "New..." dialog (`n` from Worktrees) with tabs for creating resources:

**Tabs:**
1. **Project** (placeholder) - future project creation
2. **Branch** (placeholder) - future branch creation
3. **Worktree** - create git worktree with Claude session
   - Name: becomes `azureal/{name}` branch
   - Prompt: initial message to Claude
4. **Session** - create new Claude session in existing worktree
   - Name (optional): custom name stored in `.azureal/sessions.toml`
   - Prompt: initial message to Claude
   - Worktree: select target from list

**Session Name Storage:**
Custom session names map to Claude-generated UUIDs in `.azureal/sessions.toml`:
```toml
[sessions]
"9d409dfb-422b-4f4b-9f32-755277e3e527" = "hook-visibility-fix"
"abc123-def456-..." = "filetree-operations"
```

Implementation: `src/wizard.rs` (wizard state), `src/tui/draw_wizard.rs` (rendering), `src/tui/input_wizard.rs` (input handling), `src/app/state/session_names.rs` (name storage)

# MANIFEST

```
azureal/
├── .azureal/                # Project-level azureal data (gitignored)
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
│   │   │   ├── viewer_edit.rs # Viewer edit mode operations
│   │   │   ├── session_names.rs # Custom session name storage
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
│   │   ├── render_events.rs # DisplayEvent rendering (full + incremental)
│   │   ├── render_thread.rs # Background render thread for async convo rendering
│   │   ├── render_tools.rs # Tool result rendering
│   │   ├── render_wrap.rs  # Text/span wrapping utilities
│   │   ├── draw_sidebar.rs # Sessions pane rendering
│   │   ├── draw_file_tree.rs # FileTree pane rendering
│   │   ├── draw_viewer.rs  # Viewer pane rendering
│   │   ├── draw_output.rs  # Convo pane rendering
│   │   ├── draw_*.rs       # Other rendering functions
│   │   ├── keybindings.rs  # Centralized keybinding definitions (Action enum, lookup_action(), help_sections())
│   │   ├── input_file_tree.rs # FileTree navigation (uses lookup_action())
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
azureal tui

# Or simply
azureal
```

## Keybindings

### Global (Command Mode)
| Key | Action |
|-----|--------|
| `p` | Enter prompt mode (focus input) |
| `t` | Toggle terminal pane |
| `j/k` | Navigate (worktrees, files, scroll) |
| `J/K` | Navigate projects |
| `Tab` | Cycle focus (Worktrees → FileTree → Viewer → Convo → Input) |
| `Shift+Tab` | Cycle focus reverse |
| `?` | Help |
| `⌃c` | Cancel running Claude response |
| `⌃q` | Quit |
| `⌃r` | Restart |

### Worktrees Pane
| Key | Action |
|-----|--------|
| `j/k` | Navigate worktrees |
| `J/K` | Navigate projects |
| `l/→` | Expand session files dropdown |
| `h/←` | Collapse session files dropdown |
| `Enter` | Start/resume Claude session |
| `Space` | Context menu |
| `n` | New worktree/session wizard |
| `b` | Browse branches |
| `d` | View diff |
| `r` | Run command (picker or execute) |
| `⌥r` | Add new run command |
| `R` | Rebase onto main |
| `a` | Archive worktree |

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
| `Shift+J/K` | Half-page scroll |
| `g/G` | Jump to top/bottom |
| `f/b` | Next/prev Edit (syncs Convo scroll) |
| `Esc` | Exit viewer (restores previous content if in Edit diff view) |

### Convo Pane
| Key | Action |
|-----|--------|
| `j/k` | Scroll line |
| `↑/↓` | Jump to prev/next user prompt |
| `Shift+↑/↓` | Jump to prev/next message (incl. assistant) |
| `Shift+J/K` | Half-page scroll |
| `g/G` | Jump to top/bottom |
| `d` | Git worktree diff |
| `o` | Switch to output view |
| `Esc` | Return to Worktrees |

**Clickable Edit Links:** Edit tool file paths are underlined and clickable. Click to open the full file in the Viewer with the edit region highlighted (red background for deleted lines, green background for added lines). The selected Edit is highlighted with orange background and black text in the Convo pane. Use `f/b` in the Viewer to cycle through edits (also syncs Convo scroll). The last 20 Edit calls also show inline diff previews in the Convo pane.

### Prompt Mode (Input Focused)

Prompt keybindings are displayed directly in the Input pane's title bar (not in the help panel). All title hints are dynamically sourced from the `INPUT` binding array via `find_key_for_action()` / `find_key_pair()` — changing a key in the array automatically updates the title.

**Type mode title shows:** `(Esc:exit | Enter:submit | ⇧Enter/⌃j:newline | ⌃c:cancel | ↑/↓:history | ⌥←/→:word | ⌃w:del wrd | ⌃u:clear)`
**Command mode title shows:** `(p:type | t:terminal)`

### Terminal Mode

Terminal keybindings are displayed directly in the terminal pane's title bar (not in the help panel). All title hints are dynamically sourced from the `TERMINAL` binding array via `find_key_for_action()` / `find_key_pair()` — changing a key in the array automatically updates the title.

**Command mode title shows:** `(t:type | p:prompt | Esc:close | j/k:scroll | J/K:page | g/G:top/bottom | +/-:resize)`
**Type mode title shows:** `(Esc:exit)`
**Scroll mode title shows:** `[N↑] (j/k:scroll | J/K:page | g:top | G:bottom | t:type | Esc:close)`