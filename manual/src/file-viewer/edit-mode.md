# Edit Mode

Edit mode turns the Viewer into a text editor. You can make changes to files
directly within AZUREAL without switching to an external editor or the embedded
terminal.

---

## Entering and Exiting

| Key | Action |
|-----|--------|
| `e` | Enter edit mode (from command mode, with a file open in the Viewer) |
| `Esc` | Exit edit mode and return to command mode |

Edit mode is only available for text files. Pressing `e` on an image has no
effect. For markdown files, entering edit mode replaces the prettified preview
with raw syntax-highlighted source (see
[Markdown Preview](./markdown-preview.md)).

---

## Visual Indicator

When edit mode is active, the Viewer border changes from a **solid line to a
dashed line**. This provides an immediate visual distinction between read mode
(solid border) and edit mode (dashed border), matching the visual language used
elsewhere in AZUREAL for mutable state.

---

## Text Editing

Edit mode supports standard text editing operations:

| Key | Action |
|-----|--------|
| Any printable character | Insert at cursor |
| `Backspace` | Delete character before cursor |
| `Delete` | Delete character after cursor |
| Left / Right | Move cursor one character |
| Up / Down | Move cursor one visual line |
| `Home` | Move cursor to start of line |
| `End` | Move cursor to end of line |
| `Enter` | Insert newline |

### Word-Boundary Wrapping

Long lines are wrapped at word boundaries to fit the Viewer width. The cursor
navigates through **visual lines** (wrapped rows), not just source lines. This
means:

- **Up/Down** moves through wrapped visual lines, not source lines. If a single
  source line wraps into three visual rows, pressing Down three times moves
  through all three rows before reaching the next source line.
- **Home** moves to the start of the current source line. **End** moves to the
  end of the current source line.

The wrap-aware cursor ensures that navigation feels natural regardless of line
length.

---

## Undo and Redo

| Platform | Undo | Redo |
|----------|------|------|
| macOS | `Cmd+Z` | `Cmd+Shift+Z` |
| Linux / Windows | `Ctrl+Z` | `Ctrl+Y` |

The undo/redo stack records each editing action (insertions, deletions,
replacements). The stack is capped at **100 entries** -- once the cap is
reached, the oldest entries are discarded as new ones are added.

Undo steps back through the history one action at a time. Redo moves forward
through undone actions. Performing a new edit after an undo discards all
redo-able entries beyond the current point (standard undo-tree behavior).

---

## Saving

| Platform | Save |
|----------|------|
| macOS | `Cmd+S` |
| Linux / Windows | `Ctrl+S` |

Pressing save opens a **confirmation dialog** before writing to disk. Confirm
with `Enter` (or `y`) to write, or cancel with `Esc` (or `n`) to return to
editing without saving.

---

## Discarding Changes

When you press `Esc` to exit edit mode while unsaved changes exist, a **discard
dialog** appears:

- **Discard** (`Enter` or `y`) -- Discards all changes and returns to read-only
  view with the original file content.
- **Cancel** (`Esc` or `n`) -- Returns to edit mode so you can continue editing
  or save first.

If there are no unsaved changes, `Esc` exits edit mode immediately without a
dialog.

---

## Syntax Highlighting in Edit Mode

Syntax highlighting remains active during editing. The highlighting cache is
invalidated and regenerated **per edit version** -- each time the buffer changes,
the tree-sitter AST is incrementally re-parsed and the highlight cache is
updated. Crucially, this happens per version, not per frame, so rapid typing
does not cause redundant re-parses.

---

## Mouse Support

- **Click** positions the cursor at the clicked location. The click coordinates
  are converted from screen space to source-line and source-column, accounting
  for word-wrap offsets.
- **Drag** creates a text selection from the click point to the drag endpoint.
  See [Text Selection & Copy](../tui-interface/text-selection.md) for details on
  selection behavior in edit mode.

---

## Speech-to-Text Integration

When [Speech-to-Text](../speech-to-text.md) is used while edit mode is active,
the transcribed text is **inserted at the current cursor position** rather than
being placed in the prompt input. This lets you dictate directly into the file
you are editing.
