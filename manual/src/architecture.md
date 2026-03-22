# Architecture & Internals

AZUREAL is a single-process, multi-threaded Rust application built on
[ratatui](https://ratatui.rs/) and [crossterm](https://docs.rs/crossterm/). This
section documents the internal architecture for contributors and users who want
to understand how the application works under the hood.

---

## Design Philosophy

### Mostly-Stateless

Runtime state is derived from git, not stored in custom databases. Worktrees
come from `git worktree list`. Branches come from `git branch`. The active
backend is derived from the selected model. Close the application and reopen
it -- the UI reconstructs itself from the repository.

Persistent state is minimal by design:

- **Two `azufig.toml` files** -- global and project-level configuration in TOML
  format. See [Configuration](../configuration.md).
- **One SQLite database** -- `.azureal/sessions.azs` holds all session history.
  See [Session Store](../session-store.md).
- **Temporary JSONL files** -- agent session output, parsed during streaming and
  ingested into SQLite on completion, then deleted.

### Single-Process, Multi-Threaded

There is no daemon, no server, and no IPC between separate binaries. Everything
runs in one process. Background work is offloaded to dedicated threads that
communicate with the main event loop via `mpsc` channels:

| Thread | Purpose |
|--------|---------|
| **Input reader** | Reads crossterm terminal events and forwards them to the main loop |
| **AgentProcessor** | Parses agent JSONL events in the background |
| **Render thread** | Performs markdown parsing, syntax highlighting, and text wrapping |
| **FileWatcher** | Monitors filesystem changes via `notify` crate |
| **Terminal rx** | Reads PTY output from the embedded terminal (conditional) |

### Event-Driven

The main loop is a classic event loop: wait for an event, process it, optionally
redraw the screen. Events come from multiple sources (keyboard, mouse, agent
output, file changes, timers) and are all funneled through channels into a
single-threaded dispatcher. This avoids locks on shared state and keeps the
rendering path simple.

---

## Key Abstractions

### `App`

The central state struct. Holds all UI state, all worktree state, the session
store handle, the agent process manager, and references to every background
thread's channel. The main loop owns the single `App` instance and passes
mutable references to event handlers.

### `AgentProcess`

Manages the lifecycle of agent CLI processes. Holds both a `ClaudeProcess` and a
`CodexProcess`. At spawn time, the selected model determines which backend is
invoked. The process streams JSON events on stdout, which are forwarded to the
`AgentProcessor` thread for parsing. A separate reader thread also writes events
to a temporary JSONL file for post-exit ingestion into the session store.

### `DisplayEvent`

The unified event type that both backends produce after parsing. The session
pane, session store, and render pipeline all consume `DisplayEvent` values. This
abstraction decouples the UI from any specific backend's wire format.

### `SessionStore`

The SQLite persistence layer. Handles event ingestion, context retrieval,
compaction, and completion tracking. See
[Session Store & Persistence](../session-store.md) for the full schema and
lifecycle.

---

## Thread Communication

All inter-thread communication uses `std::sync::mpsc` channels. There are no
mutexes on hot paths. The main loop drains channels on each iteration, processes
all pending messages, then decides whether to redraw.

```text
Input Reader ──→ [mpsc] ──→ Main Loop
AgentProcessor ──→ [mpsc] ──→ Main Loop
FileWatcher ──→ [mpsc] ──→ Main Loop
Terminal rx ──→ [mpsc] ──→ Main Loop (conditional)
Main Loop ──→ [mpsc] ──→ Render Thread
Render Thread ──→ [mpsc] ──→ Main Loop (rendered output)
```

---

## Chapter Contents

- **[Event Loop](./architecture/event-loop.md)** -- The main event loop: input
  handling, event batching, draw throttling, and adaptive timing.
- **[Render Pipeline](./architecture/render-pipeline.md)** -- Background
  rendering: markdown parsing, syntax highlighting, sequence numbers, viewport
  caching, and deferred initial renders.
- **[Performance](./architecture/performance.md)** -- Performance rules and
  invariants: what must never happen in the render path, caching strategies, and
  CPU budget targets.
- **[File Watcher](./architecture/file-watcher.md)** -- Filesystem monitoring:
  notify backends, noise filtering, event coalescing, and the three-phase
  parse+render pipeline.
