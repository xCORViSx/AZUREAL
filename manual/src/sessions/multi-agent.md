# Multi-Agent Concurrency

AZUREAL supports running multiple agent processes simultaneously on the same worktree. Each active process occupies a **session slot** identified by its OS process ID (PID). This goes beyond the worktree-level isolation described in [Git Worktrees](../worktrees.md) -- even within a single branch, you can have several agents working in parallel.

---

## PID-Keyed Session Slots

All session state maps are keyed by PID string, not by branch name. This is the fundamental design choice that enables multi-agent concurrency.

Two data structures manage the slot system:

### Branch Slots

```text
branch_slots: HashMap<String, Vec<String>>
```

Maps each branch name to a list of active PID strings. When a new agent process is spawned on a branch, its PID is appended to that branch's slot list. When a process exits, its PID is removed.

### Active Slot

```text
active_slot: HashMap<String, String>
```

Maps each branch name to the PID of the slot whose output is currently displayed in the session pane. Only one slot can be the active slot per branch at any given time.

---

## Display Behavior

Only the active slot's output feeds the display pipeline. When multiple agents are running on the same branch, you see the output of whichever slot is currently active. The other slots continue running and their output is drained (read from the process pipe to prevent blocking), but it is not rendered to the session pane.

This means the session pane always shows a single, coherent stream of output, even when multiple agents are working concurrently. You can switch which slot is displayed, but you never see interleaved output from multiple agents.

---

## Slot Lifecycle Rules

The slot system follows a few simple rules:

### New Spawns Become Active

When a new agent process is spawned, it always becomes the active slot for its branch. If another agent was already running and displayed, the display switches to the newly spawned process. The previous agent continues running in the background.

### Auto-Switch on Exit

When the active slot's process exits, AZUREAL automatically switches to the last remaining slot on that branch (if any). If no other slots remain, the branch returns to an idle state with no active slot.

### Cancel Kills Active Only

When you cancel an agent run (via the cancel keybinding), only the active slot's process is killed. Other slots on the same branch continue running undisturbed. After cancellation, if other slots remain, the auto-switch rule applies.

---

## Practical Example

Consider a worktree on the `feat-auth` branch:

1. You submit a prompt: "Implement the login handler." An agent spawns with PID 12345. It becomes the active slot.

2. While PID 12345 is still running, you submit another prompt: "Write tests for the auth module." A second agent spawns with PID 12346. It becomes the active slot. PID 12345 continues running but its output is no longer displayed.

3. PID 12346 finishes its response and exits. The active slot auto-switches back to PID 12345, which is still streaming its response.

4. PID 12345 finishes. No slots remain. The branch is idle.

At each step, the session pane shows exactly one agent's output. The other agent's work proceeds in the background.

---

## Session Isolation on Switch

When switching between sessions (changing worktrees, changing projects, or switching which session is viewed in the session list), AZUREAL takes several steps to ensure clean visual transitions:

### Cache Clearing

All render caches, animation state, and clickable element maps are cleared immediately on switch. This prevents stale content from a previous session from flickering into view.

### Render Sequence Advancement

The render sequence number is advanced, which causes any in-flight render results (computed asynchronously by the background render thread) to be discarded. Results carry the sequence number they were computed for, and the display only accepts results matching the current sequence.

### Historic Session Viewing

A `viewing_historic_session` flag is set when you navigate to a session other than the live session for the current worktree. While this flag is active, live events from running agents are suppressed in the display. This prevents a running agent's output from appearing in a session you are reviewing from the history.

### Slot Ownership Check

An `is_viewing_slot()` check prevents results from being applied when the currently viewed session does not belong to the active project or worktree. This handles the edge case where a background render completes for project A while you have already switched to project B.

---

## Relationship to Worktree Isolation

Multi-agent concurrency (multiple agents on the same branch) and worktree isolation (agents on different branches) are complementary:

- **Worktree isolation** ensures agents on different branches cannot interfere with each other. Each worktree has its own working directory, so file operations are naturally scoped.
- **PID-keyed slots** let you run multiple agents on the same branch when you want concurrent work within a single feature. The agents share a working directory, so you should be aware that they may modify the same files.

In most workflows, you will have one agent per worktree. Multi-agent concurrency on a single branch is useful for specific patterns like running a test-writing agent alongside a feature-implementation agent, or submitting a follow-up prompt before the first response completes.
