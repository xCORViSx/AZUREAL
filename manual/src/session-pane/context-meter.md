# Context Meter

The context meter is a color-coded percentage badge displayed on the right side
of the session pane's top border. It shows how much of the context window has
been consumed since the last compaction, giving you a real-time sense of how
close the session is to needing compaction.

---

## Display

The badge renders as a bold percentage (e.g., ` 42% `) on the right side of
the border, immediately before the PID or exit code badge:

```text
╔ Session [5/12] ════ [refactor-auth] ═══════ 67%  PID:4821 ╗
```

The badge only appears when a session is active. When no session is loaded or
the context is empty, the badge is hidden.

---

## Color Thresholds

The badge color changes based on the percentage value:

| Percentage | Color | Meaning |
|------------|-------|---------|
| Below 60% | Green | Plenty of context remaining |
| 60% to 89% | Yellow | Context usage is elevated |
| 90% and above | Red | Context is nearly full; compaction imminent |

---

## Calculation

The percentage represents `chars_since_compaction / COMPACTION_THRESHOLD`,
where:

- **`chars_since_compaction`** is the total character count of all messages in
  the current session since the last compaction event (or since session start
  if no compaction has occurred). This counter is synced from the SQLite store
  on session load and incremented in real time as new prompts and responses
  arrive.
- **`COMPACTION_THRESHOLD`** is a constant set to 400,000 characters
  (approximately 100K tokens at 4 characters per token).

The percentage is capped at 100% even if the character count overshoots the
threshold before compaction triggers.

---

## Real-Time Updates

The badge updates during streaming without store I/O. On session load, the
character count is synced from the SQLite store (the authoritative source).
During live streaming, each parsed event and submitted prompt increments the
live counter, and the badge is recomputed via `update_token_badge_live()` --
a lightweight function that only recalculates the percentage and color without
touching the database.

This two-tier approach (store sync at rest, live counter during streaming)
ensures the badge stays accurate without adding I/O overhead to the hot path.

---

## Compaction Inactivity Watcher

When the context percentage reaches 90% or above, AZUREAL starts monitoring
for inactivity. If no session events arrive for 30 seconds while the context
is at or above 90%, a yellow warning banner is injected into the conversation:

```text
       Session may be compacting...
```

This heuristic accounts for the fact that the backend may silently compact
the context without sending an explicit event. The banner disappears if new
events arrive or if a `Compacted` event confirms that compaction completed.

When the context percentage drops below 90% (for example, after compaction
resets the counter), the inactivity watcher resets and the compaction banner
state is cleared.

---

## Compaction Trigger

When `chars_since_compaction` reaches or exceeds the 400,000-character
threshold, AZUREAL flags the session for compaction. On the next event loop
tick, a background agent is spawned to summarize the conversation. After
compaction completes, the character counter resets and the badge drops
accordingly. See [Compaction](../session-store/compaction.md) for the full
compaction lifecycle.
