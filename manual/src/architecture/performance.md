# Performance

AZUREAL is designed to feel instant. The performance budget is tight: CPU must
stay below 5% during scrolling, input must be reflected in under 1ms on macOS,
and the application must remain responsive even while agents stream thousands of
events per second. This page documents the performance rules and invariants that
make this possible.

---

## The Core Rule

**Never create expensive objects in the render path.**

The render path is everything between "an event arrives" and "pixels appear on
screen." Any operation in this path directly impacts perceived responsiveness.
The single most important rule is that expensive initialization -- syntax
highlighter creation (100ms+), file I/O, network calls, process spawning -- must
happen outside the render path, either at startup or on a background thread.

---

## Performance Invariants

These are not guidelines. They are invariants that the codebase enforces:

### 1. Cache Rendered Output

The `rendered_lines_cache` stores the fully styled, wrapped output of the
session pane. Drawing reads from this cache. Only new events trigger render work;
previously rendered content is never re-rendered unless the terminal width
changes.

### 2. Decouple Animation from Content Cache

Animations (streaming cursor blink, progress indicators) are patched into the
viewport at draw time without invalidating the content cache. The cursor blink
does not trigger a re-render of the entire conversation -- it patches a single
cell in the viewport slice.

### 3. Skip Redraw When Nothing Changed

Scroll operations return a boolean indicating whether the scroll position
actually changed. If the user is already at the bottom and presses Down, scroll
returns `false` and no redraw occurs. This prevents wasted frames on no-op
inputs.

### 4. Pre-Format Expensive Data at Load Time

Data that is expensive to format (file tree entries, session list items, status
bar components) is formatted once when loaded or changed, not on every draw call.
The draw path reads pre-formatted strings.

### 5. Never Use `.wrap()` on Pre-Wrapped Content

Text wrapping is performed once during rendering. The ratatui `Paragraph` widget
is given pre-wrapped lines and is **not** configured with `.wrap()`, which would
perform a redundant wrapping pass on already-wrapped content.

### 6. Cache Edit Mode Highlighting Per Version

When the user edits a file in edit mode, syntax highlighting is cached per
edit version (a monotonically increasing counter). Highlighting is recomputed
only when the content changes, not on every cursor movement or draw cycle.

### 7. File I/O is Safe on the Render Thread

The render thread may read files from disk (e.g., for syntax detection or
grammar loading), but file I/O is **never** performed on the draw path. The
distinction matters: the render thread runs in the background and does not block
frame output. The draw path runs on the main thread and must complete within the
frame budget.

---

## CPU Budget

| Scenario | Target CPU | Notes |
|----------|-----------|-------|
| Idle (no input, no streaming) | <1% | 100ms poll timeout, no draw calls |
| Scrolling | <5% | Viewport cache hit, no re-render |
| Active typing | <3% | `fast_draw_input()` on macOS, deferred full draw |
| Agent streaming | <8% | 5fps draw throttle, incremental render |
| Agent streaming + user scrolling | <12% | 30fps draw, viewport cache rebuild |

These targets assume a modern machine (2020+ CPU). The primary lever for CPU
reduction is draw throttling -- the adaptive 5fps/30fps system described in
[Event Loop](./event-loop.md) is the single largest contributor to low idle CPU
usage.

---

## What To Watch For

Common performance mistakes and how AZUREAL avoids them:

### SyntaxHighlighter::new() in a Loop

Creating a syntax highlighter loads grammar definitions and compiles them. This
takes 100ms+ and must happen **once**, at startup. The highlighter is stored as a
long-lived value and reused across all renders.

### Allocating in the Draw Path

The draw path should allocate as little as possible. Pre-rendered lines are
stored as `Vec<Line>` and sliced by reference for the viewport. No new `String`
or `Vec` allocations occur per frame for content that has not changed.

### Unnecessary Full Redraws

A full `terminal.draw()` call costs ~18ms. The fast-path input optimization
(~0.1ms) and the skip-redraw-on-no-change logic exist specifically to avoid
paying this cost when it is not needed. Every code path that might trigger a
redraw must justify why a full draw is necessary rather than a partial update or
no update at all.

### Blocking the Main Loop

The main loop must never block. All I/O (file reads, process spawning, git
commands) runs on background threads or is dispatched asynchronously. A blocked
main loop means frozen input handling, which is the most user-visible performance
failure.
