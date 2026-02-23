# SUMMARY

Azureal (Asynchronous Zoned Unified Runtime Environment for Agentic LLMs) is a Rust TUI application that wraps Claude Code CLI to enable multi-agent development workflows. Each **worktree** is a git worktree with its own Claude **session**, allowing concurrent AI-assisted development across multiple feature branches.

**Terminology:**
- **Worktree**: A git worktree with its own working directory and branch (displayed in left panel)
- **Session**: A Claude Code conversation (stored in `~/.claude/projects/`, displayed in Convo pane)

**Mostly Stateless Architecture:** All runtime state is derived from:
- Git repository info via `git rev-parse --show-toplevel`
- Git worktrees via `git worktree list` for active worktrees
- Git branches via `git branch | grep azureal/` for archived worktrees
- Claude's session files in `~/.claude/projects/` for conversation history and `--resume` IDs

**Persistent State (azufig.toml):**
All persistent state consolidated into two TOML files named `azufig.toml` — one global and one project-local:
- **Global** `~/.azureal/azufig.toml` — app config (API key, claude path, permission mode), registered projects (paths + display names), global run commands, global preset prompts
- **Project-local** `.azureal/azufig.toml` — filetree options (hidden entry names), custom session name → UUID mappings, health scan scope (directory paths), project-local run commands, project-local preset prompts
- **Worktree-local** `<worktree_path>/.azureal/azufig.toml` — per-worktree git settings (`[git]` section). Each worktree can have its own azufig for worktree-specific config; created on demand when a worktree-level setting is toggled
All sections use single-bracket `[section]` headers with flat `key = "value"` pairs (e.g., `ProjectName = "~/path"`, `SessionUUID = "display name"`). `[runcmds]` and `[presetprompts]` keys are prefixed with a 1-based position number to preserve quick-select order: `N_Name = "value"` (e.g., `1_Build = "cargo build"`, `2_Test = "cargo test"`). Prefix stripped on load, re-written on save. Keys that qualify as TOML bare keys (`A-Za-z0-9_-` only) are written unquoted for clean output; keys with spaces or special chars (e.g., `"1_Cargo run (debug)"`) stay quoted. `#[serde(default)]` on every section for forward-compatibility. Write pattern: load-modify-save (read current, update one section, write back) to avoid clobbering unrelated sections.

# FEATURES

### Multi-Worktree Claude Management

The core feature enabling multiple concurrent Claude Code CLI instances. Each worktree supports **multiple simultaneous Claude processes** via PID-keyed session slots.

**Architecture:**
- Each prompt spawns a new process: `claude -p "prompt" --verbose --output-format stream-json`
- `spawn()` returns `(Receiver<ClaudeEvent>, u32)` — the event channel and the OS PID
- First prompt: captures `session_id` from init event in stream-json output
- Follow-up prompts: add `--resume <session_id>` for conversation context
- Process exits after each response; new process for next prompt

**PID-Keyed Session Slots:**
All session state maps (`claude_receivers`, `running_sessions`, `claude_exit_codes`, `claude_session_ids`) are keyed by **PID string** (not branch name). This enables multiple concurrent Claude processes per worktree. Two additional maps track the relationship:
- `branch_slots: HashMap<String, Vec<String>>` — branch → list of active PID strings (spawn order)
- `active_slot: HashMap<String, String>` — branch → which PID's output is displayed in the convo pane

Only the **active slot's** output feeds `display_events`; other slots' output is silently drained from their receivers. When the active slot exits, the app auto-switches to the last remaining slot on that branch (or clears if none remain). New spawns always become the active slot. `cancel_current_claude()` kills only the active slot's process.

**Critical: NO `--fork-session`**
Earlier we used `--fork-session` with `--resume`, but this creates a NEW session each time (losing conversation context and causing tool_use ID collisions). Removed in favor of simple `--resume` only.

**Why not use `--session-id`?**
`--session-id` requires a valid UUID format. Simpler to capture Claude's generated session ID from the init event.

**Why not keep process alive?**
Claude Code's interactive mode uses a full TUI that cannot be driven by simple stdin writes. The `--input-format stream-json` flag only works with `-p` mode which still exits after each response. Verified by testing - there's no headless interactive mode available.

Current approach (`-p --resume`) works reliably with ~100-200ms process spawn overhead per prompt.

Implementation: `src/claude.rs` spawns processes (returns `(Receiver, PID)`), `src/app/state/claude.rs` manages PID-keyed slots, `src/app/state/app.rs` tracks `branch_slots`/`active_slot` maps.

### Git Worktree Isolation

Each worktree provides true branch isolation:
- Has its own working directory
- Can have different uncommitted changes
- Operates on a separate branch from main
- **Archiving** removes the worktree directory but preserves the git branch (`a` key). Archived worktrees show as `◇` (diamond) with dimmed text in the sidebar.
- **Unarchiving** recreates the worktree from the preserved branch (`u` key). `Enter` on an archived session shows a status message directing the user to press `u` first.
- **Read-only main branch isolation:** Main/master branch is stored separately in `app.main_worktree: Option<Worktree>` and is **hidden from the sidebar worktree list** entirely. Press `m` from the Worktrees context to enter read-only main browse mode: the file tree opens showing main's files with a yellow "(read-only)" border suffix, and the sidebar title shows "★ main (read-only)" in yellow. All file mutations are blocked (add/delete/rename/copy/move), edit mode is blocked, prompt mode is blocked, and session start is blocked. `Esc` exits browse mode and restores the previous worktree selection. `current_worktree()` transparently returns `main_worktree` when `browsing_main` is true, so all downstream code works without special-casing. `enter_main_browse()` and `exit_main_browse()` in `src/app/state/ui.rs` manage state transitions; `switch_project()` clears browse state.
- **Sidebar icons:** Main is no longer in the worktree list (no star icon in sidebar). Archived worktrees show `◇` (diamond) with dimmed text. Feature branches use standard status circles (`●`/`○`/etc.).
- CLI: `azureal session archive <name>` / `azureal session unarchive <name>`

Implementation: `src/git.rs` handles worktree creation, deletion, and status queries. `src/app/state/app.rs` stores `main_worktree`, `browsing_main`, and `pre_main_browse_selection` fields. `src/app/state/load.rs` populates `main_worktree` separately from the `worktrees` vec. `src/app/state/health.rs` uses `current_worktree_info()` (replaced `find_main_worktree()` + `switch_to_main_worktree()`) so health scans run on the current worktree. `src/tui/draw_sidebar.rs` shows yellow "★ main (read-only)" title in browse mode. `src/tui/draw_file_tree.rs` adds "(read-only)" suffix and yellow border in browse mode. `src/tui/event_loop/actions.rs` enforces read-only guards on 7+ actions and handles `BrowseMain` action + `Esc` exit.

### TUI Interface

A ratatui-based terminal interface with 3-pane layout, toggle overlays, and status bar:

```
┌──────────┬──────────────────────────┬──────────────┐
│Worktrees │         Viewer           │              │
│  (15%)   │         (50%)            │  Convo (35%) │
├──────────┴──────────────────────────┤              │
│     Input / Terminal                │              │
├─────────────────────────────────────┴──────────────┤
│                  Status Bar                        │
└────────────────────────────────────────────────────┘
```

**Panes:**
- **Worktrees** (15%): Worktree list showing all active and archived worktrees (main branch hidden — stored separately, browse with `m`). Press `f` to toggle a **FileTree overlay** in this pane (replaces worktree list with directory tree for the selected worktree). Press `w` or `Esc` to return to worktree list.
- **Viewer** (50%): File content viewer or diff detail (dual-purpose)
- **Convo** (35%, full height): Claude conversation output with tool results — extends past input pane down to status bar. Press `s` to toggle a **Session list overlay** in this pane (replaces convo with a session file browser showing status symbol, worktree name, session name/UUID, last modified time, and `[N msgs]` count). Top border has three title positions: left shows "Convo [x/y]" message position, **center shows session name in `[brackets]`** (custom names from `.azureal/sessions` preferred; raw UUIDs shown as `[xxxxxxxx-…]`; ellipsied to fit between left and right titles; cached on session switch via `title_session_name` — zero file I/O in render path), right shows token usage + PID/exit code (border characters fill gaps). Token usage shown as color-coded percentage badge (green <60%, yellow 60-80%, red >80%). PID shown in green while running; switches to exit code on exit (green for 0, red for non-zero). Uses ratatui's multi-title API with `Alignment::Center` and `Alignment::Right`.
- **Input/Terminal**: Prompt input or embedded terminal (spans Worktrees + Viewer width only)
- **Status Bar** (1 row, bottom): Left shows worktree status dot + display name + branch (branch parenthetical hidden when identical to name, e.g. `main` not `main (main)`). Center shows context-sensitive help hints that update per focus/mode. Right shows CPU% + PID badge — rendered in AZURE (`#3399FF`) for debug builds and DarkGray for release builds, providing a quick visual indicator of the active build profile (single `cfg!(debug_assertions)` check in `src/tui/draw_status.rs`). No ViewMode indicator — help hints already change per mode.

