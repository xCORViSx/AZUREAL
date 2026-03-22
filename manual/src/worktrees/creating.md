# Creating Worktrees

Creating a worktree gives you a fully isolated working directory with its own
branch, file tree, terminal, and agent sessions. Every worktree lives under
`<repo>/worktrees/<name>` on a branch named `azureal/<name>`.

---

## The `Wn` Leader Sequence

Worktree creation uses a two-key leader sequence. Press `Shift+W` to enter
leader mode (the status bar shows `W ...`), then press `n` to open the branch
dialog. Press `Esc` at any point to cancel.

This leader pattern keeps destructive worktree operations behind an intentional
two-step keypress, preventing accidental triggers from normal typing.

---

## The Branch Dialog

The branch dialog is a centered overlay that lists all available branches in the
repository. It has two purposes: creating a brand-new branch or checking out an
existing one as a worktree.

### Layout

The first row is always **"Create new"**. Below it, every local and remote
branch is listed. Each branch row shows:

- A **checkmark** if the branch is already checked out in an active worktree
- A **worktree count** showing how many worktrees (active + archived) use that
  branch
- The full branch name

### Filtering

Start typing to filter the branch list. The filter bar accepts git-safe
characters (alphanumeric plus `-`, `_`, `.`, `+`, `@`, `/`, `!`). The list
narrows in real time as you type. `Backspace` widens the filter, and
`Left`/`Right` move the cursor within the filter text.

### Creating a New Branch

When the **"Create new"** row is selected (the default), type a name into the
filter field and press `Enter`. AZUREAL will:

1. Create a git worktree at `<repo>/worktrees/<name>`
2. Check it out on a new branch named `azureal/<name>`
3. Refresh the tab row and auto-select the new worktree
4. Enable **auto-rebase** for the new worktree by default (persisted to
   `.azureal/azufig.toml`)

The name you type becomes both the directory name and the branch suffix. For
example, typing `auth-refactor` creates `worktrees/auth-refactor` on branch
`azureal/auth-refactor`.

If a worktree with that name already exists, creation fails with an error
message in the status bar.

### Checking Out an Existing Branch

Navigate to an existing branch with `j`/`k` and press `Enter`. If the branch is
already checked out in a worktree (shown by the checkmark), AZUREAL switches
focus to that worktree instead of creating a duplicate. Otherwise, a new
worktree is created from the selected branch.

---

## What Happens After Creation

Creation runs on a background thread so the UI stays responsive. A
"Creating worktree..." loading indicator appears while git does its work. Once
complete:

- The **tab row** gains a new tab for the worktree
- The new worktree is **auto-selected** and its file tree loads immediately
- **Auto-rebase** is enabled by default, keeping the branch up to date with main
  (the green `R` indicator appears on the tab)
- No session is created yet -- the SQLite session store is only populated when
  you send your first prompt or explicitly create a session via `a` in the
  Session pane

From here, the worktree is a self-contained environment. You can open files in
the viewer, start a terminal shell, or send a prompt to begin an agent session.
