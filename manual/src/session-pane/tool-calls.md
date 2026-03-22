# Tool Call Display

When an agent invokes a tool (Read, Edit, Bash, Grep, and so on), the session
pane renders the invocation as a timeline node beneath the assistant message.
Each node shows the tool name, its primary parameter, and a status indicator
that updates in real time.

---

## Timeline Layout

Tool calls render as a vertical timeline connected by AZURE (`#3399FF`) pipe
characters:

```text
 ┃
 ┃ ● Read  /src/main.rs
 ┃  │ fn main() {
 ┃  │   (42 lines)
 ┃  └─ }
 ┃
 ┃ ○ Edit  /src/lib.rs
```

Each tool call node consists of:

1. **Status indicator** -- a colored circle or symbol (see below).
2. **Tool name** -- a display-friendly name (e.g., "Read", "Edit", "Bash").
3. **Primary parameter** -- the most relevant input for that tool type.

The primary parameter is extracted per-tool: file tools (Read, Edit, Write)
show the file path; Bash shows the command; Grep shows the search pattern;
and so on.

---

## Status Indicators

Each tool call displays one of three status indicators, patched at draw time
from the current tool state:

| Indicator | Color | Meaning |
|-----------|-------|---------|
| `●` | Green | Tool completed successfully |
| `○` | Pulsating white/gray | Tool is currently executing |
| `✗` | Red | Tool failed or returned an error |

**Pulsation:** The pending indicator (`○`) cycles through white, gray, dark
gray, and back to gray on a timer, creating a subtle animation that signals
activity without being distracting. The cycle advances every 2 animation ticks.

---

## Draw-Time Patching

Tool status indicators are not baked into the render cache at render time.
Instead, every tool call's line index and span index are recorded in an
`animation_line_indices` array. During viewport construction (which runs every
frame), the viewport builder patches the indicator character and color based on
the current state of `pending_tool_calls` and `failed_tool_calls`:

- If the tool's ID is in `pending_tool_calls`, patch to `○` with pulsating
  color.
- If the tool's ID is in `failed_tool_calls`, patch to `✗` in red.
- Otherwise, patch to `●` in green.

This approach means that when a tool completes or fails between full renders,
the status updates immediately on the next frame without waiting for a
background re-render of the entire line cache.

---

## Error Detection

Tool failure is detected through two mechanisms:

1. **`is_error` field** -- the stream-json output includes an `is_error`
   boolean on tool results. When true, the tool is added to the
   `failed_tool_calls` set.

2. **Fallback heuristic** -- when the `is_error` field is absent or ambiguous,
   the result content is checked for known error patterns:
   - Contains `<tool_use_error>` tag
   - Starts with `"Error..."`
   - Contains `"ENOENT"` (file not found)

   These patterns catch common failure modes that may not set the explicit
   error flag.

---

## File Tool Paths

Tool calls for file operations (Read, Edit, Write) render the file path with
underlined orange styling, making it visually distinct and clickable. Clicking
the path opens the file in the Viewer pane. For Edit tool calls, the click
also loads the diff context. See [Clickable Elements](./clickable-elements.md).

When a file path is too long to fit on one line, it wraps across multiple
lines. The clickable region spans all wrapped lines, and the click handler
knows how many cache lines the path occupies.

---

## Tool Result Display

Beneath the tool call node, a summarized result is shown. Each tool type has
its own result format optimized for scannability. See
[Tool Result Formats](./tool-results.md) for the per-tool breakdown.

The result lines use the same AZURE pipe gutter as the tool call, with
indented connector characters (`│` for continuation, `└─` for the final line)
to visually group the result with its tool call.

---

## Hook Lines

When a tool call triggers permission hooks or validation checks, the hook
output appears as dim gray lines near the tool call. Hooks are prefixed with
`›` and show the hook name followed by its output. Consecutive duplicate hook
lines are deduplicated.