**Splash Screen:** On startup, a 2x-scale block-character "AZUREAL" logo (10 rows × 110 chars, pure `█` blocks) in AZURE (#3399FF) is rendered centered on screen with the full acronym ("Asynchronous Zoned Unified Runtime Environment for Agentic LLMs") rendered in half-block characters (▀▄█ for 2x vertical density, 12 rows across 4 word-groups) in dim blue, followed by a "Loading project..." subtitle. Drawn immediately after terminal initialization (before `App::new()`) so the user sees branded feedback instead of a black screen while git discovery, session parsing, and file I/O run. Enforces a 3-second minimum display time (loading time counts toward it) so the branding registers even on fast machines. Replaced by the first `ui()` draw when the event loop starts.

**OS Terminal Title:** Set dynamically via crossterm `SetTitle`. Shows `AZUREAL` when no project loaded, `AZUREAL @ <project> : <branch>` when a session is selected. Updated on startup, session switch, and project switch (via `update_terminal_title()` in `src/app/state/ui.rs`, called from `load_session_output()`). Reset to empty on exit.

**Overlays:**
- **FileTree overlay** (`f` in Worktrees pane): Replaces worktree list with directory tree for the selected worktree. Uses **Nerd Font icons** (~60 file types with language-brand colors: Rust orange, Python blue, etc.) with automatic detection via `detect_nerd_font()` — probes a PUA glyph during splash and measures cursor advance via DSR. Falls back to emoji icons if the terminal font lacks Nerd Font glyphs (status bar shows "Nerd Font not detected" message). Icon mapping in `src/tui/file_icons.rs` — checks filename first (Dockerfile, Makefile, LICENSE, etc.), then extension. Border title shows `Filetree (worktree_name)` with optional `[pos/total]` scroll indicator when content overflows. Supports expand/collapse, file opening in Viewer. Focus set to `Focus::FileTree` while active. `f` or `Esc` returns to worktree list. **Options overlay** (`O`): replaces tree content with a checkbox list for toggling visibility — `worktrees`, `.git`, `.claude`, `.azureal`, `.DS_Store` (all hidden by default). QuadrantOutside AZURE border with `" Filetree Options "` title and `" Space:toggle  Esc:close "` footer. `j/k` navigate, `Space`/`Enter` toggle, `Esc`/`O` close. Hidden names stored in `file_tree_hidden_dirs: HashSet<String>` — tree rebuilds immediately on toggle. **Persisted to project azufig.toml** `[filetree].hidden` on every toggle and loaded on startup/project switch. File actions (`a`dd, `d`elete, `r`ename, `c`opy, `m`ove) show an inline action bar at the bottom of the pane. Add/Rename use text input (`⌃u` clears, `Esc` cancels, `Enter` confirms); Add with trailing `/` creates directory; Rename pre-fills with current name. Copy/Move use clipboard-style paste: press `c`/`m` to grab source file (highlighted with `┃name┃` solid border for copy or `╎name╎` dashed border for move, in magenta), navigate tree to target directory, `Enter` to paste, `Esc` to cancel. Delete uses y/N confirmation. Actions operate relative to selected entry's parent dir (or inside selected dir for Add/paste). Recursive dir copy via `copy_dir_recursive()`. State tracked as `file_tree_action: Option<FileTreeAction>` enum — `Add(String)`/`Rename(String)` hold text buffer, `Copy(PathBuf)`/`Move(PathBuf)` hold source path.
- **Session list overlay** (`s` in Convo pane): Replaces conversation view with a session file browser scoped to the currently selected worktree. Each row shows a **status dot** (green `●` if a Claude process is actively running that session, dim gray `○` if idle — mirrors the worktree sidebar dots), session name (from `.azureal/sessions`) or full UUID, right-aligned last modified time, and `[N msgs]` badge. Border title shows worktree name + position counter. Message counts computed via fast string scanning (no JSON parsing — `"type":"user"` and `"type":"assistant"` have zero false positives in Claude's compact JSON). Counts user prompt lines (no tool_result, not isMeta, not `<local-command-caveat>`/`<local-command-stdout>`/`<command-name>`/compaction summary) + assistant text blocks (type=text content). Counts cached by file size — only recomputed when a session file grows. Opening the list is two-phase: phase 1 shows the overlay immediately with a centered "Loading sessions…" dialog, phase 2 computes message counts after the dialog frame renders (so the UI never appears frozen). `j/k` navigate, `J/K` page, `Enter` loads session, `s` or `Esc` returns to convo. `/` activates name filter (case-insensitive match against session name or UUID); `//` (slash while filter is empty) switches to content search mode (searches current worktree's JSONL files for text matches, min 3 chars, capped at 100 results, skips files >5MB). Filter bar shows at top with yellow border when active. Focus cycling (Tab) closes overlays; Shift+Tab from Viewer lands on FileTree if the overlay is open (preserving it), otherwise on Worktrees.

**Loading Indicators (Deferred Actions):**
Any user action that triggers blocking I/O (session parse, file read, health scan, project switch, scope rescan) shows a centered AZURE-bordered popup with a descriptive message while the work runs. Uses a generic two-phase pattern: (1) set `loading_indicator: Option<String>` + `deferred_action: Option<DeferredAction>`, (2) event loop draws the popup via `draw_loading_indicator()`, (3) on the next frame after the draw, event loop takes `deferred_action`, clears the indicator, and calls `execute_deferred_action()` which dispatches to the actual handler. Five operations use this system:
- **Session load** (`"Loading session…"`) — Enter in session list or content search result
- **File open** (`"Loading <filename>…"`) — Enter on file in FileTree
- **Health panel** (`"Scanning project health…"`) — Shift+H to open Worktree Health
- **Project switch** (`"Switching project…"`) — Enter on project in Projects panel
- **Health scope rescan** (`"Rescanning health scope…"`) — Esc from scope mode (saves scope immediately, defers expensive rescan)

`DeferredAction` enum variants: `LoadSession { branch, idx }`, `LoadFile { path }`, `OpenHealthPanel`, `SwitchProject { path }`, `RescanHealthScope { dirs }`. The existing session list loading (`session_list_loading`) and debug dump saving (`debug_dump_saving`) use their own two-phase patterns predating this system.

Implementation: `src/app/state/app.rs` (DeferredAction enum + fields), `src/tui/run.rs` (`draw_loading_indicator()`), `src/tui/event_loop.rs` (deferred execution block), `src/tui/event_loop/actions.rs` (`execute_deferred_action()`)

**Color Identity:** All accent colors use the `AZURE` constant (`#3399FF`, defined in `src/tui/util.rs`) instead of ANSI Cyan, aligning the visual identity with the "Azureal" name. Import via `use super::util::AZURE;` (TUI modules) or `use crate::tui::util::AZURE;` (non-TUI modules).

**Viewer Dual Purpose:**
- When file selected in FileTree → shows syntax-highlighted file content with line numbers
- When image selected in FileTree → renders image via terminal graphics protocol (Kitty/Sixel/halfblock fallback) using `ratatui-image` crate. Image auto-fits viewport; no scroll/selection/edit mode. `Picker::from_query_stdio()` lazy-inits once to detect terminal capabilities. `StatefulProtocol` adapts to render area each frame.
- When diff selected in Convo → shows diff detail (future)

**Viewer Tabs:** Up to 12 tabs across 2 rows (6 per row, fixed-width). `t` saves current file to a tab, `⌥t` opens tab dialog, `[`/`]` navigate, `x` closes. Tab bar renders inside the border at rows 1-2, overlaying empty padding lines so content shifts down. `tab_bar_rows()` returns 0/1/2 based on count; `viewport_height` reduced by tab rows for correct scroll clamping. 12-tab max enforced in `viewer_tab_current()` with status message on overflow.

**Syntax Highlighting:**
- Uses syntect library with base16-ocean.dark theme
- Automatic language detection based on file extension
- Supports Rust, TOML, Markdown, JSON, YAML, and 150+ other languages

Other features:
- Vim-style modal editing
- Diff viewer with syntax highlighting
- Help overlay with keybindings
- Mouse interaction: scroll panels, click to focus panes, click sidebar/file tree to select, click input to position cursor, double-click to open files/expand dirs, drag to select text in Viewer/Convo panes
- Preset prompts (⌥P): save up to 10 prompt templates; quick-select with 1-9,0 from picker OR directly from prompt mode with ⌥1-⌥9,⌥0 (skips picker); picker footer shows shortcut hint; add/edit/delete from picker (d=delete with y/n confirmation); available only in prompt mode; hint shown in prompt title bar. Dual-scope persistence: presets can be global (`~/.azureal/azufig.toml` `[presetprompts]`, shared across all projects) or project-local (`.azureal/azufig.toml` `[presetprompts]`); toggle scope with ⌃g in add/edit dialog; picker shows G/P badge per preset

Implementation: `src/tui/event_loop.rs` + `src/tui/event_loop/` (5 submodules: actions, claude_events, coords, fast_draw, mouse) for event loop, `src/tui/run.rs` for rendering, `src/tui/render_thread.rs` for background convo rendering, `src/app/state/` for state management (split into 10 focused submodules, `health` has 2 sub-submodules).

**Mouse Click Architecture:**
- All pane `Rect`s cached on App struct during `ui()` draw: `pane_worktrees`, `pane_viewer`, `pane_convo`, `pane_todo`, `input_area`
- Pane hit-testing via `Rect::contains(Position::new(col, row))` — shared by both click and scroll handlers
- Sidebar uses `sidebar_row_map: Vec<SidebarRowAction>` built alongside `sidebar_cache` in `build_sidebar_items()` — maps visual row to `ProjectHeader` or `Worktree(idx)`
- FileTree overlay (when `show_file_tree` is active) uses the `pane_worktrees` rect area for click/scroll handling; entry index = `visual_row + file_tree_scroll`, with double-click detection via `last_click` field (same position within 500ms)
- Input click enters prompt mode and positions cursor via `click_to_input_cursor()` — uses `word_wrap_break_points()` to map screen coords to char index with word-boundary wrapping
- Overlays (help, branch_dialog, run_command_picker/dialog, creation_wizard) are dismissed on any click outside

**Text Selection (Mouse Drag):**
- `MouseDown(Left)` converts screen coords to cache coords immediately, stores as `mouse_drag_start: Option<(usize, usize, u8)>` — `(cache_line_or_char, cache_col, pane_id)`. pane_id: 0=viewer, 1=convo, 2=input, 3=edit-mode-viewer. Clears existing `viewer_selection` / `output_selection`.
- **Edit mode click:** When `viewer_edit_mode` is true and click lands in viewer pane, `screen_to_edit_pos()` maps screen coords → `(source_line, source_col)` by walking source lines and summing wrap counts. Sets `viewer_edit_cursor` and clears `viewer_edit_selection`. Drag anchor stored as pane_id=3.
- **Edit mode drag (pane_id=3):** Maps current drag position via `screen_to_edit_pos()`, sets `viewer_edit_selection = Some((anchor_line, anchor_col, drag_line, drag_col))` and moves cursor to drag end. Auto-scrolls when dragging above/below pane.
- `MouseDrag(Left)` calls `handle_mouse_drag()` which uses the cached anchor (pane_id from `mouse_drag_start`) and maps only the current cursor position from screen to cache coords via `screen_to_cache_pos()`. For input pane, uses `screen_to_input_char()` to map to char index.
- Anchor stored in cache coords so auto-scroll during drag doesn't shift the selection start
- Auto-scroll when dragging above/below pane content area
- Selection stored as `Option<(start_line, start_col, end_line, end_col)>` in cache-line indices (normalized so start <= end)
- Viewer selection rendered in `draw_viewer.rs` via `apply_selection_to_line()` (already existed)
- Convo selection rendered in `draw_output.rs` by calling `apply_selection_to_line()` after viewport build — `output_selection_cached` used as viewport cache invalidation key
- `apply_selection_to_line()` is `pub(crate)` in `draw_viewer.rs` — splits spans at selection boundaries, patches with `Rgb(60,60,100)` bg. Takes `gutter` param to skip line number column from highlighting (File mode computes from first span width; Diff/Convo pass 0). O(spans_in_line) per viewport line, negligible cost.
- `⌘C` copies from whichever pane has active selection (viewer, convo, or input) via `extract_text_from_cache()` → `arboard::Clipboard`. Viewer copy strips line number gutter (first span per line) so only file content is copied.
- Selections cleared on: click, scroll, Tab, focus change
- **Fast-path exclusion:** `fast_draw_input()` and draw deferral are both skipped when `has_input_selection()` is true — fast-path writes raw text without selection styling

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
- **Render submit throttle: 50ms** — `last_render_submit` in App state. Without this, every `poll_render_result()` completion immediately triggers another `submit_render_request()` (since `rendered_lines_dirty` is re-set by arriving events), cloning the full events array at ~60Hz. The 50ms floor batches streaming events into ~20 render cycles/sec.

### 14. True Single JSON Parse Per Claude Event

```rust
// ❌ WRONG - EventParser parses JSON, then handle_claude_output parses AGAIN
let events = self.event_parser.parse(&data);               // parse #1
let json = serde_json::from_str::<Value>(&data).ok();      // parse #2 (duplicate!)

// ✅ CORRECT - EventParser returns the parsed Value alongside events
let (events, parsed_json) = self.event_parser.parse(&data); // single parse
// parsed_json is reused for token extraction + display text
```

`EventParser::parse()` returns `(Vec<DisplayEvent>, Option<serde_json::Value>)` — the same JSON value used internally is also passed to the caller. `handle_claude_output` reuses it for token/model/context-window extraction with zero additional parsing.

**output_lines skip:** Once `rendered_lines_cache` has content, `display_text_from_json()` + `process_output_chunk()` are skipped entirely. They only feed the fallback raw output view (used before first render completes).

**Empty event batch skip:** Many stdout lines (progress, hook_started) produce 0 DisplayEvents. `display_events.extend()` + `invalidate_render_cache()` are skipped for these.

**Full render clone reduction:** The full render path clones only `display_events[deferred_start..]` instead of the entire Vec — avoids cloning early events that are never rendered.

**Reader thread optimization:** The stdout reader thread (`src/claude.rs`) only needs to extract `session_id` from the init event (happens once per session). Instead of full JSON parsing every line, it checks `line.contains("\"subtype\":\"init\"")` first — only parses JSON when the string matches.

**EventParser buffer optimization:** The parser collects all complete lines in one `drain()` call instead of re-allocating `self.buffer` on every newline (O(n) total instead of O(n²) per chunk).

**Dev profile optimization:** `Cargo.toml` sets `opt-level = 2` for `serde_json`, `serde`, and `syntect` packages in dev builds. These hot-path dependencies run 3-5x slower at opt-level 0, amplifying all parsing and highlighting costs in debug mode.

**Files:** `src/events/parser.rs` (parse returns JSON), `src/app/state/claude.rs::handle_claude_output()`, `src/app/util.rs` (`display_text_from_json`)

### 9. NEVER Use `.wrap()` on Pre-Wrapped Content

```rust
// ❌ WRONG - ratatui re-wraps every viewport line char-by-char during render()
let para = Paragraph::new(pre_wrapped_lines).wrap(Wrap { trim: false });

// ✅ CORRECT - content is already wrapped by wrap_text()/wrap_spans(), no re-wrapping needed
let para = Paragraph::new(pre_wrapped_lines);
```

Convo pane content is pre-wrapped to `inner_width` by `wrap_text()` and `wrap_spans()` in `render_events.rs`. Adding `.wrap()` causes ratatui's `Paragraph::render()` to iterate every character of every span to compute line breaks that already exist — pure redundant O(viewport_chars) work per frame.

**Files:** `src/tui/draw_output.rs` renders Paragraph without `.wrap()`. If you add a new Paragraph that displays pre-wrapped content, do NOT add `.wrap()`.

### 13. Edit Mode: Cache Highlighting + Viewport-Only Rendering

```rust
// ❌ WRONG - Re-highlights entire file and builds all visual lines EVERY FRAME
let full_content = app.viewer_edit_content.join("\n");
let highlighted = app.syntax_highlighter.highlight_file(&full_content, &path_str);
// Then iterates ALL source lines to build all_lines...

// ✅ CORRECT - Cache highlighting, only re-run on content change; only process visible lines
let edit_ver = app.viewer_edit_version;  // monotonic counter, bumped on every mutation
if app.viewer_edit_highlight_ver != edit_ver {
    app.viewer_edit_highlight_cache = app.syntax_highlighter.highlight_file(...);
    app.viewer_edit_highlight_ver = edit_ver;
}
// Walk source lines to find visible range, only build Lines for viewport
```

**Impact:** AGENTS.md (~1000+ lines) caused 90%+ CPU in edit mode — syntect was parsing the entire file every frame at 30fps. Now: highlight once on enter/edit (~50ms), then zero highlight cost per frame. Viewport-only line construction means O(viewport_height) not O(file_size) per frame.

**Cache invalidation:** `viewer_edit_highlight_ver` tracks `viewer_edit_version` — a monotonically increasing counter bumped in `push_undo()` and undo/redo. Cannot use `viewer_edit_undo.len()` because the undo stack caps at 100 entries; after 100 edits, push+trim keeps length at 100 so the cache key never changes. Scrolling, cursor movement, and selection don't bump version → cache hit → zero cost. Cleared on `exit_viewer_edit_mode()`.

**Cursor position:** Computed arithmetically by summing wrap counts for source lines before cursor. No `all_lines` array needed.

**Files:** `src/tui/draw_viewer.rs::draw_edit_mode()`, `src/app/state/app.rs` (cache fields), `src/app/state/viewer_edit.rs` (cache cleanup)

### 8. File Watching + Session File Polling (Notify + Deferred Parse + Incremental)

**Change detection** uses kernel-level filesystem notifications via the `notify` crate (kqueue on macOS, inotify on Linux, ReadDirectoryChangesW on Windows). A background `FileWatcher` thread (`src/watcher.rs`) owns a `notify::RecommendedWatcher` and forwards classified events to the main thread via mpsc channels — zero CPU between events, near-instant detection.

**Watch targets** (re-registered on session switch via `sync_file_watches()`):
- **Session JSONL file** — `NonRecursive` watch, sets `session_file_dirty = true` on change
- **Worktree directory** — `Recursive` watch, triggers debounced file tree refresh (500ms)

**Noise filtering** happens in the watcher thread: `/target/`, `/.git/`, `/node_modules/`, `.DS_Store`, `.swp`/`.swo`/`~` files are dropped before reaching the main thread. Events are coalesced (at most one `SessionFileChanged` + one `WorktreeChanged` per 200ms drain cycle).

**Graceful fallback:** If `notify` fails to initialize (`FileWatcher::spawn()` returns `None`) or the watcher thread errors at runtime (`WatcherFailed` event), the event loop falls back to the original stat()-based polling (500ms interval) seamlessly.

Three-phase parse+render pipeline for session files:

1. **Change detection** (notify-driven or fallback stat()): Sets `session_file_dirty = true`.
2. **Incremental parse** (`refresh_session_events()`): Seek to `session_file_parse_offset`, parse only new JSONL lines appended since last read. Rebuilds tool_call context from existing DisplayEvents via `IncrementalParserState`. Falls back to full re-parse if file shrank or user-message rewrite detected (parentUuid dedup).
3. **Incremental render** (`draw_output()`): If `rendered_events_count < display_events.len()` and width unchanged, renders only newly appended events and appends to `rendered_lines_cache`. Falls back to full re-render on width change or event count decrease (session switch).

```rust
// Watcher thread classifies filesystem events:
pub enum WatchEvent {
    SessionFileChanged,  // session JSONL modified
    WorktreeChanged,     // file created/deleted/modified in worktree
    WatcherFailed(String),
}

// Main thread drains events (non-blocking):
while let Some(evt) = watcher.try_recv() {
    match evt {
        SessionFileChanged => app.session_file_dirty = true,
        WorktreeChanged => { app.file_tree_refresh_pending = true; },
        WatcherFailed(_) => { app.file_watcher = None; break; },
    }
}

// Incremental parse (only new bytes since last offset)
fn refresh_session_events(&mut self) {
    let parsed = parse_session_file_incremental(
        &path, self.session_file_parse_offset,
        &self.display_events, &self.pending_tool_calls, &self.failed_tool_calls,
    );
}
```

**Streaming vs Polling (Dual-Source Prevention):**
During active Claude streaming, events are added to `display_events` by the live process handler (`handle_claude_output()` in `claude.rs`). Session file polling is **skipped** during streaming (`poll_session_file()` returns early if `is_current_session_running()`). **Important:** stream-json stdout does NOT include `user` type events — only system/assistant/result/progress. User messages are pushed as real `DisplayEvent::UserMessage` events into `display_events` at prompt submit time (`add_user_message()` in `output.rs`), ensuring they render immediately and persist throughout the conversation. The `pending_user_message` field is kept only as a dedup marker — cleared by `load.rs` when the session file's authoritative `UserMessage` appears during re-parse. When Claude exits, `handle_claude_exited()` forces a full re-parse (`session_file_parse_offset = 0`, `session_file_dirty = true`) to reconcile live-streamed events with the authoritative session file (which has hook extraction, rewrite handling, etc. that the live EventParser doesn't).

