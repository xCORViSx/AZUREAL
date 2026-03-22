# File Watcher

AZUREAL monitors the filesystem for changes using the
[notify](https://docs.rs/notify/) crate. A dedicated `FileWatcher` thread
watches for modifications to agent session files and project source files,
forwarding classified events to the main loop via an `mpsc` channel. This drives
the three-phase incremental parse+render pipeline that keeps the session pane
up-to-date during live agent streaming.

---

## Backend

The `notify` crate selects a platform-native backend automatically:

| Platform | Backend | Mechanism |
|----------|---------|-----------|
| macOS | kqueue | Kernel event queue |
| Linux | inotify | Filesystem notification API |
| Windows | ReadDirectoryChangesW | Win32 directory change notifications |

All three backends are event-driven and impose negligible CPU overhead when idle.

---

## Watch Targets

The file watcher monitors two categories of paths:

### Session JSONL Files

Watched **non-recursively**. When an agent is running, it writes events to a
temporary JSONL file. The watcher detects writes to this file and emits
`SessionFileChanged` events, triggering incremental parsing and rendering.

### Worktree Directory

Watched **recursively**. The current worktree's directory tree is monitored for
any file creation, modification, or deletion. Changes emit `WorktreeChanged`
events, which trigger a background file tree refresh and may update the file
viewer if the changed file is currently displayed.

---

## Noise Filtering

Not all filesystem events are meaningful. The watcher discards events from paths
that match known noise patterns before forwarding them to the main loop:

| Pattern | Reason |
|---------|--------|
| `/target/` | Rust build artifacts, extremely high churn during compilation |
| `/.git/` | Git internal files, updated on every commit/checkout |
| `/node_modules/` | JavaScript dependencies, irrelevant to project source |
| `.DS_Store` | macOS Finder metadata |
| `.swp`, `.swo`, `~` | Vim/editor swap and backup files |

Filtering happens at the watcher thread level, before events reach the main
loop's channel. This keeps the channel clean and avoids waking the main loop for
events that would be immediately discarded.

---

## Event Coalescing

Filesystem events often arrive in bursts. A single `cargo build` can produce
hundreds of file change events in `/target/` within milliseconds. Even after
noise filtering, legitimate events can arrive in rapid succession (e.g., an
agent writing multiple files in sequence).

The watcher coalesces events within a **200ms** window:

- At most **one** `SessionFileChanged` event per 200ms window.
- At most **one** `WorktreeChanged` event per 200ms window.

If multiple raw events arrive within the window, they are collapsed into a single
coalesced event. This prevents the main loop from processing redundant file tree
refreshes or re-parsing the same JSONL file multiple times in rapid succession.

---

## Graceful Fallback

If the `notify` backend fails to initialize (e.g., the system has exhausted its
inotify watch limit on Linux, or kqueue file descriptors are unavailable), the
watcher falls back to **stat()-based polling** with a **500ms** interval.

In polling mode, the watcher periodically checks the modification time and size
of watched files. This is less efficient than event-driven watching but provides
the same functionality. A log message is emitted when fallback mode activates, so
the user can diagnose and fix the underlying issue (typically by increasing
`/proc/sys/fs/inotify/max_user_watches` on Linux).

---

## Three-Phase Parse+Render Pipeline

The file watcher is the entry point for the incremental session update pipeline.
When a `SessionFileChanged` event arrives, three phases execute in sequence:

### Phase 1: Change Detection

The file watcher detects that the agent's JSONL output file has been modified.
On event-driven backends, this is immediate. On the stat() fallback, detection
latency is up to 500ms.

### Phase 2: Incremental Parse

The `refresh_session_events()` function seeks to its last known file offset
(`session_file_parse_offset`) and reads only the **newly appended lines** from
the JSONL file. Each line is parsed as a JSON event and converted to a
`DisplayEvent`. Previously processed content is not re-read or re-parsed.

This is the key to performance during long agent sessions. A session with 10,000
events that appends one new event pays the cost of parsing one line, not 10,000.

### Phase 3: Incremental Render

The newly parsed `DisplayEvent` values are sent to the render thread, which
renders only the new content. The rendered output is appended to the existing
cache. The viewport is updated to reflect the new content (auto-scrolling to the
bottom if the user was already at the bottom).

This three-phase pipeline means that the cost of processing a new agent event is
**constant** regardless of session length. A session with 100 events and a
session with 100,000 events both process new events in the same amount of time.
