# The Git Panel

The git panel is AZUREAL's dedicated interface for repository operations. It
replaces the normal TUI layout with a git-specific view where you can stage
files, commit with AI-generated messages, rebase, squash merge, resolve
conflicts, and push -- all without leaving the terminal. Toggle it with
`Shift+G`.

---

## Design Philosophy

AZUREAL treats git as a first-class workflow rather than something you shell out
for. The git panel reuses the same 3-pane layout structure as normal mode but
repurposes each pane for git operations. Every action is context-aware: the
available keybindings change depending on whether you are on the main branch or
a feature branch, and the panel always shows exactly the operations that make
sense for your current state.

The panel also integrates AI at two key points: **commit message generation**
(which uses your selected model to write conventional commit messages from the
diff) and **conflict resolution** (which spawns a Claude session to resolve
rebase conflicts interactively). These are not separate tools -- they are woven
into the standard git workflow so that AI assistance is a natural part of every
merge and commit.

---

## Visual Identity

The git panel uses its own color palette to make the mode switch immediately
obvious:

| Element | Color |
|---------|-------|
| Focused borders | GIT_ORANGE (`#F05032`) |
| Unfocused borders | GIT_BROWN (`#A0522D`) |

These replace the AZURE (`#3399FF`) accent used in normal mode. The shift from
blue to orange is an instant visual cue that you are in git context.

---

## Entering and Exiting

| Key | Action |
|-----|--------|
| `Shift+G` | Toggle git panel on/off |

When you enter the git panel, the layout transforms and the changed files list
and commit log populate from the current worktree's git state. When you exit,
the normal layout is restored exactly as you left it.

---

## Context-Aware Actions

The actions available in the git panel depend on which branch is active. On
main, you get pull/commit/push. On feature branches, you get squash
merge/rebase/commit/push. A few actions are always available regardless of
branch.

### On Main Branch

| Key | Action |
|-----|--------|
| `l` | Pull |
| `c` | Commit |
| `Shift+P` | Push |

### On Feature Branch

| Key | Action |
|-----|--------|
| `m` | Squash merge into main |
| `Shift+R` | Rebase onto main |
| `c` | Commit |
| `Shift+P` | Push |

### Always Available

| Key | Action |
|-----|--------|
| `r` | Refresh git state |
| `a` | Toggle auto-rebase |
| `s` | Auto-resolve settings |

---

## Chapter Contents

- **[Layout & Navigation](./git-panel/layout.md)** -- The 3-pane git layout,
  focus cycling, worktree switching, and status bar.
- **[Changed Files & Staging](./git-panel/staging.md)** -- File status
  indicators, staging/unstaging, discarding changes, and the UI-only staging
  model.
- **[Commit with AI Messages](./git-panel/commit.md)** -- AI-generated
  conventional commit messages, the commit editor, and commit+push.
- **[Squash Merge](./git-panel/squash-merge.md)** -- The multi-step squash
  merge workflow from feature branch to main.
- **[Rebase & Auto-Rebase](./git-panel/rebase.md)** -- Manual rebase, automatic
  background rebase, and the rebase status indicator.
- **[Conflict Resolution (RCR)](./git-panel/rcr.md)** -- The Rebase Conflict
  Resolution overlay and interactive Claude-assisted resolution.
- **[Auto-Resolve Settings](./git-panel/auto-resolve.md)** -- Configuring
  which files are automatically resolved via 3-way union merge.
- **[Pull & Push](./git-panel/pull-push.md)** -- Pulling, pushing, force-push
  with lease detection, and post-operation state refresh.
