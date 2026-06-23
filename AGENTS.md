# Azureal (Asynchronous Zoned Unified Runtime Environment for Agentic LLMs) is a Rust TUI application that wraps Claude Code CLI and OpenAI Codex CLI to enable multi-agent development workflows. Each worktree is a git worktree with its own agent session, allowing concurrent AI-assisted development across multiple feature branches. See CLAUDE.md for the full authoritative specification.

# FEATURES

### Process Cleanup on Quit and Cancel

All agent processes are fully terminated when the user quits Azureal or cancels an agent. Three mechanisms work together to guarantee no orphaned processes survive:

**Process group kill (SIGTERM):** `kill_process_tree(pid)` in `src/backend.rs` sends SIGTERM to the entire process group (`libc::killpg` on Unix, `taskkill /T /F` on Windows). Agent processes are spawned as process group leaders (`process_group(0)` in `claude.rs` / `codex.rs`), so the signal reaches all descendants — cargo test subprocesses, Claude subagents, etc.

**SIGKILL follow-up:** `kill_process_tree_force(pid)` (Unix only) sends SIGKILL to the process group, which cannot be caught or ignored. On app quit, the event loop calls `cancel_all_claude()`, collects the killed PIDs, sleeps 200ms (grace period for clean shutdown), then calls `kill_process_tree_force()` on each PID to eliminate any survivors.

**Commit gen PID tracking:** Commit message generation processes are spawned via `std::process::Command` on background threads — they have no streaming event channel and are never added to `running_sessions`. They are tracked separately in `commit_gen_pids: Arc<Mutex<Vec<u32>>>` on `App`, registered by `generate_commit_message_with_claude/codex()` at spawn and deregistered on completion. `cancel_all_claude()` iterates this list and kills each one.

`cancel_all_claude()` now returns `Vec<u32>` (all PIDs it sent SIGTERM to) so the event loop can follow up with SIGKILL after the grace period.

**Files:** `src/backend.rs` (`kill_process_tree`, `kill_process_tree_force`), `src/app/state/app.rs` (`commit_gen_pids` field), `src/app/state/ui.rs` (`cancel_all_claude` returns `Vec<u32>`), `src/tui/event_loop.rs` (SIGTERM → 200ms → SIGKILL quit flow), `src/tui/input_git_actions/operations.rs` (`generate_commit_message_with_claude/codex` register/deregister PIDs).

### Multi-Worktree Agent Management

See CLAUDE.md `# FEATURES` → `Multi-Worktree Agent Management` for the full specification.

### Git Panel and Worktree Operations

See CLAUDE.md `# FEATURES` → `Git Panel` for the full specification.

### Session Store (SQLite)

See CLAUDE.md for the full session store specification including compaction, context injection, and portable sessions.

# MANIFEST

See CLAUDE.md `# MANIFEST` for the complete file tree.

Key files for process management:
- `src/backend.rs` — `kill_process_tree()`, `kill_process_tree_force()` (Unix SIGTERM/SIGKILL), Windows `taskkill /T /F`
- `src/app/state/app.rs` — `commit_gen_pids: Arc<Mutex<Vec<u32>>>` field
- `src/app/state/ui.rs` — `cancel_all_claude() -> Vec<u32>` (kills agents + compaction + commit gen, returns PIDs)
- `src/tui/event_loop.rs` — quit flow: SIGTERM → 200ms sleep → SIGKILL survivors
- `src/tui/input_git_actions/operations.rs` — `generate_commit_message_with_claude/codex()` track PIDs in `commit_gen_pids`
- `src/claude.rs` — spawns agents with `process_group(0)` (process group leader)
- `src/codex.rs` — spawns agents with `process_group(0)` (process group leader)

# ROADMAP

See CLAUDE.md `# ROADMAP` for the full phased roadmap.

# TESTING REQUIREMENTS

See CLAUDE.md `# TESTING REQUIREMENTS` for domain-specific guidelines and the full test coverage table.

