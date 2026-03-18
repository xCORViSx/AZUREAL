# Agent Sessions

AZUREAL orchestrates AI coding agents by wrapping two CLI tools -- **Claude Code CLI** and **OpenAI Codex CLI** -- behind a unified interface. You interact with both backends through the same prompt input, the same session pane, and the same keybindings. The backend is determined automatically from whichever model you have selected.

---

## Dual Backend Architecture

Two backends exist:

| Backend | CLI Tool | Triggered By |
|---------|----------|--------------|
| `Backend::Claude` | Claude Code CLI | Any non-`gpt-*` model (opus, sonnet, haiku) |
| `Backend::Codex` | Codex CLI | Any `gpt-*` model (gpt-5.4, gpt-5.3-codex, etc.) |

The `AgentProcess` struct holds both a `ClaudeProcess` and a `CodexProcess`. At spawn time, the active model determines which backend handles the request. You never configure the backend directly -- switching models with `Ctrl+M` is all it takes.

Both backends produce the same `AgentEvent` and `DisplayEvent` types, so the rest of the application (session pane, session store, rendering) is backend-agnostic. A single session can span prompts to both Claude and Codex models if you switch models mid-conversation.

---

## Process-Per-Prompt Model

Agent processes do not persist between prompts. Each time you press Enter to submit a prompt, a new CLI process is spawned. The process streams its response back as JSON events, and when the response is complete, the process exits. The next prompt spawns a fresh process.

This design exists because Claude Code's interactive mode uses a full TUI that cannot be driven via stdin. The `-p` (prompt) mode exits after each response. The spawn overhead is approximately 100-200ms -- imperceptible in practice.

Conversation continuity is maintained not by keeping a process alive, but by injecting context from the SQLite session store into each new prompt. See [Session Store & Persistence](./session-store.md) for details on how context injection works.

---

## Chat Bubble Headers

Each response in the session pane is displayed in a chat bubble. The header of each bubble shows the agent name left-aligned and the model ID right-aligned in a subdued style. This makes it easy to see which model produced a given response, especially in sessions that span multiple models.

---

## Chapter Contents

- **[Claude Backend](./sessions/claude.md)** -- How AZUREAL invokes Claude Code CLI, captures session IDs, and handles permission modes.
- **[Codex Backend](./sessions/codex.md)** -- How AZUREAL invokes Codex CLI, captures thread IDs, and handles permission modes.
- **[Model Switcher](./sessions/model-switcher.md)** -- The unified model cycle (`Ctrl+M`), model colors, backend derivation, and model persistence across sessions.
- **[Multi-Agent Concurrency](./sessions/multi-agent.md)** -- PID-keyed session slots, running multiple agents per worktree, and slot switching.
- **[Session Lifecycle](./sessions/lifecycle.md)** -- The full prompt-to-response cycle, from user input through context injection, process spawn, streaming, parsing, and store ingestion.
