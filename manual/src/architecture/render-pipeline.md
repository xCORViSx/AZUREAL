# Render Pipeline

The render pipeline transforms raw agent events into the styled, wrapped,
syntax-highlighted text that appears in the session pane. Because this work is
computationally expensive -- markdown parsing, syntax highlighting, and Unicode
text wrapping all have nontrivial cost -- it runs on a dedicated background
thread, decoupled from the main event loop.

---

## Background Render Thread

A dedicated render thread receives raw content from the main loop, performs all
parsing and styling, and sends the rendered output back. This keeps the main
loop's draw path fast: it simply blits pre-rendered lines to the terminal rather
than parsing markdown or highlighting code inline.

```text
Main Loop ──→ [render request + seq] ──→ Render Thread
                                              │
                                         parse markdown
                                         highlight code
                                         wrap text
                                              │
Render Thread ──→ [rendered output + seq] ──→ Main Loop
```

---

## Sequence Numbers

Every render request carries a monotonically increasing sequence number. When the
rendered output arrives back on the main loop, the sequence number is checked
against the latest submitted request. If the returned sequence is stale (a newer
request was submitted while this one was processing), the result is **discarded**.

This is the "latest-wins" policy. It handles the common case where the user
scrolls rapidly or an agent emits multiple events in quick succession -- only the
most recent render matters, and older results are dropped without being
displayed.

---

## Incremental Renders

When new agent events arrive during an active session, the render pipeline does
not re-render the entire conversation. It renders **only the newly appended
events**, using a zero-clone approach that avoids copying previously rendered
content.

The rendered output for older events is retained in a cache. New events are
rendered, and their output is appended to the cache. This means the cost of
rendering is proportional to the number of new events, not the total conversation
length.

---

## Render Submit Throttle

The main loop does not submit a render request on every single agent event. A
minimum interval of **50ms** is enforced between render submissions. Events that
arrive within this window are batched and submitted together on the next tick.

This prevents the render thread from being overwhelmed during high-throughput
agent output (e.g., a large file write that produces hundreds of events in
milliseconds).

---

## Viewport Cache

The session pane displays a scrollable view of the rendered conversation. Rather
than re-slicing the full rendered output on every frame, the viewport is
**cached** as a pre-computed slice of lines.

The viewport cache is invalidated and rebuilt only when:

- The user scrolls (scroll position changes).
- New content is appended (total line count changes).
- An animation tick fires (e.g., a streaming cursor blink).

If none of these conditions are met, the cached viewport slice is reused
directly, and the draw call simply blits it to the terminal buffer with no
additional processing.

---

## Deferred Initial Render

When loading a session with a long history (200+ events), rendering all events
upfront would cause a visible delay before the session pane becomes interactive.
AZUREAL handles this by **deferring** the initial render:

1. On session load, only the **last 200 events** are rendered immediately.
2. The session pane becomes interactive as soon as these are ready.
3. If the user scrolls to the top of the conversation, the remaining older events
   are rendered on demand.

This means opening a session with 2000 events feels as fast as opening one with
200 -- the user sees the most recent content immediately and only pays the
rendering cost for older content if they scroll back to view it.

---

## What Gets Rendered

The render thread handles three categories of work:

### Markdown Parsing

Agent responses contain markdown (headings, lists, code blocks, bold/italic
text, links). The render thread parses this into styled spans that ratatui can
display with correct formatting.

### Syntax Highlighting

Code blocks in agent responses are syntax-highlighted using the appropriate
language grammar. The `SyntaxHighlighter` instance is created once and reused
across renders -- it is **never** created in the render path, because
initialization takes 100ms+ (see [Performance](./performance.md)).

### Text Wrapping

All rendered text is pre-wrapped to the current terminal width. This wrapping is
performed once during rendering, not during drawing. The draw path receives
already-wrapped lines and writes them directly to the terminal buffer.

If the terminal is resized, the wrapping width changes and a full re-render is
triggered to rewrap all content at the new width.
