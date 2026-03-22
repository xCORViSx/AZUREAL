# Background Processes

When you switch away from a project, its agent sessions keep running. AZUREAL
tracks these background processes and handles their lifecycle events -- including
exit -- without requiring the project to be active.

---

## Background Exit Handling

When an agent session finishes in a background project, AZUREAL's
`handle_claude_exited()` function is responsible for processing the event. The
handler checks the `slot_to_project` map to determine whether the exiting
session belongs to the active project or a background one.

### For Background Projects

When the exiting session belongs to a background project, the handler:

1. **Updates the snapshot** -- The project's `ProjectSnapshot` is modified
   directly. The session's `branch_slots` and `active_slot` entries are updated
   to reflect the completed/failed state.
2. **Marks unread** -- The worktree that owned the session is flagged as having
   unread output. When you switch back to the project, the worktree tab will
   show the unread indicator.
3. **Status message** -- A status bar message is shown, prefixed with the
   project's display name, so you know which background project had an agent
   finish. For example: `[my-api] Claude session completed`.

### For Active Projects

When the exiting session belongs to the active project, normal exit handling
applies -- the session pane updates, the tab row status icon changes, and the
status bar reflects the result. No snapshot manipulation is needed because the
active project's state is live.

---

## Process Continuity

Agent processes are **not** killed when you switch projects. All running
sessions continue executing in the background. Their output is captured to
session files on disk and handled via the background exit flow (see above).
When you switch back, the project snapshot is restored and includes any output
that was produced while the project was backgrounded.

> **Note:** Session file capture ensures no output is ever lost, even though
> the background project's display is not being updated in real time.

---

## Monitoring Background Activity

You do not need to switch to a project to check on its agents. The project list
in the Projects panel shows activity status icons for every project, including
background ones:

| Icon meaning | What it tells you |
|--------------|-------------------|
| Running | At least one agent is still executing |
| Failed | An agent encountered an error |
| Waiting | An agent needs user input |
| Completed | All agents finished successfully |
| Stopped | All agents are stopped |

Background project status is determined by checking the project's snapshot
against the set of currently running sessions. This is a lightweight check
that does not require restoring the full project state.

---

## Practical Workflow

A typical multi-project workflow looks like this:

1. Start an agent task in Project A (e.g., "refactor the auth module").
2. Press `P` to open the Projects panel.
3. Switch to Project B and work on something else.
4. Glance at the project list periodically -- Project A's icon shows "Running".
5. When Project A's icon changes to "Completed", switch back.
6. The snapshot restores. The session pane shows the agent's full output. The
   worktree tab has an unread indicator. Everything is exactly as if you had
   been watching the whole time.
