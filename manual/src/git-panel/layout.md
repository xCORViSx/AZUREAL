# Layout & Navigation

The git panel reuses AZUREAL's 3-pane layout structure but fills each pane with
git-specific content. The result is a purpose-built git interface that feels
familiar because it shares the same spatial organization as normal mode.

---

## Pane Layout

```text
╔════════════════════════════════════════════════════╗
║  [* main] │ [○ feat-a] │ [● feat-b]   (tab bar)  ║
╠═══════════╦════════════════════════╦═══════════════╣
║  Actions  ║                        ║               ║
║───────────║       Viewer           ║  Commit Log   ║
║  Changed  ║    (diffs / editor)    ║               ║
║  Files    ║                        ║               ║
╠═══════════╩════════════════════════╩═══════════════╣
║  GIT: branch-name (Tab/Shift+Tab: cycle | hints)  ║
╚════════════════════════════════════════════════════╝
```

### Top: Worktree Tab Bar

The same tab row from normal mode, but rendered with the git color palette
(GIT_ORANGE for selected, GIT_BROWN for unselected). You can switch between
worktrees within the git panel using `[` and `]` -- the entire panel updates
to reflect the newly selected worktree's git state.

### Left Sidebar (Split)

The left sidebar is divided into two sections stacked vertically:

- **Actions** (top) -- Lists the available git operations for the current
  branch context. The actions shown here change between main and feature
  branches (see [The Git Panel](../git-panel.md) for the full action table).
- **Changed Files** (bottom) -- Lists all modified, added, deleted, renamed,
  and untracked files with their staging status. See
  [Changed Files & Staging](./staging.md) for details.

### Center: Viewer

The viewer pane serves double duty. In its default state, it shows diffs for
selected files. When you press `c` to commit, it transforms into the commit
editor with the AI-generated message. During conflict resolution, it displays
the conflict overlay.

### Right: Commit Log

The commit log shows the branch's commit history. On feature branches, this
is scoped to branch-only commits (`git log main..HEAD`). On main, it shows the
full history. Unpushed commits render in green; pushed commits render dimmed.

The bottom border of the commit log displays **divergence badges** showing how
far the branch has diverged from main and from its remote tracking branch:

```text
 ↑2 ↓0 main  ↑1 ↓3 remote
```

These badges are color-coded: green for ahead, yellow for behind.

### Bottom: Status Bar

A full-width status bar replaces the normal status bar. It displays:

- The current branch name
- Navigation hints (Tab/Shift+Tab to cycle panes, Enter to act)
- Operation progress messages during long-running actions

---

## Focus Cycling

Three panes participate in the focus cycle:

| Order | Pane | Description |
|-------|------|-------------|
| 1 | Actions | The git operations list |
| 2 | Changed Files | The file list with staging controls |
| 3 | Commit Log | The branch commit history |

`Tab` moves focus forward through this cycle (Actions, Files, Commits, Actions,
...). `Shift+Tab` moves backward. The viewer pane does not participate in the
cycle -- it updates reactively based on selections in the other panes.

The focused pane is indicated by a GIT_ORANGE (`#F05032`) border. Unfocused
panes use GIT_BROWN (`#A0522D`).

---

## Worktree Switching

| Key | Action |
|-----|--------|
| `[` | Switch to previous worktree |
| `]` | Switch to next worktree |
| `{` | Jump one page backward in worktree list |
| `}` | Jump one page forward in worktree list |

Switching worktrees within the git panel refreshes all panes: the actions list
updates for the new branch context, the changed files list repopulates, the
viewer clears, and the commit log reloads. You do not need to exit the git
panel to work on a different branch.

---

## Entering and Exiting

`Shift+G` toggles the git panel from any context. On entry, the panel loads the
git state for the currently selected worktree. On exit, the normal layout
restores with all prior pane state intact -- the file tree position, viewer
content, and session scroll position are all preserved.
