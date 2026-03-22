# Leader Sequences

Leader sequences are multi-key commands that start with a **leader key**. AZUREAL uses `W` (Shift+W) as the leader key for worktree operations. This avoids overloading single-key bindings while keeping worktree management fast and discoverable.

---

## How Leader Sequences Work

1. Press `W` from command mode, regardless of which pane has focus.
2. The status bar updates to show `[W ...]`, indicating a leader sequence is in progress.
3. Press the second key to resolve the action.
4. The action executes and the leader state clears.

If you press `W` and then change your mind, press `Esc` to cancel the sequence and return to normal command mode. The `[W ...]` indicator disappears.

---

## Worktree Leader Sequences

| Sequence | Action |
|----------|--------|
| `W` then `n` | Create a new worktree |
| `W` then `r` | Rename the active worktree |
| `W` then `a` | Archive the active worktree |
| `W` then `d` | Delete a worktree |

### Create (Wn)

Opens the worktree creation dialog. You provide a branch name, and AZUREAL creates a new git worktree with that branch, adds it to the tab row, and switches to it.

### Rename (Wr)

Opens a rename dialog pre-filled with the current branch suffix. On confirm, the underlying git branch is renamed (`git branch -m`), the new branch is pushed to the remote, and the old remote branch is deleted. All branch-keyed app state is migrated to the new name.

### Archive (Wa)

Archives the active worktree. The worktree directory is removed, but the git branch is preserved. Archived worktrees remain visible in the tab row with a dim `◇` prefix and can be unarchived with `u` to recreate the worktree from the preserved branch.

### Delete (Wd)

Deletes a worktree from disk. This runs `git worktree remove` and is destructive -- uncommitted changes in the worktree are lost. A confirmation prompt is shown before deletion.

---

## Shortcut When Worktrees Pane Is Focused

When the Worktrees pane has focus, the leader prefix is not required. The second key resolves directly:

| Key | Action |
|-----|--------|
| `n` | Create a new worktree |
| `r` | Rename worktree |
| `a` | Archive worktree |
| `d` | Delete worktree |

This shortcut exists because when you are already looking at the worktrees list, the `W` prefix is redundant. The direct keys are faster and more natural in that context.

Outside the Worktrees pane, these single keys have other meanings (e.g., `r` runs a command, `n` may be unbound), so the `W` leader prefix is required to disambiguate.

---

## Status Bar Feedback

The status bar provides real-time feedback during leader sequences:

- **Before pressing leader key**: Status bar shows normal state (mode, branch, model).
- **After pressing `W`**: Status bar shows `[W ...]` to indicate a leader sequence is pending.
- **After pressing the second key**: The action executes and the status bar returns to normal.
- **After pressing `Esc`**: The leader sequence is cancelled and the status bar returns to normal.

This visual feedback ensures you always know whether a leader sequence is in progress, eliminating ambiguity about what your next keystroke will do.
