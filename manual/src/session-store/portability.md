# Portability

The session store is designed for zero-friction portability. Moving your
entire conversation history between machines requires copying a single file.

---

## Transferring Sessions

To transfer all session data from one machine to another:

```sh
# On the source machine
cp /path/to/project/.azureal/sessions.azs /media/transfer/

# On the target machine
mkdir -p /path/to/project/.azureal/
cp /media/transfer/sessions.azs /path/to/project/.azureal/
```

That is the complete process. There are no companion files, no indexes to
rebuild, and no migration scripts to run.

---

## What the File Contains

The single `.azs` file contains **everything**:

- **All sessions** -- every session ever created in the project, with names,
  worktree associations, creation timestamps, and completion status.
- **All events** -- every prompt, agent response, tool call, and tool result,
  already in their compacted form.
- **All compaction summaries** -- the full text of every compaction summary
  generated during the project's lifetime.
- **Runtime metadata** -- the active session ID, PID-to-session mappings, and
  schema version.

There are no external files that the store depends on. The temporary JSONL
files produced by agent processes during streaming are ingested into the store
on exit and are not needed afterward.

---

## Backend Agnosticism

Sessions in the store are not tied to a specific backend. A single session
can contain events from both Claude and Codex prompts, interleaved in
chronological order. This means the transferred file works regardless of
which backends are available on the target machine -- the history is just
data, not executable state.

If the target machine only has Claude Code installed (not Codex), all
historical Codex events are still visible in the session pane. You simply
cannot send new Codex prompts without the Codex CLI. The same applies in
reverse.

---

## Cross-Machine Worktree Paths

The `sessions` table records the `worktree` path associated with each
session. If the project lives at a different absolute path on the target
machine (e.g., `/home/alice/project` vs. `/Users/bob/project`), the stored
worktree paths will not match the new filesystem layout.

AZUREAL resolves this at runtime by matching sessions to worktrees based on
the **relative worktree name** (the branch or directory name), not the
absolute path. Sessions created on the source machine will associate with
the correct worktrees on the target machine as long as the worktree names
match.

---

## Version Control Considerations

The `.azureal/` directory is typically added to `.gitignore` because session
data is personal to each developer. In a team setting, each developer
maintains their own session store with their own conversation history.

If you want to share session data (e.g., for onboarding or debugging), copy
the `.azs` file directly rather than committing it to the repository. Session
files can grow to several megabytes in active projects, and conversation
content may contain sensitive information (API keys in tool results, file
contents from private repositories, etc.).
