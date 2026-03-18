# Session List & Search

Pressing `s` replaces the session pane conversation with a full-pane session
browser. The list is scoped to the current worktree -- only sessions belonging
to the active worktree's branch are shown.

---

## Layout

Each row in the session list displays:

| Element | Position | Description |
|---------|----------|-------------|
| Status dot | Left | Green `●` for running sessions, gray `○` for idle |
| Session name | Left (after dot) | Custom name or truncated UUID |
| Last modified | Right-aligned | Relative or absolute timestamp |
| Message count | Right (badge) | `[N msgs]` badge showing total messages |

The currently selected row is highlighted with an AZURE background and black
text. The session name on the selected row renders bold.

---

## Navigation

| Key | Action |
|-----|--------|
| `j` / Down | Move selection down one row |
| `k` / Up | Move selection up one row |
| `J` | Page down (jump by viewport height) |
| `K` | Page up (jump by viewport height) |
| `Enter` | Load the selected session |
| `a` | Start a new session |
| `s` / `Esc` | Close the session list and return to conversation |

---

## Two-Phase Loading

Opening the session list uses a two-phase approach to avoid blocking the UI:

1. **Immediate:** A centered "Loading sessions..." dialog appears on the first
   frame after `s` is pressed.
2. **Background:** Message counts are computed and the full list renders once
   counting is complete.

Message counts are computed via fast string scanning of session files -- no
JSON parsing is performed. This keeps the count computation fast even for large
session files.

---

## Name Filter

Pressing `/` activates a filter bar at the top of the session list. Typing
filters sessions by name (case-insensitive substring match). The filter bar
has a yellow border when active, gray when inactive. Press `Esc` to deactivate
the filter input (the filter text persists and continues filtering). Press
`Esc` again or `s` to close the session list entirely.

---

## Content Search

Typing `//` (two slashes) switches from name filtering to content search mode.
Content search looks inside session file contents rather than matching names:

- **Minimum query length:** 3 characters (no search runs below this).
- **Result cap:** 100 results maximum.
- **File size limit:** Files larger than 5 MB are skipped.

The filter bar shows the search mode prefix (`//`) and a result count badge
on the right side (e.g., `42 results`).

Content search results display differently from the normal session list. Each
row shows:

- The session name (or truncated session ID if no custom name exists).
- A preview of the matching content, truncated to fit the available width.

The selected row uses the same AZURE highlight style. Pressing `Enter` loads
the session containing the selected match.

---

## Rename Dialog

While in the session list, a rename overlay can appear as a centered input box
over the list. This allows renaming the selected session without leaving the
browser.
