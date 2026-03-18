# Projects Panel

AZUREAL supports working across multiple repositories through its Projects
panel. Each project is a git repository registered with AZUREAL, and you can
switch between them while preserving the full state of every project -- running
agent sessions, file trees, terminal shells, and all. This makes it possible to
manage an entire portfolio of repositories from a single AZUREAL instance.

---

## How It Works

Projects are stored persistently in `~/.azureal/azufig.toml` under the
`[projects]` section. Each project entry records the repository path and a
display name. AZUREAL loads this list on startup and provides a panel for
switching between projects, adding new ones, and managing the list.

When you switch projects, AZUREAL takes a snapshot of the current project's
entire state and restores the target project's snapshot. Agent processes
continue running in the background for the project you leave -- their output is
captured to session files even though it is not displayed until you switch back.

---

## Automatic Registration

When AZUREAL launches inside a git repository, it automatically registers that
repository as a project. The display name is derived from the git remote URL
when available, falling back to the folder name if no remote is configured. You
never need to manually add the repository you launched from.

---

## Opening the Panel

| Context | Key | Effect |
|---------|-----|--------|
| Worktree view | `P` | Open Projects panel |
| Startup (not in a git repo) | Automatic | Projects panel shown full-screen |

When AZUREAL starts outside of a git repository, the Projects panel appears
automatically as a full-screen view with a prompt to initialize a new repository
or select an existing project.

---

## Auto-Pruning

On load, AZUREAL silently prunes the project list: any entry whose directory no
longer exists or is no longer a valid git repository is removed from the
configuration file. This keeps the project list clean without manual
intervention.

---

## Chapter Contents

- **[Managing Projects](./projects/managing.md)** -- Adding, removing, renaming,
  initializing, and switching between projects.
- **[Parallel Projects](./projects/parallel.md)** -- How project state is
  snapshotted and restored, and what happens to background processes.
- **[Background Processes](./projects/background.md)** -- How agent exit events
  are handled for non-active projects, and activity status indicators.
