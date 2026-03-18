# Diff Viewer

When an agent makes file edits via the Edit tool, the Viewer switches to
**diff mode** to display the changes inline. Diffs are syntax-highlighted and
color-coded, giving you immediate visual feedback on what the agent changed.

---

## When Diffs Appear

Diffs are generated from **Edit tool calls** in agent sessions. When the Session
pane displays a tool call that modified a file, selecting that tool call (or
navigating to it) loads the corresponding diff into the Viewer. The Viewer
title updates to reflect the file being diffed.

---

## Visual Format

Diffs are displayed as **inline diffs** -- removed and added lines are shown
interleaved at their actual file positions, not in a side-by-side layout.

### Removed Lines

- **Text color:** Dark grey.
- **Background:** Dim red.
- **Syntax highlighting:** None. Removed lines are rendered in flat grey text
  to visually de-emphasize them.

### Added Lines

- **Text color:** Full syntax highlighting (using the file's tree-sitter
  grammar).
- **Background:** Dim green.
- **Syntax highlighting:** Yes. Added lines receive the same highlighting they
  would have in normal file view, making it easy to read the new code.

### Unchanged Lines

Context lines surrounding the diff are rendered with normal syntax highlighting
and no background color, providing visual anchoring for the changes.

---

## Line Numbers

Diff view displays **real file line numbers** in the gutter. These correspond
to the line numbers in the actual file on disk, not sequential diff-line
indices. This means you can reference a specific line number from the diff and
find it directly in the source file.

---

## Navigating Between Edits

When an agent session contains multiple Edit tool calls, you can cycle through
them:

| Key | Action |
|-----|--------|
| `Alt+Left` | Previous edit |
| `Alt+Right` | Next edit |

Each navigation step loads the next (or previous) diff into the Viewer,
updating the displayed file, line numbers, and diff highlights.

---

## Scroll Correction

When a diff is loaded, the Viewer **auto-scrolls to the diff location** so the
changed lines are visible immediately. The scroll calculation accounts for
word-wrap -- if long lines in the file are wrapped across multiple visual rows,
the scroll offset is adjusted so the diff target appears at the correct screen
position rather than being pushed offscreen by wrapped lines above it.

This ensures that regardless of file length or line wrapping, navigating to a
diff always places the changes in view.
