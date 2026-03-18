# The Session Pane

The session pane occupies the rightmost 35% of the terminal, spanning the full
height from below the worktree tab row down to the status bar. It displays the
agent conversation -- prompts, responses, tool calls, and tool results -- for
the currently active session.

## Border Titles

The session pane border carries information in three positions:

| Position | Content |
|----------|---------|
| Left | `Session [x/y]` -- current message position within the session |
| Center | `[session name]` -- the session's display name in brackets |
| Right | Context usage badge + PID or exit code |

**Session names:** Custom names are preferred for readability. When no custom
name has been set, UUIDs are shown in truncated form as `[xxxxxxxx-...]` to
save horizontal space.

**PID badge:** While an agent process is running, the process ID is shown in
green (`PID:12345`). When the process exits, the badge switches to an exit
code: green for exit code 0, red for any non-zero exit code.

## Border Styling

The border appearance changes based on context:

- **Focused:** AZURE (`#3399FF`) with double-line border and bold.
- **Unfocused:** White with plain single-line border.
- **RCR active:** Green with bold border, indicating active conflict
  resolution. A bottom-border hint shows `Ctrl+a Accept/Abort`.

The bottom border also displays the current model name, color-coded by model
family:

| Model | Color |
|-------|-------|
| Opus | Magenta |
| Sonnet | Cyan |
| Haiku | Yellow |
| GPT-5.4 | Green |
| GPT-5.3-codex | Light green |
| GPT-5.2-codex | Teal |
| GPT-5.1-codex-max | Blue |
| GPT-5.1-codex-mini | Light blue |

## Conversation Layout

Messages render in an iMessage-style bubble layout:

- **User messages** are right-aligned with an AZURE accent bar and `You <`
  header. Text wraps within the bubble width.
- **Assistant messages** are left-aligned with a header showing the backend
  name (`Claude >` in orange, `Codex >` in cyan) and model identifier. The
  header fills the full bubble width with the model name right-aligned.
- **Tool calls** appear as timeline nodes beneath assistant messages (see
  [Tool Call Display](./session-pane/tool-calls.md)).

An empty session pane (no session loaded) shows a hint: "Press `s` to choose a
session or `a` to create one."

## Filtered Messages

Several message types are hidden from the conversation display to reduce noise:

- **Meta messages** (`isMeta: true`) -- internal Claude instructions that the
  user never needs to see.
- **Internal markers** -- `<local-command-caveat>`, `<task-notification>`, and
  `<local-command-stdout>` tags are stripped before rendering.
- **Rewound/edited user messages** -- when a user edits a previous message,
  only the corrected version is shown. Deduplication uses the `parentUuid`
  field to identify and suppress superseded messages.
- **TodoWrite tool calls** -- suppressed from the inline stream because they
  are rendered separately in the sticky [Todo Widget](./session-pane/todo-widget.md).

## Slash Commands

User messages containing `<command-name>/xxx</command-name>` tags are rendered
as centered 3-line magenta banners instead of normal user bubbles. The command
name is displayed prominently, making it easy to spot slash command invocations
when scrolling through a session.

## Compaction Banners

Two banners indicate context compaction status:

- **"Context compacted"** -- green banner, centered, shown after compaction
  finishes successfully (both the `Compacting` and `Compacted` events produce
  this).
- **"Session may be compacting..."** -- yellow warning banner, injected by the
  inactivity heuristic when context usage is at or above 90% and no events have
  arrived for 20 seconds.

## Hook Display

Hooks (permission checks, tool validations) are captured from multiple sources
in the event stream (`hook_progress`, `system-reminder` tags,
`hook_response`). They render as dim gray lines near their corresponding tool
calls, prefixed with `>`. Consecutive duplicate hooks are deduplicated to
prevent visual clutter.

## Scrolling and Navigation

- `j` / `k` or arrow keys scroll one line at a time.
- `g` jumps to the top of the session; `G` jumps to the bottom.
- Mouse wheel scrolling works regardless of keyboard focus.
- The session pane auto-follows new content (pinned to bottom) until the user
  scrolls up. Scrolling up detaches from the bottom; pressing `G` or scrolling
  back to the end re-attaches.

## Session Find

Press `/` to open an in-session search bar at the bottom of the session pane.
Type a query to highlight matches in the conversation. `n` and `N` navigate
between matches. The search bar shows a `current/total` match counter. Press
`Esc` to dismiss the search bar; matches remain highlighted until the query is
cleared.

## What This Chapter Covers

- [Markdown Rendering](./session-pane/markdown.md) -- how assistant text is
  styled with headers, code blocks, tables, lists, and inline formatting.
- [Tool Call Display](./session-pane/tool-calls.md) -- timeline nodes, status
  indicators, and draw-time patching.
- [Tool Result Formats](./session-pane/tool-results.md) -- per-tool
  summarization of tool output.
- [Session List & Search](./session-pane/session-list.md) -- the `s` overlay
  for browsing and filtering sessions.
- [Todo Widget](./session-pane/todo-widget.md) -- the sticky task progress
  tracker at the bottom of the pane.
- [AskUserQuestion](./session-pane/ask-user-question.md) -- interactive
  question prompts from the agent.
- [Context Meter](./session-pane/context-meter.md) -- the color-coded context
  usage badge.
- [Clickable Elements](./session-pane/clickable-elements.md) -- mouse-clickable
  file paths, links, tables, and status bar interactions.
