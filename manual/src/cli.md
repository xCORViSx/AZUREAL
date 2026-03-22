# CLI Reference

AZUREAL is invoked from the command line as `azureal`. With no arguments, it
launches the TUI. Subcommands provide headless operations for session and project
management.

---

## Usage

```text
azureal [OPTIONS] [SUBCOMMAND]
```

Running `azureal` with no arguments launches the full TUI interface inside the
current terminal. AZUREAL expects to be run from within a git repository (or with
a registered project -- see [Projects Panel](./projects.md)).

### Global Options

| Flag | Description | Default |
|------|-------------|---------|
| `-o, --output <format>` | Output format: `table`, `json`, or `plain` | `table` |
| `-v, --verbose` | Enable verbose output | off |
| `--config <path>` | Path to config file | (auto-detected) |

These flags can be placed before or after any subcommand.

---

## Self-Installation

The AZUREAL binary is **self-installing**. On first run, it detects whether it
has been installed to a standard location on your `PATH`. If not, it copies
itself to the appropriate system or user-local bin directory:

| Platform | Install Path |
|----------|-------------|
| macOS | `/usr/local/bin/azureal` (falls back to `~/.local/bin/azureal`) |
| Linux | `/usr/local/bin/azureal` (falls back to `~/.local/bin/azureal`) |
| Windows | `%USERPROFILE%\.azureal\bin\azureal.exe` |

After the self-install completes, `azureal` is available globally. Subsequent
runs skip the install step. See [Installation](./getting-started/installation.md)
for the full details.

---

## Subcommands

### `azureal session`

Session management subcommands for worktree-based sessions.

#### `azureal session archive <name>`

Archive a worktree session. This removes the worktree's working directory from
disk while preserving the git branch. The worktree appears dimmed in the tab
row and can be restored later.

```sh
azureal session archive my-feature
```

This is equivalent to pressing `Wa` or `a` (with the worktree focused) inside
the TUI. The branch `azureal/my-feature` remains in the local repository and
can be pushed to a remote as usual.

#### `azureal session unarchive <name>`

Unarchive a previously archived session. This recreates the worktree directory
from the preserved branch, restoring it to an active state.

```sh
azureal session unarchive my-feature
```

After unarchiving, the worktree reappears as a normal tab in the TUI with its
full session history intact (stored in the SQLite session store, not in the
worktree directory).

#### `azureal session list`

List all sessions. Alias: `azureal session ls`.

```sh
azureal session list
azureal session list --project /path/to/project --all
```

| Flag | Description |
|------|-------------|
| `-p, --project` | Filter by project path |
| `-a, --all` | Show archived sessions too |

#### `azureal session new`

Create a new session.

```sh
azureal session new -p "Fix the login bug"
azureal session new -p "Add tests" --name my-session --project /path
```

| Flag | Description |
|------|-------------|
| `-p, --prompt` | Initial prompt for the agent (required) |
| `-d, --project` | Project path (defaults to current directory) |
| `-n, --name` | Custom session name |

#### `azureal session status <name>`

Show the status of a session.

#### `azureal session stop <name>`

Stop a running session.

| Flag | Description |
|------|-------------|
| `-f, --force` | Force stop (SIGKILL instead of SIGTERM) |

#### `azureal session delete <name>`

Delete a session and its worktree.

| Flag | Description |
|------|-------------|
| `-y, --yes` | Skip confirmation prompt |

#### `azureal session resume <name>`

Resume a stopped or waiting session.

| Flag | Description |
|------|-------------|
| `-p, --prompt` | Additional prompt to send |

#### `azureal session logs <name>`

Show session logs/output.

| Flag | Description |
|------|-------------|
| `-f, --follow` | Follow output in real-time |
| `-l, --lines` | Number of lines to show (default: 50) |

#### `azureal session diff <name>`

Show diff for a session's worktree.

| Flag | Description |
|------|-------------|
| `--stat` | Show stat only (files changed summary) |

#### `azureal session cleanup`

