# Codex Backend

The Codex backend wraps the **OpenAI Codex CLI** (`codex` command) to execute prompts against OpenAI's GPT models. Like the Claude backend, AZUREAL uses a non-interactive execution mode that exits after producing a response.

---

## Command Structure

Codex invocations differ depending on whether the session is new or being resumed.

### New Session

```sh
codex exec --json "<prompt>"
```

### Resume Session

```sh
codex exec --json resume <UUID> "<prompt>"
```

When a Codex session has a thread ID from a previous exchange, AZUREAL passes it via the `resume <UUID>` argument. This tells the Codex CLI to continue an existing conversation thread. Unlike the Claude backend (which uses context injection exclusively), the Codex backend uses the CLI's native resume mechanism for thread continuity.

| Flag / Argument | Purpose |
|-----------------|---------|
| `exec` | Non-interactive execution mode. |
| `--json` | Emits structured JSON output for machine parsing. |
| `resume <UUID>` | Continues an existing conversation thread (optional, omitted for the first prompt). |

---

## Session ID Capture

When a Codex process starts a new thread, it emits a `thread.started` event containing a `thread_id` field. AZUREAL captures this ID and stores it for use in subsequent `resume` commands within the same session.

The thread ID persists across prompts, enabling the Codex CLI to maintain its own conversation state. This is in addition to AZUREAL's own context injection -- both mechanisms contribute to continuity.

---

## Permission Modes

Codex CLI supports two permission modes:

### Dangerously Bypass Approvals and Sandbox

```sh
codex exec --json --dangerously-bypass-approvals-and-sandbox "<prompt>"
```

This flag disables all approval prompts and sandbox restrictions. The agent can read files, write files, execute commands, and perform any action without confirmation. This is the Codex equivalent of Claude's `--dangerously-skip-permissions` flag.

### Full Auto

```sh
codex exec --json --full-auto "<prompt>"
```

Full auto mode allows the agent to operate autonomously while still respecting sandbox boundaries. The agent can proceed without manual approval for standard operations, but destructive or out-of-scope actions may still be restricted. This is a middle ground between fully restricted and fully unrestricted operation.

---

## Model Selection

The Codex backend serves six models:

| Model | Alias |
|-------|-------|
| GPT-5.4 | `gpt-5.4` |
| GPT-5.3 Codex | `gpt-5.3-codex` |
| GPT-5.2 Codex | `gpt-5.2-codex` |
| GPT-5.2 | `gpt-5.2` |
| GPT-5.1 Codex Max | `gpt-5.1-codex-max` |
| GPT-5.1 Codex Mini | `gpt-5.1-codex-mini` |

All models with names starting with `gpt-` are automatically routed to the Codex backend. See [Model Switcher](./model-switcher.md) for the full model cycle.

---

## Streaming and Event Parsing

The `--json` flag causes Codex CLI to emit structured JSON events. AZUREAL reads these events from the process output and converts them into the same `AgentEvent` and `DisplayEvent` types used by the Claude backend. The key events include:

- **thread.started** -- thread creation, carrying the `thread_id` used for subsequent resume commands.
- **Assistant text** -- incremental response text from the model.
- **Tool calls and results** -- file operations, command execution, and their outcomes.
- **Error** -- error conditions reported by the CLI.

Because both backends produce the same `DisplayEvent` values, the session pane, session store, and rendering pipeline handle Claude and Codex output identically. You can switch between Claude and Codex models mid-session and the conversation displays seamlessly.

---

## Process Lifecycle

Each Codex process follows the same lifecycle as Claude:

1. **Spawn**: A new `codex exec` process is started with the prompt and, if available, a thread ID for resumption.
2. **Stream**: JSON events are read and parsed in real time.
3. **Exit**: The process exits when the response is complete.
4. **Ingest**: Events are appended to the SQLite store and temporary output files are cleaned up.

The process does not persist between prompts. See [Session Lifecycle](./lifecycle.md) for the full end-to-end flow.
