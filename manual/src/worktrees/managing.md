# Managing Worktrees

Once a worktree exists, you can rename it, archive it for later, or delete it
permanently. All management actions use the `W` leader sequence: press
`Shift+W`, then the action key. The status bar shows `W ...` while waiting for
the second keypress.

| Sequence | Action |
|----------|--------|
| `Wn` | New worktree |
| `Wr` | Rename worktree |
| `Wa` | Archive / unarchive worktree |
| `Wd` | Delete worktree |

The main branch cannot be renamed, archived, or deleted.

---

## Renaming (`Wr`)

Renaming changes the git branch name and migrates all internal state. Press
`Wr` to open a centered dialog with a cyan double border, pre-filled with the
current branch suffix (without the `azureal/` prefix).

### Dialog Controls

- **Type** to edit the name. The cursor supports `Left`/`Right` movement,
  `Backspace`, and character insertion at the cursor position.
- **Enter** confirms the rename.
- **Esc** cancels.

### What Happens on Confirm

1. All branch-keyed state maps are migrated immediately on the main thread:
   session files, display events cache, branch slots, active slot, unread
   sessions, and auto-rebase configuration.
2. The worktree entry's branch name updates in-place.
3. A background thread handles the git operations:
   - `git branch -m <old> <new>` renames the local branch
   - Pushes the new name to the remote
   - Deletes the old remote branch
   - Sets upstream tracking for the new name
4. The tab row refreshes with the updated name.

The worktree directory on disk is not renamed -- only the git branch changes.

---

## Archiving (`Wa`)

Archiving removes the worktree's working directory from disk but preserves the
git branch. This is useful for parking a feature branch you want to return to
later without cluttering the filesystem.

Press `Wa` on an active worktree to archive it. The operation runs on a
background thread and shows an "Archiving worktree..." loading indicator.

### Visual Changes

Archived worktrees remain in the tab row but appear dimmed with a diamond prefix
(`◇`) instead of the normal status circle. They cannot be opened or prompted
until unarchived.

Pressing `Enter` on an archived worktree's session shows a status message
directing you to unarchive first.

### Unarchiving

Press `Wa` again on an archived worktree to restore it. AZUREAL recreates the
git worktree from the preserved branch. The "Unarchiving worktree..." loading
indicator appears while the working directory is rebuilt.

After unarchiving, the worktree returns to its normal state with full file tree,
terminal, and session access. The tab icon reverts from `◇` back to the standard
status circle.

---

## Deleting (`Wd`)

Deleting is permanent: it removes the worktree directory, deletes the local git
branch, pushes a branch deletion to the remote, and prunes the local
remote-tracking ref. All associated session state (session files, branch slots,
active slot, unread sessions, auto-rebase config) is cleaned up.

Press `Wd` to open a centered confirmation dialog with a red double border.

### Safety Warnings

Before showing the dialog, AZUREAL checks for potential data loss:

- **Uncommitted changes**: runs `git status --porcelain` on the worktree path
  (skipped for archived worktrees that have no working directory)
- **Unmerged commits**: runs `git log main..<branch> --oneline` to count
  commits not yet merged to main

If either condition is found, yellow warning lines appear in the dialog between
the question and the action keys:

```text
! 3 uncommitted changes
! 2 commits not merged to main
```

These warnings are informational -- they do not block deletion.

### Sole Worktree Dialog

When the worktree is the only one on its branch (the common case), the dialog
shows a simple confirmation:

- `y` or `Enter` -- confirm deletion
- `Esc` or any other key -- cancel

### Sibling Guard Dialog

When multiple worktrees share the same branch (rare, but possible), git prevents
branch deletion while any worktree is still checked out on it. The dialog
adjusts to offer two choices:

- `y` -- delete **all** sibling worktrees on this branch and the branch itself
- `a` -- **archive** only the current worktree (keeps siblings and branch
  intact)
- `Esc` or any other key -- cancel

### After Deletion

The deletion runs on a background thread. Once complete, the tab row updates,
and the selection moves to the nearest remaining worktree. If no worktrees
remain, the welcome modal appears.

---

## Cross-Machine Cleanup

On startup and project switch, AZUREAL runs `git remote prune origin` followed
by a cleanup pass that deletes local `azureal/*` branches which are fully merged
to main and have no remote counterpart. This prevents worktrees deleted on one
machine from appearing as archived on another.
