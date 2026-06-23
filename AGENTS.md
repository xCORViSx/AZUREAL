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

# REFERENCES

(None fetched yet)

---

## **CONFLICTS**

(None)

# USE

See README.md for installation and usage instructions.