For process management specifically:
- `cancel_all_claude` unit tests verify it clears all state maps and returns killed PIDs
- `kill_process_tree` / `kill_process_tree_force` are OS-level calls — tested via integration (spawn a process, kill it, verify it's gone)
- `commit_gen_pids` registration/deregistration tested in `src/app/state/app.rs` tests

# VIZIA GOTCHAS

### Render Cache Replacement Is Not Append Invalidation

**Problem:** When exit-time JSONL reconciliation replaces `display_events` for the viewed session, the session pane renderer must not keep incremental counters from the old event array. Keeping `rendered_events_count` makes the render thread treat the replacement as an append-only tail, so final turn content can disappear when a session completes or when the user returns after switching worktrees.

**Solution:** Use a replacement helper that resets render bookkeeping, drops any in-flight render result from the old event array, and keeps the previous rendered lines visible until the full replacement render lands.

**WRONG:**

```rust
self.display_events = prefix_events;
self.display_events.extend(events.clone());
self.invalidate_render_cache();
```

**CORRECT:**

```rust
let mut display_events = prefix_events;
display_events.extend(events.clone());
self.replace_display_events_for_render(display_events);
```

### Parsed JSONL Must Not Drop Optimistic User Prompts

**Problem:** Codex/Claude JSONL can contain richer final assistant/tool output than the live stream while missing the optimistic `UserMessage` Azureal inserted on submit. Choosing the parsed suffix solely because it has more recovered text makes the submitted prompt disappear on completion.

**Solution:** Before choosing or displaying parsed reconciliation events, copy any missing live/cache `UserMessage` events into the parsed suffix so the final richer answer still keeps the submitted prompt bubble.

**WRONG:**

```rust
if parsed_message_chars >= live_message_chars {
    parsed_events
} else {
    live_events
}
```

**CORRECT:**

```rust
let parsed_events = preserve_live_user_messages(parsed_events, &live_events);
if parsed_message_chars >= live_message_chars {
    parsed_events
} else {
    live_events
}
```

### Assistant Headers Need Model State For Event Slices

**Problem:** Deferred or incremental renders can start at an `AssistantText` event without a preceding `Init` or `ModelSwitch`. If the renderer falls back to no model state, Codex responses can render with the Claude header/color.

**Solution:** Seed render requests with the selected/restored session model when the scanned event prefix has not provided a model yet.

**WRONG:**

```rust
RenderRequest {
    pre_scan: PreScanState::default(),
    // ...
}
```

**CORRECT:**

```rust
RenderRequest {
    pre_scan: pre_scan_events_with_fallback(&events, app.display_model_name()),
    // ...
}
```

### Active Codex Full Reparses Must Reset In-Flight Renders

**Problem:** Active Codex sessions force full JSONL reparses while a turn is still streaming so disk-parsed edit payloads can replace placeholder live events. If that path assigns `display_events` directly and only resets a few counters, an older in-flight incremental render can still land afterward and append already-rendered user/assistant bubbles to the pane.

**Solution:** Any full reparse that replaces the whole `display_events` array must use `replace_display_events_for_render()`. That helper resets incremental counters, marks stale render results as already applied, and cancels the in-flight flag while keeping the previous lines visible until the full replacement render completes.

**WRONG:**

```rust
self.display_events = parsed.events;
self.rendered_events_count = 0;
self.rendered_content_line_count = 0;
self.rendered_events_start = 0;
self.invalidate_render_cache();
```

**CORRECT:**

```rust
self.replace_display_events_for_render(parsed.events);
```

### Active Codex Reparses Must Preserve Live Pre-Turn Events

**Problem:** Active Codex JSONL reparses rebuild the pane from SQLite plus the current rollout file. SQLite can lag behind the pane by one live turn, so rebuilding from store-only history can make the newly submitted prompt appear before the prior live prompt until exit-time storage catches up.

**Solution:** During active Codex full reparses, merge the stored prefix, any in-memory events before the current prompt, and then the parsed current-turn events. If the JSONL has not emitted the user prompt yet, recover the prompt from the last matching pending message in the previous display snapshot.

**WRONG:**

```rust
parsed.events = self.merge_store_prefix_for_current_session(parsed.events);
parsed.events = self.preserve_pending_user_message(parsed.events);
```

**CORRECT:**

```rust
parsed.events =
    self.merge_live_prefix_for_active_codex_reparse(parsed.events, &previous_display_events);
parsed.events = self.preserve_pending_user_message(parsed.events, turn_events_offset);
```

### Empty Compaction Retries Must Not Re-Banner Forever

**Problem:** When a background compaction agent exits without assistant summary text, setting `compaction_retry_needed` on every empty completion lets the event loop respawn compaction on every tick. Each retry can append another `MayBeCompacting` event, creating an endless stream of compaction banners while the status bar repeats "produced no summary text — retrying".

**Solution:** Treat an empty-summary retry as a bounded lifecycle. Use a retry latch before the retry spawn, collapse duplicate trailing compaction banners, and disable hidden auto-continue if the retry also produces no summary text.

**WRONG:**

```rust
if let Some((session_id, wt_path)) = app.compaction_retry_needed.as_ref() {
    if spawn_compaction_agent(app, process, *session_id, wt_path) {
        app.compaction_retry_needed = None;
    }
}
```

**CORRECT:**

```rust
if let Some((session_id, wt_path)) = take_empty_compaction_retry(app) {
    if spawn_compaction_agent(app, process, session_id, &wt_path) {
        app.compaction_retry_needed = None;
        collapse_trailing_compaction_banner(app);
    } else {
        stop_empty_compaction_retry(app, "Compaction stopped: summary retry failed to spawn.");
    }
}
```

### Compaction Auto-Continue Must Ignore Killed-Turn Completion Banners

**Problem:** Mid-turn compaction intentionally kills the active Codex process and then starts a hidden continuation after the compaction summary is stored. Codex can still leave a `Complete` event while finalizing the killed turn, and older turns can also have recent completion banners. Treating any recent `Complete` as "session already finished" strands the interrupted turn after compaction.

**Solution:** Let `auto_continue_after_compaction` be the source of truth. That flag is set only when Azureal crossed the threshold before seeing a natural completion and then killed the active process. After compaction receivers and retries clear, send the hidden continuation regardless of rendered completion banners.

**WRONG:**

```rust
if app.auto_continue_after_compaction && app.compaction_receivers.is_empty() {
    if app.display_events.iter().rev().take(20).any(|e| matches!(e, DisplayEvent::Complete { .. })) {
        app.auto_continue_after_compaction = false;
        return true;
    }
    send_prompt_to_current_worktree(app, process, None, AUTO_CONTINUE_PROMPT, "...", "...");
}
```

**CORRECT:**

```rust
if app.auto_continue_after_compaction
    && app.compaction_receivers.is_empty()
    && app.compaction_retry_needed.is_none()
{
    app.auto_continue_after_compaction = false;
    send_prompt_to_current_worktree(app, process, None, AUTO_CONTINUE_PROMPT, "...", "...");
}
```

### Render Results Must Not Apply After A Newer Submit

**Problem:** Large live sessions can have a full or deferred render in flight when a new user prompt or assistant chunk arrives. If the event loop refuses to submit the newer snapshot until the old render completes, live turns can remain invisible. If the old result is then applied after a newer snapshot was queued, an incremental result can append stale bubbles and duplicate prompts.

**Solution:** When display events change, mark the current in-flight render snapshot ineligible so the event loop can submit a replacement under its normal throttle. While polling results, discard any result older than the render thread's newest submitted sequence or any result that arrives while the cache is dirty.

**WRONG:**

```rust
if app.rendered_lines_dirty && !app.render_in_flight {
    submit_render_request(app, session_w);
}

if result.seq <= app.render_seq_applied {
    return false;
}
```

**CORRECT:**

```rust
pub fn invalidate_render_cache(&mut self) {
    self.rendered_lines_dirty = true;
    self.render_in_flight = false;
}

if result.seq < app.render_thread.current_seq()
    || app.rendered_lines_dirty
    || result.seq <= app.render_seq_applied
{
    return false;
}
```

### Exit-Time Store Append Must Refresh The Viewed Pane

**Problem:** A Codex turn can finish after the visible pane has only rendered the last tool call, such as `poll session ...`. `handle_claude_exited()` may clear the running slot and JSONL watch state before `store_append_from_jsonl()` ingests the final rollout. If visible replacement depends on `session_file_path` still matching the JSONL, SQLite gets the final assistant summary but the pane stays stuck on the pre-final live snapshot.

**Solution:** After exit-time JSONL ingestion succeeds, if the currently displayed store session matches the appended session, reload that session's events from SQLite through `replace_display_events_for_render()`. The store is authoritative after append and already contains the final assistant text and completion banner.

**WRONG:**

```rust
if !events.is_empty() && self.session_file_path.as_ref() == Some(path) {
    let mut display_events = prefix_events;
    display_events.extend(events.clone());
    self.replace_display_events_for_render(display_events);
}
```

**CORRECT:**

```rust
store.append_events(session_id, &events)?;
self.refresh_display_from_store_after_append(session_id, &wt_path);
```

### Clipboard Copy Must Use System Result And Flow Text

**Problem:** UI copy actions can show "Copied to clipboard" after only updating Azureal's internal clipboard or after ignoring a failed system clipboard write. Session-pane selection can also serialize rendered bubble rows directly, copying visual wrap line breaks instead of the original flowing message text.

**Solution:** Route copy actions through `copy_to_clipboard()` and use its boolean result for status text. When extracting session-pane bubble text, strip bubble chrome and join wrapped prose fragments with spaces while preserving literal code-like rows.
### Sidebar Path Truncation Must Respect Character Boundaries

**Problem:** Git sidebar paths and discard prompts can contain Unicode. Slicing strings by byte length while fitting text to a terminal width can panic when the slice starts or ends inside a multi-byte character.

**Solution:** Use display-width-aware helpers that walk characters and never slice `&str` by computed byte offsets.

**WRONG:**

```rust
app.copy_to_clipboard(&text);
app.set_status("Copied to clipboard");
parts.join("\n")
let path_display = if file.path.len() > path_budget {
    format!("…{}", &file.path[file.path.len().saturating_sub(path_budget - 1)..])
} else {
    file.path.clone()
};
```

**CORRECT:**

```rust
let copied = app.copy_to_clipboard(&text);
app.set_clipboard_copy_status(copied, "Copied to clipboard");
append_session_copy_fragment(&mut out, &fragment, kind, previous_kind);
let path_display = truncate_path_tail_to_width(&file.path, path_budget);
let padding =
    inner_w.saturating_sub(prefix.len() + 2 + text_width(&path_display) + stat_len);
```

### Health Panel Paths And Bars Must Sanitize Display Inputs

**Problem:** Health-panel source paths and coverage percentages come from scanned project state. Unicode paths can panic if they are truncated by byte offsets, and corrupt or out-of-range coverage values can underflow fixed-width bar rendering.

**Solution:** Truncate paths by terminal display width with character iteration, compute padding from display width, and clamp non-finite or out-of-range percentages before building progress bars.

**WRONG:**

```rust
let path_display = if entry.rel_path.len() > path_max {
    format!("…{}", &entry.rel_path[entry.rel_path.len().saturating_sub(path_max - 1)..])
} else {
    entry.rel_path.clone()
};
let filled = (entry.coverage_pct / 100.0 * bar_width as f32).round() as usize;
let bar = "█".repeat(filled) + &"░".repeat(bar_width - filled);
```

**CORRECT:**

```rust
let path_display = truncate_path_tail_to_width(&entry.rel_path, path_max);
let padding = inner_w.saturating_sub(fixed_width + display_width(&path_display));
let filled = filled_bar_cells(entry.coverage_pct, bar_width);
let bar = "█".repeat(filled) + &"░".repeat(bar_width - filled);
```

### Styled Wrap Ranges Must Use Character Offsets

**Problem:** Styled text wrapping can flatten spans into a string, but byte offsets from `String::len()` do not match character offsets used by `.chars().skip()` and `.take()`. Unicode assistant output or tool text can render with the wrong style spans when a range boundary lands after a multi-byte character.

**Solution:** Track style ranges in character offsets while flattening spans, and advance wrapped line offsets by `wrapped.chars().count()`.

**WRONG:**

```rust
let start = full_text.len();
full_text.push_str(&span.content);
let end = full_text.len();
style_ranges.push((start, end, span.style));
let line_end = char_offset + wrapped.len();
```

**CORRECT:**

```rust
let start = full_text_chars;
full_text.push_str(&span.content);
full_text_chars += span.content.chars().count();
let end = full_text_chars;
style_ranges.push((start, end, span.style));
let line_end = char_offset + wrapped.chars().count();
```

### Wrap Fast Paths Must Use Display Width

**Problem:** Terminal wrapping is measured in display columns, not Unicode scalar count. A short CJK string can have `chars().count() <= max_width` while still being too wide for the terminal, causing a fast path to skip wrapping and let text overflow.

**Solution:** Use `UnicodeWidthStr::width()` for no-wrap fast paths and keep character counts only for character-index mapping.

**WRONG:**

```rust
if text.chars().count() <= max_width && !text.contains('\n') {
    return vec![text.to_string()];
}
```

**CORRECT:**

```rust
if UnicodeWidthStr::width(text) <= max_width && !text.contains('\n') {
    return vec![text.to_string()];
}
```

### Tool Parameter Truncation Must Use Display Width

**Problem:** Tool parameter previews are rendered into fixed terminal column budgets. Truncating by `chars().count()` can still overflow for CJK, emoji, or other wide glyphs, and returning an ellipsis for a zero-width budget draws outside the allocated area.

**Solution:** Measure the trimmed text with `UnicodeWidthStr::width()`, walk characters with `UnicodeWidthChar::width()`, and only append the ellipsis when it fits inside the requested width.

**WRONG:**

```rust
if trimmed.chars().count() <= max_len {
    trimmed.to_string()
} else if max_len > 1 {
    format!("{}…", trimmed.chars().take(max_len - 1).collect::<String>())
} else {
    "…".to_string()
}
```

**CORRECT:**

```rust
if UnicodeWidthStr::width(trimmed) <= max_width {
    return trimmed.to_string();
}
if max_width == 0 {
    return String::new();
}
let content_width = max_width - ellipsis_width;
```

### Tool Result Reminder Blocks Must Be Removed In Place

**Problem:** Tool output can contain hidden `<system-reminder>...</system-reminder>` blocks followed by real command output. Truncating at the first opening tag removes the hidden reminder, but it also hides legitimate output that appears after a closed block.

**Solution:** Remove each closed reminder block in place and keep surrounding output. If an opening tag is unmatched, truncate from that tag because the remainder cannot be safely separated from hidden reminder text.

**WRONG:**

```rust
let content = if let Some(start) = content.find("<system-reminder>") {
    &content[..start]
} else {
    content.as_str()
}
.trim_end();
```

**CORRECT:**

```rust
let content = strip_system_reminder_blocks(&content);
let content = content.trim_end();
```

### Tool Call Click Hitboxes Must Use Display Columns

**Problem:** Tool-call file paths can contain wide Unicode characters. If clickable path regions use `chars().count()` or byte length, the mouse hitbox ends before the rendered path does, so clicks on the visible tail of a CJK or emoji path miss the file link.

**Solution:** Compute both the prefix start column and the wrapped path end column with terminal display width helpers.

**WRONG:**

```rust
let prefix_len = 3 + 2 + display_name.len() + 2;
let start_col = prefix_len;
let end_col = start_col + wrapped.chars().count();
```

**CORRECT:**

```rust
let prefix_width = tool_call_prefix_width(indicator, display_name);
let start_col = prefix_width;
let end_col = start_col + display_width(&wrapped);
```

### Tool Parameter Extraction Must Share Path-Key Fallbacks

**Problem:** Tool-call preview payloads can use provider-specific path keys such as `notebook_path`, `target_file`, `relative_path`, or `filePath`. Checking only `file_path` and `path` makes the session pane render a blank tool parameter even though a usable file path is present.

**Solution:** Centralize path-like key lookup and reuse it for explicit file tools, LSP paths, notebook edits, and unknown-tool fallbacks.

**WRONG:**

```rust
input
    .get("file_path")
    .or_else(|| input.get("path"))
    .or_else(|| input.get("command"))
    .and_then(|v| v.as_str())
```

**CORRECT:**

```rust
path_field(input)
    .or_else(|| first_string_field(input, &["command", "cmd", "query", "pattern"]))
```

### Issue Panel Title Budgets Must Use Display Width

**Problem:** GitHub issue titles and labels can contain Unicode. Truncating titles with byte offsets derived from terminal column budgets can panic inside multi-byte characters, and label budgets based on byte length can leave wide labels overlapping the title.

**Solution:** Compute label and title budgets with display-width helpers, then truncate issue titles by walking characters.

**WRONG:**

```rust
let label_len: usize = issue.labels.iter().map(|l| l.len() + 3).sum();
let avail = inner_w.saturating_sub(10 + label_len);
let title_display = if issue.title.len() > avail && avail > 3 {
    format!("{}...", &issue.title[..avail - 3])
} else {
    issue.title.clone()
};
```

**CORRECT:**

```rust
let label_len = issue_labels_width(&issue.labels);
let avail = inner_w.saturating_sub(ISSUE_ROW_PREFIX_WIDTH + label_len);
let title_display = truncate_text_to_width(&issue.title, avail);
```

### File Tree Action Wrapping Must Use Display Width

**Problem:** File tree action bars can include Unicode filenames or typed paths. Wrapping by `chars().count()` treats CJK and emoji as one column even when the terminal renders them wider, so action text can overflow or wrap too late.

**Solution:** Measure tokens with `UnicodeWidthStr::width()` and hard-break long tokens by accumulating `UnicodeWidthChar::width()` display columns.

**WRONG:**

```rust
let len = token.chars().count();
if col + len <= max_width {
    current_spans.push(Span::styled(token.to_string(), style));
    col += len;
}
```

**CORRECT:**

```rust
let len = UnicodeWidthStr::width(token);
if col + len <= max_width {
    current_spans.push(Span::styled(token.to_string(), style));
    col += len;
}
```

### Session List Names And Previews Must Use Display Width

**Problem:** Session names, search previews, and suffix padding are rendered into fixed terminal row budgets. Measuring them with `chars().count()` lets CJK, emoji, and other wide glyphs overflow into timestamps or message badges.

**Solution:** Compute row budgets with display-width helpers, truncate by walking characters, and derive padding from the rendered display width of the truncated text.

**WRONG:**

```rust
let prefix_len = name_display.chars().count() + 4;
let preview_space = inner_width.saturating_sub(prefix_len);
let trunc_preview: String = preview.chars().take(preview_space).collect();
let pad = name_space.saturating_sub(truncated_name.chars().count());
```

**CORRECT:**

```rust
let prefix_len = display_width(&name_display) + 4;
let preview_space = inner_width.saturating_sub(prefix_len);
let trunc_preview = truncate_text_to_width(preview, preview_space);
let pad = name_space.saturating_sub(display_width(&truncated_name));
```

### Viewer Wrap Break Fast Paths Must Use Display Width

**Problem:** Viewer wrapping uses break offsets for both display rows and cursor/scroll math. If the no-wrap fast path checks `chars().count() <= max_width`, short CJK or emoji text can skip wrapping even though it exceeds the available terminal columns.

**Solution:** Gate the no-wrap fast path with `UnicodeWidthStr::width()`, while still storing returned break offsets as character indices for span slicing and cursor mapping.

**WRONG:**

```rust
let char_count = text.chars().count();
if char_count <= max_width {
    return vec![0];
}
```

**CORRECT:**

```rust
if UnicodeWidthStr::width(text) <= max_width {
    return vec![0];
}
```

# REFERENCES

(None fetched yet)

---

## **CONFLICTS**

(None)

# USE

See README.md for installation and usage instructions.
