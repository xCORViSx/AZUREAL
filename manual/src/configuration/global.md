# Global Config

The global configuration file lives at `~/.azureal/azufig.toml`. It holds
settings that apply across all projects: your API key, the path to the Claude
Code binary, your permission mode, the list of registered projects, and any
global run commands or preset prompts.

---

## Location

```text
~/.azureal/
  azufig.toml    <-- global config
  AZUREAL.app/   <-- macOS app bundle (if present)
  speech/        <-- Whisper model files
```

The `~/.azureal/` directory is created automatically on first launch if it does
not exist. The config file itself is created when you first register a project or
save a setting through the UI.

---

## Sections

### `[config]`

Core application settings.

```toml
[config]
api_key = "sk-ant-..."
claude_path = "/usr/local/bin/claude"
permission_mode = "plan"
```

| Key | Description | Default |
|-----|-------------|---------|
| `api_key` | Anthropic API key. Used by Claude Code CLI for authentication. | (none) |
| `claude_path` | Absolute path to the Claude Code CLI binary. AZUREAL uses this to spawn agent processes. | Auto-detected from `$PATH` |
| `permission_mode` | Permission mode passed to Claude Code via `--permission-mode`. Valid values: `"default"`, `"plan"`, `"bypasstool"`. | `"default"` |

The `permission_mode` controls how Claude Code handles tool use permissions:

- `"default"` -- Claude Code asks for permission on each tool use.
- `"plan"` -- Claude Code plans tool use but does not execute without approval.
- `"bypasstool"` -- Claude Code executes tools without asking for permission.

### `[projects]`

Registered projects. Each entry maps a display name to a filesystem path.

```toml
[projects]
AZUREAL = "~/AZUREAL"
Website = "~/projects/website"
```

Keys are display names shown in the Projects Panel. Values are paths to the
project root (the directory containing `.git`). Tilde (`~`) is expanded at
runtime. Paths must point to a valid git repository.

When you register a project through the Projects Panel in the UI, it writes an
entry here. When you unregister a project, the entry is removed.

### `[runcmds]`

Global run commands available in every project. Entries use numbered prefixes to
preserve display order.

```toml
[runcmds]
1_Build = "cargo build --release"
2_Test = "cargo test"
3_Lint = "cargo clippy -- -W clippy::all"
```

The prefix format is `N_Name`, where `N` is a positive integer and `Name` is the
display label shown in the run commands menu. The value is the shell command to
execute.

Global run commands appear alongside project-local run commands in the run
commands list. See [Run Commands](../run-commands.md) for how these are executed.

### `[presetprompts]`

Global preset prompts available in every project. Same prefix format as run
commands.

```toml
[presetprompts]
1_Review = "Review this file for bugs and suggest improvements."
2_Explain = "Explain what this code does, step by step."
3_Refactor = "Refactor this code for clarity without changing behavior."
```

The prefix format is `N_Name`, where `N` determines order and `Name` is the
display label. The value is the prompt text that gets injected into the input
field when selected.

Global preset prompts appear alongside project-local preset prompts. See
[Preset Prompts](../preset-prompts.md) for how these are used in the UI.

---

## Example File

A complete global config:

```toml
[config]
api_key = "sk-ant-api03-xxxxxxxxxxxxxxxxxxxx"
claude_path = "/opt/homebrew/bin/claude"
permission_mode = "bypasstool"

[projects]
AZUREAL = "~/AZUREAL"
Webapp = "~/projects/webapp"
Infra = "~/work/infrastructure"

[runcmds]
1_Build = "cargo build --release"
2_Test = "cargo test -- --nocapture"
3_Format = "cargo fmt --all"

[presetprompts]
1_CodeReview = "Review this code for correctness, performance, and style."
2_WriteTests = "Write comprehensive tests for this module."
```
