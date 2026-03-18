# File Tree

The File Tree occupies the left 15% of the terminal and is always visible. It
displays the directory structure of the currently selected worktree, serving as
the primary entry point for opening files in the Viewer.

---

## Border Title

The File Tree's border title follows the format:

```text
Filetree (worktree_name)                    [pos/total]
```

- **Left:** The literal text `Filetree` followed by the worktree's display name
  in parentheses.
- **Right:** A scroll indicator showing the cursor position and total entry
  count, displayed as `[pos/total]`. This only appears when the tree content
  overflows the visible area.

---

## Nerd Font Icons

Every file and directory in the tree is prefixed with an icon. AZUREAL maps
approximately 60 file types to specific Nerd Font glyphs, each rendered in the
language's brand color:

| File Type | Color | Example |
|-----------|-------|---------|
| Rust (`.rs`) | Orange | Rust gear glyph |
| Python (`.py`) | Blue | Python logo glyph |
| JavaScript (`.js`) | Yellow | JS logo glyph |
| TypeScript (`.ts`) | Blue | TS logo glyph |
| Go (`.go`) | Cyan | Go gopher glyph |
| Markdown (`.md`) | White | Markdown glyph |
| TOML (`.toml`) | Gray | Config glyph |
| Directory (expanded) | Blue | Open folder glyph |
| Directory (collapsed) | Blue | Closed folder glyph |

This is a representative subset. The full mapping covers common source files,
configs, data formats, images, lock files, and more.

### Auto-Detection

Nerd Font availability is detected automatically via `detect_nerd_font()`. This
function runs during the splash screen and works by probing a Private Use Area
(PUA) glyph -- if the terminal can render it, Nerd Font mode is enabled. If the
probe fails, the tree falls back to **emoji icons** (for example, a folder emoji
for directories, a page emoji for generic files). The detection runs once per
application launch.

---

## Navigation

| Key | Action |
|-----|--------|
| `j` / Down | Move cursor down one entry |
| `k` / Up | Move cursor up one entry |
| `Enter` | Open file in Viewer, or expand/collapse directory |
| `l` / Right | Expand directory |
| `h` / Left | Collapse directory |
| `g` | Jump to first entry |
| `G` | Jump to last entry |

Double-clicking a file opens it in the Viewer. Double-clicking a directory
toggles its expanded/collapsed state.

---

## Options Overlay

Pressing `O` (Shift+O) while the File Tree is focused opens the **Options
overlay** -- a checkbox list for controlling which entries are visible in the
tree.

### Filter Targets

| Entry | Default |
|-------|---------|
| Worktrees directory | Hidden |
| `.git` | Hidden |
| `.claude` | Hidden |
| `.azureal` | Hidden |
| `.DS_Store` | Hidden |

Each entry is a toggle. Checked entries are visible; unchecked entries are
filtered out of the tree. Navigate the list with `j`/`k` and toggle with
`Space` or `Enter`. Press `Esc` or `O` again to close the overlay.

### Persistence

Filter settings are persisted to the project's `azufig.toml` configuration
file. Changes take effect immediately and survive application restarts. See
[Project Config](../configuration/project.md) for details on the config file
format.

---

## Directory Behavior

Directories display with an expand/collapse indicator. When expanded, their
children are indented below them with a visual tree guide. Collapsed directories
show only the directory name with a closed-folder icon.

The tree is populated from disk on worktree selection and updated by the file
watcher when changes occur. See [File Watcher](../architecture/file-watcher.md)
for details on how filesystem events propagate to the tree.

---

## Worktree Scoping

The File Tree is always scoped to a single worktree. When you switch worktrees
(via `[`/`]`, clicking a worktree tab, or pressing `Shift+M` for main browse),
the tree reloads from the new worktree's root directory. There is no
cross-worktree file browsing -- each worktree is an isolated view of its own
working directory.