**Files:**
- `src/watcher.rs` - `FileWatcher` thread, `WatchEvent`/`WatchCommand` types, noise filtering
- `src/app/session_parser.rs` - `parse_session_file_incremental()`, `IncrementalParserState`
- `src/app/state/load.rs` - `check_session_file()`, `poll_session_file()`, `refresh_session_events()`, `sync_file_watches()`
- `src/app/state/claude.rs` - `handle_claude_output()` (live events), `handle_claude_exited()` (full re-parse trigger)
- `src/tui/render_events.rs` - `render_display_events_incremental()`, `render_display_events_with_state()`
- `src/tui/draw_output.rs` - incremental render path selection, `pre_scan_events()`
- `src/tui/event_loop.rs` - watcher event drain, fallback polling, debounced file tree refresh

**App state for incremental tracking:**
- `file_watcher: Option<FileWatcher>` — background watcher thread handle (None = fallback to polling)
- `file_tree_refresh_pending: bool` — set by WorktreeChanged, cleared after debounced refresh
- `worktree_last_notify: Instant` — timestamp of last worktree change (for 500ms debounce)
- `rendered_content_line_count: usize` — total line count of rendered cache (equals `rendered_lines_cache.len()`)
- `session_file_parse_offset: u64` — byte offset after last successful parse
- `rendered_events_count: usize` — how many events were rendered into current cache
- `rendered_events_start: usize` — start index for deferred render (>0 means early events skipped)

