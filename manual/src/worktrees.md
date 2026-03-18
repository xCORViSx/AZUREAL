# Git Worktrees

Worktrees are the organizing unit of all work in AZUREAL. Every feature branch
lives inside its own **git worktree** -- a separate working directory with its
own checked-out branch, file tree, terminal shell, and agent sessions. This
isolation is what makes true concurrent AI-assisted development possible: you can
have multiple agents running in parallel across different feature branches
without any cross-contamination of files, git state, or conversation history.

---

## How It Works

AZUREAL sits on top of the standard `git worktree` mechanism. When you create a
worktree through AZUREAL, three things happen:

1. **A git worktree is created** under `<repo>/worktrees/<name>`, giving you a
   full working directory on a new branch named `azureal/<name>`.
2. **A session slot is allocated** in the SQLite session store, ready to hold
   agent conversations for that branch.
3. **The tab row updates** to show the new worktree alongside your existing
   ones.

From that point on, the worktree is a self-contained development environment.
Switching between worktrees (with `[`/`]` or by clicking tabs) swaps the file
tree, viewer, session history, terminal, and git state all at once.

---

## Main Branch

The main branch (typically `main` or `master`) is not a worktree in the
traditional sense -- it is the original repository checkout. AZUREAL treats it
specially: it always appears as the first tab (`[* main]`), it cannot be
deleted or archived, and its git panel offers pull/commit/push instead of
squash/rebase. You can browse and work on main at any time via `Shift+M`.

See [Main Branch Browse](./worktrees/main-browse.md) for the full details.

---

## Lifecycle

A worktree moves through a simple lifecycle:

```text
Create  -->  Active  -->  Archive or Delete
                ^              |
                |   Unarchive  |
                +--------------+
```

- **Active** worktrees have a working directory on disk and appear as normal
  tabs.
- **Archived** worktrees have their working directory removed but their git
  branch preserved. They appear dimmed with a diamond prefix and can be
  restored at any time.
- **Deleted** worktrees are fully removed: the working directory, the local
  branch, the remote branch, and all associated session state.

---

## Chapter Contents

- **[Creating Worktrees](./worktrees/creating.md)** -- How to spin up a new
  worktree and optionally start an agent session immediately.
- **[Managing Worktrees](./worktrees/managing.md)** -- Renaming, archiving,
  unarchiving, and deleting worktrees.
- **[Main Branch Browse](./worktrees/main-browse.md)** -- Working on the main
  branch without leaving the worktree interface.
- **[Worktree Tab Row](./worktrees/tab-row.md)** -- The horizontal tab bar:
  status icons, pagination, unread indicators, and visual states.
