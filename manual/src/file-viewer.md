# The File Browser & Viewer

The left half of AZUREAL's interface is dedicated to browsing and viewing the
files in your worktree. It is composed of two panes -- the **FileTree** on the
left (15% of terminal width) and the **Viewer** in the center (50%) -- that
work together as a unified file exploration system. The FileTree lets you
navigate the directory structure, while the Viewer renders file content with
full syntax highlighting, markdown preview, image display, and inline diff
viewing.

---

## Architecture

```text
┌──────────┬───────────────────────────┐
│          │  [tab1] [tab2] [tab3]     │
│ FileTree │───────────────────────────│
│  (15%)   │                           │
│          │         Viewer (50%)      │
│          │                           │
│          │                           │
└──────────┴───────────────────────────┘
```

The FileTree always reflects the currently selected worktree. When you switch
worktrees via `[`/`]` or by clicking a tab in the worktree row, the FileTree
swaps to show that worktree's directory contents, and the Viewer resets to its
default state for that worktree.

---

## Capabilities at a Glance

| Feature | Description |
|---------|-------------|
| **File Tree** | Directory browser with Nerd Font icons, expand/collapse, filtering options |
| **Syntax Highlighting** | Tree-sitter based highlighting for 25 languages |
| **Markdown Preview** | Prettified rendering of `.md` files with styled headers, tables, and code blocks |
| **Image Viewer** | Terminal graphics protocol rendering for PNG, JPG, GIF, BMP, WebP, and ICO |
| **Diff Viewer** | Inline diffs from agent Edit tool calls with syntax highlighting |
| **Viewer Tabs** | Up to 12 pinned file tabs across 2 rows |
| **Edit Mode** | Full text editing with undo/redo, word-wrap, and save |
| **File Actions** | Add, delete, rename, copy, and move files directly from the tree |

---

## Interaction Model

The FileTree and Viewer follow the standard focus and navigation model described
in [Focus & Navigation](./tui-interface/focus-navigation.md). In command mode,
`j`/`k` scroll through the file tree or viewer content, `Enter` opens a file
from the tree into the Viewer, and arrow keys or `h`/`l` expand and collapse
directories.

The Viewer supports multiple display modes depending on what is loaded:

- **Source view** -- syntax-highlighted code with line numbers.
- **Markdown preview** -- rendered markdown without line numbers.
- **Image view** -- terminal graphics rendering, no scroll or selection.
- **Diff view** -- inline diffs with red/green backgrounds and real line numbers.
- **Edit mode** -- full text editing with a dashed border indicator.

Only one display mode is active at a time. The mode is determined automatically
by file type, with edit mode toggled manually via `e`.

---

## Chapter Contents

- **[File Tree](./file-viewer/file-tree.md)** -- The directory browser: Nerd
  Font icons, filtering options, and scroll indicators.
- **[Syntax Highlighting](./file-viewer/syntax-highlighting.md)** -- Tree-sitter
  grammars, language detection, and capture-to-color mappings.
- **[Markdown Preview](./file-viewer/markdown-preview.md)** -- Prettified
  rendering of markdown files with styled elements.
- **[Image Viewer](./file-viewer/image-viewer.md)** -- Terminal graphics
  protocol support for image files.
- **[Diff Viewer](./file-viewer/diff-viewer.md)** -- Inline diff display from
  agent edit operations.
- **[Viewer Tabs](./file-viewer/viewer-tabs.md)** -- Pinning files to tabs for
  quick access.
- **[Edit Mode](./file-viewer/edit-mode.md)** -- In-place file editing with
  undo, redo, and save.
- **[File Actions](./file-viewer/file-actions.md)** -- Creating, deleting,
  renaming, copying, and moving files from the tree.