**Fallback triggers (reverts to full re-parse/re-render):**
- File shrank (shouldn't happen with append-only JSONL)
- User-message rewrite detected (parentUuid dedup → events reference earlier indices)
- Terminal width changed (need to re-wrap all text)
- Session switched (event count drops to 0)

### 10. Deferred Initial Render for Large Conversations

For conversations with 200+ events, only the last 200 events are rendered on initial load. The user starts at the bottom (`output_scroll = usize::MAX`) so they see recent messages instantly. Full render happens lazily when the user reaches scroll position 0 — both `scroll_output_up()` and `jump_to_prev_bubble()` set `rendered_lines_dirty = true` when they hit scroll 0 with `rendered_events_start > 0`, triggering deferred render expansion on the next event loop frame.

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

**Message count denominator:** The convo title `[x/y]` denominator counts `UserMessage` + `AssistantText` from the **full** `display_events` array — not from `message_bubble_positions` which only covers rendered events. This ensures the denominator shows the true total even when deferred rendering has skipped early events. The numerator uses `unrendered_offset = total - rendered_bubbles` so position numbering is correct before full render triggers.

**Files:** `src/tui/draw_output.rs` (DEFERRED_RENDER_TAIL const, deferred render logic, title denominator count)

### 11. NEVER Do File I/O in the DRAW Path (Render Thread Is Fine)

File I/O in `terminal.draw()` or any function called during frame rendering blocks the event loop. However, `render_edit_diff()` runs on the **background render thread** — file I/O there is safe because it doesn't block input or drawing.

`render_edit_diff()` reads the file once per Edit event to find the actual line number of the edit. It tries `new_string` first (post-edit state), then falls back to `old_string` (pre-edit state, for live preview mid-streaming when the edit hasn't been applied yet). Both give the correct position since they occupy the same location in the file at their respective points in time. Falls back to line 1 if the file can't be read or both strings are empty (pure deletion).

**Edit diff styling:** Removed lines (red) use dark grey text (`Rgb(100,100,100)`) on dim red bg — no syntax highlighting, deliberately darker than comment grey in syntax-highlighted green lines. Only added lines (green) get syntax highlighting. This keeps removed lines visually receded and reduces highlight calls to 1 per Edit event.

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
- **Fast-path input rendering:** When keys arrive in Claude prompt mode (NOT terminal mode) with **single-line input** (no `\n`) and **no active selection**, `fast_draw_input()` writes the input box content directly to the terminal via crossterm (~0.1ms), completely bypassing `terminal.draw()` (~18ms). The key is processed immediately and the input character appears instantly. The full `terminal.draw()` is deferred to the next quiet frame. Ratatui's diff naturally reconciles on the next full draw (no buffer invalidation needed). `app.input_area` (cached from last full draw in `ui()`) provides the screen coordinates. **Must exclude terminal mode** — terminal uses `prompt_mode=true` for "type mode", but `fast_draw_input()` writes `app.input` (empty in terminal) over the input_area, wiping PTY display. **Must exclude multi-line input** — the input box resizes dynamically when newlines are added/removed, but `input_area` reflects the old height, causing cursor mispositioning. **Must exclude active selection** — `fast_draw_input()` writes raw text without selection highlighting; `has_input_selection()` check added to both fast-path and draw deferral conditions.
- **Deferred draw on key events:** When typing single-line in Claude prompt mode with no selection, `terminal.draw()` is SKIPPED — the key is processed, fast-path renders the input, and the loop iterates back immediately. Full draw happens on the next quiet iteration (no key events). A `draw_pending` flag on App tracks deferred draws. **Terminal type mode, multi-line input, and active selection are NOT deferred** — they need immediate `terminal.draw()` calls (PTY has no fast-path; multi-line needs layout resize; selection needs full render for highlight styling).
- **Pre-draw event drain with abort:** Right before `terminal.draw()`, drain any key events that arrived since the top-of-loop drain (~0-5ms gap). If a key is found, the draw is ABORTED (loop continues without drawing).
- **Draw throttle (33ms / ~30fps):** Even quiet-iteration draws are throttled to 33ms minimum interval to avoid burning CPU on rapid background updates.
- **Adaptive poll timeout:** 16ms when busy (draw pending, render in-flight, or Claude streaming), 100ms when idle. Ensures fast draw after typing stops without burning CPU when nothing is happening.
- **Viewport cache:** Convo pane caches the cloned viewport slice (`output_viewport_cache`). Only rebuilds when scroll position, content, or animation tick changes. On typing-only frames, serves from cache instead of re-cloning from the full `rendered_lines_cache`.
- **Background render thread:** Expensive convo rendering (markdown parsing, syntax highlighting, text wrapping via `render_display_events`) runs on a dedicated background thread (`RenderThread`). The event loop sends render requests via `submit_render_request()` (non-blocking channel send) and polls for results via `poll_render_result()` (non-blocking channel recv). Input is NEVER blocked by rendering — the main thread only does cheap draw operations. Sequence numbers ensure stale results are discarded (latest-wins). The render thread drains to the latest request when multiple are queued, and uses zero CPU when idle (blocks on `mpsc::recv`). `draw_output()` has a width-mismatch fallback that re-renders if the terminal width changed since the request was submitted (rare, only on resize). `poll_render_result()` re-sets `output_scroll = usize::MAX` (follow-bottom sentinel) when the user was at/near the bottom of the OLD cache — this ensures newly appended content (e.g. pending user bubble, streaming events) is visible without requiring manual scroll-down.

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
    for wt in &app.worktrees { ... }  // O(worktrees) per frame
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
- Call `app.invalidate_sidebar()` when worktrees, selection, or expansion changes:
  - `src/app/state/sessions.rs` - selection, expansion, file navigation
  - `src/app/state/claude.rs` - running_sessions changes
  - `src/app/state/load.rs` - worktrees list changes

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
- **Incremental renders clone only NEW events** — `pre_scan_events()` scans already-rendered events on the main thread (zero-cost reads, no allocation), then only `display_events[rendered_events_count..]` is cloned for the render thread. `PreScanState` carries the pre-computed flags. Full renders (width change, initial load) still clone all events but happen rarely.
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
- `last_render_submit: Instant` — throttle: min 50ms between submits to batch streaming events

**Evolution:** Previous iterations moved rendering outside `terminal.draw()` (commit 8834050) and split render/draw into separate loop iterations (commit 5192228), but those were still synchronous. The background thread makes rendering fully asynchronous.

**Files:** `src/tui/render_thread.rs` (RenderThread struct, request/result types), `src/tui/draw_output.rs` (`submit_render_request()`, `poll_render_result()`), `src/tui/event_loop.rs` (submit/poll integration), `src/app/state/app.rs` (render_thread fields)

**Startup sequence** (`src/tui/run.rs::run`): `draw_splash()` → `App::new()` → `app.load()` → `app.load_session_output()` → `event_loop::run_app()`. The splash screen renders immediately after terminal init (before any App state exists) so the user sees the AZUREAL logo with a dim spring azure butterfly outline (the app mascot) in the background while git discovery and session parsing run. Two-layer rendering: butterfly background using outlined wings (box-drawing chars + `░` fill, `║` body column, `╱╲` antennae) at `Color::Rgb(15, 45, 80)`, then logo + half-block acronym + "Loading project..." on top overwriting butterfly cells where they overlap. Both layers share the same vertical center point so butterfly wings extend above and below the text. Displayed for minimum 3 seconds. The first `ui()` draw in `run_app()` replaces the splash with the full layout.

### Vim-Style Input Mode

The input box uses vim-style modal editing:
- **Command mode** (red border): Keys are commands, not text input
- **Prompt mode** (yellow border): Keys are typed as Claude prompts

**Rationale:** Allows single-letter commands like 't' for terminal toggle without conflicting with text input. The red border in command mode provides immediate visual feedback that typing will execute commands, preventing accidental command execution.

Key mappings:
- `p` (global, except edit mode): Enter prompt mode and focus input (closes terminal/help if open)
- `T` (Shift+T, global, except edit mode): Toggle terminal pane
- `G` (Shift+G, global, except edit mode): Toggle Git panel

**CRITICAL: All keybinding guards are centralized in `lookup_action()`.** The skip logic in `lookup_action()` prevents single-key globals (`p`, `T`, `G`, `?`, `Tab`, `Shift+Tab`) from firing during text input, edit mode, terminal mode, sidebar filter, or wizard. `⌘C` is skipped in edit mode so the edit handler owns clipboard. Tab/Shift+Tab skipped in edit mode, help overlay, and wizard. **NEVER add guard conditions in event_loop.rs or input handlers** — add them to the skip match in `lookup_action()` instead.
- `Escape` / click another pane / `Tab` (in prompt mode): Return to command mode
- `Enter` (in prompt mode): Submit prompt. If Claude is already running, a single Enter cancels the current run and auto-sends the new prompt once the process exits (via `staged_prompt` mechanism — no second Enter needed)

Multi-line input is supported via Shift+Enter. The Kitty keyboard protocol is enabled on startup via `PushKeyboardEnhancementFlags` (DISAMBIGUATE + REPORT_EVENT_TYPES). We intentionally omit `REPORT_ALL_KEYS_AS_ESCAPE_CODES` because it causes Shift+letter to arrive as `(SHIFT, Char('1'))` instead of `(NONE, Char('!'))`, breaking secondary character input. With DISAMBIGUATE alone, Shift+Enter sends `CSI 13;2u` → `(SHIFT, Enter)`, which is sufficient. An `(ALT, Enter)` arm is kept as a safety net for Kitty-macOS edge cases. Release events are dropped; both Press and Repeat are processed (Repeat fires when a key is held down, enabling fast cursor movement with held arrow keys). The input field dynamically grows in height (up to 3/4 of terminal height) with proper cursor positioning for newlines and character-level wrapping. When content exceeds the visible area, the view scrolls to keep the cursor visible.

**CRITICAL: Uppercase letter keybinding matching.** Without `REPORT_ALL_KEYS`, shifted letters arrive inconsistently: `(NONE, Char('G'))`, `(SHIFT, Char('G'))`, or `(SHIFT, Char('g'))` depending on terminal. `KeyCombo::matches()` handles all three by accepting: `(SHIFT, any_case)` → uppercase match, or `(NONE, uppercase_only)` → match. Plain lowercase `(NONE, Char('g'))` is explicitly rejected to avoid `g` triggering a `Shift+G` binding. Always use `KeyCombo::shift(KeyCode::Char('T'))` for uppercase bindings — the match logic is centralized.

**Pre-wrapped input rendering:** The input Paragraph does NOT use ratatui's `.wrap()`. Instead, `build_wrapped_content()` pre-wraps text at word boundaries (one `Line` per visual row) and computes cursor position in the same pass. Word-wrap break points are computed by `word_wrap_break_points()` which prefers breaking at the last space before the width limit, falling back to hard char-boundary break when a single word exceeds the width. This guarantees cursor math and text layout always agree. All 6 locations that interact with input wrapping share `word_wrap_break_points()` from `draw_input.rs`: `build_wrapped_content()` (rendering + cursor), `fast_draw_input()` (fast-path rendering), `compute_cursor_row_fast()` (scroll offset), `click_to_input_cursor()` (mouse click), `screen_to_input_char()` (mouse drag), and `row_col_to_char_index()` (shared visual→char mapping). The `display_width()` helper computes unicode display width of char slices for accurate cursor column positioning.

**Theme-independent text color:** Input text is forced to `Color::White` regardless of terminal color scheme — applied in both `build_wrapped_content()` (`normal_style`) and `fast_draw_input()` (`SetForegroundColor(White)`). This ensures consistent visibility on light and dark terminal backgrounds.

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
- `T` (Shift+T, global command mode): Toggle terminal; `t` (terminal command mode): Enter type mode
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

**ALL keybindings are defined once** in `src/tui/keybindings.rs`. The `lookup_action()` function is the **SINGLE source of truth** for key → action resolution. Input handlers only receive keys that `lookup_action()` returned `None` for (text input, dialog nav, etc.). **Modal panels** (Health, Git, Projects, pickers, branch dialog) use per-modal lookup functions that resolve keys to the same `Action` enum — draw functions source hint labels from keybinding arrays via hint generators, never hardcoded strings.

**Architecture:**
- `Action` enum: All possible keybinding actions (~60 variants: navigation, editing, viewer tabs, file tree operations, modal-specific actions like `HealthSwitchTab`, `GitSquashMerge`, `ProjectsAdd`, `PickerQuickDelete`, `BranchDialogOpen`, `BrowseMain`, etc.)
- `KeyCombo`: Key + modifier combination with display helpers
- `Keybinding`: Primary key, alternatives (j/↓), description, action, `pair_with_next` (merges with next binding on one help line — for counterpart pairs like up/down, next/prev)
- `KeyContext`: Captures all guard state from App (focus, prompt_mode, edit_mode, terminal_mode, filter_active, wizard_active, help_open). Built via `KeyContext::from_app(app)`.
- Static arrays per context: `GLOBAL`, `WORKTREES` (15 entries — includes `m` for BrowseMain), `FILE_TREE`, `VIEWER`, `EDIT_MODE`, `OUTPUT`, `INPUT`, `TERMINAL`, `WIZARD`, `HEALTH_SHARED` (9 entries — `Tab:tab` top-left and `s:scope` top-right of panel border), `HEALTH_GOD_FILES` (4 entries — god-files-specific keys), `HEALTH_DOCS`, `GIT_ACTIONS` (13 entries — 3 context-aware actions + navigation/focus/refresh/diff), `PROJECTS_BROWSE`, `PICKER`, `BRANCH_DIALOG`
- Guard logic lives **inside** `lookup_action()` — skip conditions prevent globals from firing during text input, edit mode, terminal mode, filter, or wizard. No guard duplication in event_loop.rs.
- **Per-modal lookup functions:** `lookup_health_action(tab, mods, code)`, `lookup_git_actions_action(actions_focused, is_on_main, mods, code)`, `lookup_projects_action(mods, code)`, `lookup_picker_action(mods, code)`, `lookup_branch_dialog_action(mods, code)` — each checks its modal's arrays and returns `Option<Action>`
- **Hint generators:** `health_god_files_hints()`, `health_docs_hints()`, `git_actions_labels()`, `git_actions_footer()`, `projects_browse_hint_pairs()`, `picker_title()`, `dialog_footer_hint_pairs()` — draw functions call these instead of hardcoding footer/hint strings
- `execute_action()` in `event_loop.rs` dispatches all actions to their side effects
- Global, Terminal, and Input bindings shown in title bars only (not in help panel) via title functions
- Modal panels with visible footer hints (Health, Git, Projects) are excluded from the help panel — their keys are already self-documenting in the panel UI
- Title functions return `(short_label, full_title, hints)` tuples: `prompt_type_title()`, `prompt_command_title()`, `terminal_type_title()`, `terminal_command_title()`, `terminal_scroll_title()`. `split_title_hints()` packs as many hint segments as fit on the top border after the mode label, then puts remaining segments on the bottom border wrapped in parentheses via ratatui's `.title_bottom()`. Bottom title uses the same style as the top (border color + bold when focused). No content shifting or padding needed.

**Resolution flow in `handle_key_event()` (event_loop.rs):**
1. Modal overlays (help, wizard, projects, health, git, pickers, branch dialog, session list) intercept ALL input first — each modal uses its per-modal lookup function
2. `KeyContext::from_app(app)` + `lookup_action()` resolves key → action for main views
3. If action found → `execute_action()` dispatches it (except input-specific actions like Submit/InsertNewline/ToggleStt which fall through to handle_input_mode when `Focus::Input`)
4. If `None` → focus-specific handler processes unresolved keys (text editing, dialog nav, sidebar filter)

**Input handlers only handle unresolved keys:**
- `input_viewer.rs` — tab dialog, save dialog, discard dialog, edit mode text editing
- `input_output.rs` — session list overlay input
- `input_file_tree.rs` — clipboard mode (Copy/Move paste target), text-input actions (Add, Rename, Delete confirmation)
- `input_worktrees.rs` — file tree overlay routing, sidebar filter text input, 's' stop-tracking
- `input_health.rs` — `lookup_health_action()` → Action match (tab switching, panel-level keys like scope, per-tab keys)
- `input_git_actions.rs` — `lookup_git_action()` → Action match (git ops, file nav, commit overlay text editing)
- `input_projects.rs` — `lookup_projects_action()` → Action match (browse mode only; text input stays raw)
- `input_dialogs.rs` — `lookup_branch_dialog_action()`, `lookup_picker_action()` → Action matches; text input and number quick-select stay raw

**macOS ⌥+letter gotcha:** On macOS, `Option+letter` produces Unicode characters (e.g., `⌥c` → `ç`, `⌥r` → `®`), so crossterm sees `KeyCode::Char('ç')` with `KeyModifiers::NONE` — NOT `ALT + 'c'`. For keybindings that use `⌥+letter`, add the unicode char as an alternative via `with_alt()` and `ALT_MACOS_R` style statics (e.g., `⌥r` has `®` as alternative). `macos_opt_key()` maps all 26 unicode chars back to their letter for runtime lookups. `⌥+arrow` keys work fine since arrows don't produce Unicode. In text input modes, prefer `⌃+letter` (Ctrl) instead since those send real control codes. **Help panel display:** `display_keys()` filters out non-ASCII bare-char alternatives (®, π, †) so the help panel shows clean `⌥r` instead of `⌥r/®` — the unicode chars are internal matching details, not user-facing.

**input_cursor is a CHAR INDEX, not a byte offset.** `String::insert()` and `String::remove()` take byte offsets. Use `char_to_byte(char_idx)` to convert before calling them. Comparing `input_cursor` against `String::len()` (bytes) is wrong — use `.chars().count()` instead. See `src/app/input.rs`.

**Enforcement hooks:** `.claude/scripts/enforce-keybindings.sh` runs as a PreToolUse hook on every Edit/Write. Catches 3 violations: (1) raw `KeyCode::`/`KeyModifiers::` in `input_*.rs` (must use `lookup_*_action()`), (2) hardcoded key label strings in `draw_*.rs` without `keybindings::` import (must use hint generators), (3) new static binding arrays in `keybindings.rs` without companion lookup/hint functions. Configured in `.claude/settings.json`.

Implementation: `src/tui/keybindings.rs` (KeyContext, Action enum (~60 variants incl GitPull, BrowseMain), ~17 static arrays (WORKTREES: 15 entries), lookup_action() + 6 per-modal lookup fns (lookup_git_actions_action takes is_on_main), guard logic, help_sections(), title generators, hint generators), `src/tui/event_loop/actions.rs` (execute_action(), dispatch helpers, BrowseMain handler, read-only guards, Esc browse-mode exit), `src/tui/draw_dialogs.rs::draw_help_overlay()` (uses `keybindings::help_sections()`), `.claude/scripts/enforce-keybindings.sh` (PreToolUse enforcement hook)

### Wrap-Aware Edit Cursor

The viewer edit mode cursor navigates wrapped visual lines, not just source lines. Long lines wrap at `content_width = viewport_width - line_num_width - 3` characters. The wrap width is cached in `app.viewer_edit_content_width` (set by `draw_edit_mode()`).

**Word-boundary wrapping:** Both read-only and edit modes use `textwrap::wrap()` for word-boundary wrapping. The `word_wrap_breaks(text, max_width)` function returns `Vec<usize>` of char offsets where each visual row starts. All cursor math uses these break positions instead of fixed-width `col / cw` assumptions.

**Up/Down navigation:** `viewer_edit_up()` / `viewer_edit_down()` call `word_wrap_breaks()` to find which wrap row the cursor is on. Moving up from wrap_row > 0 stays on the same source line; from wrap_row 0 it jumps to the previous source line's last wrap row. Same logic in reverse for down. The visual column offset from the break position is preserved across wrap rows.

**Scroll-to-cursor:** `viewer_edit_scroll_to_cursor()` sums `word_wrap_breaks().len()` for all source lines before the cursor line, adds the cursor's wrap offset, and scrolls the viewport to keep that visual line visible.

**Mouse click/drag:** `screen_to_edit_pos()` maps screen coordinates to `(source_line, source_col)` by walking source lines and summing their wrap counts (via `word_wrap_breaks()`) until the clicked visual row is found. Click column mapped through break positions to get correct char offset. Stored as drag anchor with pane_id=3 for edit-mode drag selection.

**Display wrapping:** `wrap_spans_word()` wraps styled spans using word-boundary break positions from `word_wrap_breaks()`. Used by both read-only viewer and edit mode display. `word_wrap_breaks()` is `pub(crate)` in `draw_viewer.rs` and duplicated privately in `viewer_edit.rs` (app module can't import from tui).

Implementation: `src/app/state/viewer_edit.rs` (cursor movement, scroll, local `word_wrap_breaks()`), `src/tui/event_loop/coords.rs` (`screen_to_edit_pos()`), `src/tui/event_loop/mouse.rs` (pane_id=3 drag handling), `src/tui/draw_viewer.rs` (`word_wrap_breaks()`, `wrap_spans_word()`, caches `content_width`)

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
| Edit | Clickable link + inline diff (last 20) | File path underlined; click to view in Viewer with diff overlay. Last 20 Edit calls show inline diff: removed lines in grey on dim red bg (no syntax highlighting), added lines syntax-highlighted on dim green bg, with actual file line numbers |
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

**Debug Output:**
`⌃D` opens a naming dialog, then dumps diagnostic output to `.azureal/debug-output[_name]`. Enter with empty name saves as `debug-output`; typing a name saves as `debug-output_<name>` (e.g., `debug-output_scroll_bug`). Esc cancels. A "Saving…" dialog is shown while the dump I/O runs (two-phase: draw dialog first, run dump next frame) so the app doesn't appear frozen on large sessions. All user/assistant message content, file paths, and rendered conversation text are **obfuscated** via deterministic word replacement (same word → same fake word) so the file can be safely attached to GitHub issues without exposing sensitive project details. Tool names, event types, parsing stats, and structural markers are preserved for diagnostic value. Contains: parsing stats, event type breakdown, last 5 events (obfuscated previews), and full rendered output (obfuscated).

**Markdown Rendering:**
Claude responses are parsed for markdown syntax and rendered with proper styling:
- `# H1`, `## H2`, `### H3` headers → styled with block chars (█, ▓, ▒) and colors, prefix removed
- `**bold**` → bold text without markers
- `*italic*` → italic text without markers
- `` `inline code` `` → yellow text on dark background
- ``` code blocks ``` → box-drawn borders with language label
- `| table | rows |` → box-drawing characters (│, ├, ┼, ┤), column widths clamped to fit bubble width (cells truncated with `…` when too wide)
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

### Token Usage Counter

Color-coded context window usage percentage displayed on the Convo pane's right border title. Helps users predict when context compaction will occur.

**Data source:** Claude's JSONL session files already contain `message.usage` on every assistant event with `input_tokens`, `output_tokens`, `cache_read_input_tokens`, and `cache_creation_input_tokens`. No external tokenization library needed — exact counts from the API.

**Metric:** `input_tokens + cache_read_input_tokens + cache_creation_input_tokens` = effective context size. Each assistant event overwrites the previous — the last one reflects the most recent context window consumption.

**Color thresholds:**
| Range | Color | Meaning |
|-------|-------|---------|
| 0–59% | Green | Plenty of context remaining |
| 60–79% | Yellow | Context getting full |
| 80–100% | Red | Near compaction threshold |

**Context window detection (two-tier):**
1. **Authoritative (result event):** The `result` event's `modelUsage.<model_id>.contextWindow` field provides the exact context window from the API. Extracted in both `parse_result_event()` (session file) and `handle_claude_output()` (live stream). This is the definitive value.
2. **Heuristic fallback (assistant event):** `context_window_for_model()` in `src/app/session_parser.rs` maps model ID prefixes to their standard context window (currently 200k for all). Used as an early estimate before the first `result` event fires. Only sets `model_context_window` if it's still `None` (won't override the authoritative value).
3. **Auto-bump to 1M:** If actual token usage exceeds the detected window, the display logic in `draw_output.rs` bumps to 1M (1M beta detection).

Stored as `model_context_window: Option<u64>` on App state — `None` until first event is parsed.

**Data flow:**
1. **Session file parse:** `parse_assistant_event()` in `src/app/session_parser.rs` extracts `message.usage` → `ParsedSession.session_tokens` and `message.model` → `ParsedSession.context_window` (heuristic). `parse_result_event()` extracts `modelUsage.*.contextWindow` → `ParsedSession.context_window` (authoritative, overwrites heuristic).
2. **Load propagation:** `load_session_output()` and `refresh_session_events()` in `src/app/state/load.rs` copy to `app.session_tokens` and `app.model_context_window`, then call `update_token_badge()`
3. **Live stream:** `handle_claude_output()` in `src/app/state/claude.rs` extracts usage + model from assistant events (heuristic) and contextWindow from result events (authoritative), then calls `update_token_badge()`
4. **Badge cache:** `update_token_badge()` in `src/app/state/app.rs` precomputes `(String, Color)` from session_tokens + model_context_window. Only called when token data changes — draw path reads the cached value with zero computation
5. **Display:** `draw_output_pane()` in `src/tui/draw_output.rs` reads `token_badge_cache` and renders as right-aligned spans before PID/exit code

**Reset:** `session_tokens` and `model_context_window` cleared to `None` on session switch (in `load_session_output()`). Badge hidden when no token data available.

Implementation: `session_tokens: Option<(u64, u64)>`, `model_context_window: Option<u64>`, `token_badge_cache: Option<(String, Color)>` in `src/app/state/app.rs`, `update_token_badge()` method, `context_window_for_model()` in `src/app/session_parser.rs`, display in `src/tui/draw_output.rs`

### TodoWrite Sticky Widget

Claude's `TodoWrite` tool calls are parsed from session JSONL and rendered as a persistent checkbox widget at the bottom of the Convo pane instead of inline generic tool call JSON. The widget stays visible as the user scrolls through conversation history and hides when all todos are completed. When a subagent (Task tool) is active, its TodoWrite calls render as indented subtasks directly beneath the parent todo item (the in-progress item when the Task spawned), tracked via `subagent_parent_idx`, and prefixed with `↳`. Subagent todos are cleared when the Task tool completes.

**Height cap and scrollbar:** The widget grows to fit its content but caps at 20 visual lines (including wrapped text). When content exceeds 20 lines, a scrollbar column appears on the rightmost border position (AZURE `█` thumb on `│` track) and the widget responds to mouse wheel scrolling. Scroll offset (`todo_scroll`) resets to 0 whenever todos are updated (new TodoWrite tool call). The `pane_todo` rect is cached during draw for mouse hit-testing — checked before `pane_convo` in `apply_scroll_cached()` since the todo widget overlaps the convo area.

**Status icons:**
| Icon | Color | Meaning |
|------|-------|---------|
| ✓ | Green | Completed |
| ● | Yellow (pulsating) | In progress |
| ○ | Dim gray | Pending |

In-progress items show their `activeForm` text (present tense, e.g., "Building project"), while pending/completed items show `content` (imperative, e.g., "Build project").

**Data flow:**
1. **Live stream:** `handle_claude_output()` in `src/app/state/claude.rs` detects `TodoWrite` ToolCall events and routes them: if a Task tool is active (`active_task_tool_ids` non-empty), todos go to `app.subagent_todos` and `subagent_parent_idx` is set to the index of the current in-progress item; otherwise to `app.current_todos`. Task tool calls are tracked via `active_task_tool_ids` — when the last Task completes, subagent todos are cleared.
2. **Session load:** `extract_skill_tools_from_events()` in `src/app/state/load.rs` scans all display_events forward to find the latest TodoWrite and restore todo state
3. **Session switch:** `current_todos` cleared on session switch and rebuilt from new session's events
4. **Rendering:** `draw_todo_widget()` in `src/tui/draw_output/todo_widget.rs` splits the convo area with `Layout::vertical()` — scrollable content above, sticky todo box below. Height capped at 22 rows (20 content + 2 borders); when content overflows, accepts `scroll` offset and renders a proportional scrollbar on the rightmost column via direct buffer writes

**Lifecycle:** Widget stays visible even after all items are completed (showing all checkmarks). It clears when the user submits their next prompt (`current_todos.clear()` in the Enter handler). This ensures the user sees the final completed state before it disappears.

**Inline suppression:** TodoWrite tool calls and their results are suppressed from the inline convo stream (`render_display_events()` skips them). The sticky widget is the only representation.

Implementation: `TodoItem` struct + `TodoStatus` enum in `src/app/state/app.rs` (includes `subagent_todos`, `active_task_tool_ids`, `pane_todo`, `todo_scroll`, `todo_total_lines` fields), `parse_todos_from_input()` in `src/app/state/claude.rs`, `draw_todo_widget()` in `src/tui/draw_output/todo_widget.rs` (renders subtasks beneath parent item via `subagent_parent_idx` with `↳` prefix, scroll offset, scrollbar column), mouse scroll routing in `src/tui/event_loop/mouse.rs` (`pane_todo` hit-test before `pane_convo`), suppression in `src/tui/render_events.rs`

### AskUserQuestion Options Box

Claude's `AskUserQuestion` tool calls are parsed from session JSONL and rendered as a numbered options box (similar to plan approval prompts) instead of raw JSON. The user responds by typing a number or custom text.

**Rendering:** A magenta-bordered box per question with the question header, numbered options (label + description), and an implicit "Other" option at the end. Multi-select questions are annotated. Rendered inline in the convo stream when the tool result arrives (positioned after the result, before user response).

**Input handling:** When `awaiting_ask_user_question` is true, the user's response gets a hidden system context prefix (`build_ask_user_context()` in `src/tui/input_terminal.rs`) listing the questions and numbered options that were shown. This lets Claude interpret "1", "2", etc. as option selections. The context is invisible to the user — they just see their typed response.

**State tracking:**
- `awaiting_ask_user_question: bool` — set when AskUserQuestion ToolCall detected, cleared on user submit
- `ask_user_questions_cache: Option<serde_json::Value>` — cached input JSON for building context prefix
- `saw_ask_user_question` / `saw_user_after_ask` in render pipeline for conditional box display

**Session load:** `extract_skill_tools_from_events()` tracks whether the last AskUserQuestion was answered by scanning for a subsequent UserMessage. If unanswered, restores the awaiting state.

Implementation: `render_ask_user_question()` in `src/tui/render_events.rs`, `build_ask_user_context()` in `src/tui/input_terminal.rs`, state in `src/app/state/app.rs`

### Session Search/Filter

Press `/` in the Worktrees pane to activate a search filter. Type to narrow the sidebar (case-insensitive substring match). The filter searches across project names and worktree display names (branch name without `azureal/` prefix). Matching items are shown with their parent hierarchy preserved — e.g. a matching worktree appears under its project header even if the project name doesn't match the filter.

**Hierarchy rules:**
- **Project name matches** → all worktrees shown (no filtering below)
- **Worktree name matches** → that worktree shown
- **No match** → worktree hidden entirely

**Keybindings (while filter is active):**
- Type characters — appended to filter, sidebar updates live
- `Backspace` — remove last char (auto-deactivates when empty)
- `Esc` — clear filter and deactivate
- `Enter` — accept filter (keep text visible, exit filter input mode)
- `↑/↓` — navigate filtered results while typing

**Selection tracking:** When the filter changes, if the current selection doesn't match, it auto-snaps to the first matching worktree. `j/k` navigation skips filtered-out worktrees via `snap_selection_to_filter()`.

**Global key suppression:** While `sidebar_filter_active` is true, global single-letter bindings (`p`, `T`, `G`, `?`, `D`) are suppressed so typed chars go to the filter input. Tab/Shift+Tab clear the filter before cycling focus.

**Rendering:** `build_sidebar_items()` filters worktrees by match, then builds the item list showing only matching items with parent project context. A 3-line filter bar (borders + text) is rendered above the worktree list via `Layout::vertical()` split. The filter bar shows yellow border when active, dim gray when accepted. Match count (visible worktrees) shown as right-aligned title (e.g., ` 3/12 `).

Implementation: `sidebar_filter: String`, `sidebar_filter_active: bool` in `src/app/state/app.rs`, `snap_selection_to_filter()` in `src/app/state/sessions.rs`, hierarchical filter logic in `src/tui/draw_sidebar.rs`, input handling in `src/tui/input_worktrees.rs`, global key guards in `src/tui/keybindings.rs` (`lookup_action()`)

### Speech-to-Text Input

Press `⌃s` in prompt mode or file edit mode to toggle speech recording. Audio is captured via cpal (CoreAudio on macOS), transcribed locally via whisper.cpp with Metal GPU acceleration, and inserted at the cursor position. In edit mode, text goes into the viewer edit buffer; in prompt mode, into the prompt input field.

**Architecture:**
- Background thread (`stt_loop`) blocks on `mpsc::recv()` when idle (zero CPU)
- Same pattern as RenderThread and Terminal PTY: mpsc channels, `try_recv()` polling in event loop
- `SttHandle` lazy-initialized on first `⌃s` press (no resources allocated until needed)
- `WhisperContext` lazy-loaded on first transcription and cached for reuse

**Audio pipeline:**
1. cpal callback pushes `f32` samples to `Arc<Mutex<Vec<f32>>>` (~10μs lock per callback)
2. Device sample rate captured from default input config (typically 44100 or 48000 Hz)
3. Stereo/multi-channel audio mixed down to mono in the callback
4. On stop: samples drained, resampled to 16kHz mono via linear interpolation
5. Whisper transcription with `Greedy { best_of: 1 }`, single-segment mode
6. Transcribed text inserted at cursor with smart space handling

**Visual feedback:**
- Recording: magenta border + `REC` prefix in input title
- Transcribing: magenta border + `...` prefix in input title
- Status bar shows progress messages (Recording..., Transcribing Xs of audio..., Loading Whisper model...)

**Model:** `~/.azureal/speech/ggml-base.en.bin` (~142MB). If missing, status bar shows download instructions:
```bash
mkdir -p ~/.azureal/speech && curl -L -o ~/.azureal/speech/ggml-base.en.bin \
  https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.en.bin
```

**Event loop integration:**
- `poll_stt()` called every iteration when `stt_handle` exists
- Events collected into Vec first (avoids borrow conflict: `try_recv` borrows handle, processing borrows `&mut self`)
- Short poll timeout (16ms) when `stt_recording || stt_transcribing`

Implementation: `src/stt.rs` (engine), `stt_handle`, `stt_recording`, `stt_transcribing` in `src/app/state/app.rs`, `toggle_stt()`, `poll_stt()`, `insert_stt_text()` methods, `⌃s` binding in `src/tui/keybindings.rs` (`EDIT_MODE` and `INPUT` arrays), `Action::ToggleStt` dispatched via `execute_action()` in `actions.rs` (edit mode) and raw match in `handle_input_mode()` (prompt mode), polling in `src/tui/event_loop.rs`, visual feedback in `src/tui/draw_input.rs` and `src/tui/draw_viewer.rs` (edit mode magenta border + REC indicator)

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

Implementation: `find_latest_claude_session()`, `list_claude_sessions()` in `src/config.rs`, `load_worktrees()` in `src/app/state/load.rs`

**Fixed Bug: tool_use ID Collision**
Previously when using `-p --resume` with parallel tool calls, Claude Code 2.1.19 would return "tool_use ids must be unique" error (GitHub issues #20508, #20527, #13124).

**Status:** Fixed in Claude Code 2.1.22. All resume + tools combinations now work correctly.

### Worktree Health Panel

Tabbed modal overlay (`Shift+H` toggles open/close, global keybinding) housing multiple health-check systems. Green accent color (`Rgb(80,200,80)`, `GF_GREEN` constant) with QuadrantOutside border. Centered modal (55% × 70%, min 50×16). Bold title: `" Worktree Health "`. Reopens on the last-visited tab (`last_health_tab` in App state, defaults to God Files).

**Tab Bar:**
Row 0 inside border: `[ God Files ]  [ Documentation ]` — active tab bright green + bold, inactive dim gray. Tab key switches between tabs.

**God Files Tab:**
Scans the project for "god files" — source files exceeding 1000 lines. Same checkbox list as the old standalone panel.

*Scanning:*
- **Source-root detection:** If well-known source directories exist under the project root (`src/`, `lib/`, `crates/`, `cmd/`, `pkg/`, `internal/`, `app/`, `core/`, `common/`, `modules/`, `services/`, `packages/`, `components/`, `Sources/`, `include/`, `source/`), ONLY those directories are scanned (plus top-level files). Falls back to full project root if none found.
- **Skip directories:** Hidden dirs, build artifacts, dependency caches, IDE dirs, non-source content (~55 common non-source directories). Case-insensitive matching.
- Source extensions (~60): Systems, JVM, .NET, Web, Scripting, Functional, Shell, Infra/Query, Schema.
- Threshold: >1000 LOC. Results sorted by line count descending.
- Synchronous scan — fast enough for typical projects.

*Keybindings (God Files tab):*
- `j/↓`, `k/↑` — navigate; `J/K` — page scroll (page size = `screen_height` minus chrome, NOT the embedded terminal pane's `terminal_height`); `⌥↑/⌥↓` — jump top/bottom
- Mouse wheel scrolls the list (modal intercepts all scroll events)
- `Space` — toggle check; `a` — toggle all
- `v` — view checked files as Viewer tabs (up to 12)
- `Enter`/`m` — modularize checked files
- `Tab` — switch to Documentation tab
- `Esc` — close panel

*Scope Mode (`s` — panel-level, accessible from any tab):*
`s` is a shared health panel keybinding (in `HEALTH_SHARED`, displayed as `s:scope` in the panel border top-right; `Tab:tab` mirrors it on the top-left). Opens the FileTree overlay in scope mode with green highlights on directories in the scan scope. Subdirectories of accepted dirs automatically inherit accepted status (bright green). Files inside scoped dirs dimmed green; everything else dimmed gray. Green double-line border with `" Health Scope (N dirs) "` title. Enter toggles dirs in/out of scope. Esc persists scope to project azufig.toml `[healthscope].dirs` (alias `[godfilescope]` for backward compatibility via `#[serde(alias = "godfilescope")]`), rescans both god files and documentation, and reopens the health panel with updated results. Scope auto-loaded on panel open.

*Module Style Selection:*
When checked files include `.rs` or `.py`, pressing Enter/m shows a **module style selector** dialog before spawning. The dialog lets users choose between dual-style module conventions:
- **Rust**: File-based root (`modulename.rs` + `modulename/`, modern) vs directory module (`modulename/mod.rs`, legacy)
- **Python**: Package (`__init__.py` directory) vs single-file modules

The choice is embedded in each file's modularization prompt. Languages without dual-style conventions (Go, Java, TypeScript, etc.) skip the dialog entirely. `Space` toggles between styles, `j/k` navigates between language rows (when both Rust and Python are checked), `Enter` confirms and spawns, `Esc` cancels back to the file list.

*Parallel Modularization:*
All checked files spawned simultaneously as concurrent Claude processes on the current worktree. Each session named `[GFM] <filename>`. Changes merge back to main via squash-merge.

**Documentation Tab:**
Scans all source files for documentation coverage — counts documentable items (`fn`, `struct`, `enum`, `trait`, `const`, `static`, `type`, `impl`, `mod`) and checks whether each has a preceding `///` or `//!` doc comment. Line-based heuristic, no AST parsing.

*Display:*
- `[DH] (Documentation Health)` session naming hint at top
- Overall score header: `Overall Documentation Score: XX.X%` color-coded (green ≥80%, yellow ≥50%, red <50%) with file count
- Per-file list sorted by coverage ascending (worst-documented first) with `[x]/[ ]` checkboxes
- Each row: checkbox, file path, coverage percentage, visual bar (`█░` blocks), documented/total ratio
- Selected row highlighted in green; checked count shown in header

*Keybindings (Documentation tab):*
- `j/↓`, `k/↑` — navigate; `J/K` — page scroll (page size = `screen_height` minus chrome, NOT the embedded terminal pane's `terminal_height`); `⌥↑/⌥↓` — jump top/bottom
- Mouse wheel scrolls the list (modal intercepts all scroll events)
- `Space` — toggle check on selected file
- `a` — check all non-100% files (toggle: if all non-100% checked, unchecks them)
- `v` — view checked files as Viewer tabs (up to 12)
- `Enter` — complete checked files (spawns concurrent [DH] Claude sessions, one per file)
- `Tab` — switch to God Files tab
- `Esc` — close panel

*[DH] Session Spawning:*
Checked files spawn concurrent Claude sessions on the current worktree, each prefixed `[DH] filename`. The prompt instructs Claude to add `///` and `//!` doc comments to all undocumented items without modifying executable code. Shows current documented/total ratio so Claude knows the starting coverage. Changes merge back to main via squash-merge.

Implementation: `src/app/state/health.rs` (module root: shared constants, open/close panel, scope persistence via `load_health_scope()` / `save_health_scope()`, `AzufigHealthScope` struct), `src/app/state/health/god_files.rs` (scan, scope mode, modularize, module style selector, view_checked), `src/app/state/health/documentation.rs` (doc scanner, DH session spawning, toggle/view), `src/tui/input_health.rs` (uses `lookup_health_action()` → Action match; `HealthScopeMode` action handled as panel-level, not tab-specific), `src/tui/draw_health.rs` (panel rendering with tab bar, `Tab:tab` label top-left + `s:scope` label top-right in panel border, footer hints from `keybindings::health_god_files_hints()` / `health_docs_hints()`), `src/app/types.rs` (GodFileEntry, HealthPanel, HealthTab, DocEntry), `src/tui/keybindings.rs` (HEALTH_SHARED (9 entries, includes scope) + HEALTH_GOD_FILES (4 entries) + HEALTH_DOCS arrays, `lookup_health_action()`, hint generators, `Shift+H` in GLOBAL)

### Git Panel

Centered modal overlay (`Shift+G` toggles open/close, global keybinding) providing common git operations and a changed-files browser. Uses Git brand orange (#F05032, `GIT_ORANGE` constant) for border styling with QuadrantOutside (`▛▀▜▌ ▐▙▄▟`) border type and Git brown (#A0522D, `GIT_BROWN` constant) for secondary text (headers, key hints, separators, footer). Accessible from any pane (skipped in prompt mode, edit mode, terminal mode, filter, wizard).

**Panel Layout:**
- Title bar: `" Git: <branch_name> "` bold centered in orange QuadrantOutside border
- Actions section: 3 context-aware git operations with single-key shortcuts, navigable with j/k
- Changed files section: working tree changes (staged + unstaged vs HEAD) + untracked files, underlined paths
- Result message area: green (success) or red (error) after each operation
- Footer: `Tab:switch  Enter:exec/view  R:refresh  Esc`

**Context-Aware Actions (when actions section focused):**
Actions change based on whether the current worktree is the main/master branch or a feature branch. `is_on_main: bool` on `GitActionsPanel` determines which set is shown, set by comparing `worktree_name == main_branch` in `open_git_actions_panel()`.

*On main branch:*
- `l` / Enter on index 0 — Pull (`exec_pull()`) — pulls latest changes from remote
- `c` / Enter on index 1 — Commit (see below)
- `Shift+P` / Enter on index 2 — Push current branch to remote

*On feature branches:*
- `m` / Enter on index 0 — Squash merge to main (`Git::squash_merge_into_main()`) — pull main → squash merge → commit (see data flow below). Does NOT auto-push; user triggers push separately.
- `c` / Enter on index 1 — Commit (see below)
- `Shift+P` / Enter on index 2 — Push current branch to remote

**Mutual exclusivity guards:** `lookup_git_actions_action()` blocks `GitSquashMerge` when `is_on_main` is true (cannot squash-merge main into itself) and blocks `GitPull` when `is_on_main` is false (pull only available on main). Both also require `actions_focused`.

**File list (when file list focused):**
- Each file shows status char (M=yellow, A=green, D=red, R=cyan, ?=magenta untracked), underlined path, right-aligned `+N/-N` stats (green for additions, red for deletions; orange override when row is selected). Header totals also color-coded green/red.
- `j/k` — navigate files; `Enter`/`d` — view selected file's diff in Viewer pane (closes panel)
- Scroll indicator shown when list overflows

**Global within panel:**
- `Tab` — toggle focus between actions and file list
- `R` — refresh changed file list
- `Esc` — close panel
- Click outside — dismiss (mouse.rs)

**Commit overlay:**
Pressing `c` or Enter on index 1 stages all changes (`git add -A`), gets `git diff --staged` + `git diff --staged --stat`, and spawns `claude -p` as a one-shot background thread to generate a conventional commit message. While generating (~3 sec), the overlay shows "Generating..." with a spinner. The overlay is a centered dialog rendered on top of the Git panel with the generated message in an editable text area. `Enter` commits (deferred with "Committing..." loading indicator), `⌘P` commits + pushes (deferred with "Committing and pushing..." loading indicator), `Shift+Enter` inserts a newline, `Esc` cancels. Both commit actions use the `DeferredAction` two-phase pattern so the loading popup renders before the blocking git operation runs. Full text editing with word-wrap: type, backspace, delete, left/right arrows, up/down line navigation, home/end. Session persistence is disabled via `--no-session-persistence` so no .jsonl file is created. No streaming occurs — uses `std::process::Command` stdout capture. Markdown code fences are stripped from the output. State managed by `GitCommitOverlay` struct on `GitActionsPanel` (`commit_overlay: Option<GitCommitOverlay>`). `ACTION_COUNT` = 3. Confirm-index mapping: 0=pull (main) or squash-merge (feature), 1=commit, 2=push. Commit message receiver polled in event loop with short-poll (250ms) while generating.

**Data flow:** On open, `open_git_actions_panel()` reads `current_worktree().worktree_path` and calls `Git::get_diff_files()` which combines `git diff HEAD --name-status` + `git diff HEAD --numstat` (working tree vs last commit) plus `git ls-files --others --exclude-standard` (untracked files), then filters all paths through `git check-ignore --stdin` to drop tracked-but-gitignored files (e.g., `.DS_Store`). Panel stores `worktree_path`, `repo_root` (project path, always on main), and `main_branch` locally to avoid reborrow conflicts during input handling. After operations that modify the working tree, `refresh_changed_files()` re-scans.

**Data flow (squash-merge-to-main):** `exec_squash_merge()` uses `panel.repo_root` (the main worktree path, always checked out on main) and `panel.worktree_name` (the feature branch name) to call `Git::squash_merge_into_main(repo_root, branch)`. Executes a 3-step cycle from the repo root:
1. `git pull --ff-only` on main — non-fatal if offline (merge continues with local state), fatal if main has diverged (fast-forward required)
2. `git merge --squash <branch>` — collapses all branch commits into staged changes
3. `git commit -m "feat: merge <branch> into main"` — commits the squashed changes

Push is a separate user-triggered action (`Shift+P`). Result message reports merge status with optional "(pull skipped)" suffix if step 1 failed. Reports conflicts with count and auto-merged file count. Returns "Already up to date" when nothing to merge. `get_main_branch()` dynamically detects main/master/HEAD so any naming convention works.

Implementation: `src/tui/input_git_actions.rs` (uses `lookup_git_action()` → Action match; `handle_commit_overlay()` for overlay text editing, `exec_commit_start()` to stage/spawn, `exec_pull()` for pull, `exec_push()` for push; `ACTION_COUNT` = 3, confirm index mapping: 0=pull/squash-merge (context-aware), 1=commit, 2=push), `src/tui/draw_git_actions.rs` (rendering, labels from `keybindings::git_actions_labels(is_on_main)`, footer from `keybindings::git_actions_footer()`, commit overlay dialog with cursor and hint bar), `src/app/state/ui.rs` (open/close methods, `is_on_main` set from `worktree_name == main_branch`, `commit_overlay: None` in panel constructor), `src/git/core.rs` (7 methods: `get_diff_files`, `get_file_diff`, `squash_merge_into_main` (internally runs pull/merge/commit — no push), `stage_all`, `get_staged_diff`, `get_staged_stat`, `commit`), `src/app/types.rs` (GitActionsPanel with `repo_root`, `is_on_main`, `commit_overlay` fields, GitChangedFile, GitCommitOverlay), `src/tui/keybindings.rs` (GIT_ACTIONS array (13 entries), `lookup_git_actions_action()` takes `is_on_main` param with mutual exclusivity guards, `git_actions_labels()` takes `is_on_main` param, hint generators, `GitPull`/`GitPush` actions, `l`/`Shift+P` bindings, `G` in GLOBAL), `src/tui/event_loop.rs` (polls commit message receiver, short-poll when generating)

### Rebase Support (Dead Code)

Legacy rebase infrastructure preserved on disk but no longer compiled or reachable. `src/git/rebase.rs` and `src/tui/input_rebase.rs` still exist but `input_rebase` is removed from the `tui.rs` module list. The rebase view UI code (`src/tui/draw_output/rebase_view.rs`) still compiles but has no entry points. Replaced by squash merge workflow in the Git panel.

Implementation (legacy, unreachable): `src/git/rebase.rs`, `src/tui/draw_output/rebase_view.rs`, `RebaseStatus` in `src/models.rs`

### Run Commands

User-defined shell commands that can be saved and executed from the Worktrees pane. Commands are executed in the embedded terminal. Dual-scope persistence: commands can be **global** (`~/.azureal/azufig.toml` `[runcmds]`, shared across all projects) or **project-local** (`.azureal/azufig.toml` `[runcmds]`); toggle scope with `⌃s` in add/edit dialog; picker shows G/P badge per command.

**Keybindings (from Worktrees pane):**
- `r` — Open picker (if multiple saved commands) or execute directly (if only 1)
- `⌥r` — Open dialog to create a new run command

**Picker overlay:**
- `j/k` / `↑/↓` — Navigate selection
- `1-9` — Quick-select by number
- `Enter` — Execute selected command
- `e` — Edit selected command
- `d` — Delete selected command (y/n confirmation)
- `a` — Add new command

**Dialog overlay:**
- `Tab` — In Name field: advance to Command/Prompt field. In Command/Prompt field: cycle between Command and Prompt modes.
- `⇧Tab` — Go back to Name field from Command/Prompt field
- `⌃s` — Toggle global/project scope (shown as [GLOBAL]/[PROJECT] badge in title bar)
- `Enter` — In Name field: advance. In Command mode: save. In Prompt mode: generate (spawns Claude session).
- `Esc` — Cancel

**Command vs Prompt mode:** The second field has a right-aligned title showing the current mode and Tab hint. In **Command** mode, user types a raw shell command directly. In **Prompt** mode, user types a natural-language description and Enter spawns a new Claude session on the current worktree that reads the description, determines the right shell command, and writes it to `.azureal/runcmds`. The session is named `[NewRunCmd] <name>` in `.azureal/sessions`. Run commands auto-reload when the `[NewRunCmd]` session exits (via `handle_claude_exited()` check on `title_session_name`).

**Storage:** Global commands in `~/.azureal/azufig.toml` `[runcmds]`, project-local in `.azureal/azufig.toml` `[runcmds]` — keys prefixed with 1-based position number: `N_name = "command"` (e.g., `1_Build = "cargo build"`). Prefix preserves quick-select number across restarts; stripped on load, re-written on save. Merged on load (globals first, then locals). Loaded on startup.

Implementation: Types in `src/app/types.rs` (RunCommand, RunCommandDialog, RunCommandPicker, CommandFieldMode), state methods in `src/app/state/ui.rs`, input handling + `spawn_run_command_prompt()` in `src/tui/input_dialogs.rs`, rendering in `src/tui/draw_dialogs.rs`, auto-reload in `src/app/state/claude.rs`

### Projects Panel

Persistent project management across azureal sessions. Projects are stored in `~/.azureal/azufig.toml` `[projects]` section (`DisplayName = "~/path"` pairs). Opened with `P` from Worktrees pane, or shown automatically on startup when not inside a git repo.

**Behavior:**
- When launched inside a git repo, auto-registers the repo in `projects` and loads normally. Display name derived from `git remote get-url origin` (repo name from SSH/HTTPS URL, `.git` stripped); folder name fallback if no remote. `Project::from_path()` reads display name via `project_display_name()` so title bar, sidebar, and terminal title all use it.
- When launched outside a git repo, shows the Projects panel full-screen with "Project not initialized. Press i to initialize or choose another project." message; clears on first keypress
- The sidebar no longer shows a project header row — project name appears in the Worktrees pane border title instead

**Panel Actions:**
- `Enter`: switch to selected project (validates git repo first — shows error if not a valid repo; kills all Claude processes on success, reloads sessions/files)
- `a`: add a new project by path (validates it's a git repo)
- `d`: delete selected project from list (does NOT delete the repo)
- `n`: rename the selected project's display name
- `i`: initialize a new git repo at a specified path (or cwd if blank); rejects paths that are already git repos
- `Esc`: close panel (only if a project is already loaded)
- `⌃Q`: quit azureal

**Project Switching:**
When switching projects, azureal kills all running Claude processes, clears all session/render state (sessions, display events, caches, file watcher), sets the new project via `Project::from_path()`, and reloads sessions, output, and run commands.

**Auto-pruning:** `load_projects()` validates every entry on load — directories that don't exist or aren't git repos are silently removed from `projects`. This prevents ghost entries after a repo's `.git` directory is deleted.

Implementation: `src/config.rs` (persistence: `load_projects()`, `save_projects()`, `register_project()`, `project_display_name()`, `repo_name_from_origin()`), `src/app/types.rs` (`ProjectsPanel`, `ProjectsPanelMode`), `src/tui/draw_projects.rs` (rendering), `src/tui/input_projects.rs` (key handling), `src/app/state/ui.rs` (`switch_project()`, `cancel_all_claude()`)

### Creation Wizard

Unified "New..." dialog (`n` from Worktrees) with tabs for creating resources:

**Tabs:**
1. **Project** (placeholder) - future project creation
2. **Branch** (placeholder) - future branch creation
3. **Worktree** - create git worktree with Claude session
   - Name: becomes `azureal/{name}` branch
   - Prompt: initial message to Claude
4. **Session** - create new Claude session in existing worktree
   - Name (optional): custom name stored in `.azureal/sessions`
   - Prompt: initial message to Claude
   - Worktree: select target from list

**Session Name Storage:**
Custom session names map to Claude-generated UUIDs in `.azureal/azufig.toml`:
```toml
[sessions]
"9d409dfb-422b-4f4b-9f32-755277e3e527" = "hook-visibility-fix"
"abc123-def456-..." = "filetree-operations"
```

Implementation: `src/wizard.rs` (wizard state), `src/tui/draw_wizard.rs` (rendering), `src/tui/input_wizard.rs` (input handling), `src/app/state/session_names.rs` (name storage)

### Completion Notifications

macOS notification sent when any Claude instance finishes its response. Fires for every session exit (not just the currently viewed one), so the user sees alerts even when working in another app.

**Notification format:**
- Title: `worktree:session_name`
- Body: "Response complete" (exit 0), "Exited with error" (non-zero), or "Process terminated" (signal)
- Session name uses custom name from `sessions` if set, otherwise first 8 chars of UUID
- Branded Azureal icon (not Finder/Terminal)

**Implementation details:**
- Uses `notify-rust` crate with `set_application("com.xcorvisx.azureal")` for branded icon
- `.app` bundle auto-created at `~/.azureal/AZUREAL.app` on first launch — zero manual setup
- `.icns` icon embedded in binary via `include_bytes!()` and extracted to bundle on first run
- Binary copied into bundle (`Contents/MacOS/azureal`) — NOT symlinked, because `proc_pidpath()` resolves symlinks and Activity Monitor needs the real path inside the `.app` to show the custom icon
- On startup, process re-execs through the bundle copy via `Command::exec()` so `proc_pidpath()` returns the bundle path
- `TransformProcessType(psn, kProcessTransformToUIElementAppType)` registers the process with the macOS window server — without this, `NSRunningApplication` returns nil and Activity Monitor shows a generic icon despite correct `proc_pidpath()`
- `AZUREAL_REEXEC` env var prevents infinite re-exec loop; `already_in_bundle` check provides secondary guard
- Bundle ad-hoc codesigned after binary copy (source has linker ad-hoc signature that fails validation inside a `.app` bundle)
- Bundle registered with macOS Launch Services via `lsregister` on creation/update
- Activity Monitor shows "AZUREAL" as process name with branded icon
- Notification permissions auto-enabled on first launch by writing `ALLOW_NOTIFICATIONS|BANNERS|SOUND|BADGE|PREVIEW_ALWAYS` flags to `~/Library/Preferences/com.apple.ncprefs.plist` via Python's `plistlib` (the only reliable way to edit macOS binary plists). Marker file `~/.azureal/.notif_enabled` prevents overriding user's preference on subsequent launches
- Binary mtime comparison detects when source binary changed (e.g., after `cargo install`) and re-copies
- Notification runs in a fire-and-forget background thread (never blocks event loop)
- Called from `handle_claude_exited()` BEFORE state cleanup (needs session info still available)
- For current session: uses cached `title_session_name`; for background sessions: looks up from `session_files` + `session_names` TOML

Implementation: `src/app/state/claude.rs` (`send_completion_notification()`), `src/main.rs` (bundle creation + re-exec)

# MANIFEST

```
azureal/
├── .azureal/                # Project-level azureal data (gitignored)
│   └── azufig.toml         # Project-local unified config (TOML): filetree options, sessions, healthscope (alias: godfilescope), local runcmds, local presetprompts
├── .claude/                 # Project-level Claude Code config
│   ├── settings.json        # Hook configuration (PreToolUse keybinding enforcement)
│   └── scripts/
│       └── enforce-keybindings.sh  # Catches raw KeyCode in input_*.rs, hardcoded labels in draw_*.rs, new arrays without companions in keybindings.rs
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
│   │   │   ├── app.rs      # App struct definition + new(); includes `screen_height`, `browsing_main`, `pre_main_browse_selection`, `main_worktree: Option<Worktree>` (separate from worktrees vec); `current_worktree()` returns main_worktree when browsing_main is true
│   │   │   ├── load.rs     # Session loading and discovery; main stored in main_worktree (not worktrees vec)
│   │   │   ├── sessions.rs # Worktree navigation, session file selection, archive
│   │   │   ├── output.rs   # Output processing
│   │   │   ├── scroll.rs   # Scroll operations
│   │   │   ├── claude.rs   # Claude session handling
│   │   │   ├── file_browser.rs # File tree and viewer
│   │   │   ├── ui.rs       # Focus, dialogs, menus, wizard, enter_main_browse/exit_main_browse (clears in switch_project)
│   │   │   ├── viewer_edit.rs # Viewer edit mode: wrap-aware cursor, mouse click/drag, clipboard
│   │   │   ├── session_names.rs # Custom session name storage
│   │   │   ├── health.rs    # Health module root: shared constants (SOURCE_EXTENSIONS, SKIP_DIRS, SOURCE_ROOTS), scope persistence (load_health_scope/save_health_scope, AzufigHealthScope), open/close panel, current_worktree_info() (replaced find_main_worktree + switch_to_main_worktree)
│   │   │   ├── health/     # Health submodules (file-based module root)
│   │   │   │   ├── god_files.rs     # God File System: scan, scope mode, parallel modularize, module style selector
│   │   │   │   └── documentation.rs # Doc coverage scanner, DH session spawning, doc toggle/view
│   │   │   └── helpers.rs  # Utility functions
│   │   ├── session_parser.rs # Claude session file parsing
│   │   ├── terminal.rs     # PTY terminal management
│   │   ├── types.rs        # Enums (Focus, ViewMode, WorktreeAction, SidebarRowAction, FileTreeAction, ProjectsPanel, GitActionsPanel with is_on_main, GitCommitOverlay, dialogs)
│   │   ├── input.rs        # Input handling methods
│   │   └── util.rs         # ANSI stripping, JSON parsing
│   ├── tui.rs              # Module root (re-exports only)
│   ├── tui/                # Terminal UI module
│   │   ├── run.rs          # TUI entry point, splash screen, and 3-pane layout
│   │   ├── event_loop.rs   # Event loop module root (run_app + submodule declarations)
│   │   ├── event_loop/     # Event loop submodules
│   │   │   ├── actions.rs  # Key dispatch, execute_action (incl BrowseMain, read-only guards, Esc browse-mode exit), nav/escape dispatch
│   │   │   ├── claude_events.rs # Claude process event handling + staged prompt
│   │   │   ├── coords.rs   # Screen-to-content coordinate mapping
│   │   │   ├── fast_draw.rs # Fast-path input rendering (~0.1ms bypass)
│   │   │   └── mouse.rs    # Click, drag, scroll, selection copy
│   │   ├── util.rs         # Display utilities (re-exports)
│   │   ├── colorize.rs     # Output colorization
│   │   ├── markdown.rs     # Markdown parsing
│   │   ├── render_markdown.rs # Markdown rendering (tables, headers, lists, quotes, code blocks)
│   │   ├── render_events.rs # DisplayEvent rendering (full + incremental)
│   │   ├── render_thread.rs # Background render thread (PreScanState, RenderRequest/Result, sequence numbers)
│   │   ├── render_tools.rs # Tool result rendering
│   │   ├── render_wrap.rs  # Text/span wrapping utilities
│   │   ├── draw_projects.rs # Projects panel modal (full-screen project selection/management)
│   │   ├── draw_sidebar.rs # Worktrees pane rendering (project name in border title, yellow "★ main (read-only)" title in browse mode) + FileTree overlay delegate
│   │   ├── file_icons.rs  # File tree icon mapping — Nerd Font glyphs (primary) + emoji fallback
│   │   ├── draw_file_tree.rs # FileTree overlay rendering (called from draw_sidebar when overlay active); "(read-only)" suffix + yellow border in browse mode
│   │   ├── draw_viewer.rs  # Viewer pane rendering
│   │   ├── draw_output.rs  # Convo pane module root (re-exports + main draw_output fn)
│   │   ├── draw_output/    # Convo pane submodules
│   │   │   ├── render_submit.rs  # Background render thread submit/poll (submit_render_request, poll_render_result)
│   │   │   ├── session_list.rs   # Session list overlay (filter, content search, name list)
│   │   │   ├── todo_widget.rs    # Sticky todo/tasks widget at bottom of convo pane (20-line cap, scrollbar, mouse wheel)
│   │   │   └── rebase_view.rs    # Git rebase status display (dead code — no entry points)
│   │   ├── draw_health.rs   # Worktree Health panel modal (tabbed: God Files + Documentation)
│   │   ├── draw_git_actions.rs # Git panel modal (centered overlay with git ops + changed files)
│   │   ├── draw_*.rs       # Other rendering functions
│   │   ├── keybindings.rs  # SINGLE SOURCE OF TRUTH: Action enum (~60 variants incl GitPull, BrowseMain), KeyContext, lookup_action() + 6 per-modal lookups (lookup_git_actions_action takes is_on_main), ~17 static arrays (WORKTREES: 15 entries, GIT_ACTIONS: 13 entries), hint generators, help_sections()
│   │   ├── input_projects.rs # Projects panel input (browse, add, delete, rename, init)
│   │   ├── input_file_tree.rs # FileTree: clipboard mode + text-input actions only (commands resolved upstream)
│   │   ├── input_viewer.rs # Viewer: tab/save/discard dialogs + edit mode text editing (commands resolved upstream)
│   │   ├── input_output.rs # Convo: convo search + session list overlay only (commands resolved upstream)
│   │   ├── input_health.rs  # Worktree Health panel input (tab switching, per-tab keys)
│   │   ├── input_git_actions.rs # Git panel input (actions, file nav, git operations)
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
│   │   ├── rebase.rs       # Rebase operations (dead code — preserved on disk but not compiled from tui.rs)
│   │   └── worktree.rs     # Worktree create/delete/list
│   ├── cmd.rs              # CLI command handler routing (file-based module root)
│   ├── cmd/                # CLI command handler submodules
│   │   ├── session.rs      # Session list/show commands
│   │   └── project.rs      # Project info command
│   ├── azufig.rs           # Unified config: GlobalAzufig + ProjectAzufig structs (HashMap-based flat sections), load/save/update helpers (TOML I/O with bare-key post-processing)
│   ├── claude.rs           # Claude CLI process management
│   ├── cli.rs              # CLI argument parsing (clap definitions)
│   ├── config.rs           # Configuration (permissions, API key), Claude session discovery, projects persistence (reads from azufig)
│   ├── main.rs             # Entry point
│   ├── models.rs           # Domain models (Worktree, WorktreeStatus, Project, RebaseStatus)
│   ├── stt.rs              # Speech-to-text engine (cpal + whisper-rs + background thread)
│   ├── syntax.rs           # Syntax highlighting for diffs
│   ├── watcher.rs          # Filesystem watcher (notify crate — kqueue/inotify/ReadDirectoryChangesW)
│   └── wizard.rs           # Session creation wizard
├── worktrees/              # Git worktrees for sessions
├── AGENTS.md               # This file
├── CHANGELOG.md            # Version history
├── Cargo.toml              # Rust dependencies
└── README.md               # User-facing documentation
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
- [x] Squash merge to main (replaced rebase/merge)
- [x] Vim-style modal input (command/insert modes)
- [x] Embedded terminal pane for shell commands

## Phase 2: Enhanced UX
- [x] File viewer pane (3-pane layout: Worktrees, Viewer, Convo; FileTree as overlay)
- [x] Session list overlay in Convo pane (`s` toggle — browse current worktree's session files with message counts)
- [x] Token usage percentage on Convo pane title
- [x] TodoWrite sticky widget (persistent checkbox list at bottom of Convo pane)
- [x] AskUserQuestion options box (numbered options with context-aware response handling)
- [x] Squash merge to main (collapses all branch commits into single main commit, replaced rebase/merge/auto-rebase)
- [ ] Session templates
- [ ] Per-project configuration
- [ ] Theme customization
- [x] Input history persistence
- [x] Search/filter sessions (`/` in Worktrees pane)
- [x] Convo search (`/` in Convo pane — find text in current session, `n/N` to cycle matches)
- [x] Session list search (`/` name filter, `//` cross-session content search)
- [x] Speech-to-text input (`⌃s` in prompt mode and file edit mode)

## Phase 3: Advanced Features
- [x] God File System (scan >1000 LOC files, batch-modularize via concurrent Claude sessions)
- [x] Worktree Health Panel (tabbed modal: God Files tab + Documentation coverage tab, Shift+H global)
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
- Unit tests for TodoWrite parsing (`parse_todos_from_input` — 5 tests)
- Unit tests for AskUserQuestion context builder (`build_ask_user_context` — 5 tests)
- Unit tests for AskUserQuestion rendering (`render_ask_user_question` — 4 tests)
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
| `T` | Toggle terminal pane |
| `G` | Toggle Git panel |
| `j/k` | Navigate / scroll line |
| `J/K` | Page scroll (Viewer/Convo/Terminal); Select project (Worktrees) |
| `Tab` | Cycle focus (Worktrees → Viewer → Convo → Input), closes overlays |
| `Shift+Tab` | Cycle focus reverse |
| `?` | Help |
| `⌃c` | Cancel agent |
| `⌃q` | Quit |
| `⌃r` | Restart |

### Worktrees Pane
| Key | Action |
|-----|--------|
| `j/k` | Navigate worktrees |
| `J/K` | Page scroll |
| `⌥↑/⌥↓` | Jump to top/bottom |
| `f` | Toggle FileTree overlay (browse worktree files) |
| `Enter` | Start/resume session |
| `/` | Search/filter sessions |
| `n` | New worktree/session wizard |
| `b` | Browse branches |
| `r` | Run command (picker or execute) |
| `⌥r` | Add new run command |
| `a` | Archive worktree (blocked on main branch) |
| `u` | Unarchive worktree (recreate from branch) |
| `P` | Projects panel |
| `⌥↑/⌥↓` | Jump to first/last worktree |

### FileTree Overlay (`f` in Worktrees)
| Key | Action |
|-----|--------|
| `j/k` | Navigate up/down |
| `⌥↑/⌥↓` | Jump to first/last sibling in current folder |
| `Enter` | Open file in Viewer / Expand directory |
| `h/l` | Collapse/Expand directory |
| `Space` | Toggle directory expand |
| `a` | Add file (trailing `/` creates directory) |
| `d` | Delete selected file/directory (y/N confirm) |
| `r` | Rename selected file/directory |
| `c` | Copy selected file/directory (clipboard-style: navigate to target dir, Enter to paste) |
| `m` | Move selected file/directory (clipboard-style: navigate to target dir, Enter to paste) |
| `O` | Options overlay (toggle visibility of `.git`, `.claude`, `.azureal` dirs) |
| `w/Esc` | Return to worktree list |

### Viewer Pane
| Key | Action |
|-----|--------|
| `j/k` | Scroll up/down |
| `J/K` | Page scroll (viewport minus 2 overlap) |
| `⌥↑/⌥↓` | Jump to top/bottom |
| `⌥←/⌥→` | Prev/next Edit (syncs Convo scroll) |
| `⌘A` | Select all (then `⌘C` to copy) |
| `t` | Tab current file (save to tab list) |
| `⌥t` | Open tab dialog (browse/switch tabs) |
| `]/[` | Next/prev tab |
| `x` | Close current tab |
| `Esc` | Exit viewer (restores previous content if in Edit diff view) |

### Convo Pane
| Key | Action |
|-----|--------|
| `j/k` | Scroll line |
| `↑/↓` | Jump to prev/next message (user + assistant) |
| `Shift+↑/↓` | Jump to prev/next user prompt only |
| `J/K` | Page scroll (viewport minus 2 overlap) |
| `⌥↑/⌥↓` | Jump to top/bottom |
| `s` | Toggle Session list overlay (browse all session files) |
| `/` | Search text in current session (yellow highlights, `[N/M]` counter) |
| `n/N` | Next/prev search match (after `/` search confirmed with Enter) |
| `Esc` | Return to Worktrees |

**Clickable File Paths:** Edit, Read, and Write tool file paths are underlined in orange and clickable. Clicking an Edit path opens the full file in the Viewer with the edit region highlighted (red background for deleted lines, green background for added lines) and sets the `selected_tool_diff` index so `⌥←/⌥→` cycling continues from that position. Clicking a Read or Write path opens the file plain in the Viewer. The clicked/cycled path is highlighted with inverted colors (orange background, black text) in the Convo pane — highlight covers all wrapped continuation lines via `wrap_line_count` field in `ClickablePath`. Clicking a continuation line of a wrapped path also triggers the file open. Use `⌥←/⌥→` in the Viewer to cycle through edits (also syncs Convo scroll and sets the highlight). The border title shows `[Edit N/M]` where N is the current edit-only position and M is the total number of Edit tool calls (excludes Read/Write). The last 20 Edit calls also show inline diff previews in the Convo pane.

### Prompt Mode (Input Focused)

Prompt keybindings are displayed directly in the Input pane's title bar (not in the help panel). All title hints are dynamically sourced from the `INPUT` binding array via `find_key_for_action()` / `find_key_pair()` — changing a key in the array automatically updates the title. When the terminal is too narrow for the full title, `split_title_hints()` packs as many hint segments as fit on the top border, then overflow hints go on the bottom border in parentheses with the same style (color + bold) as the top title.

**Type mode title shows:** `(Esc:exit | Enter:submit | ⇧Enter:newline | ⌃c:cancel agent | ↑/↓:history | ⌥←/→:word | ⌃w:del wrd | ⌃s:speech | ⌥p:presets)`
**Command mode title shows:** `(p:PROMPT | T:TERMINAL | G:Git | ?:help | Tab/⇧Tab:focus | ⌃c:cancel agent | ⌃q:quit | ⌃r:restart | ⌃d:dump debug output)`

### Terminal Mode

Terminal keybindings are displayed directly in the terminal pane's title bar (not in the help panel). All title hints are dynamically sourced from the `TERMINAL` binding array via `find_key_for_action()` / `find_key_pair()` — changing a key in the array automatically updates the title.

**Command mode title shows:** `(t:type | p:prompt | Esc:close | j/k:scroll | J/K:page | ⌥↑/⌥↓:top/bottom | +/-:resize)`
**Type mode title shows:** `(Esc:exit)`
**Scroll mode title shows:** `[N↑] (j/k:scroll | J/K:page | ⌥↑:top | ⌥↓:bottom | t:type | Esc:close)`