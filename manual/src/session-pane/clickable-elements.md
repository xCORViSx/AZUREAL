# Clickable Elements

The session pane supports mouse-driven interactions on several types of
rendered content. Clickable regions are registered during the background render
pass and stored as coordinate ranges that the mouse handler checks on each
click event.

---

## Tool Call File Paths

File paths in tool call headers (Read, Edit, Write) are rendered as underlined
orange text. Clicking a path opens the corresponding file in the Viewer pane.

**Read/Write paths:** A single click opens the file for viewing with syntax
highlighting and line numbers.

**Edit paths:** A single click opens the diff view in the Viewer, with the
old and new strings from the Edit tool pre-loaded. This lets you see exactly
what the agent changed without navigating to the file manually.

**Clickable region:** Each path stores its cache line index, start column, end
column, the file path string, and (for Edit tools) the old/new edit strings.
When a path wraps across multiple lines, the `wrap_line_count` field tells the
click handler how many cache lines to check.

**Hover highlight:** When the mouse hovers over a clickable file path, the
path region is highlighted to indicate it is interactive.

---

## Assistant Markdown File Links

When the assistant mentions file paths in its prose text (outside of tool
calls), the renderer detects and styles them as underlined orange links. These
are registered as clickable regions in the same way as tool call paths.

Clicking an assistant-mentioned file path opens it in the Viewer. If the path
references a file that was recently edited, the click may also show the diff.

---

## Tables

Rendered markdown tables are registered as clickable regions. Each table stores
its cache line range (start and end) and the raw markdown source text.

Clicking anywhere within a rendered table opens a full-width popup that
re-renders the table without column width constraints. This is useful for wide
tables that had columns truncated to fit within the session pane's bubble
width. The popup renders the table at the full terminal width, showing
complete cell content.

---

## Status Bar Center

The status bar's center section is clickable. Clicking it copies the current
status message to the system clipboard. This is useful for grabbing error
messages, file paths, or other transient status information that would
otherwise require manual selection.

The status bar stores its screen rectangle during each render frame, enabling
precise hit-testing for the click.

---

## Click Detection

All clickable regions are tracked through typed tuples stored alongside the
render cache:

- **`ClickablePath`:** `(line_idx, start_col, end_col, file_path, old_string,
  new_string, wrap_line_count)` -- identifies a clickable file path with
  optional Edit diff context.
- **`ClickableTable`:** `(cache_line_start, cache_line_end,
  raw_markdown_text)` -- identifies a clickable table region.

On a mouse click, the event handler translates the screen coordinates to cache
line and column positions (accounting for scroll offset), then checks against
the stored regions. The first matching region triggers the corresponding
action.