Clean up worktrees from completed/failed/archived sessions.

| Flag | Description |
|------|-------------|
| `-d, --project` | Project path (defaults to current directory) |
| `--delete-branches` | Also delete the associated git branches |
| `-y, --yes` | Perform cleanup without confirmation |
| `--dry-run` | Only show what would be cleaned up |

### `azureal project`

Project management subcommands.

#### `azureal project list`

List all registered projects. Alias: `azureal project ls`.

#### `azureal project show [project]`

Show details for a project. Defaults to the current directory if no project is
specified.

#### `azureal project remove <project>`

Remove a project from tracking (does not delete the repository).

| Flag | Description |
|------|-------------|
| `-y, --yes` | Skip confirmation prompt |

#### `azureal project config`

Show or update project configuration.

| Flag | Description |
|------|-------------|
| `-p, --project` | Project path (defaults to current directory) |
| `--main-branch` | Set the main branch name |

### Shortcut Commands

Several session operations have top-level shortcuts:

| Shortcut | Equivalent |
|----------|-----------|
| `azureal list` (or `azureal ls`) | `azureal session list` |
| `azureal new -p "prompt"` | `azureal session new -p "prompt"` |
| `azureal status <name>` | `azureal session status <name>` |
| `azureal diff <name>` | `azureal session diff <name>` |

---

## Logging

AZUREAL uses `tracing-subscriber` with `env-filter` for structured logging.
Log output is controlled through the standard `RUST_LOG` environment variable.

### Setting the Log Level

```sh
# Errors only (default when RUST_LOG is unset)
RUST_LOG=error azureal

# Warnings and errors
RUST_LOG=warn azureal

# Info-level logging
RUST_LOG=info azureal

# Debug logging (verbose)
RUST_LOG=debug azureal

# Trace logging (extremely verbose)
RUST_LOG=trace azureal
```

### Module-Level Filtering

`env-filter` supports per-module log levels, which is useful for isolating
specific subsystems without drowning in output from everything else:

```sh
# Debug logging for the session store, info for everything else
RUST_LOG=info,azureal::session_store=debug azureal

# Trace the event loop only
RUST_LOG=warn,azureal::event_loop=trace azureal
```

Logs are written to stderr, so they do not interfere with TUI rendering. When
diagnosing issues, redirect stderr to a file:

```sh
RUST_LOG=debug azureal 2> azureal.log
```

---

## Debug Dump

The `Ctrl+D` keybinding inside the TUI triggers a debug dump -- a snapshot of
internal state written to a named text file in `.azureal/`. This is covered in detail at
[Debug Dump](./debug-dump.md). The dump is useful for filing bug reports or
inspecting runtime state without attaching a debugger.

---

## Version

```sh
azureal --version
```

Prints the version number and exits.

---

## Summary

| Command | Description |
|---------|-------------|
| `azureal` | Launch the TUI |
| `azureal tui` | Launch the TUI (explicit subcommand) |
| `azureal --version` | Print version and exit |
| `azureal session list` | List all sessions |
| `azureal session new -p "prompt"` | Create a new session |
| `azureal session status <name>` | Show session status |
| `azureal session stop <name>` | Stop a running session |
| `azureal session delete <name>` | Delete a session and its worktree |
| `azureal session archive <name>` | Archive a worktree (remove directory, keep branch) |
| `azureal session unarchive <name>` | Unarchive a worktree (recreate directory from branch) |
| `azureal session resume <name>` | Resume a stopped session |
| `azureal session logs <name>` | Show session logs |
| `azureal session diff <name>` | Show worktree diff |
| `azureal session cleanup` | Clean up completed/failed worktrees |
| `azureal project list` | List registered projects |
| `azureal project show [project]` | Show project details |
| `azureal project remove <project>` | Remove project from tracking |
| `azureal project config` | Show/update project configuration |

| Environment Variable | Description |
|---------------------|-------------|
| `RUST_LOG` | Controls log verbosity via `tracing-subscriber` env-filter |
