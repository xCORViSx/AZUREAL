# SQLite Store (.azs)

All session data lives in a single file: `.azureal/sessions.azs`. This is a
standard SQLite database using DELETE journal mode, accessible by any SQLite
client. The `.azs` extension is intentional -- it discourages users from
casually browsing or editing the file, signaling that AZUREAL owns the format
and schema.

---

## Lazy Creation

The store file is **not** created when you open a project. It is created
lazily on first use -- specifically, when you create your first session.
Projects that have never had a session will not have a `.azureal/` directory.
This keeps the filesystem clean and avoids polluting repositories with empty
database files.

---

## Session Numbering

Sessions are numbered sequentially: **S1**, **S2**, **S3**, and so on. These
are display identifiers used in the session list and status bar. The numbering
is simple and monotonic -- there is no reuse of session numbers after deletion.

---

## Schema

The database contains four tables:

### `sessions`

The primary session record.

| Column | Type | Description |
|--------|------|-------------|
| `id` | INTEGER | Primary key, auto-incremented |
| `name` | TEXT | User-assigned session name (or default "S*n*") |
| `worktree` | TEXT | Path to the git worktree this session belongs to |
| `created` | TEXT | ISO 8601 timestamp of session creation |
| `completed` | TEXT | ISO 8601 timestamp of completion (NULL if still active) |
| `duration_ms` | INTEGER | Total session duration in milliseconds |
| `cost_usd` | REAL | Accumulated cost in USD (populated on completion) |

### `events`

Every prompt, response, tool call, and tool result is stored as an event.

| Column | Type | Description |
|--------|------|-------------|
| `session_id` | INTEGER | Foreign key to `sessions.id` |
| `seq` | INTEGER | Sequence number within the session (monotonically increasing) |
| `kind` | TEXT | Event type (e.g., `UserPrompt`, `AssistantResponse`, `ToolCall`, `ToolResult`, `Complete`) |
| `data` | TEXT | JSON-encoded event payload |
| `char_len` | INTEGER | Character length of the serialized data |

### `compactions`

Compaction summaries that replace older event ranges.

| Column | Type | Description |
|--------|------|-------------|
| `session_id` | INTEGER | Foreign key to `sessions.id` |
| `after_seq` | INTEGER | The sequence number after which events were compacted |
| `summary` | TEXT | The 2000--4000 character compaction summary |

### `meta`

Key-value store for runtime state and schema versioning.

| Column | Type | Description |
|--------|------|-------------|
| `key` | TEXT | Metadata key |
| `value` | TEXT | Metadata value |

Notable keys include:

- `current_session_id` -- tracks which session is currently active in the UI.
- `pid_session_target` -- maps PIDs to `(session_id, worktree_path)` tuples,
  recorded at agent spawn time so that AZUREAL can associate a running process
  with the correct session and worktree even after restarts.

---

## Event Storage and Compaction

Events are not stored verbatim from the agent's JSONL output. Before
serialization, `append_events()` applies `compact_event()` to each event,
which reduces storage size by truncating event content to match what the UI
actually renders:

### ToolResult Truncation

| Tool | Truncation Rule |
|------|----------------|
| Read | First line + last line only |
| Bash | Last 2 lines only |
| Grep | First 3 lines only |
| Glob | File count only (individual paths discarded) |
| Task | First 5 lines only |
| Default (all others) | First 3 lines |

### ToolCall Input Stripping

Tool call inputs are stripped down to their key field only. Two exceptions:

- **Edit** -- preserved in full, because the session pane renders diffs from
  the `old_string` and `new_string` fields.
- **Write** -- summarized rather than stored verbatim, since file contents can
  be arbitrarily large.

This compaction is applied at ingestion time, not at query time. The stored
events are already in their compact form.

---

## Completion Persistence

When an agent process sends a `Complete` event, AZUREAL calls
`mark_completed(session_id, duration_ms, cost_usd)` on the store. This
populates the `completed`, `duration_ms`, and `cost_usd` columns in the
`sessions` table.

The session list in the UI reads these fields to render **completion badges**:

- **Green check** -- session completed successfully.
- **Red X** -- session completed with a failure status.

Sessions that have not yet completed (or are still active) show no badge.
