# Project Config

The project-level configuration file lives at `.azureal/azufig.toml` relative to
the main worktree root. It holds settings scoped to a single project: file tree
hidden entries, health panel scan directories, git automation rules, and
project-specific run commands and preset prompts.

---

## Location

```text
<project-root>/
  .azureal/
    azufig.toml      <-- project config
    sessions.azs     <-- session store (SQLite)
  .git/
  src/
  ...
```

The `.azureal/` directory is gitignored. The project config lives alongside the
session store and is local to the machine -- it is not committed to the
repository.

Because git worktrees share a common working tree root, the project config file
at the main worktree root is shared by all worktrees in the project. You do not
maintain separate configs per worktree.

---

## Sections

### `[filetree]`

Controls which entries are hidden in the file tree pane.

```toml
[filetree]
hidden = ["worktrees", ".git", "target", "node_modules", ".DS_Store"]
```

| Key | Description | Default |
|-----|-------------|---------|
| `hidden` | Array of file/directory names to hide from the file tree. | `[]` |

Entries are matched by exact name against the filename component (not the full
path). If a directory is named `target`, adding `"target"` hides it at every
level of the tree. This is a display-only filter -- hidden entries still exist on
disk and are accessible via the embedded terminal.

### `[healthscope]`

Directories included in health panel scans. Also aliased as `[godfilescope]` for
backward compatibility.

```toml
[healthscope]
directories = ["src", "crates", "lib"]
```

| Key | Description | Default |
|-----|-------------|---------|
| `directories` | Array of directory names to include when scanning for god files and documentation coverage. | (scans entire project) |

When this section is absent or empty, the health panel scans the entire project
tree. When populated, only the listed directories are scanned. This is useful for
large monorepos where you want to focus health checks on specific crates or
modules. See [Scope Mode](../health-panel/scope-mode.md) for how to configure
this interactively from the health panel UI.

### `[runcmds]`

Project-local run commands. Same format as the global `[runcmds]` section.

```toml
[runcmds]
1_Dev = "cargo run -- --dev"
2_Migrate = "sqlx migrate run"
3_Seed = "cargo run --bin seed"
```

Project-local run commands appear in the run commands menu alongside global ones.
They are only visible when this project is active.

### `[presetprompts]`

Project-local preset prompts. Same format as the global `[presetprompts]`
section.

```toml
[presetprompts]
1_FixLint = "Fix all clippy warnings in this file."
2_AddDocs = "Add documentation comments to all public items in this module."
```

Project-local preset prompts appear alongside global ones in the preset prompts
menu and are only visible when this project is active.

### `[git]`

Git automation settings, organized into subsections.

#### `[git.auto-rebase]`

Per-branch auto-rebase settings. When enabled for a branch, AZUREAL
automatically rebases the branch onto `main` after each agent session completes.

```toml
[git.auto-rebase]
feat-auth = "true"
fix-parser = "true"
```

Keys are branch names. Values are `"true"` to enable auto-rebase or `"false"`
(or absent) to disable it. Auto-rebase is deferred while an agent is actively
streaming to avoid interrupting work in progress.

See [Rebase & Auto-Rebase](../git-panel/rebase.md) for how auto-rebase
integrates with the git panel.

#### `[git.auto-resolve]`

Per-file auto-resolve settings for merge conflicts. When enabled for a file,
AZUREAL automatically resolves conflicts in that file during rebase operations
without prompting.

```toml
[git.auto-resolve]
Cargo.lock = "true"
package-lock.json = "true"
```

Keys are filenames (not paths). Values are `"true"` to enable auto-resolve. This
is most useful for lock files and generated files that will be regenerated after
the rebase completes.

See [Auto-Resolve Settings](../git-panel/auto-resolve.md) for details.

---

## Example File

A complete project config:

```toml
[filetree]
hidden = ["worktrees", ".git", "target", "node_modules", ".DS_Store", ".azureal"]

[healthscope]
directories = ["src", "crates"]

[runcmds]
1_Dev = "cargo run"
2_Test = "cargo test"
3_Check = "cargo check --all-targets"

[presetprompts]
1_Modularize = "Break this file into smaller modules, one feature per file."
2_Optimize = "Profile and optimize the hot path in this function."

[git.auto-rebase]
feat-session-store = "true"
feat-render-pipeline = "true"

[git.auto-resolve]
Cargo.lock = "true"
```
