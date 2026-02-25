# Changelog

All notable changes to Azureal will be documented in this file.

## [Unreleased]

### Added
- **Rebase-before-merge flow** — Squash merge (`m`) now rebases the feature branch onto main before merging, ensuring the merge is always clean and linear. Conflicts are resolved during rebase (on the feature branch), never during merge (on main).
- **Manual rebase action** — Press `r` in the Git Actions panel to rebase the current feature branch onto main. On conflict, shows the same conflict overlay with RCR option.
- **RCR auto-continues squash merge** — When rebase conflicts occur during a squash merge, RCR tracks `continue_with_merge=true`. After accepting the resolution, the squash merge executes automatically instead of requiring the user to re-trigger it. Manual rebase RCR just shows "Rebase complete".
- **RCR (Rebase Conflict Resolution) interactive mode** — When rebase encounters conflicts, Claude is spawned on the feature branch worktree to resolve them. The convo pane enters RCR mode: green borders and title `[RCR] <worktree>`. Claude uses `git add` + `git rebase --continue` (repeating for multi-step rebases). After Claude exits, an approval dialog asks to accept or review more. State tracked by `RcrSession` struct with `worktree_path`, `repo_root`, and `continue_with_merge` fields.
- **Conflict resolution overlay** — When rebase produces conflicts, a red-bordered `GitConflictOverlay` opens listing conflicted files (red) and auto-merged files (green). `[y] Resolve with Claude` spawns RCR; `[n] Abort rebase` runs `git rebase --abort`. Rebase is left in progress (not auto-aborted) so RCR can resolve it.
- **Branch dialog** — Press `b` to browse all branches with `[active]` indicators for checked-out branches. Active branches switch focus; inactive branches create new worktrees. Main/master branches filtered out.
- **"Add session" renamed** — "New session" action renamed to "Add session" to be alliterative with the `a` keybind.
- **Auto-rebase toggle** — Press `a` in the Git panel (feature branches) to toggle auto-rebase per worktree. When enabled, the worktree is automatically rebased onto main every 2 seconds when main is ahead — but only when no Claude agent is running, no file is being edited, and no RCR is active. On conflict, opens the conflict overlay with full RCR flow. Sidebar shows colored `R` indicator: green (enabled), orange (RCR active), blue (approval pending). Setting persisted in `.azureal/azufig.toml`. Success dialog auto-dismisses after 2 seconds.

### Changed
- **Rich squash merge commit messages** — `squash_merge_into_main()` now collects all individual commit messages via `git log HEAD..branch --reverse --format="- %s"` and includes them as bullet points in the commit body, preserving the detail of individual commits that would otherwise be lost in a squash merge.
- **RCR runs on feature branch worktree** — RCR sessions now spawn Claude in the feature branch worktree directory (where the rebase is happening) instead of the repo root. Prompts are routed to `rcr.worktree_path`. Abort uses `git rebase --abort` on the worktree instead of `git reset --hard` on main.
- **Feature branches have 4 Git Actions** — Feature branch action list expanded from 3 to 4: squash-merge (`m`), rebase (`r`), commit (`c`), push (`P`). `action_count(is_on_main)` replaces the old `ACTION_COUNT = 3` constant.
- **Health panel title shows worktree name** — Panel title changed from static `" Worktree Health "` to `" Health: <worktree> "`, matching the Git panel's naming pattern.

### Fixed
- **Auto-rebase skips dirty worktrees** — `check_auto_rebase()` now checks `git status --porcelain` before rebasing; worktrees with uncommitted changes are silently skipped instead of failing.
- **Orphaned rebase state cleaned up on startup** — On launch, all worktrees are scanned for `.git/rebase-merge/` or `.git/rebase-apply/` directories (left by app crashes during rebase) and auto-aborted.
- **Stash orphaned after merge-triggered RCR** — `accept_rcr()` now pops any stash left on main before re-calling `squash_merge_into_main()`, preventing orphaned stash entries.
- **Auto-rebase config not cleaned up on worktree removal** — Post-merge dialog archive/delete handlers now remove the `auto-rebase/<branch>` key from `azufig.toml` and the in-memory `HashSet`.
- **Push/pull fails on worktree branches without upstream** — `Git::push()` now auto-sets upstream with `git push -u origin <branch>` on first push. `Git::pull()` falls back to `git pull origin <branch>` when no upstream is configured.
- **Squash merge fails when main has dirty working tree** — `squash_merge_into_main()` now stashes dirty state before merging and pops after commit.
- **Squash merge with uncommitted changes loses work** — `exec_squash_merge()` now blocks if the feature branch has uncommitted changes.
- **RCR convo disappears after Claude exits** — Fixed by skipping session-file re-parse when the exiting slot's file doesn't exist in the current worktree's session directory.
- **Branch dialog showed no branches** — All branches were checked out in worktrees and filtered out. Fixed by showing all branches with checked-out indicators instead of filtering.
- **Branch dialog froze UI** — `list_remote_branches` did `git fetch --all --prune` (blocking network call). Replaced with `list_remote_branches_cached` that reads local cache only.
- **Branch dialog created wrong worktree type** — Used `Git::create_worktree` (creates new branch with `-b`) instead of `Git::create_worktree_from_branch` for existing branches.

### Removed
- **Auto-rebase of peer worktrees** — Removed automatic rebase of peer worktrees after squash merge. Manual rebase (`r`) replaces it — auto-rebase wastes resources when not needed.
- **`pre_merge_head` from RcrSession** — No longer needed since RCR uses `git rebase --abort` (which handles state restoration) instead of `git reset --hard`.
- **1,562 lines of dead code** — Removed unused types (`ContextMenu`, `WorktreeAction`, `ViewMode::Diff/Messages/Rebase`, `RebaseState`, `RebaseStatus`), dead methods (`load_diff`, `scroll_diff_*`, `destroy_terminal`, `session_has_terminal`), the entire `InteractiveSession` system (`spawn_interactive`, `send_prompt`), unused git ops (`fetch`, `merge_abort`, `rebase_onto_main`, 6 more rebase helpers), `DiffHighlighter` (syntect-based, replaced by Viewer pane), `DisplayEvent::Error`, `ClaudeEvent::Error`, and all orphaned match arms/imports. Compiler now produces zero warnings.
