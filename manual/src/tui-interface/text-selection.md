# Text Selection & Copy

AZUREAL supports mouse-driven text selection with clipboard integration.
Drag to select text in the Viewer, Session, Input, or Edit Mode, then copy
with a platform-appropriate shortcut.

## Selecting Text

**Click and drag** within a pane to create a selection. The selection model
works as follows:

1. **Mouse down** records the anchor point, converting screen coordinates to
   cache coordinates immediately. The anchor is tagged with a pane identifier
   (Viewer, Session, Input, or Edit Mode) so that drag events route to the
   correct handler.

2. **Mouse drag** extends the selection from the anchor to the current cursor
   position. Only the cursor position is re-mapped from screen to cache
   coordinates on each drag event -- the anchor remains fixed in cache space,
   so auto-scrolling during drag does not shift the selection start.

3. **Mouse up** finalizes the selection.

Selections are stored as a four-tuple:

```text
(start_line, start_col, end_line, end_col)
```

Values are normalized so that start is always before or equal to end,
regardless of drag direction.

## Auto-Scroll

When dragging above or below a pane's visible content area, the pane
automatically scrolls in that direction. Because the anchor is stored in cache
coordinates (not screen coordinates), the starting point of the selection
remains stable as the viewport moves.

## Pane-Specific Behavior

### Viewer

- Selection highlighting is applied per-line via `apply_selection_to_line()`.
- **Line number gutter is excluded from selection.** When copying, the gutter
  (line numbers) is stripped so only file content reaches the clipboard.
- In markdown preview mode (no line numbers), selection covers the full
  rendered width.

### Session

- Selection respects **content bounds** -- bubble chrome is excluded.
  Gutters, borders, headers, and decorative elements (the orange `|` gutter,
  AZURE `|` border, colored-background headers, bottom borders, code fences)
  are not selectable.
- Each cache line's selectable region is computed by
  `compute_line_content_bounds()`. Lines that are entirely decorative
  (bounds `(0, 0)`) are skipped during selection highlighting.
- Copy extracts only the textual content within those bounds, trims leading
  and trailing blank lines, and skips decoration lines.

### Input

- Selection maps screen coordinates to character indices using
  `screen_to_input_char()`, accounting for word-wrap break points.
- The fast-draw path for the input pane is bypassed when a selection is
  active, ensuring selection styling renders correctly.

### Edit Mode

- When the Viewer is in edit mode, clicks and drags use
  `screen_to_edit_pos()` to map screen coordinates to source-line and
  source-column positions, walking source lines and summing wrap counts.
- Edit mode selections are stored separately as
  `viewer_edit_selection: Option<(anchor_line, anchor_col, drag_line, drag_col)>`
  and move the edit cursor to the drag endpoint.

## Copying

**Cmd+C** (macOS) or **Ctrl+C** (Windows / Linux) copies selected text from
whichever pane has an active selection.

| Pane | Copy behavior |
|------|--------------|
| Viewer | Copies file content; line number gutter stripped |
| Session | Copies conversation text; bubble chrome excluded |
| Input | Copies the selected portion of the prompt input |
| Edit Mode | Copies the selected source text |
| Git Mode (Viewer) | Copies diff content (gutter=0, no line numbers to strip) |
| Git Mode (Status Box) | Copies the result message when the status box is selected |

Clipboard access uses the `arboard` crate for cross-platform system clipboard
integration.

### Git Mode

In Git Mode, **Cmd+C** and **Cmd+A** are intercepted before the git panel's
own input handler. If no viewer selection exists, **Cmd+C** falls back to
copying the `result_message` from the git status box. **Cmd+A** selects the
status box when the viewer cache is empty.

## Clearing Selections

Selections are cleared automatically when any of the following occurs:

- **Click** anywhere (a new click starts fresh).
- **Scroll** in any pane.
- **Tab** or **Shift+Tab** (focus change).
- **Focus change** via mouse click on a different pane.

This ensures stale selections do not linger after navigation.

## Selection Rendering

Selected text is highlighted with an `Rgb(60, 60, 100)` background color.
The highlighting is applied by splitting styled spans at the selection
boundaries and patching the background color of the overlapping region.
This runs at O(spans-in-line) per visible line -- negligible rendering cost.
