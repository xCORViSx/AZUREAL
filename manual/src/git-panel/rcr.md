# Conflict Resolution (RCR)

RCR -- Rebase Conflict Resolution -- is AZUREAL's workflow for handling rebase
conflicts. Rather than dropping you into a manual conflict editor, RCR spawns a
Claude session dedicated to resolving the conflict, with a structured overlay
that guides the process from detection through resolution and approval.

---

## When RCR Triggers

RCR activates whenever a rebase encounters a conflict. This can happen during:

- A **manual rebase** (`Shift+R`)
- A **squash merge** (which rebases as its first step)
- An **auto-rebase** cycle

In all three cases, the same conflict overlay appears.

---

## The Conflict Overlay

When a conflict is detected, the viewer pane displays a conflict overlay with
a red border:

```text
╔══════════ REBASE CONFLICT ══════════╗
║                                     ║
║  Conflicted files:                  ║
║    src/auth/middleware.rs            ║
║    src/auth/token.rs                ║
║                                     ║
║  Auto-merged files:                 ║
║    src/lib.rs                       ║
║    src/config.rs                    ║
║                                     ║
║  [y] Resolve with Claude            ║
║  [n] Abort rebase                   ║
║                                     ║
╚═════════════════════════════════════╝
```

The overlay lists two categories of files:

- **Conflicted files** -- Files with merge conflicts that need resolution.
- **Auto-merged files** -- Files that git merged successfully without conflicts.

Two options are presented:

| Key | Action |
|-----|--------|
| `y` | Resolve with Claude -- spawn an RCR session |
| `n` | Abort rebase -- run `git rebase --abort` and return to the pre-rebase state |

---

## The RCR Session

Pressing `y` spawns a Claude session specifically for conflict resolution. This
session is distinct from your normal development sessions:

- **Session name**: `[RCR] <branch-name>` (e.g., `[RCR] azureal/auth-refactor`)
- **Working directory**: The feature worktree where the rebase is in progress
- **Session pane theme**: Green-themed borders, replacing the normal AZURE
  borders, to make it visually distinct from regular sessions

Claude receives the conflict context -- the conflicted file list, the conflict
markers, and the surrounding code -- and begins working through the resolution.
The session streams in real time in the session pane, just like any other Claude
session.

### Interactive Follow-Up

RCR sessions are fully interactive. While Claude is resolving conflicts (or
after it finishes), you can send follow-up prompts to refine the resolution.
For example, you might ask Claude to keep a specific side of a conflict, adjust
a merged implementation, or explain what caused the conflict.

---

## The Approval Dialog

When Claude exits (either by completing the resolution or when you stop it),
an **approval dialog** appears with a green border:

| Key | Action |
|-----|--------|
| `y` or `Enter` | Accept resolution |
| `n` | Abort rebase |
| `Esc` | Dismiss dialog (re-show with `Ctrl+A`) |

### Accept (`y` / `Enter`)

Accepting the resolution performs the following:

1. The RCR session file is deleted (cleanup).
2. The stash is popped (restoring any uncommitted changes from before the
   rebase).
3. If the rebase was the first step of a squash merge (`continue_with_merge`
   flag is set), the squash merge automatically proceeds from where it left
   off.

### Abort (`n`)

Aborting runs `git rebase --abort`, which restores the branch to its
pre-rebase state. The stash is popped and no changes are made to the branch
history.

### Dismiss (`Esc`)

Dismissing the dialog hides it without taking any action. The rebase remains
in its mid-conflict state. You can re-show the approval dialog at any time by
pressing `Ctrl+A`. This is useful if you want to inspect the resolved files
before deciding.

---

## RCR and Squash Merge Integration

When a squash merge triggers RCR (because the rebase step hit a conflict), the
`continue_with_merge` flag is set internally. This flag tells the approval
handler that accepting the resolution should not just complete the rebase but
also continue with the remaining squash merge steps (merge into main, commit,
push). The entire squash merge resumes automatically -- you do not need to
re-trigger it.

---

## RCR and Auto-Rebase Integration

When auto-rebase detects a conflict, it switches to the affected worktree and
opens the conflict overlay. The tab row indicator for that worktree changes
from green (idle) to orange (RCR active), and then to blue (approval pending)
once Claude finishes. See [Rebase & Auto-Rebase](./rebase.md) for the
indicator color table.
