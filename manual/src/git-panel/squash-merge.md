# Squash Merge

Squash merge is the primary workflow for landing feature branches into main.
Pressing `m` on a feature branch runs a multi-step background operation that
rebases the branch, squash merges it into main, and pushes the result -- all
with progress feedback in the status bar.

---

## Keybinding

| Key | Context | Action |
|-----|---------|--------|
| `m` | Git panel, feature branch, Actions focused | Squash merge into main |

This action is only available on feature branches. It does not appear in the
actions list when you are on main.

---

## The Multi-Step Process

Squash merge is not a single git command. AZUREAL runs a carefully ordered
sequence to ensure a clean, linear history on main:

### Step 1: Abort Stale State

Any leftover rebase or merge state is cleaned up (`git rebase --abort`,
`git merge --abort`) to prevent interference from a previously interrupted
operation.

### Step 2: Stash Dirty Working Tree

If the working tree has uncommitted changes, they are stashed automatically.
The stash is popped on success and failure exit paths. On conflict, the stash
remains until the conflict is resolved (popped after RCR accept or abort) so
your uncommitted work is never lost.

### Step 3: Rebase Feature onto Main

The feature branch is rebased onto main using `exec_rebase_inner`. This ensures
that any upstream changes on main are incorporated before the merge, avoiding
merge conflicts at the squash step.

### Step 4: Push Rebased Branch

The rebased feature branch is pushed to the remote. If the branch has diverged
(which it will after rebasing), `--force-with-lease` is used automatically.

### Step 5: Pull Main

AZUREAL switches to main and runs `git pull --ff-only` to ensure main is
up to date before merging.

### Step 6: Squash Merge

```sh
git merge --squash <feature-branch>
```

This stages all of the feature branch's changes as a single set of changes on
main, without creating a merge commit yet.

### Step 7: Commit with Rich Message

The squash merge commit is created with a structured message. The commit
subject follows the format `feat: merge <branch> into main`, and the body
includes the individual commit messages from the feature branch as bullet
points:

```text
feat: merge auth-refactor into main

- feat: add JWT token validation
- fix: handle expired refresh tokens
- refactor: extract auth middleware
```

### Step 8: Push Main

The merged main branch is pushed to the remote.

### Step 9: Pop Stash

If changes were stashed in Step 2, they are popped back onto the working tree.

---

## Progress Feedback

The status bar displays progress messages as each phase executes:

1. `Rebasing onto main...`
2. `Pushing rebased branch...`
3. `Merging into main...`
4. `Pushing to remote...`

The entire operation runs in a background thread, keeping the UI responsive.

---

## On Conflict

If the rebase step encounters a conflict, the squash merge pauses and opens the
**conflict overlay** (the RCR flow). See
[Conflict Resolution (RCR)](./rcr.md) for details on how conflicts are
resolved.

If the conflict is resolved successfully, the squash merge automatically
resumes from where it left off. The `continue_with_merge` flag ensures that
accepting the conflict resolution proceeds directly to the merge step without
requiring you to restart the operation.

---

## On Success: Post-Merge Dialog

After a successful squash merge, AZUREAL displays a **PostMergeDialog** that
offers two options for the feature worktree:

- **Archive** -- Remove the worktree's working directory but keep the branch.
  The worktree appears dimmed in the tab row and can be restored later.
- **Delete** -- Fully remove the worktree, its local branch, and its remote
  branch.

This dialog appears because after a squash merge, the feature branch's work
has been incorporated into main and the branch is typically no longer needed.
