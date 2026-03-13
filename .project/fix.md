# Bug Queue

(Empty - no known bugs)

## Recently Fixed

- **False error indicators on successful tool results** — Heuristic error detection matched "error:", "failed" etc. in tool output content, producing red ✗ on successful tools. Fixed by reading `is_error` from Claude Code's stream-json `tool_result` blocks.
- **Session content bleeds across sessions** — Stale render cache, live event routing, and PID display all lacked session-file-level isolation. Fixed with render cache clearing, `viewing_historic_session` flag, and PID suppression.
