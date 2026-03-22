# Session Store & Persistence

AZUREAL persists all conversation history in a single SQLite database file:
`.azureal/sessions.azs`. This file is the single source of truth for session
data -- every prompt, every tool call, every agent response, and every
compaction summary lives here. There are no external dependencies, no daemon
processes, and no cloud sync. Copy the file, and you copy your entire history.

---

## Design Principles

### Single-File Storage

One file holds everything. The `.azs` extension is a standard SQLite database
with DELETE journal mode. The custom extension discourages casual tampering --
renaming it to `.db` or `.sqlite` and opening it in a database browser works
fine, but the intent is that AZUREAL owns this file.

### Lazy Initialization

The store is **not** created when you load a project. It is opened (and the
file created, if absent) only when you create your first session. Projects
that have never had a session will not have a `sessions.azs` file (though
the `.azureal/` directory may exist for configuration).

### Backend-Agnostic

A single session can span prompts sent to both Claude and Codex backends.
The store does not partition by backend -- it records events in the order they
occurred, regardless of which model produced them.

### Context Injection Over Resume

AZUREAL does not use `--resume` flags to continue conversations. Instead, it
reads the session history from the store, wraps it into a context prompt, and
spawns a fresh agent process with that context injected. This decouples
conversation continuity from any particular backend's session management.

---

## How It Fits Together

The session store connects several subsystems:

1. **Event ingestion** -- After an agent process exits, AZUREAL reads the
   temporary JSONL output file, applies event compaction (truncating verbose
   tool results to match what the UI displays), strips any injected context
   prefix, and appends the events to the store.

2. **Context injection** -- Before spawning a new agent prompt in an existing
   session, `build_context()` reads the stored events and
   `build_context_prompt()` wraps them in `<azureal-session-context>` tags.
   The agent sees the full conversation history; the user sees only their
   clean prompt in the UI.

3. **Compaction** -- When accumulated context exceeds 400K characters (~100K
   tokens), a background compaction agent summarizes the older history into a
   2000--4000 character summary, keeping the last few user exchanges verbatim.
   This prevents context window overflow while preserving conversational
   continuity.

4. **Completion tracking** -- When an agent session ends, the store records
   duration and cost. The session list in the UI renders completion badges
   (green check for success, red X for failure) based on this data.

---

## Chapter Contents

- **[SQLite Store (.azs)](./session-store/sqlite.md)** -- File format, schema
  tables, session numbering, event storage, and completion persistence.
- **[Context Injection](./session-store/context-injection.md)** -- How stored
  history replaces `--resume`, the prompt flow, and event stripping on
  ingestion.
- **[Compaction](./session-store/compaction.md)** -- The character threshold,
  compaction agent, boundary preservation, auto-continue, and the context
  meter.
- **[Portability](./session-store/portability.md)** -- Transferring session
  data between machines by copying a single file.
