# Rebase & Auto-Rebase

AZUREAL provides two ways to rebase feature branches onto main: a manual rebase
triggered by keybinding, and an automatic background rebase that keeps branches
up to date without intervention. Both use the same underlying rebase logic and
conflict handling.

---

## Manual Rebase

| Key | Context | Action |
|-----|---------|--------|
| `Shift+R` | Git panel, feature branch, Actions focused | Rebase onto main |

Pressing `Shift+R` rebases the current feature branch onto main. The rebase
uses the `--onto` form with the merge-base fork point, which produces a clean
linear history even when the branch has been previously rebased.

### Dirty Working Tree Handling

If the working tree has uncommitted changes, AZUREAL stashes them before
rebasing and pops the stash on all exit paths:

- **Success** -- Stash popped after rebase completes.
- **Conflict** -- Stash popped after conflict resolution (accept or abort).
- **Failure** -- Stash popped after rebase abort.

You never need to worry about losing uncommitted work during a rebase.

### Push After Rebase

After a successful rebase, the branch will have diverged from its remote
tracking branch (because rebase rewrites commit history). AZUREAL detects this
automatically and uses `--force-with-lease` instead of a regular push. The
status bar appends "(force-pushed)" to confirm the force push occurred.

`--force-with-lease` is used rather than `--force` because it refuses to
overwrite the remote branch if someone else has pushed to it since your last
fetch. This prevents accidentally destroying work on shared branches.

---

## Auto-Rebase

Auto-rebase is a background process that automatically rebases eligible
worktrees onto main at regular intervals. Toggle it with `a` in the git panel
actions.

### Toggle

| Key | Context | Action |
|-----|---------|--------|
| `a` | Git panel, Actions focused | Toggle auto-rebase on/off |

The setting is persisted in `azufig.toml`, so it survives restarts.

### How It Works

When auto-rebase is enabled, AZUREAL checks all worktrees every **2 seconds**.
For each worktree, it evaluates whether a rebase is appropriate. A worktree is
skipped if any of the following conditions are true:

| Condition | Reason |
|-----------|--------|
| Claude session is running | Rebase would disrupt the agent's working state |
| RCR (conflict resolution) is active | A conflict is already being handled |
| Working tree is dirty | Uncommitted changes would complicate the rebase |
| Git panel is open for that worktree | User may be in the middle of a manual operation |

If none of these conditions apply, the worktree is eligible and auto-rebase
proceeds.

### Outcomes

Auto-rebase produces one of two outcomes:

**Rebased (no conflicts)**

The branch is rebased cleanly. AZUREAL automatically pushes the rebased branch
(with `--force-with-lease`) and displays a **green success dialog** for 2
seconds before it auto-dismisses. No user action is required.

**Conflict detected**

AZUREAL switches to the conflicted worktree and opens the **conflict overlay**
(the RCR flow). See [Conflict Resolution (RCR)](./rcr.md) for the full
resolution workflow.

---

## Tab Row Indicator

When auto-rebase is enabled, each worktree tab in the tab row displays a
colored **R** indicator showing the rebase state:

| Color | State |
|-------|-------|
| Green | Idle -- auto-rebase is enabled and no action is in progress |
| Orange | RCR active -- a conflict is being resolved for this worktree |
| Blue | Approval pending -- conflict resolution is complete and awaiting your approval |

This indicator is visible at all times, not just in the git panel, so you can
monitor rebase status from normal mode as well.
