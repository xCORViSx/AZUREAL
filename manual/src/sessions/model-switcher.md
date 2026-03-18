# Model Switcher

The model switcher provides a unified way to cycle through all available models across both backends. Pressing `Ctrl+M` advances to the next model in the pool. The backend (Claude or Codex) is derived automatically from the selected model -- there is no separate "switch backend" action.

---

## The Model Cycle

`Ctrl+M` cycles through nine models in a fixed order, wrapping from the last back to the first:

```text
opus --> sonnet --> haiku --> gpt-5.4 --> gpt-5.3-codex --> gpt-5.2-codex --> gpt-5.2 --> gpt-5.1-codex-max --> gpt-5.1-codex-mini --> (wrap to opus)
```

The first three models (opus, sonnet, haiku) use the Claude backend. The remaining six (all `gpt-*` models) use the Codex backend. The transition from haiku to gpt-5.4 automatically switches the backend from Claude to Codex, and the wrap from gpt-5.1-codex-mini back to opus switches it back to Claude.

---

## Backend Derivation

The rule is simple: if the model name starts with `gpt-`, the Codex backend is used. Everything else uses the Claude backend.

```text
gpt-*  -->  Backend::Codex
*      -->  Backend::Claude
```

When the backend changes as a result of a model switch, the background agent processor is reset so that it uses the new backend's event format for parsing.

---

## Model Colors

Each model has an assigned color used in the status bar and session pane headers:

| Model | Color |
|-------|-------|
| `opus` | Magenta |
| `sonnet` | Cyan |
| `haiku` | Yellow |
| `gpt-5.4` | Green |
| `gpt-5.3-codex` | Light Green |
| `gpt-5.2-codex` | RGB(0, 200, 200) |
| `gpt-5.2` | Light Cyan |
| `gpt-5.1-codex-max` | Blue |
| `gpt-5.1-codex-mini` | Light Blue |

These colors appear in the model badge on the status bar and in the right-aligned model label on chat bubble headers. They provide an at-a-glance visual indicator of which model produced a given response.

---

## Auto-Spawned Processes

When AZUREAL spawns agent processes automatically -- such as for Recursive Conflict Resolution (RCR), God File Mitigation (GFM), or Documentation Health (DH) -- those processes also follow the currently selected model. If you switch from opus to gpt-5.4 and then trigger a conflict resolution, the RCR agent will use the Codex backend with gpt-5.4.

---

## Model Persistence

Model selection is persisted to the session store so that it survives application restarts and session switches.

### How It Works

Each time you press `Ctrl+M`, a `DisplayEvent::ModelSwitch` event is injected into the display event stream and appended to the SQLite session store. This event records which model you switched to.

### Restoration on Load

When a session is loaded (on startup, project switch, or worktree switch), AZUREAL scans the session's events in reverse order to determine the model:

1. **ModelSwitch events take priority.** These represent explicit user choices made via `Ctrl+M`. The most recent ModelSwitch event determines the model.
2. **Init events as fallback.** If no ModelSwitch event exists, the model is read from the most recent Init event (which records the model that was active when the agent process started).
3. **Default to opus.** If the session is empty or contains no recognizable model information, the model defaults to `opus`.

This means that if you switch models mid-session, close the application, and reopen it, the session will restore to whichever model you last selected -- not the model the session was originally started with.

### Cross-Backend Sessions

A single session can span both backends. For example, you might start a conversation with opus (Claude), switch to gpt-5.4 (Codex) mid-session, then switch back to sonnet (Claude). The session store records all of these switches, and on reload, the most recent switch is restored. The chat bubble headers show which model produced each response, making the conversation history clear.

---

## Legacy Model Strings

Older sessions may contain the string `"codex"` as a model identifier (from before the unified model pool was introduced). When encountered during session loading, this is mapped to the first Codex model in the pool (currently `gpt-5.4`). Similarly, full Claude API model names like `"claude-3-5-sonnet-20241022"` are recognized and mapped back to their short aliases (`"sonnet"`).
