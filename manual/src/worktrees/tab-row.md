# Worktree Tab Row

The tab row is a single horizontal bar at the top of the layout showing all
worktrees as clickable tabs. It replaces the older sidebar design, giving more
horizontal space to the three-pane layout below.

---

## Layout

The `[★ main]` tab is always first. Feature worktrees follow in the order they
were discovered by `git worktree list`. Each tab shows a status icon prefix
followed by the worktree's display name (the branch suffix after `azureal/`).

Tabs are separated by `│` dividers. The divider color is AZURE when the tab row
has focus, dark gray otherwise.

---

## Status Icons

Every tab has a leading icon that communicates the worktree's current state at a
glance. Icons follow a strict priority order -- the first matching condition
wins:

| Icon | Color | Condition | Meaning |
|------|-------|-----------|---------|
| `●` | Green | Agent process is running | An active agent session is streaming |
| `◐` | AZURE | Unread session finished | A session completed while you were viewing a different worktree |
| `◇` | Dim gray | Worktree is archived | Working directory removed, branch preserved |
| `○` | Gray | Default idle state | No agent running, nothing unread |

The running check (`●`) always takes priority. If an agent is actively streaming
and the session also has unread events, you see the green filled circle, not the
half-filled unread indicator.

### Unread Tracking

When any agent session finishes on a worktree you are not currently viewing, that
worktree's tab shows `◐` in AZURE. The unread indicator clears per-session when
you view that specific session, and the branch-level `◐` disappears only when
all unread sessions on the branch have been viewed. Unread state only clears
when the session pane is visible (normal mode or after closing the git panel).

---

## Tab Styling

| State | Foreground | Background | Modifier |
|-------|-----------|-----------|----------|
| Active feature worktree | White | AZURE (`#3399FF`) | Bold |
| Active `★ main` | Black | Yellow | Bold |
| Running (not selected) | Green | -- | -- |
| Unread (not selected) | AZURE | -- | -- |
| Archived | Dim gray | -- | -- |
| Inactive `★ main` | Yellow | -- | -- |
| Inactive feature worktree | Gray / Dark gray | -- | -- |

When the tab row itself has focus, inactive tabs use `Gray`; when another pane
is focused, they use `DarkGray`.

---

## Navigation

### Keyboard

- `[` / `]` -- switch to the previous / next worktree tab. These keys are
  **global** and work from any pane (file tree, viewer, session, input,
  terminal). Navigation wraps around at both ends.
- When browsing main, `[` exits to the last worktree and `]` exits to the
  first.

### Mouse

Click any tab to select it. Click `★ main` to enter main browse mode. Tab
hit-test regions are cached during each render for accurate click detection.

---

## Pagination

When tabs do not fit in the available width, they are packed into pages using a
greedy algorithm. The page containing the active tab is always shown. A dim
`N/M` indicator appears at the end of the row (e.g., `2/3` means page 2 of 3).

Use `{` / `}` (in the git panel) to jump between pages. In normal mode, `[`
and `]` cycle through worktrees and automatically advance the page when crossing
a page boundary.

---

## Auto-Rebase Indicator

Worktrees with auto-rebase enabled show an `R` suffix after their tab label.
The `R` is bold and color-coded to reflect the current rebase state:

| Color | Meaning |
|-------|---------|
| Green | Auto-rebase enabled, idle |
| Orange (GIT_ORANGE) | RCR (Rebase Conflict Resolution) actively running |
| Blue | RCR complete, awaiting approval |

The indicator is rendered as a separate span after the tab label, so it does not
interfere with the tab's own styling. Its width (1 character) is included in the
tab width calculation for pagination accuracy.

Auto-rebase is enabled by default for newly created worktrees. Toggle it with
`a` in the git panel's actions section (feature branches only). The setting is
persisted to `.azureal/azufig.toml`.

---

## The Tab Row Is Not Focusable

The tab row occupies a single row at the top of the layout and is not part of
the `Tab`/`Shift+Tab` focus cycle. Focus cycles through **FileTree -> Viewer ->
Session -> Input**. The `[`/`]` global keys and mouse clicks are the only ways
to interact with the tab row.
