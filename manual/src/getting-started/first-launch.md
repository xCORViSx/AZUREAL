# First Launch

This page describes what happens the first time you run `azureal`, and what each
screen means.

## The Splash Screen

When AZUREAL starts, it displays a splash screen: a 2x-scale block-character
rendering of the word "AZUREAL" with a dim butterfly mascot in the background.
This screen is shown for a minimum of 3 seconds while the application performs
git discovery -- scanning the current directory for a git repository, resolving
the main worktree root, and enumerating existing worktrees.

No input is accepted during the splash. It transitions automatically once both
the timer and git discovery complete.

## Project Detection

What happens next depends on where you launched `azureal` from.

### Inside a Git Repository

If you run `azureal` from within a git repository (or any subdirectory of one),
the project is **auto-registered** and loads immediately. AZUREAL resolves the
repository root via `git rev-parse --git-common-dir`, registers it in the global
config, and transitions to the main interface.

### Outside a Git Repository

If you run `azureal` from a directory that is not part of any git repository,
AZUREAL opens the **Projects panel** full-screen. From here you can:

- Select an existing registered project to open.
- Register a new project by navigating to its path.
- Initialize a new git repository and register it as a project.

## Welcome Modal

When a project loads but has **no worktrees** yet (only the main branch exists),
AZUREAL displays a welcome modal with the following options:

| Key | Action |
|-----|--------|
| `M` | Browse the main branch -- opens the file browser and session pane on `main` |
| `Wn` | Create a new worktree -- `W` leader sequence followed by `n` to open the branch dialog |
| `P` | Open the Projects panel -- switch to a different project or register a new one |
| `Ctrl+Q` | Quit AZUREAL |

This modal appears only when there are no feature worktrees. Once you create
your first worktree, subsequent launches skip the modal and load the last active
worktree directly.

## Configuration Files Created

On first launch, AZUREAL creates two configuration files:

### Global Config

```
~/.azureal/azufig.toml
```

Stores application-wide settings: registered project paths and display names,
permission mode preferences, global run commands, and global preset prompts.
This file is shared across all projects.

### Project-Local Config

```
<project-root>/.azureal/azufig.toml
```

Stores project-specific settings: file tree hidden entries, health scan scope
directories, project-local run commands, preset prompts, and git settings like
per-branch auto-rebase rules and auto-resolve file lists.

This file lives at the **main worktree root** (the original clone directory) and
is shared by all worktrees in the project. The entire `.azureal/` directory is
**gitignored by default** to prevent the session store and runtime files from
causing rebase conflicts.

## Session Store

The session store file is **not** created at first launch. It is created lazily:

```
<project-root>/.azureal/sessions.azs
```

This SQLite database (with a custom `.azs` extension) is created only when you
send your first prompt to an agent or open the session list. It stores all
conversation history, compaction summaries, and session metadata. The `.azs`
extension signals that it is a managed binary file and should not be edited
manually.

## What to Do Next

After the splash screen and project detection complete, you are ready to start
working:

1. **Create a worktree** (`Wn` from the welcome modal -- `W` leader followed by
   `n`) to start an isolated feature branch.
2. **Send a prompt** to an agent by typing in the session pane input area and
   pressing Enter.
3. **Browse the file tree** to review changes the agent has made.

For a full tour of the interface layout and navigation, continue to
[The TUI Interface](../tui-interface.md).
