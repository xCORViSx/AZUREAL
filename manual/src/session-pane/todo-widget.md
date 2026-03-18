# Todo Widget

When Claude calls the `TodoWrite` tool to track task progress, the session pane
renders a persistent sticky widget at the bottom showing the current task list.
This widget stays anchored to the bottom of the pane regardless of scroll
position, providing at-a-glance progress visibility.

---

## Appearance

The widget has a rounded border in dark gray with a bold AZURE title: `Tasks`.
Todo items are listed vertically, each prefixed with a status icon:

| Icon | Color | Status |
|------|-------|--------|
| `✓` | Green | Completed |
| `●` | Yellow (pulsating) | In progress |
| `○` | Dim gray | Pending |

**Pulsation:** The in-progress icon (`●`) cycles through yellow, light yellow,
yellow, and dark gray on a timer, creating a breathing animation that draws
attention to the currently active task.

---

## Text Display

The text shown for each todo depends on its status:

- **In-progress** items display the `activeForm` text when it is non-empty.
  The `activeForm` uses present tense (e.g., "Building the session parser")
  to indicate ongoing work.
- **Completed** and **pending** items display the `content` text, which uses
  imperative form (e.g., "Build the session parser").

Completed items render with dim gray text to visually de-emphasize them, while
in-progress and pending items use white text.

---

## Subagent Todos

When a subagent (spawned via the Task tool) has its own todo list, those items
appear as indented subtasks beneath the parent task that spawned them:

```text
 ✓ Refactor session store
 ● Implement context injection
   ↳ ○ Parse compaction markers
   ↳ ○ Build injection payload
 ○ Write integration tests
```

The `↳` prefix is rendered in dim gray. Subagent todos are inserted directly
after the parent todo item (tracked by `parent_idx`). If no parent index is
available, subtasks are appended after the last main task.

---

## Height and Scrolling

The widget's height adapts to its content:

- **Content cap:** Maximum 20 visual lines of content (plus 2 lines for the
  top and bottom border, totaling 22 rows maximum).
- **Minimum session space:** The widget never consumes so much vertical space
  that the session content area has fewer than 10 rows.
- **Text wrapping:** Todo text wraps within the available width. The height
  calculation accounts for wrapped lines, so a single long todo item may
  occupy multiple visual rows.

When the content exceeds the visible area, a scrollbar appears on the right
edge:

- **Track:** Dark gray `│` characters.
- **Thumb:** AZURE `█` characters, sized proportionally to the
  visible/total content ratio (minimum 1 row).
- **Scrolling:** Mouse wheel over the todo widget scrolls its content
  independently from the session pane. The scroll offset is clamped to valid
  bounds.

---

## Lifecycle

- **Appears** when Claude calls `TodoWrite` -- the `current_todos` list is
  populated from the tool call parameters.
- **Stays visible** after all tasks are completed. The widget does not auto-
  dismiss on completion, allowing the user to review the final state.
- **Clears** on the next user prompt or when switching sessions. This prevents
  stale task lists from persisting across unrelated prompts.

---

## Stream Suppression

`TodoWrite` tool calls are suppressed from the inline session stream. They do
not appear as tool call timeline nodes in the conversation. The widget is the
sole visual representation of todo state.
