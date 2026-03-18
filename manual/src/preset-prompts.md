# Preset Prompts

Preset Prompts are reusable prompt templates that you can insert into the prompt
buffer with a single keybinding. They are useful for frequently repeated
instructions -- things like "review this file for bugs", "add tests for the
selected function", or "refactor to reduce complexity". You can define up to 10
presets, each accessible by a dedicated `Alt+number` shortcut.

---

## Quick-Select

The fastest way to use a preset is the direct shortcut:

| Key | Action |
|-----|--------|
| `Alt+1` through `Alt+9` | Insert preset 1-9 directly |
| `Alt+0` | Insert preset 10 directly |

These shortcuts work in **prompt mode only** and skip the picker entirely. The
preset text is inserted into the prompt buffer immediately.

---

## The Picker

Press **`Alt+P`** in prompt mode to open the preset prompt picker. The picker
shows all defined presets with their position numbers and scope badges.

| Key | Action |
|-----|--------|
| `1`-`9`, `0` | Quick-select by number |
| `Enter` | Insert the highlighted preset |
| `a` | Add a new preset |
| `e` | Edit the highlighted preset |
| `d` | Delete the highlighted preset (prompts `y`/`n` confirmation) |

Each preset in the picker displays a **G** (global) or **P** (project) badge
indicating its scope.

---

## Dual Scope

Presets support two scopes, just like [Run Commands](./run-commands.md):

| Scope | Config File | Badge |
|-------|-------------|-------|
| **Global** | `~/.azureal/azufig.toml` | G |
| **Project** | `.azureal/azufig.toml` (in project root) | P |

Press **`Ctrl+G`** in the preset dialog to toggle between global and project
scope.

Global presets are available everywhere. Project-local presets are specific to
the current project and override global presets at the same position number.

---

## Hint Display

A hint is shown in the prompt title bar reminding you that preset prompts are
available. This serves as a passive reminder of the feature without requiring you
to memorize the keybinding.

---

## Quick Reference

```text
Alt+P         Open preset prompt picker (prompt mode only)
Alt+1 - Alt+9 Quick-select preset 1-9 (prompt mode only)
Alt+0         Quick-select preset 10 (prompt mode only)
Ctrl+G        Toggle global/project scope (in dialog)
```
