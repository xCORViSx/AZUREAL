# Event Loop

The main event loop is the heart of AZUREAL. It runs on the main thread,
processes all events from every source, and decides when to redraw the screen.
The design prioritizes input responsiveness above all else -- keystrokes must
feel instant even while agents are streaming thousands of events per second.

---

## Input Reader Thread

Terminal input (keyboard, mouse, resize) is read on a dedicated background
thread. This thread calls `crossterm::event::read()` in a blocking loop and
forwards each event to the main loop via an `mpsc` channel. The dedicated thread
exists so that the main loop never blocks on I/O -- it can always drain pending
events and proceed to rendering.

---

## Event Sources

The main loop receives events from multiple channels:

| Source | Channel | Contents |
|--------|---------|----------|
| Input reader | `input_rx` | Keyboard, mouse, and resize events |
| AgentProcessor | `agent_rx` | Parsed `DisplayEvent` values from agent JSONL |
| FileWatcher | `watcher_rx` | `SessionFileChanged` and `WorktreeChanged` events |
| Terminal rx | `terminal_rx` | PTY output bytes (only polled when `terminal_mode` is active) |
| Render thread | `render_rx` | Completed rendered output with sequence numbers |

---

## Event Batching

The loop does not process one event and then redraw. Instead, it **drains all
pending events** from every channel before considering a redraw. This is critical
for throughput: if an agent emits 50 events between frames, processing them one
at a time would mean 50 separate redraws. Batching collapses them into one.

The drain order is: input events first (highest priority), then agent events,
then file watcher events, then render completions. Input always wins.

### Claude Event Cap

As a safety guard, the loop processes at most **10 agent events per tick**. This
prevents a burst of agent output from starving input handling. If more than 10
events are pending, the remaining events are processed on the next tick.

---

## Motion Discard

Mouse motion events (`MouseEventKind::Moved`) are dropped immediately upon
receipt. These events fire at high frequency (every pixel of mouse movement) and
carry no useful information for AZUREAL's UI. Discarding them at the earliest
possible point prevents them from consuming processing time or triggering
unnecessary redraws.

---

## Conditional Terminal Polling

The embedded terminal's PTY output channel (`terminal_rx`) is only polled when
`terminal_mode` is active. When the terminal is hidden, its channel is ignored
entirely. This avoids waking the main loop for terminal output that would not be
displayed.

---

## Cached Terminal Size

The terminal dimensions (columns and rows) are cached in the `App` struct and
updated only on resize events. Every component that needs the terminal size reads
the cached value rather than calling `crossterm::terminal::size()`, which
involves a system call. The cache is invalidated and refreshed whenever a
`Resize` event arrives.

---

## Fast-Path Input (macOS)

On macOS, AZUREAL uses `fast_draw_input()` for text input rendering. This
function writes the input field directly to the terminal via VT escape sequences,
bypassing ratatui's full `terminal.draw()` call entirely.

The performance difference is significant:

| Path | Latency |
|------|---------|
| `fast_draw_input()` | ~0.1ms |
| `terminal.draw()` | ~18ms |

This means keystrokes in the input field are echoed to the screen roughly 180
times faster. The fast path is used only for input field updates where the rest
of the screen has not changed. Any event that requires a full layout
recalculation falls back to the normal draw path.

This optimization is macOS-only. On Windows, direct VT writes conflict with the
console input parser. On Linux, the standard draw path is fast enough that the
optimization is not needed, though it may be enabled in the future.

---

## Extended Typing Deferral

When the user is actively typing, AZUREAL suppresses `terminal.draw()` calls
for **300ms** after the last keystroke. During this window, only
`fast_draw_input()` updates the input field. If no keystroke arrives within
300ms, the next tick triggers a full redraw to sync the rest of the UI.

This prevents the expensive full-draw path from running on every keystroke during
rapid typing, while ensuring the screen stays current during pauses.

---

## Force Full Redraw

Certain layout changes require a complete screen repaint:

- Opening or closing the git panel (Shift+G)
- Opening or closing overlay panels (health, projects, help)
- Terminal mode toggle

These events set a `force_full_redraw` flag that bypasses all draw suppression
and throttling, ensuring the layout transition renders immediately with a full
`terminal.clear()` before the draw.

---

## Pre-Draw Event Drain with Abort

Immediately before calling `terminal.draw()`, the loop performs one final drain
of the input channel. If any keyboard event is found during this drain, the draw
is **aborted** and the event is processed instead. This prevents the situation
where a keystroke arrives just before a draw begins, forcing the user to wait
18ms for the draw to complete before their input is processed.

---

## Adaptive Draw Throttle

The draw rate adapts to what is happening:

| State | Target FPS | Frame Interval |
|-------|-----------|----------------|
| User interaction (typing, scrolling, navigation) | 30 fps | ~33ms |
| Idle streaming (agent output, no user input) | 5 fps | ~200ms |

During idle streaming, there is no reason to redraw at 30fps -- the agent's
output arrives in bursts and the user is just watching. Dropping to 5fps saves
significant CPU. The moment a keystroke or mouse event arrives, the throttle
switches back to 30fps for immediate responsiveness.

---

## Adaptive Poll Timeout

The `crossterm::event::poll()` timeout also adapts:

| State | Poll Timeout |
|-------|-------------|
| Busy (recent input, active streaming) | 16ms |
| Idle (no input, no streaming) | 100ms |

A shorter poll timeout means faster event pickup at the cost of more CPU usage.
The 100ms idle timeout lets the CPU sleep longer when nothing is happening.

---

## Background Refreshes

File tree discovery and worktree list refresh run in the background and do not
block the event loop. When a `WorktreeChanged` event arrives from the file
watcher, the refresh is scheduled but does not freeze the UI while scanning the
filesystem.

---

## Streaming Deferrals

Two operations are deferred while an agent is actively streaming:

- **Auto-rebase** -- Rebasing while an agent is writing files would corrupt the
  agent's working state. Auto-rebase waits until the stream completes.
- **Health panel refresh** -- Scanning the filesystem during active streaming
  would produce inaccurate results and waste I/O bandwidth. The refresh is
  queued and runs after the stream ends.
