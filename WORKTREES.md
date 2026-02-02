# Azureal Worktree Status

## Active Worktrees

| Worktree | Purpose | Status | Priority |
|----------|---------|--------|----------|
| `worktree-auto-discovery` | Scan git worktrees directly instead of database for session list | **New** | High |
| `git-status-checking` | Detailed git status parsing and file-level tracking | **Partial** | High |
| `session-resume-with-history` | Resume sessions with message history from Claude | **Missing** | High |
| `session-status-transitions` | Complete session state machine with Waiting state | **Partial** | Medium |
| `tui-input-history` | Input history ring buffer with up/down navigation | **Missing** | Medium |
| `tui-help-overlay` | Help screen showing all keybindings | **New** | Medium |
| `session-output-persistence` | Save/load session output to disk | **New** | Medium |
| `tui-search-filter` | Search/filter sessions and projects | **New** | Medium |
| `tui-resize-handling` | Proper terminal resize handling | **New** | Low |
| `config-per-project` | Per-project configuration overrides | **New** | Low |
| `session-templates` | Predefined prompts/templates for common tasks | **New** | Low |
| `tui-themes` | Color theme customization | **New** | Low |
| `export-session-report` | Export session diffs/outputs as markdown/html | **New** | Low |

## Summary

| Status | Count |
|--------|-------|
| **Partial** | 2 |
| **Missing** | 2 |
| **New** | 9 |
| **Total** | 13 |

## Completed (Deleted)

The following 31 worktrees were fully implemented and have been deleted:

- `cli-argument-parsing` - CLI argument parsing via clap
- `config-file-toml` - TOML config file support
- `database-crud-outputs` - Session output storage
- `database-crud-projects` - Project CRUD operations
- `database-crud-sessions` - Session CRUD operations
- `database-schema-migrations` - Database migration system
- `git-diff-generation` - Generate diffs between main and HEAD
- `git-rebase-operations` - Interactive rebase with conflict resolution
- `git-repo-detection` - Detect if directory is git repo
- `git-worktree-cleanup` - Remove worktrees
- `git-worktree-creation` - Create new worktrees
- `process-lifecycle-management` - Process spawning and exit tracking
- `pty-input-sending` - Send input to PTY
- `pty-output-streaming` - Stream output from PTY
- `pty-process-spawning` - Spawn processes in PTY
- `session-archiving-cleanup` - Archive and cleanup sessions
- `session-creation-flow` - Create new sessions
- `tui-app-state-and-event-loop` - TUI state management and event loop
- `tui-diff-view-syntax-highlight` - Syntax highlighting in diff view
- `tui-input-cursor-movement` - Cursor movement in input fields
- `tui-keyboard-navigation` - Keyboard navigation throughout TUI
- `tui-multiline-text-input` - Multi-line input support
- `tui-output-colorization` - ANSI color support in output
- `tui-output-view-scrolling` - Scrolling in output view
- `tui-project-list-display` - Project list pane
- `tui-session-context-actions` - Context menu for session actions
- `tui-session-creation-input` - Session creation wizard
- `tui-session-list-with-status` - Session list with status indicators
- `tui-split-pane-layout` - Split pane layout
- `tui-status-bar` - Status bar at bottom
- `tui-view-tab-switching` - Switch between view tabs

## Priority Order

### High Priority
1. **worktree-auto-discovery** - Core architectural change to use git as source of truth
2. **git-status-checking** - Better file-level status display
3. **session-resume-with-history** - Core feature for resuming Claude sessions with context

### Medium Priority
4. **session-status-transitions** - Handle Waiting state properly
5. **tui-input-history** - Quality of life for re-running similar prompts
6. **tui-help-overlay** - Discoverability of keybindings
7. **session-output-persistence** - Preserve outputs across restarts
8. **tui-search-filter** - Navigation in large session lists

### Low Priority
9. **tui-resize-handling** - Edge case handling
10. **config-per-project** - Power user feature
11. **session-templates** - Convenience feature
12. **tui-themes** - Aesthetic customization
13. **export-session-report** - Reporting/documentation
