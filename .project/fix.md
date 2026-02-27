# Bug Queue

(Empty - no known bugs)

## Recently Fixed

- **Session content bleeds across sessions** — Stale render cache, live event routing, and PID display all lacked session-file-level isolation. Fixed with render cache clearing, `viewing_historic_session` flag, and PID suppression.
