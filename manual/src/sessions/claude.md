# Claude Backend

The Claude backend wraps the **Claude Code CLI** (`claude` command) to execute prompts against Anthropic's Claude models. AZUREAL uses the CLI's non-interactive `-p` mode, which accepts a prompt string and exits after producing a response.

---

## Command Structure

Every Claude invocation follows this pattern:

```sh
claude -p "<prompt>" --verbose --output-format stream-json
```

The flags serve specific purposes:

| Flag | Purpose |
|------|---------|
| `-p "<prompt>"` | Non-interactive mode. Accepts a single prompt and exits after the response. |
| `--verbose` | Enables detailed output including tool call information. |
| `--output-format stream-json` | Emits one JSON object per line as events arrive, rather than buffering the entire response. |

The prompt string includes any injected context from the session store (see [Context Injection](../session-store/context-injection.md)). The full prompt is constructed before the process is spawned.

---

## Context Injection (No --resume)

AZUREAL does **not** use Claude Code's `--resume` flag. Instead, conversation continuity is handled entirely through context injection. On each prompt:

1. The session store builds the conversation context from all prior events via `build_context()`.
2. The context is formatted and prepended to the user's prompt inside `<azureal-session-context>` XML tags.
3. A fresh `claude -p` process is spawned with the context-injected prompt.

This approach gives AZUREAL full control over what context the agent sees, enables cross-backend continuity (a session can include both Claude and Codex responses), and avoids dependency on Claude Code's internal session file format.

---

## Session ID Capture

When a Claude process starts, the CLI emits a `subtype:init` event in its JSON stream. This event contains the session ID assigned by the Claude backend. AZUREAL captures this ID and associates it with the active session slot.

The session ID is used for display and diagnostics. It is not used for resumption -- context injection replaces that role.

---

## Permission Modes

Claude Code supports two permission modes, controlled by configuration:

### Dangerously Skip Permissions

```sh
claude -p "..." --dangerously-skip-permissions
```

This flag bypasses all tool-use approval prompts. The agent can read files, write files, execute commands, and perform any other tool action without asking for permission. This is the mode most users run in for unattended workflows.

### Approve Mode

Without the `--dangerously-skip-permissions` flag, Claude Code may pause to request approval for certain tool actions. AZUREAL surfaces these approval requests in the session pane via the `AskUserQuestion` display (see [AskUserQuestion](../session-pane/ask-user-question.md)).

---

## Model Selection

The Claude backend serves three models:

| Model | Alias |
|-------|-------|
| Claude Opus | `opus` |
| Claude Sonnet | `sonnet` |
| Claude Haiku | `haiku` |

The `--model` flag is passed to the CLI based on the currently selected model alias. See [Model Switcher](./model-switcher.md) for how model selection works.

---

## Streaming and Event Parsing

The `--output-format stream-json` flag causes Claude Code to emit one JSON object per line as events arrive. AZUREAL reads this stream line by line, parses each JSON object into an `AgentEvent`, and dispatches it to the event loop. Events include:

- **Init** -- session start with session ID and model information.
- **AssistantText** -- incremental text output from the model.
- **ToolUse / ToolResult** -- tool invocations and their results.
- **UserMessage** -- the original prompt, echoed back.
- **Error** -- error conditions reported by the CLI.

These `AgentEvent` values are converted into `DisplayEvent` values for rendering in the session pane and persisting to the session store. The conversion is backend-agnostic -- both Claude and Codex events feed into the same `DisplayEvent` pipeline.

---

## Process Lifecycle

Each Claude process follows a simple lifecycle:

1. **Spawn**: A new `claude -p` process is started with the context-injected prompt.
2. **Stream**: JSON events are read from stdout and parsed in real time.
3. **Exit**: The process exits when the response is complete.
4. **Ingest**: The JSONL output file is parsed, events are appended to the SQLite store, and the JSONL file is deleted.

The process does not persist between prompts. See [Session Lifecycle](./lifecycle.md) for the full end-to-end flow.
