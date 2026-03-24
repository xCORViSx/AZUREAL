# Azureal (Asynchronous Zoned Unified Runtime Environment for Agentic LLMs) is a Rust TUI application that wraps Claude Code CLI and OpenAI Codex CLI to enable multi-agent development workflows. Each worktree is a git worktree with its own agent session, allowing concurrent AI-assisted development across multiple feature branches. See CLAUDE.md for the full authoritative specification.

# FEATURES

### Process Cleanup on Quit and Cancel

All agent processes are fully terminated when the user quits Azureal or cancels an agent. Three mechanisms work together to guarantee no orphaned processes survive:

**Process group kill (SIGTERM):** `kill_process_tree(pid)` in `src/backend.rs` sends SIGTERM to the entire process group (`libc::killpg` on Unix, `taskkill /T /F` on Windows). Agent processes are spawned as process group leaders (`process_group(0)` in `claude.rs` / `codex.rs`), so the signal reaches all descendants — cargo test subprocesses, Claude subagents, etc.

**SIGKILL follow-up:** `kill_process_tree_force(pid)` (Unix only) sends SIGKILL to the process group, which cannot be caught or ignored. On app quit, the event loop calls `cancel_all_claude()`, collects the killed PIDs, sleeps 200ms (grace period for clean shutdown), then calls `kill_process_tree_force()` on each PID to eliminate any survivors.

**Commit gen PID tracking:** Commit message generation processes are spawned via `std::process::Command` on background threads — they have no streaming event channel and are never added to `running_sessions`. They are tracked separately in `commit_gen_pids: Arc<Mutex<Vec<u32>>>` on `App`, registered by `generate_commit_message_with_claude/codex()` at spawn and deregistered on completion. `cancel_all_claude()` iterates this list and kills each one.

`cancel_all_claude()` now returns `Vec<u32>` (all PIDs it sent SIGTERM to) so the event loop can follow up with SIGKILL after the grace period.

**Files:** `src/backend.rs` (`kill_process_tree`, `kill_process_tree_force`), `src/app/state/app.rs` (`commit_gen_pids` field), `src/app/state/ui.rs` (`cancel_all_claude` returns `Vec<u32>`), `src/tui/event_loop.rs` (SIGTERM → 200ms → SIGKILL quit flow), `src/tui/input_git_actions/operations.rs` (`generate_commit_message_with_claude/codex` register/deregister PIDs).

### Multi-Worktree Agent Management

See CLAUDE.md `# FEATURES` → `Multi-Worktree Agent Management` for the full specification.

### Git Panel and Worktree Operations

See CLAUDE.md `# FEATURES` → `Git Panel` for the full specification.

### Session Store (SQLite)

See CLAUDE.md for the full session store specification including compaction, context injection, and portable sessions.

# MANIFEST

See CLAUDE.md `# MANIFEST` for the complete file tree.

Key files for process management:
- `src/backend.rs` — `kill_process_tree()`, `kill_process_tree_force()` (Unix SIGTERM/SIGKILL), Windows `taskkill /T /F`
- `src/app/state/app.rs` — `commit_gen_pids: Arc<Mutex<Vec<u32>>>` field
- `src/app/state/ui.rs` — `cancel_all_claude() -> Vec<u32>` (kills agents + compaction + commit gen, returns PIDs)
- `src/tui/event_loop.rs` — quit flow: SIGTERM → 200ms sleep → SIGKILL survivors
- `src/tui/input_git_actions/operations.rs` — `generate_commit_message_with_claude/codex()` track PIDs in `commit_gen_pids`
- `src/claude.rs` — spawns agents with `process_group(0)` (process group leader)
- `src/codex.rs` — spawns agents with `process_group(0)` (process group leader)

# ROADMAP

See CLAUDE.md `# ROADMAP` for the full phased roadmap.

# TESTING REQUIREMENTS

See CLAUDE.md `# TESTING REQUIREMENTS` for domain-specific guidelines and the full test coverage table.

For process management specifically:
- `cancel_all_claude` unit tests verify it clears all state maps and returns killed PIDs
- `kill_process_tree` / `kill_process_tree_force` are OS-level calls — tested via integration (spawn a process, kill it, verify it's gone)
- `commit_gen_pids` registration/deregistration tested in `src/app/state/app.rs` tests

# REFERENCES

(None fetched yet)

---

## **CONFLICTS**

(None)

# USE

See README.md for installation and usage instructions.
