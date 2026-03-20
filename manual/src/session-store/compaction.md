# Compaction

Agent conversations accumulate context quickly. A long debugging session with
verbose tool results can easily exceed 400K characters in a single sitting.
Left unchecked, this would overflow the model's context window and degrade
response quality long before that. AZUREAL's compaction system addresses this
by automatically summarizing older conversation history while preserving
recent exchanges verbatim.

---

## Character Threshold

A live character counter -- `chars_since_compaction` -- tracks the total
characters accumulated in the current session since the last compaction (or
since session start, if no compaction has occurred). The threshold is
**400,000 characters**, which corresponds to roughly 100K tokens.

This counter updates in real-time during streaming. It feeds the context meter
displayed on the session pane border (see [Context Meter](#context-meter)
below).

---

## What Happens When the Threshold Is Crossed

When `chars_since_compaction` exceeds 400K characters mid-turn, the following
sequence triggers:

### 1. Partial Turn Storage

The current turn's events are stored to SQLite immediately, even though the
agent has not finished. This ensures no data is lost in the next steps.

### 2. Flag Set

The `auto_continue_after_compaction` flag is set. This tells the system to
automatically resume the conversation after compaction completes, without
requiring user intervention.

### 3. Active Process Killed

The running agent process is terminated immediately. This prevents it from
piling more content onto an already-overflowing context window. The partial
response is preserved via step 1.

### 4. Compaction Agent Spawned

A background compaction agent is spawned using the currently selected model.
This agent receives the conversation history up to the compaction boundary
(see [Boundary Selection](#boundary-selection) below) and produces a
**2,000--4,000 character summary** that captures the key decisions, file
changes, and current state of the work.

### 5. Auto-Continue

Once the compaction agent finishes, AZUREAL automatically sends a hidden
**"Continue."** prompt. This prompt:

- Uses the new compacted context (summary + preserved recent exchanges).
- Does not create a user bubble in the session pane -- it is invisible to the
  user.
- Resumes the agent's work seamlessly from where it left off.

From the user's perspective, there may be a brief pause while compaction runs,
but the conversation continues without any manual intervention.

---

## Boundary Selection

Compaction does not summarize the entire conversation. It preserves recent
exchanges verbatim to maintain conversational coherence.

`spawn_compaction_agent()` tries `compaction_boundary(session_id, from_seq, keep)`
with progressively smaller `keep` values (**3 → 2 → 1**). `keep=3` is ideal —
it preserves the **last 3 user prompts** along with all interleaved agent
responses, tool calls, and tool results. However, sessions that cross the
threshold with ≤3 user messages would never find a boundary at `keep=3`,
leaving compaction stuck. Falling back to `keep=2` then `keep=1` ensures
compaction can always run as long as at least one user message boundary exists.

Everything before the boundary is summarized by the compaction agent.
Everything after it is kept verbatim and included in the next context
injection as-is.

This means the agent always sees:

- The compaction summary (covering all older history).
- The last 1–3 user-agent exchanges in full detail (depending on how many
  exist since the last compaction).
- The new prompt.

---

## Guard Rails

Several mechanisms prevent compaction from misbehaving:

### Double-Compaction Prevention

A guard prevents a second compaction from being triggered while one is already
in flight. If the threshold is crossed again during the compaction agent's own
execution, the system waits for the current compaction to finish before
evaluating whether another is needed.

### Deferred Spawn

If `compaction_boundary()` cannot find enough user messages to establish a
boundary (e.g., the session has fewer than 3 user prompts),
`compaction_spawn_deferred` is set. This suppresses compaction retries until a
new user message arrives, at which point the boundary calculation is
re-attempted.

### Cross-Backend Fallback

If the compaction agent fails on the primary backend (e.g., Claude returns an
error), the system retries with the alternate backend (e.g., Codex). This
ensures compaction is not blocked by a single backend's transient failure.

### Empty Output Retry

If the compaction agent returns an empty summary, `compaction_retry_needed` is
set, triggering a re-spawn of the compaction agent. An empty summary would
leave the context without any historical record, so this case is always
retried.

---

## Context Meter

The session pane border displays a color-coded percentage badge showing how
close the current session is to the compaction threshold:

| Range | Color | Meaning |
|-------|-------|---------|
| 0--59% | Green | Plenty of headroom |
| 60--79% | Yellow | Approaching threshold |
| 80--100% | Red | Compaction imminent or in progress |

The percentage is calculated as:

```text
chars_since_compaction / 400,000 * 100
```

The meter updates in real-time during streaming, giving continuous visibility
into context consumption.

### Inactivity Watcher

When the meter reaches **90% or higher** and no new events arrive for **30
seconds**, a yellow banner appears in the session pane:

> Session may be compacting...

This alerts the user that the pause they are experiencing is likely due to an
active compaction cycle, not a stalled agent. The banner disappears when
events resume.
