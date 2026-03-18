# Mouse Support

AZUREAL supports mouse interaction across every pane. Click to focus, scroll
to navigate content, double-click to open files, and drag to select text.

## Click Behavior

All pane rectangles are cached during each render frame, enabling precise
hit-testing for every mouse event.

### Worktree Tab Row

- **Click a tab** to switch to that worktree. The tab row stores per-tab
  screen x-ranges during rendering, so clicks resolve to the correct tab even
  with variable-width labels.

### FileTree

- **Click an entry** to highlight it.
- **Double-click a file** to open it in the Viewer.
- **Double-click a directory** to expand or collapse it.
- Double-click detection uses a 500ms window at the same screen position.

### Viewer

- **Click** to focus the Viewer pane.
- **Drag** to select text (see
  [Text Selection & Copy](./text-selection.md)).

### Session

- **Click** to focus the Session pane.
- **Drag** to select text within the conversation output.

### Input / Terminal

- **Click** to focus and enter prompt mode. The cursor is positioned at the
  clicked location using word-wrap-aware coordinate mapping -- the click
  position on screen is translated through the wrapping break points to the
  correct character index in the input buffer.

### Status Bar

- **Click the center section** to copy the current status message to the
  system clipboard.

### Overlays

- **Click outside an overlay** (help, branch dialog, run command picker) to
  dismiss it. The click is consumed by the dismissal -- it does not pass
  through to the pane underneath.

## Scroll Behavior

Scrolling works in any pane that has scrollable content:

| Pane | Scroll effect |
|------|--------------|
| FileTree | Scrolls the directory listing |
| Viewer | Scrolls the file content or diff |
| Session | Scrolls the conversation output |
| Terminal | Scrolls the terminal scrollback buffer |
| Todo Widget | Scrolls the todo checklist |

Scroll events are routed to whichever pane the mouse cursor is over,
regardless of which pane currently has keyboard focus. This means you can
scroll the Session pane while the Input pane has focus, for example.

## Hit-Testing Architecture

During each render pass, AZUREAL caches the screen rectangles for all panes:

- `pane_worktree_tabs` -- the tab row area
- `pane_worktrees` -- the FileTree area
- `pane_viewer` -- the Viewer area
- `pane_session` -- the Session pane area
- `pane_todo` -- the Todo Widget area (when visible)
- `input_area` -- the Input/Terminal area
- `pane_status` -- the Status Bar area

Mouse events are tested against these rects using
`Rect::contains(Position::new(col, row))`. The same rect cache is shared
between click and scroll handlers, so hit-testing is consistent across all
interaction types.

The Worktree Tab Row additionally stores a
`worktree_tab_hits: Vec<(u16, u16, Option<usize>)>` mapping each tab's screen
x-range to its worktree index (`None` for the main-browse tab, `Some(idx)`
for feature worktrees). This is rebuilt during `draw_worktree_tabs()` each
frame.
