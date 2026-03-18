# Run Commands

Run Commands let you save and execute shell commands from anywhere in AZUREAL.
Instead of switching to a terminal to run your build, test, or deploy scripts,
you define them once and trigger them with a couple of keystrokes. Commands can
be scoped globally (available in every project) or locally (specific to a single
project).

---

## Opening the Picker

Press **`r`** (global keybinding) to open the run command picker. If only one
command is defined, it executes immediately without showing the picker.

Press **`Alt+r`** to open the new command dialog directly, bypassing the picker.

---

## The Picker

The picker lists all defined run commands for the current scope. Each entry shows
its position number and name.

| Key | Action |
|-----|--------|
| `j` / `k` | Navigate up and down |
| `1`-`9` | Quick-select by position number |
| `Enter` | Execute the highlighted command |
| `e` | Edit the highlighted command |
| `d` | Delete the highlighted command (prompts `y`/`n` confirmation) |
| `a` | Add a new command |

---

## Creating a Command

The new command dialog (opened via `Alt+r` or `a` from the picker) has two
fields:

1. **Name** -- A short label for the command (e.g., "Build", "Test", "Deploy").
2. **Command / Prompt** -- The shell command or natural-language prompt to
   execute.

**`Tab`** cycles focus between the Name field and the Command/Prompt field.

### Command Mode vs Prompt Mode

Inside the Command/Prompt field, pressing **`Tab`** toggles between two input
modes:

- **Command mode** -- The text is treated as a raw shell command and executed
  directly (e.g., `cargo build --release`).
- **Prompt mode** -- The text is treated as a natural-language description.
  AZUREAL spawns a `[NewRunCmd]` agent session that asks Claude to generate the
  appropriate shell command. Once the session completes, the generated command is
  automatically reloaded into your run command list.

---

## Dual Scope

Run commands support two scopes:

| Scope | Config File | Availability |
|-------|-------------|--------------|
| **Global** | `~/.azureal/azufig.toml` | Every project |
| **Project** | `.azureal/azufig.toml` (in project root) | Current project only |

Press **`Ctrl+S`** in the dialog to toggle between global and project scope.

Both scopes are merged in the picker, with project-local commands taking
precedence if names collide.

---

## Storage Format

Commands are stored under the `[runcmds]` table in the respective `azufig.toml`
file. Keys are prefixed with a position number that determines their order in the
picker:

```toml
[runcmds]
1_Build = "cargo build"
2_Test = "cargo test"
3_Deploy = "./scripts/deploy.sh"
```

The position prefix (`1_`, `2_`, etc.) controls the display order and maps to the
`1`-`9` quick-select keys in the picker.

---

## Quick Reference

```text
r          Open run command picker (or execute if only 1 defined)
Alt+r      Open new command dialog
Ctrl+S     Toggle global/project scope (in dialog)
Tab        Cycle fields / toggle Command vs Prompt mode (in dialog)
1-9        Quick-select in picker
Enter      Execute selected command
e          Edit selected command
d          Delete selected command (y/n)
a          Add new command from picker
```
