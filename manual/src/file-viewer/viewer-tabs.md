# Viewer Tabs

Viewer Tabs let you pin frequently accessed files for quick switching. Up to 12
files can be tabbed at once, displayed in two rows of six across the top of the
Viewer pane.

---

## Tab Bar Layout

The tab bar renders **inside the Viewer border** at rows 1 and 2 (immediately
below the top border). Each row holds up to 6 tabs at a fixed width. The layout
looks like this when tabs are active:

```text
┌─ Viewer ──────────────────────────────┐
│ [main.rs] [lib.rs] [mod.rs]          │
│ [config.toml] [README.md]            │
│───────────────────────────────────────│
│                                       │
│  (file content starts here)           │
│                                       │
└───────────────────────────────────────┘
```

When tabs are present, the file content area shifts down by 2 rows to
accommodate the tab bar. When no tabs exist, those rows are reclaimed by the
content area.

---

## Keybindings

| Key | Action |
|-----|--------|
| `t` | Save current file to a new tab |
| `Alt+t` | Open tab dialog |
| `[` | Switch to previous tab |
| `]` | Switch to next tab |
| `x` | Close current tab |

### Saving a Tab

Pressing `t` while viewing a file saves that file's path to a new tab. The tab
appears at the next available slot. If the file is already tabbed, no duplicate
is created.

### Tab Dialog

`Alt+t` opens a tab dialog overlay that shows all current tabs in a navigable
list. You can select a tab from the dialog to switch to it, or manage tabs from
within the overlay.

### Navigating Tabs

`[` and `]` cycle through open tabs in order. Navigation wraps at both ends --
pressing `]` on the last tab moves to the first, and pressing `[` on the first
tab moves to the last.

When you navigate to a tab, the Viewer loads that file's content immediately.
The file tree cursor does not move -- tabs provide an independent navigation
path from the tree.

### Closing Tabs

`x` closes the currently active tab. If the closed tab was the one being
viewed, the Viewer switches to the next available tab. If no tabs remain, the
Viewer returns to its default empty state.

---

## Tab Limit

A maximum of **12 tabs** is enforced (6 per row, 2 rows). Attempting to save a
13th tab displays a status message indicating the tab limit has been reached.
Close an existing tab with `x` to free a slot before adding a new one.

---

## Tab Persistence

Viewer tabs are associated with the current worktree. Switching worktrees does
not carry tabs across -- each worktree maintains its own tab set. Tabs persist
for the duration of the session but are not saved to disk across application
restarts.
