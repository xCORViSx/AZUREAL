# Configuration

AZUREAL uses two TOML configuration files, both named `azufig.toml`. One lives
in your home directory and holds global preferences that apply everywhere. The
other lives inside each project and holds project-specific settings. Both use the
same format conventions, and both are optional -- AZUREAL runs with sensible
defaults when neither file exists.

---

## Two-File Model

| File | Location | Scope |
|------|----------|-------|
| **Global** | `~/.azureal/azufig.toml` | All projects, all worktrees |
| **Project** | `.azureal/azufig.toml` (at main worktree root) | One project, shared across all its worktrees |

The global config stores your API key, the path to your Claude Code binary, your
permission mode, your registered projects list, and any run commands or preset
prompts you want available in every project. The project config stores
project-local overrides: file tree hidden entries, health panel scan scope, git
auto-rebase rules, and project-specific run commands and preset prompts.

There is no merging or inheritance between the two files. Global run commands and
project run commands both appear in the run commands list, but they are loaded
from their respective files independently.

---

## Format Conventions

Both files follow the same TOML conventions:

- **Section headers** use single-bracket `[section]` notation (e.g., `[config]`,
  `[git]`, `[filetree]`).
- **Key-value pairs** use `key = "value"` format. Keys that qualify as TOML bare
  keys (alphanumeric, dashes, underscores) are written unquoted. Values are
  always quoted strings.
- **Numbered prefixes** are used for ordered entries in `[runcmds]` and
  `[presetprompts]`: `1_Build = "cargo build"`, `2_Test = "cargo test"`. The
  numeric prefix determines display order; the part after the underscore becomes
  the display name.
- **Every section** uses `#[serde(default)]` in the Rust deserialization, so
  missing sections are silently filled with empty defaults. You never need to
  include a section you have no keys for.

---

## Write Pattern

AZUREAL never edits config files in place. The write cycle is always
**load-modify-save**: read the entire file into a struct, update the relevant
field, serialize the full struct back to disk. This avoids partial writes,
preserves all sections, and keeps the file in a consistent state even if the
process is interrupted.

---

## Project Config Location

The project-level `azufig.toml` always lives at the **main worktree root**
inside `.azureal/`. Because git worktrees share a common `.git` directory, the
project config is effectively shared by all worktrees in the project. You do not
create separate configs per worktree.

The `.azureal/` directory is **gitignored by default** -- AZUREAL automatically
adds `.azureal/` to `.gitignore` (alongside `worktrees/`) on first load. This
prevents the session store, worktree-level configs, and other runtime files from
causing rebase conflicts.

---

## Chapter Contents

- **[Global Config](./configuration/global.md)** -- The `~/.azureal/azufig.toml`
  file: API key, Claude path, permission mode, registered projects, global run
  commands, and global preset prompts.
- **[Project Config](./configuration/project.md)** -- The
  `.azureal/azufig.toml` file: file tree hidden entries, health scan scope, git
  auto-rebase and auto-resolve settings, project-local run commands, and
  project-local preset prompts.
