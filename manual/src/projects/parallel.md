# Parallel Projects

AZUREAL does not shut down background projects when you switch away from them.
Agent sessions continue running, output is captured, and the full project state
is preserved in a snapshot. When you switch back, everything is restored exactly
as you left it.

---

## Project Snapshots

When you switch away from a project, AZUREAL captures its entire state into a
`ProjectSnapshot`. When you switch back, the snapshot is restored and the
project resumes as if you never left.

### What Is Snapshotted

The snapshot includes everything needed to reconstruct the project's UI and
runtime state:

| Component | Details |
|-----------|---------|
| Display events | The rendered session output for each worktree |
| Worktrees | All worktree metadata and their current states |
| File tree | Expanded/collapsed nodes, selected file |
| Viewer tabs | Open files, scroll positions, active tab |
| Branch-to-slot maps | Which session slot belongs to which branch |
| Unread sessions | Which worktrees have new agent output since last viewed |
| Terminals | Shell state for each worktree |
| Run commands | Configured run commands and their states |
| Presets | Prompt presets configured for the project |

### Stale Slot Cleanup

When a snapshot is restored, AZUREAL performs a cleanup pass. Session slots that
reference branches or worktrees that no longer exist are pruned from the
snapshot. This handles the case where a branch was deleted externally (e.g., by
another developer or a CI system) while the project was in the background.

---

## Background Agent Sessions

Agent processes (Claude sessions) continue running in the background when you
switch to a different project. Their behavior while backgrounded:

- **Output is captured** -- The agent's responses are written to the session
  file on disk, ensuring nothing is lost.
- **Output is not rendered** -- The display events for the background project
  are not updated in real time. There is no wasted rendering work for a project
  you are not looking at.
- **On switch back** -- The snapshot is restored and the session pane reflects
  the current state of all sessions, including any output produced while the
  project was backgrounded.

This means you can start a long-running agent task in one project, switch to
another project to do different work, and come back later to find the results
waiting for you.

---

## Activity Status Icons

Each project in the project list displays an activity status icon drawn from
the same symbol set used in the worktree tab row. The icon reflects the
aggregate status of all worktrees in the project.

### Priority Order

Status icons follow a priority hierarchy (highest to lowest):

1. **Running** -- At least one agent is actively executing.
2. **Failed** -- At least one agent has failed.
3. **Waiting** -- At least one agent is waiting for user input.
4. **Pending** -- At least one agent is queued to start.
5. **Completed** -- All agents have finished successfully.
6. **Stopped** -- All agents are stopped.

The highest-priority status among all worktrees determines the icon shown for
the project.

### Active vs. Background Status

- **Active project** -- Status is derived from live worktree statuses in real
  time.
- **Background projects** -- Status is derived by checking the project's
  snapshot against the set of currently running sessions. This provides an
  accurate status without needing to render or process the background project's
  full state.
