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

This is equivalent to pressing `Wx` or `x` (with the worktree focused) inside
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

### `azureal project`

Project management subcommands. These mirror the operations available in the
[Projects Panel](./projects.md).

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
| `azureal --version` | Print version and exit |
| `azureal session archive <name>` | Archive a worktree (remove directory, keep branch) |
| `azureal session unarchive <name>` | Unarchive a worktree (recreate directory from branch) |
| `azureal project` | Project management subcommands |

| Environment Variable | Description |
|---------------------|-------------|
| `RUST_LOG` | Controls log verbosity via `tracing-subscriber` env-filter |
