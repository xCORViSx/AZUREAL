# Context Injection

AZUREAL does not use backend-specific resume mechanisms (such as `--resume`
flags) to continue conversations across prompts. Instead, it injects
conversation history from the session store directly into each new prompt.
This approach is backend-agnostic -- the same session can span Claude and
Codex prompts without either backend needing to know about the other's session
format.

---

## The Problem with --resume

Backend CLIs typically offer a `--resume` flag that continues a previous
conversation by loading the backend's own session file. This creates several
issues for a multi-backend orchestrator like AZUREAL:

- **Backend lock-in.** A session started with Claude cannot be resumed with
  Codex, and vice versa.
- **Opaque state.** The backend owns the session file format, so AZUREAL
  cannot inspect, compact, or modify the conversation history.
- **Stale context.** The backend's session files may not reflect compaction or
  event truncation that AZUREAL has applied.

Context injection solves all three. AZUREAL owns the conversation history in
its SQLite store and feeds it to whichever backend is selected for the next
prompt.

---

## Prompt Flow

When a user sends a prompt in an existing session (one that already has stored
events), the following sequence occurs:

### 1. Build Context

`build_context()` reads the session's events from the store and assembles
them into a conversation transcript. If compaction summaries exist, they
replace the events they cover -- the context starts with the summary, then
continues with the verbatim events that follow it.

### 2. Wrap in Context Tags

`build_context_prompt()` takes the assembled context and wraps it in XML-style
tags:

```text
<azureal-session-context>
[assembled conversation history]
</azureal-session-context>

[user's actual prompt]
```

The tags give the agent a clear signal that the prefixed content is historical
context, not a new instruction.

### 3. Spawn Agent

The agent process is spawned with the wrapped prompt and `resume_id = None`.
From the backend's perspective, this is a brand-new conversation that happens
to start with a detailed context block. There is no dependency on any prior
backend session state.

### 4. UI Display

The UI shows **only the user's clean prompt** in the session pane. The
injected context prefix is invisible to the user -- it exists solely for the
agent's benefit. The user sees their prompt as they typed it, and the agent's
response appears below it as usual.

---

## First Prompt (Empty Session)

When a session has no prior events (the very first prompt), there is no
context to inject. The prompt is passed to the agent unchanged -- no
`<azureal-session-context>` tags, no wrapping, just the raw user input.

---

## Event Stripping on Ingestion

After the agent process exits, AZUREAL ingests the session's events from the
temporary JSONL output file via `store_append_from_jsonl()`. During this
ingestion, the injected context prefix is **stripped** from the stored events.

This is critical: without stripping, the context prefix would be stored as
part of the first user prompt event, and subsequent context injections would
nest -- each new prompt would include the previous context prefix inside its
own context block, growing unboundedly.

The stripping logic removes the `<azureal-session-context>...</azureal-session-context>`
wrapper from the first event's content, leaving only the user's original
prompt text. All subsequent events (agent responses, tool calls, tool results)
are stored as-is after applying the standard event compaction described in
[SQLite Store (.azs)](./sqlite.md).
