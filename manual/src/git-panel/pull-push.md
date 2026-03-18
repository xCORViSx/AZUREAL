# Pull & Push

Pull and push are the simplest operations in the git panel. They map directly
to `git pull` and `git push` with one important enhancement: AZUREAL
automatically detects diverged branches and switches to `--force-with-lease`
when a regular push would be rejected.

---

## Pull

| Key | Context | Action |
|-----|---------|--------|
| `l` | Git panel, main branch, Actions focused | Pull from remote |

Pull runs `git pull` on the main branch. It is only available when you are on
main -- feature branches are kept in sync with main through rebasing, not
pulling.

After the pull completes, the changed files list and commit log refresh
automatically to reflect any new changes.

---

## Push

| Key | Context | Action |
|-----|---------|--------|
| `Shift+P` | Git panel, any branch, Actions focused | Push to remote |

Push works on any branch -- main or feature. The behavior adapts based on the
branch state:

### Regular Push

If the local branch is ahead of its remote tracking branch and has not
diverged (no rewritten history), AZUREAL runs a standard push:

```sh
git push
```

### Force Push with Lease

If the local branch has **diverged** from its remote tracking branch -- which
happens after a rebase rewrites commit history -- a regular push would be
rejected. AZUREAL detects this condition and automatically uses:

```sh
git push --force-with-lease
```

The status bar appends **(force-pushed)** to confirm that a force push was
performed rather than a regular push.

`--force-with-lease` is a safer alternative to `--force`. It refuses to
overwrite the remote branch if someone else has pushed commits that you have
not fetched. This prevents accidentally destroying work on shared branches
while still allowing you to push rebased history.

---

## Post-Operation Refresh

Both pull and push trigger a full state refresh after completion:

- The **changed files list** is repopulated from `git status`.
- The **commit log** is reloaded to reflect any new or removed commits.
- The **divergence badges** on the commit log border are recalculated.

This ensures the git panel always shows current state after any remote
operation.
