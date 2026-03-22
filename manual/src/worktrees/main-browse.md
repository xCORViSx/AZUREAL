# Main Branch Browse

The main branch (typically `main` or `master`) is the original repository
checkout and always appears as the first tab in the tab row. While it is not a
worktree in the `<repo>/worktrees/` sense, AZUREAL lets you browse and work on
it with the same interface as any feature worktree.

---

## Entering and Exiting

Press `Shift+M` from any pane to enter main branch browse mode. The `[★ main]`
tab highlights in **yellow** (distinct from the AZURE highlight used for feature
worktrees) as a visual cue that you are working on the primary branch.

To exit, press `Esc` or `Shift+M` again. AZUREAL restores your previous
worktree selection and focus state.

`Shift+M` works globally -- from the file tree, viewer, session pane, or input.
It also works from inside the git panel, where clicking the `★ main` tab or
pressing `Shift+M` opens the main branch's git view.

---

## Full Functionality

Main browse is not a read-only mode. Everything works the same as it does on a
feature worktree:

- **File tree** shows the main branch's working directory
- **Viewer** opens and edits files on main
- **Terminal** spawns a shell in the repository root
- **Sessions** are fully functional -- you can create sessions, send prompts,
  and run agents on main
- **Edit mode** works normally for file editing

The main worktree is stored separately from the feature worktrees vec
(`app.main_worktree`), and `current_worktree()` transparently returns it when
`browsing_main` is true. This means all code paths that operate on "the current
worktree" work seamlessly on main.

---

## Git Panel Differences

The git panel (`Shift+G`) is context-aware and shows different actions depending
on whether you are on main or a feature branch:

| Main Branch | Feature Branch |
|-------------|----------------|
| `l` Pull | `m` Squash merge to main |
| `c` Commit | `Shift+R` Rebase onto main |
| `Shift+P` Push | `c` Commit |
| `z` Stash | `Shift+P` Push |
| `Shift+Z` Stash pop | `z` Stash |
| | `Shift+Z` Stash pop |

Main does not offer squash merge or rebase (you cannot merge main into itself),
and feature branches do not offer pull (pulling is done on main, then
auto-rebase propagates changes to feature branches).

The different action set and the yellow tab highlight serve as indirect cues that
you are operating on the primary branch -- there is no separate "main mode"
overlay or banner.

---

## Interaction With Tab Switching

The `[` and `]` keys cycle through feature worktree tabs. When browsing main:

- Pressing `[` exits main browse and selects the **last** worktree
- Pressing `]` exits main browse and selects the **first** worktree

This makes it easy to hop between main and your feature branches without
reaching for `Shift+M`.

---

## State Isolation

Entering and exiting main browse saves and restores display events and terminal
state, just like switching between feature worktrees. Your main branch session
history, file tree expansion state, and terminal shell are all preserved
independently from any feature worktree.

`switch_project()` clears main browse state, so switching projects always starts
fresh.
