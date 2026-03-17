# Bug Queue

(Empty - no known bugs)

## Recently Fixed

- **Session pane empty after project switch-back** — `display_events` was not saved in `ProjectSnapshot`. Switching projects cleared the in-memory conversation history; switching back to a project with a live session showed an empty pane until new streaming events arrived. If the session had ended, it appeared completely blank. Fixed by adding `display_events` to `ProjectSnapshot` — saved on switch-away, restored before `load_session_output()` runs. Also fixed: active terminal was not saved to `worktree_terminals` before snapshot creation (`save_current_terminal()` was missing), causing the current worktree's shell session to be lost on project switch. Modified: `project_snapshot.rs`, `ui.rs`.
- **False error indicators on successful tool results** — Heuristic error detection matched "error:", "failed" etc. in tool output content, producing red ✗ on successful tools. Fixed by reading `is_error` from Claude Code's stream-json `tool_result` blocks.
- **Session content bleeds across sessions** — Stale render cache, live event routing, and PID display all lacked session-file-level isolation. Fixed with render cache clearing, `viewing_historic_session` flag, and PID suppression.
