# Codex Backend

The Codex backend wraps the **OpenAI Codex CLI** (`codex` command) to execute prompts against OpenAI's GPT models. Like the Claude backend, AZUREAL uses a non-interactive execution mode that exits after producing a response.

---

## Command Structure

The default GPT-5.6 Sol invocation follows this pattern, with the prompt read from standard input:

```sh
codex exec --json --model gpt-5.6-sol --config 'model_reasoning_effort="ultra"' -
```

Like the Claude backend, AZUREAL does **not** use the Codex CLI's native `resume` mechanism. Conversation continuity is handled entirely through context injection from the SQLite session store. Each prompt spawns a fresh process with the full context prepended.

| Flag / Argument | Purpose |
|-----------------|---------|
| `exec` | Non-interactive execution mode. |
| `--json` | Emits structured JSON output for machine parsing. |
| `--model gpt-5.6-sol` | Selects Azureal's default Codex model. |
| `--config 'model_reasoning_effort="ultra"'` | Enables Sol's maximum reasoning mode with automatic task delegation. |
| `-` | Reads the context-injected prompt from standard input. |

---

## Session ID Capture

When a Codex process starts a new thread, it emits a `thread.started` event containing a `thread_id` field. AZUREAL captures this ID and associates it with the active session slot.

The thread ID is used for display and diagnostics. It is not used for resumption -- context injection replaces that role, just as with the Claude backend.

---

## Permission Modes

Codex CLI supports two permission modes:

### Dangerously Bypass Approvals and Sandbox

```sh
codex exec --json --model gpt-5.6-sol --config 'model_reasoning_effort="ultra"' --dangerously-bypass-approvals-and-sandbox -
```

This flag disables all approval prompts and sandbox restrictions. The agent can read files, write files, execute commands, and perform any action without confirmation. This is the Codex equivalent of Claude's `--dangerously-skip-permissions` flag.

### Full Auto

```sh
codex exec --json --model gpt-5.6-sol --config 'model_reasoning_effort="ultra"' --full-auto -
```

Full auto mode allows the agent to operate autonomously while still respecting sandbox boundaries. The agent can proceed without manual approval for standard operations, but destructive or out-of-scope actions may still be restricted. This is a middle ground between fully restricted and fully unrestricted operation.

---

## Model Selection

The Codex backend serves the pinned GPT-5.6 family, with Sol as the default,
plus the OpenAI frontier catalog:

| Model | Alias |
|-------|-------|
| GPT-5.6 Sol | `gpt-5.6-sol` |
| GPT-5.6 Terra | `gpt-5.6-terra` |
| GPT-5.6 Luna | `gpt-5.6-luna` |
| GPT-5.2 | `gpt-5.2` |
| GPT-5.5 | `gpt-5.5` |
| GPT-5.5 Pro | `gpt-5.5-pro` |
| GPT-5.4 | `gpt-5.4` |
| GPT-5.4 Pro | `gpt-5.4-pro` |
| GPT-5.4 Mini | `gpt-5.4-mini` |
| GPT-5.4 Nano | `gpt-5.4-nano` |
| GPT-5 Mini | `gpt-5-mini` |
| GPT-5 Nano | `gpt-5-nano` |
| GPT-5 | `gpt-5` |
| GPT-4.1 | `gpt-4.1` |

All models with names starting with `gpt-` are automatically routed to the Codex backend. Bare GPT-5.6 Sol keeps the explicit `ultra` default; `model:effort` entries pass their suffix as `model_reasoning_effort` after stripping it from the `--model` value.

### Reasoning Effort Entries

The switcher exposes supported effort suffixes as `model:effort` entries:

| Codex models | Efforts |
|--------------|---------|
| GPT-5.6 Sol, GPT-5.6 Terra | `low`, `medium`, `high`, `xhigh`, `max`, `ultra` |
| GPT-5.6 Luna | `low`, `medium`, `high`, `xhigh`, `max` |
| GPT-5.2, GPT-5.5, GPT-5.4, GPT-5.4 Mini | `low`, `medium`, `high`, `xhigh` |

For example, `gpt-5.6-sol:xhigh` launches with `--model gpt-5.6-sol` and `model_reasoning_effort="xhigh"`. See [Model Switcher](./model-switcher.md) for the full cycle.

---

## Streaming and Event Parsing

The `--json` flag causes Codex CLI to emit structured JSON events. AZUREAL reads these events from the process output and converts them into the same `AgentEvent` and `DisplayEvent` types used by the Claude backend. The key events include:

- **thread.started** -- thread creation, carrying the `thread_id` for session identification.
- **Assistant text** -- incremental response text from the model.
- **Tool calls and results** -- file operations, command execution, and their outcomes.
- **Error** -- error conditions reported by the CLI.

Because both backends produce the same `DisplayEvent` values, the session pane, session store, and rendering pipeline handle Claude and Codex output identically. You can switch between Claude and Codex models mid-session and the conversation displays seamlessly.

---

## Process Lifecycle

Each Codex process follows the same lifecycle as Claude:

1. **Spawn**: A new `codex exec` process is started with the context-injected prompt.
2. **Stream**: JSON events are read and parsed in real time.
3. **Exit**: The process exits when the response is complete.
4. **Ingest**: Events are appended to the SQLite store and temporary output files are cleaned up.

The process does not persist between prompts. See [Session Lifecycle](./lifecycle.md) for the full end-to-end flow.
