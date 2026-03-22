# Session Lifecycle

This page describes the complete lifecycle of a single prompt-response exchange, from the moment you press Enter to the moment the response is persisted in the session store and the system is ready for the next prompt.

---

## The Seven Steps

### 1. User Submits Prompt

The user types a prompt in the input box (prompt mode) and presses `Enter`. If an agent is already running, the current run is cancelled and the prompt is staged for automatic resubmission once cancellation completes.

### 2. Context Is Built from the Store

Before spawning the agent process, AZUREAL calls `build_context()` on the SQLite session store. This function retrieves all prior events for the current session and constructs a context string. The context includes:

- Previous user messages.
- Previous assistant responses (text content).
- Tool call summaries.
- Any compaction summaries that replaced older events.

The context is formatted as structured text and wrapped in `<azureal-session-context>` XML tags. This tagged context is prepended to the user's prompt, giving the agent full conversational history without relying on the CLI's own session management.

Both backends receive context injection identically -- neither uses a CLI-native resume flag.

### 3. Agent Process Is Spawned

A new CLI process is spawned based on the active backend:

- **Claude**: `claude -p "<context + prompt>" --verbose --output-format stream-json`
- **Codex**: `codex exec --json "<context + prompt>"`

The process runs in the worktree's directory, so all file operations performed by the agent are scoped to the correct working tree. The process's PID is registered as a new session slot (see [Multi-Agent Concurrency](./multi-agent.md)).

### 4. Streaming JSON Events Are Received and Parsed

The agent process writes JSON events to stdout as they are generated. AZUREAL reads these events line by line in a background thread and sends them to the event loop via a channel.

For Claude, the `--output-format stream-json` flag produces one JSON object per line. For Codex, the `--json` flag produces a similar structured output. Both are parsed into the unified `AgentEvent` type.

Key events during streaming:

| Event | Meaning |
|-------|---------|
| Init / thread.started | Session or thread ID captured. |
| AssistantText | Incremental text appended to the response. |
| ToolUse | The agent is invoking a tool (file read, file write, command execution, etc.). |
| ToolResult | The tool returned a result. |
| Error | An error occurred in the CLI or the model. |

### 5. Events Are Displayed in Real Time

As `AgentEvent` values arrive, they are converted into `DisplayEvent` values and appended to the in-memory display event list. The render pipeline picks these up and updates the session pane on the next frame.

Text appears incrementally as the model generates it. Tool calls appear as collapsible sections with their results. The context meter updates to reflect the growing conversation size.

### 6. Process Exits

When the agent has finished its response, the CLI process exits. AZUREAL detects the exit via the process handle and performs cleanup:

- The process PID is removed from the branch's slot list.
- If this was the active slot, auto-switch to the last remaining slot occurs (if any).
- The JSONL output file (a temporary file written during streaming) is ready for ingestion.

### 7. JSONL Parsed, Events Stored, File Deleted

After the process exits, the JSONL output file is parsed into a final, authoritative list of `DisplayEvent` values. These events are appended to the SQLite session store. Once the store write is confirmed, the JSONL file is deleted.

This two-phase approach (stream for live display, then parse-and-store for persistence) ensures that the session store always contains cleanly parsed events, even if the live stream was interrupted or contained partial JSON lines.

---

## The Cycle Repeats

After step 7, the system is idle and ready for the next prompt. The session store now contains the full history of the conversation, including the most recent exchange. When the user submits the next prompt, the cycle begins again at step 1, with step 2 building context that includes the just-completed exchange.

```text
[User types prompt] --> [Build context] --> [Spawn process] --> [Stream events]
         ^                                                           |
         |                                                           v
         +--- [Ready for next prompt] <-- [Store events] <-- [Process exits]
```

---

## Error Handling

If the agent process exits with a non-zero status code or emits an error event:

- The error is displayed in the session pane as a system message.
- The process PID is still removed from the slot list.
- Any partial events that were already streamed are still persisted to the store.
- The system returns to idle, ready for the next prompt.

Transient errors (network issues, rate limits) do not corrupt the session state. The store always reflects what was actually received, and the next prompt starts fresh with the full context.

---

## Why Not Keep the Process Alive?

Claude Code's interactive mode uses a full terminal UI (TUI) that is designed for human interaction. It cannot be driven programmatically via stdin. The `-p` flag provides a non-interactive mode, but it exits after each response by design.

Codex CLI's `exec` mode behaves similarly -- it processes one request and exits.

The spawn-per-prompt approach has practical advantages:

- **Clean process state.** Each prompt starts with a fresh process, avoiding accumulated state or memory leaks.
- **Backend flexibility.** Switching models mid-session is trivial because each prompt can use a different backend.
- **Failure isolation.** A crashed process does not take down the session. The next prompt simply spawns a new one.

The overhead is approximately 100-200ms per spawn, which is negligible compared to the time the model spends generating a response.
