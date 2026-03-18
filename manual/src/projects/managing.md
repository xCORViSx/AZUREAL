# Managing Projects

The Projects panel provides all the operations needed to maintain your project
list: adding repositories, switching between them, renaming display names,
initializing new repos, and removing entries.

---

## Panel Actions

| Key | Action |
|-----|--------|
| `Enter` | Switch to selected project |
| `a` | Add a project by path |
| `d` | Delete project from list |
| `n` | Rename display name |
| `i` | Initialize a new git repository |
| `Esc` | Close panel (only if a project is loaded) |
| `Ctrl+Q` | Quit AZUREAL |

---

## Switching Projects (Enter)

Pressing Enter on a project in the list performs a full project switch:

1. **Validation** -- The target directory is checked to confirm it is still a
   valid git repository. If it is not, the entry is pruned and an error message
   appears.
2. **Process cleanup** -- All Claude processes for the current project are
   killed. This ensures clean separation between projects.
3. **State snapshot** -- The current project's full state is captured (see
   [Parallel Projects](./parallel.md) for what is included in the snapshot).
4. **Reload** -- AZUREAL reloads with the target project's repository,
   restoring its snapshot if one exists.

The switch is effectively a full context change: the file tree, worktrees,
sessions, terminals, viewer tabs, and git state all update to reflect the new
project.

---

## Adding a Project (a)

Pressing `a` opens a path input dialog. Enter the absolute path to a git
repository. AZUREAL validates that the path points to a valid git repository
before adding it to the project list. If validation fails, an error message
explains why.

The display name for an added project is derived automatically from the git
remote URL. If no remote is configured, the folder name is used.

---

## Deleting a Project (d)

Pressing `d` removes the selected project from AZUREAL's project list. This
only removes the entry from `~/.azureal/azufig.toml` -- it does **not** delete
the repository from disk. Your files, branches, and commit history remain
untouched.

---

## Renaming a Project (n)

Pressing `n` opens a text input to change the selected project's display name.
The display name is what appears in the project list and in status messages. It
has no effect on the actual repository directory or git configuration.

---

## Initializing a Repository (i)

Pressing `i` prompts for a directory path. If the path is left blank, AZUREAL
uses the current working directory. A new git repository is initialized at that
location via `git init`, and the new repository is automatically added to the
project list.

This is primarily useful when AZUREAL starts outside of a git repository and
presents the full-screen project panel. You can initialize a repo right from
the panel without dropping to a shell.

---

## Closing the Panel (Esc)

Pressing `Esc` closes the Projects panel and returns to the normal worktree
view. This only works if a project is currently loaded -- if AZUREAL started
without a project (outside a git repo), the panel cannot be dismissed until you
select or initialize a project.

`Ctrl+Q` quits AZUREAL entirely, regardless of whether a project is loaded.
