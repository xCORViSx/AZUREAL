# Plan: Auto-gitignore worktrees/ on project load

## Goal

When the app loads a project, automatically ensure `worktrees/` is in the project's `.gitignore` and commit that change. This prevents future worktrees from inheriting the `worktrees/` folder.

## Changes

### 1. `src/git/core.rs` — New function `Git::ensure_worktrees_gitignored()`

- Read `.gitignore` at `repo_root` (or treat as empty if it doesn't exist)
- Check if any line matches `worktrees/` or `/worktrees/` or `worktrees`
- If not found: append `worktrees/` to the file (create if needed)
- Stage `.gitignore` with `git add .gitignore`
- Commit with message `chore: add worktrees/ to .gitignore`
- Silently no-op if already present or if any git command fails

### 2. `src/app/state/load.rs` — Call from `App::load()`

- Add `Git::ensure_worktrees_gitignored(&repo_root);` right after the `register_project` call (line ~30), before `load_worktrees()`
- This runs once per app launch, idempotent

## Notes

- Pattern follows existing `untrack_gitignored_files()` style — fire-and-forget, silent on failure
- The commit only happens once per project (subsequent loads find `worktrees/` already present)
- Also add `.azureal/` if not present since that's project-local config that shouldn't propagate to worktrees either
