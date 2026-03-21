# SUMMARY

Azureal (Asynchronous Zoned Unified Runtime Environment for Agentic LLMs) is a Rust TUI application that wraps Claude Code CLI and OpenAI Codex CLI to enable multi-agent development workflows. Each **worktree** is a git worktree with its own agent **session**, allowing concurrent AI-assisted development across multiple feature branches.

**Terminology:**
- **Worktree**: A git worktree with its own working directory and branch (displayed in left panel)
- **Session**: An agent conversation — Claude sessions stored in `~/.claude/projects/`, Codex sessions in `~/.codex/sessions/YYYY/MM/DD/` (displayed in Session pane)
- **Backend**: `Backend::Claude` or `Backend::Codex` — derived dynamically from the active model (`gpt-*` → Codex, everything else → Claude)

**Mostly Stateless Architecture:** All runtime state is derived from:
- Git repository info via `git rev-parse --git-common-dir` (resolves to main worktree root, not worktree-local)
- Git worktrees via `git worktree list`, filtered to only include worktrees under `<repo_root>/worktrees/` (the project's `worktrees_dir()`). External worktrees (e.g. Claude Code subagent worktrees in `.git/worktrees/` or `.claude/worktrees/`) are excluded.
- Git branches via `git branch | grep {BRANCH_PREFIX}/` for archived worktrees (prefix defined in `src/models.rs::BRANCH_PREFIX`)
- Agent session files: Claude in `~/.claude/projects/` (path-encoded directories), Codex in `~/.codex/sessions/YYYY/MM/DD/` (date-based hierarchy, CWD matched from first line's `session_meta.payload.cwd`)

**Session Store (SQLite):**
Sessions are stored in `.azureal/sessions.azs` — a single SQLite database (DELETE journal mode) with S-numbered sessions (S1, S2, S3...). Backend-agnostic: a single session can span Claude and Codex prompts. The `.azs` extension discourages users from tampering with the binary file. Schema: `sessions` (id, name, worktree, created, completed BOOLEAN, duration_ms INTEGER, cost_usd REAL), `events` (session_id, seq, kind, data, char_len), `compactions` (session_id, after_seq, summary), `meta` (key, value). Store opened lazily on first use (`ensure_session_store()` creates the file; `try_open_session_store()` opens only if file exists). The `.azs` file is NOT created on project load — only when the user explicitly adds a session or opens the session list. `current_session_id` tracks the active session. `pid_session_target` maps PIDs to `(session_id, worktree_path)` tuples at spawn time so results write to the correct session even after switching worktrees. **Context injection replaces `--resume`:** The prompt flow always builds a `ContextPayload` from the store (`build_context()`), wraps it in `<azureal-session-context>` tags via `build_context_prompt()`, and spawns the agent with `resume_id = None`. The UI sees only the clean user prompt (`add_user_message(input)` is called before injection). Empty sessions (first prompt) pass the prompt unchanged. Staged prompts follow the same injection path. **Post-exit flow:** When a Claude process exits and `pid_session_target` has an entry for its PID, `store_append_from_jsonl()` parses the JSONL session file, strips any injected `<azureal-session-context>` prefix from UserMessage events, appends all DisplayEvents to the SQLite store, then **deletes the source JSONL file** (cleaning up `session_file_path`/`session_file_dirty` to prevent stale reads). Background project exits use `store_append_background()` which opens a temporary store connection to the target project's `.azs` file and also deletes the JSONL after successful ingestion. **Session loading:** `load_session_output()` checks whether the session is live (active Claude process running) — if so, it loads from the JSONL file for real-time display; otherwise it loads from the SQLite store. The session list overlay queries `SessionStore::list_sessions()` to populate the session picker; cross-session search uses `SessionStore::search_events()` instead of scanning JSONL files. **Compaction:** A live character counter (`chars_since_compaction`) tracks accumulated chars in real-time during event streaming. Initialized from `store.total_chars_since_compaction()` at session load/switch (synced in `update_token_badge()`), incremented in `apply_parsed_output()` (the live streaming path, via `AgentProcessor`) and in `handle_claude_output()` (legacy direct path) as each event arrives, and in `add_user_message()` for user prompts. Both `apply_parsed_output()` and `handle_claude_output()` call `update_token_badge_live()` to refresh the badge cache in real-time during streaming. When the counter crosses 400K (~100K tokens) mid-turn: (1) `store_append_from_display()` stores the partial turn's events to SQLite immediately, (2) `auto_continue_after_compaction` is set, (3) `cancel_current_claude()` kills the active process — letting it run longer would pile more uncompacted content onto an already-full context window. The normal exit flow fires, and the event loop spawns a background compaction agent via `spawn_compaction_agent()` using the currently selected model (`app.selected_model`). Once compaction finishes and no retry is pending, the event loop auto-sends a hidden "Continue." prompt with fresh context injection (including the new compaction summary) — no user bubble, no `add_user_message()`. The conversation resumes transparently. The flag is cleared on any manual user prompt. Double-compaction is prevented by guarding both threshold checks with `compaction_receivers.is_empty()` (skip if compaction already in-flight). A secondary threshold check in `store_append_from_jsonl()` acts as a safety net for sessions not actively viewed during streaming. **Load-time trigger:** `update_token_badge()` checks whether synced `store_chars >= COMPACTION_THRESHOLD` and sets `compaction_needed` if no compaction is already in-flight — this catches sessions that accumulated chars across prior runs but never triggered compaction (e.g. after app restart). **Spawn resilience:** `spawn_compaction_agent()` returns `bool` — if it fails (no boundary found, store missing, etc.), `compaction_needed` is NOT consumed. To prevent tick-spam when the boundary can't be found (not enough user messages), `compaction_spawn_deferred` suppresses retries until a new user message is stored (via `add_user_message()` or `store_append_from_jsonl()`), which may create a valid boundary. **Boundary-based:** `spawn_compaction_agent()` tries `compaction_boundary(session_id, from_seq, keep)` with progressively smaller `keep` values (3, 2, 1). `keep=3` is ideal (preserves last 3 user prompts verbatim), but sessions with ≤3 user messages since last compaction would never find a boundary, leaving the context badge stuck at 100%. Falling back to `keep=2` then `keep=1` ensures compaction can always run as long as there is at least one user message boundary to split on. `load_events_range()` fetches the pre-boundary events for the compaction transcript. The compaction agent receives a `build_compaction_prompt()` transcript and produces a 2000-4000 character summary. Compaction runs in isolated `compaction_receivers`/`compaction_output` maps (invisible to UI — no banners, no slot registration). **Fallback:** If the primary backend fails to spawn, `spawn_compaction_agent()` retries with the alternate backend (no specific model, uses CLI default) and shows a status message. If a compaction completes with empty output (e.g. backend hit usage limit), `poll_compaction_agents()` sets `compaction_retry_needed`; the event loop re-spawns on the next tick. On completion, `poll_compaction_agents()` stores the summary at the boundary seq via `store_compaction()` and subsequent `build_context()` calls include it as a prefix, with the last 3 exchanges loaded as raw events after it. Compaction job metadata (`CompactionJob` struct: rx, session_id, boundary_seq, wt_path) is stored per PID in `compaction_receivers` to enable retry with context. **Model selection:** Both `spawn_compaction_agent()` and commit message generation (`exec_commit_start()`) use `app.selected_model` directly — the same model the user has selected in the session pane. No hardcoded per-backend model mapping. `compaction_needed` is `Option<(i64, PathBuf)>` (session_id, worktree_path) — backend is derived from the selected model at spawn time.

**Persistent State (azufig.toml):**
All persistent state consolidated into two TOML files named `azufig.toml` — one global and one project-local:
- **Global** `~/.azureal/azufig.toml` — app config (API key, claude path, permission mode), registered projects (paths + display names), global run commands, global preset prompts
- **Project-local** `.azureal/azufig.toml` — filetree options (hidden entry names), health scan scope (directory paths), project-local run commands, project-local preset prompts, git settings (auto-rebase per branch, auto-resolve file list). Always at the **main worktree root** (resolved via `git rev-parse --git-common-dir` parent), shared by all worktrees — no per-worktree copies. **Tracked in git** — shared across machines for consistent settings and sessions. Session display names stored in `.azureal/sessions/index.json` alongside cache index.
All sections use single-bracket `[section]` headers with flat `key = "value"` pairs (e.g., `ProjectName = "~/path"`). `[runcmds]` and `[presetprompts]` keys are prefixed with a 1-based position number to preserve quick-select order: `N_Name = "value"` (e.g., `1_Build = "cargo build"`, `2_Test = "cargo test"`). Prefix stripped on load, re-written on save. Keys that qualify as TOML bare keys (`A-Za-z0-9_-` only) are written unquoted for clean output; keys with spaces or special chars (e.g., `"1_Cargo run (debug)"`) stay quoted. `#[serde(default)]` on every section for forward-compatibility. Write pattern: load-modify-save (read current, update one section, write back) to avoid clobbering unrelated sections.

# FEATURES

### Multi-Worktree Agent Management

The core feature enabling multiple concurrent agent CLI instances. Each worktree supports **multiple simultaneous agent processes** via PID-keyed session slots. Backend selection (`Backend::Claude` or `Backend::Codex`) determines which CLI is spawned.

**Architecture:**
- `AgentProcess` enum wraps `ClaudeProcess` or `CodexProcess` — dispatch via `AgentProcess::spawn()` / `AgentProcess::kill()`
- Claude: `claude -p "prompt" --verbose --output-format stream-json` (context injected via store, no `--resume`)
- Codex: `codex exec --json "prompt"` (new) or `codex exec --json resume <UUID> "prompt"` (resume)
- `spawn()` accepts optional `model: Option<&str>` — when set, adds `--model <name>` to the CLI args
- `spawn()` returns `(Receiver<AgentEvent>, u32)` — the event channel and the OS PID
- First prompt: captures `session_id` from init event (Claude: `subtype:init` + `session_id`, Codex: `thread.started` + `thread_id`)
- Follow-up prompts: add resume flag for conversation context
- Model override: `⌃m` cycles `selected_model` through a unified pool of all models (Claude + Codex in one cycle). Cycle order: opus → sonnet → haiku → gpt-5.4 → gpt-5.3-codex → gpt-5.2-codex → gpt-5.2 → gpt-5.1-codex-max → gpt-5.1-codex-mini → wrap. Colors: opus=magenta, sonnet=cyan, haiku=yellow, gpt-5.4=green, gpt-5.3-codex=lightgreen, gpt-5.2-codex=rgb(0,200,200), gpt-5.2=lightcyan, gpt-5.1-codex-max=blue, gpt-5.1-codex-mini=lightblue. Backend is derived dynamically from the selected model via `backend_for_model()` — gpt-* models use Codex backend, all others use Claude. `AgentProcess` holds both backends and dispatches at spawn time. Always set (never None) — the displayed name is exactly what gets passed as `--model` to `spawn()`. `detected_model` (separate field) is used only for context window heuristics, not display. The selected model is threaded through `AgentProcessor::spawn()`/`reset()` into `CodexEventParser`, which embeds it in `Init` events so the renderer can derive the correct agent label ("Claude"/"Codex") and color from the model string. Chat bubble headers show agent name left-aligned and model ID right-aligned (subdued style). **Backend availability gating:** At startup, `Config::is_backend_installed()` probes PATH for the `claude`/`codex` executables (via `which` on Unix, `where` on Windows). Results stored as `app.claude_available` / `app.codex_available` (both default `true`). `available_models()` filters `ALL_MODELS` to only include models whose backend is installed — if neither is found, falls back to the full pool (the app can't function without at least one). `cycle_model()` and `restore_model_from_session()` both use this filtered pool so users never land on a model they can't run. If Claude is unavailable, the default model shifts to the first Codex model (and vice versa). Detection runs before `load_session_output()` to ensure the initial model restore respects availability. Implementation: `src/config.rs` (`is_backend_installed()`), `src/app/state/app.rs` (`claude_available`/`codex_available` fields), `src/app/state/app/model.rs` (`available_models()`/`first_available_model()`), `src/tui/run.rs` (detection + initial model override). **Model persistence:** `restore_model_from_session()` is called at the end of every `load_session_output()` — on startup, worktree switch, project switch, and session list selection. It calls `last_session_model()` which scans `display_events` in reverse for `ModelSwitch` tags first (explicit user choice, highest priority), then falls back to `Init` events, mapping model strings back to `ALL_MODELS` aliases via `model_alias_from_init()`. Updates both `selected_model` and `backend`. Falls back to `first_available_model()` for empty/unrecognized sessions, or when the restored model's backend is not installed. **Auto-spawned processes follow model switcher:** RCR, GFM, and DH spawns pass `selected_model` directly to `AgentProcess::spawn()`, which auto-selects the correct backend via `backend_for_model()`. If the user has a Codex model selected, auto-spawned processes use Codex; if Claude, they use Claude.
- Permission mapping: Claude `--dangerously-skip-permissions` → Codex `--dangerously-bypass-approvals-and-sandbox`; Claude Approve → Codex `--full-auto`
- Process exits after each response; new process for next prompt

**PID-Keyed Session Slots:**
All session state maps (`agent_receivers`, `running_sessions`, `agent_exit_codes`, `agent_session_ids`) are keyed by **PID string** (not branch name). This enables multiple concurrent Claude processes per worktree. Two additional maps track the relationship:
- `branch_slots: HashMap<String, Vec<String>>` — branch → list of active PID strings (spawn order)
- `active_slot: HashMap<String, String>` — branch → which PID's output is displayed in the session pane

Only the **active slot's** output feeds `display_events`; other slots' output is silently drained from their receivers. When the active slot exits, the app auto-switches to the last remaining slot on that branch (or clears if none remain). New spawns always become the active slot. `cancel_current_claude()` kills only the active slot's process.

**Session Isolation on Switch:**
When the user switches sessions (worktree navigation or session list selection), `load_session_output()` enforces a hard boundary:
- Clears `rendered_lines_cache`, `output_viewport_cache`, animation/bubble/clickable caches immediately (not just marks dirty) so no stale frames flash from the previous session
- Advances `render_seq_applied` to discard any in-flight render results from the previous session's render thread work
- Sets `viewing_historic_session` flag when the selected session file doesn't match the active slot's Claude session UUID — this suppresses live event display (`handle_claude_output` skips events) and hides the PID/exit badge in the convo border, preventing a running process's output from bleeding into a different session's view
- `register_claude()` resets `viewing_historic_session = false` so new prompts always show live output
- Helper `viewed_session_id(branch)` resolves the UUID of the currently displayed session file
- `apply_parsed_output()` in the event loop checks `is_viewing_slot()` before applying background `AgentProcessor` results — stale results from a previous project's session are silently discarded, preventing event leakage across project/worktree switches
- Live session restore re-parses the JSONL file from disk (store events + JSONL turn) instead of relying on `live_display_events_cache`, which only snapshots at switch-away time and misses events generated while viewing another worktree
- `store_append_from_jsonl()` uses `parse_jsonl_for_store()` when the exiting slot is not being viewed, avoiding reading `display_events` (which belongs to a different worktree) and preventing data corruption or silent event loss
- **Explicit session selection honored over live session:** When the user selects a specific session from the session list while a Claude process is running, `load_session_output()` detects that `session_selected_file_idx` points to a different store ID than `active_store_id` (from `pid_session_target`). Sets `viewing_explicit_historic = true` and takes the historic/store load path, preventing the live session's JSONL output from overriding the user's selection
- **JSONL chars added to context badge on switch-back:** When loading a live session from JSONL, `event_char_len()` is summed for all parsed events *before* they are consumed into `display_events`. After `update_token_badge()` syncs from the store, the JSONL char total is added to `chars_since_compaction` and `update_token_badge_live()` refreshes the badge — preventing the badge from under-reporting when the current turn's events haven't been stored yet

**Critical: Context injection, not `--resume`**
The `--resume` flag is no longer used. Conversation context is built from the SQLite session store and injected into each prompt via `<azureal-session-context>` tags. This eliminates dependency on Claude's JSONL session files for conversation continuity — the `.azs` store is the single source of truth.

**Why not keep process alive?**
Claude Code's interactive mode uses a full TUI that cannot be driven by simple stdin writes. The `--input-format stream-json` flag only works with `-p` mode which still exits after each response. Verified by testing - there's no headless interactive mode available.

Current approach (`-p` with context injection) works reliably with ~100-200ms process spawn overhead per prompt.

Implementation: `src/claude.rs` (Claude) and `src/codex.rs` (Codex) spawn processes. `src/backend.rs` defines `Backend` enum + `AgentProcess` wrapper. `src/app/state/claude.rs` manages PID-keyed slots, `src/app/state/app.rs` tracks `branch_slots`/`active_slot` maps + `backend: Backend` field.

### Git Worktree Isolation

Each worktree provides true branch isolation:
- Has its own working directory
- Can have different uncommitted changes
- Operates on a separate branch from main
- **Archiving** removes the worktree directory but preserves the git branch (`⌘a` key). Archived worktrees show as `◇` (diamond) with dimmed text in the tab row.
- **Unarchiving** recreates the worktree from the preserved branch (`u` key). `Enter` on an archived session shows a status message directing the user to press `u` first.
- **Deleting** removes the worktree directory AND deletes the git branch permanently — both local and remote (`Wd` leader sequence). Opens a centered dialog box with red double-border. **Safety warnings:** before showing the dialog, runs `git status --porcelain` on the worktree path (skipped for archived) and `git log main..branch --oneline` from the repo root. If uncommitted changes or unmerged commits are found, yellow `! N uncommitted change(s)` / `! N commit(s) not merged to main` warning lines appear in the dialog between the question and the action keys. Does not block deletion — just warns. **Sibling guard:** if other worktrees share the same branch, the dialog offers to delete all siblings + branch (`y`) or archive the current worktree only (`a`) — git prevents branch deletion while worktrees are checked out. Sole worktrees show a simple y/Esc confirmation. State: `delete_worktree_dialog: Option<DeleteWorktreeDialog>` enum with `Sole { name, warnings }` and `Siblings { branch, sibling_indices, count, warnings }` variants. Also cleans up auto-rebase config and all session state maps (`session_files`, `session_selected_file_idx`, `claude_session_ids`, `branch_slots`, `active_slot`, `running_sessions`, `claude_receivers`, `claude_exit_codes`, `unread_sessions`) for the deleted branch. `delete_branch()` deletes local branch, pushes `--delete` to remote, and prunes the local remote-tracking ref (`origin/<branch>`) so stale refs don't appear in branch dialogs.
- **Renaming** changes the git branch name and migrates all keyed app state (`Wr` leader sequence). Opens a centered dialog box with cyan double-border, pre-filled with the current branch suffix (without the `azureal/` prefix). Text input supports cursor movement (Left/Right), Backspace, and character insertion at cursor. Enter confirms, Esc cancels. On confirm: spawns a background thread that runs `git branch -m old new`, pushes the new name to remote, deletes the old remote branch, and sets upstream tracking. All branch-keyed state maps are migrated on the main thread before the background op: `session_files`, `session_selected_file_idx`, `live_display_events_cache`, `branch_slots`, `active_slot`, `unread_sessions`, `auto_rebase_enabled`. The worktree entry's `branch_name` is updated in-place. Cannot rename main branch. State: `rename_worktree_dialog: Option<RenameWorktreeDialog>` struct with `old_name`, `input`, `cursor` fields. Background op sends `BackgroundOpOutcome::Renamed { new_branch }` — handler refreshes worktrees and re-selects the renamed branch.
- **Main branch browse:** Main/master branch is stored separately in `app.main_worktree: Option<Worktree>`. Press `Shift+M` globally to browse main: the `[★ main]` tab highlights in yellow as a visual distinction from feature worktrees. Main is fully functional — editing, prompting, and sessions all work the same as feature worktrees. The different git overview (pull/commit/push instead of squash/rebase) and yellow tab styling serve as indirect cues that you're on main. `Esc` or `Shift+M` again exits browse mode. `current_worktree()` transparently returns `main_worktree` when `browsing_main` is true. `enter_main_browse()` and `exit_main_browse()` in `src/app/state/ui.rs` manage state transitions; `switch_project()` clears browse state.
- **Tab row icons:** `[★ main]` tab always first (yellow when active). Archived worktrees show `◇` (diamond) with dimmed text. Feature branches use standard status circles (`●`/`○`/etc.). Priority order: **Running `●` (green) > Unread `◐` (AZURE) > normal status**. Running is checked first via `is_session_running()` so an active agent always shows the filled green circle, even if unread events also exist. Unread worktrees show `◐` (half-filled circle) in AZURE. Per-session granularity: if ANY session in the session list finishes while unviewed (different branch, or background slot on same branch), the tab shows `◐`. Clears per-session when that specific session is viewed; branch `◐` disappears only when all unread sessions on the branch are viewed. Only clears when session pane is visible (normal mode or git panel close). No leading space before icons — symbol sits flush against the left separator.
- **Cross-machine cleanup:** On startup and project switch, `Git::prune_remote_refs()` runs `git remote prune origin` then deletes local `azureal/*` branches that are fully merged to main and have no remote counterpart. Prevents worktrees deleted on one machine from appearing as archived on another.
- CLI: `azureal session archive <name>` / `azureal session unarchive <name>`

Implementation: `src/git.rs` module root declares submodules (`core`, `branch`, `commit`, `diff`, `merge`, `rebase`, `remote`, `staging`, `worktree`) and re-exports `Git`, `SquashMergeResult`, `WorktreeInfo`. `src/git/core.rs` holds types and basic ops (repo detection, branch listing, status). Sibling modules add `impl Git` methods via `use super::Git`. `src/git.rs` handles worktree creation, deletion, and status queries. `src/app/state/app.rs` stores `main_worktree`, `browsing_main`, and `pre_main_browse_selection` fields. `src/app/state/load.rs` populates `main_worktree` separately from the `worktrees` vec. `src/app/state/health.rs` uses `current_worktree_info()` (replaced `find_main_worktree()` + `switch_to_main_worktree()`) so health scans run on the current worktree. `src/tui/draw_file_tree.rs` renders the file tree pane. `src/tui/event_loop/actions.rs` handles `BrowseMain` action + `Esc` exit.

### TUI Interface

A ratatui-based terminal interface with 3-pane layout, toggle overlays, and status bar:

```
Normal Mode:                              Git Mode (Shift+G):
┌─ [★ main] │ [○ feat-a] │ [● feat-b] ┐   ╔════════════════════════════════════╗
├──────────┬───────────────┬───────────┤   ║ [main] [feat-a] [feat-b] (tab bar) ║
│FileTree  │    Viewer     │           │   ╠══════════╦═══════════════╦═════════╣
│  (15%)   │    (50%)      │Session(35%)│   ║ Actions  ║   Viewer      ║Commits  ║
├──────────┴───────────────┤           │   ║──────────║               ║         ║
│  Input / Terminal        │           │   ║ Files    ║               ║         ║
├──────────────────────────┴───────────┤   ╠══════════╩═══════════════╩═════════╣
│             Status Bar               │   ║ GIT: wt (Tab/⇧Tab:cycle | Enter)  ║
└──────────────────────────────────────┘   ╚════════════════════════════════════╝
                                            Status Bar (minimal)
```

**Panes:**
- **Worktree Tab Row** (1 row, top): Horizontal tab bar showing all worktrees (not focusable). `[★ main]` always first (Shift+M toggles main browse). Active tab: AZURE bg + white fg + bold; archived tabs: dim gray with `◇` prefix; inactive tabs: gray with status symbol prefix (`●`/`○`/etc.). Navigation: `[`/`]` globally switch tabs (wraps around at both ends). Worktree actions use `W` leader sequence (see Keybindings section). Pagination: greedy tab packing with `N/M` page indicator. Mouse: click tab to select. Tab row rect cached as `pane_worktree_tabs`, click regions as `worktree_tab_hits`.
- **FileTree** (15%): Always-visible directory tree for the selected worktree. Uses Nerd Font icons with automatic detection. Focus cycle includes it as a separate pane.
- **Viewer** (50%): File content viewer or diff detail (dual-purpose)
- **Session** (35%, full height): Claude conversation output with tool results — extends past input pane down to status bar. Press `s` to toggle a **Session list overlay** in this pane (replaces session output with a session file browser showing status symbol, worktree name, session name/UUID, last modified time, and `[N msgs]` count). Top border has three title positions: left shows "Session [x/y]" message position, **center shows session name in `[brackets]`** (custom names from `.azureal/sessions` preferred; raw UUIDs shown as `[xxxxxxxx-…]`; ellipsied to fit between left and right titles; cached on session switch via `title_session_name` — zero file I/O in render path), right shows token usage + PID/exit code (border characters fill gaps). Token usage shown as color-coded percentage badge (green <60%, yellow 60-80%, red >80%). PID shown in green while running; switches to exit code on exit (green for 0, red for non-zero). Uses ratatui's multi-title API with `Alignment::Center` and `Alignment::Right`.
- **Input/Terminal**: Prompt input or embedded terminal (spans FileTree + Viewer width only)
- **Status Bar** (1 row, bottom): Left shows worktree status dot + display name + branch (branch parenthetical hidden when identical to name, e.g. `main` not `main (main)`). Center shows status messages (clickable — copies to system clipboard). Right shows CPU% + PID badge in AZURE (`#3399FF`). Rect stored as `pane_status` for mouse hit-testing. No ViewMode indicator — help hints already change per mode.

**Splash Screen:** On startup, a 2x-scale block-character "AZUREAL" logo (10 rows × 110 chars, pure `█` blocks) in AZURE (#3399FF) is rendered centered on screen with the full acronym ("Asynchronous Zoned Unified Runtime Environment for Agentic LLMs") rendered in half-block characters (▀▄█ for 2x vertical density, 12 rows across 4 word-groups) in dim blue, followed by a "Loading project..." subtitle. Drawn immediately after terminal initialization (before `App::new()`) so the user sees branded feedback instead of a black screen while git discovery, session parsing, and file I/O run. Enforces a 3-second minimum display time (loading time counts toward it) so the branding registers even on fast machines. Replaced by the first `ui()` draw when the event loop starts.

**OS Terminal Title:** Set dynamically via crossterm `SetTitle`. Shows `AZUREAL` when no project loaded, `AZUREAL @ <project> : <branch>` when a session is selected. Updated on startup, session switch, and project switch (via `update_terminal_title()` in `src/app/state/ui.rs`, called from `load_session_output()`). Reset to empty on exit.

**Overlays:**
- **FileTree pane** (always visible in left column): Directory tree for the selected worktree. Uses **Nerd Font icons** (~60 file types with language-brand colors: Rust orange, Python blue, etc.) with automatic detection via `detect_nerd_font()` — probes a PUA glyph during splash and measures cursor advance via DSR. Falls back to emoji icons if the terminal font lacks Nerd Font glyphs (status bar shows "Nerd Font not detected" message). Icon mapping in `src/tui/file_icons.rs` — checks filename first (Dockerfile, Makefile, LICENSE, etc.), then extension. Border title shows `Filetree (worktree_name)` with optional `[pos/total]` scroll indicator when content overflows. Supports expand/collapse, file opening in Viewer. Focus set to `Focus::FileTree` while active. `f` or `Esc` returns to worktree list. **Options overlay** (`O`): replaces tree content with a checkbox list for toggling visibility — `worktrees`, `.git`, `.claude`, `.azureal`, `.DS_Store` (all hidden by default). QuadrantOutside AZURE border with `" Filetree Options "` title and `" Space:toggle  Esc:close "` footer. `j/k` navigate, `Space`/`Enter` toggle, `Esc`/`O` close. Hidden names stored in `file_tree_hidden_dirs: HashSet<String>` — tree rebuilds immediately on toggle. **Persisted to project azufig.toml** `[filetree].hidden` on every toggle and loaded on startup/project switch. File actions (`a`dd, `d`elete, `r`ename, `c`opy, `m`ove) show an inline action bar at the bottom of the pane. Add/Rename use text input (`⌃u` clears, `Esc` cancels, `Enter` confirms); Add with trailing `/` creates directory; Rename pre-fills with current name. Copy/Move use clipboard-style paste: press `c`/`m` to grab source file (highlighted with `┃name┃` solid border for copy or `╎name╎` dashed border for move, in magenta), navigate tree to target directory, `Enter` to paste, `Esc` to cancel. Delete uses y/N confirmation. Actions operate relative to selected entry's parent dir (or inside selected dir for Add/paste). Recursive dir copy via `copy_dir_recursive()`. State tracked as `file_tree_action: Option<FileTreeAction>` enum — `Add(String)`/`Rename(String)` hold text buffer, `Copy(PathBuf)`/`Move(PathBuf)` hold source path.
- **Session list overlay** (`s` in Session pane): Replaces conversation view with a session file browser scoped to the currently selected worktree. Each row shows a **status dot** (green `●` if a Claude process is actively running that session, dim gray `○` if idle — mirrors the worktree sidebar dots), session name (from `.azureal/sessions`) or full UUID, right-aligned last modified time, and `[N msgs]` badge. Border title shows `Sessions [N/M]` position counter (`[0/0]` when empty). Empty states: session list shows centered "No sessions" in DarkGray; content search shows "No results". The session pane itself shows a hint ("Press s to choose a session") when no session is loaded. Message counts computed via fast string scanning (no JSON parsing — `"type":"user"` and `"type":"assistant"` have zero false positives in Claude's compact JSON). Counts user prompt lines (no tool_result, not isMeta, not `<local-command-caveat>`/`<local-command-stdout>`/`<command-name>`/compaction summary) + assistant text blocks (type=text content). Counts cached by file size — only recomputed when a session file grows. Opening the list is two-phase: phase 1 shows the overlay immediately with a centered "Loading sessions…" dialog, phase 2 computes message counts after the dialog frame renders (so the UI never appears frozen). `j/k` navigate, `J/K` page, `Enter` loads session, `a` starts new session (closes list, enters prompt mode), `s` or `Esc` returns to session. `/` activates name filter (case-insensitive match against session name or UUID); `//` (slash while filter is empty) switches to content search mode (searches current worktree's JSONL files for text matches, min 3 chars, capped at 100 results, skips files >5MB). Filter bar shows at top with yellow border when active. Focus cycling (Tab) closes overlays; Shift+Tab from Viewer lands on FileTree if the overlay is open (preserving it), otherwise on Worktrees.

- **Welcome modal** (auto-shown when `needs_welcome_modal()` — project loaded, no worktrees, not browsing main): Centered double-border AZURE panel with centered border title "AZUREAL" and four options: `M` Browse main branch, `w` Create a worktree, `P` Open projects, `⌃q` Quit. All other input is blocked — `handle_key_event()` intercepts keys before any modal/focus dispatch and only forwards `BrowseMain`, `AddWorktree`, `OpenProjects`, or `Quit`. Auto-dismisses when the user takes any of those actions (state becomes `!needs_welcome_modal()`). Drawn at high z-order in `ui()` via `draw_dialogs::draw_welcome_modal()`. Keybindings resolved dynamically via `find_key_for_action()`. Implementation: `src/app/state/app/queries.rs` (`needs_welcome_modal()`), `src/tui/draw_dialogs.rs` (`draw_welcome_modal()`), `src/tui/event_loop/actions.rs` (input guard), `src/tui/run.rs` (render call).

**Loading Indicators (Deferred Actions):**
Any user action that triggers blocking I/O (session parse, file read, health scan, project switch, scope rescan) shows a centered AZURE-bordered popup with a descriptive message while the work runs. Uses a generic two-phase pattern: (1) set `loading_indicator: Option<String>` + `deferred_action: Option<DeferredAction>`, (2) event loop draws the popup via `draw_loading_indicator()`, (3) on the next frame after the draw, event loop takes `deferred_action`, clears the indicator, and calls `execute_deferred_action()` which dispatches to the actual handler. Five operations use this system:
- **Session load** (`"Loading session…"`) — Enter in session list or content search result
- **File open** (`"Loading <filename>…"`) — Enter on file in FileTree
- **Health panel** (`"Scanning project health…"`) — Shift+H to open Worktree Health
- **Project switch** (`"Switching project…"`) — Enter on project in Projects panel
- **Health scope rescan** (`"Rescanning health scope…"`) — Esc from scope mode (saves scope immediately, defers expensive rescan)

`DeferredAction` enum variants: `LoadSession { branch, idx }`, `LoadFile { path }`, `OpenHealthPanel`, `SwitchProject { path }`, `RescanHealthScope { dirs }`. The existing session list loading (`session_list_loading`) uses its own two-phase pattern predating this system.

**Background thread progress** (non-deferred): Squash merge uses `loading_indicator` directly without `DeferredAction`. A background thread sends `SquashMergeProgress` updates via `mpsc` channel; the event loop polls the receiver and updates `loading_indicator` with each phase string. On completion, the final `SquashMergeOutcome` is applied (PostMergeDialog, conflict overlay, or error). File tree refresh and worktree tab refresh also use background threads (no loading indicator — old data stays visible until replacement arrives). This pattern avoids blocking the event loop for multi-step git/FS operations.

Implementation: `src/app/state/app.rs` (DeferredAction enum + fields), `src/tui/run/overlays.rs` (`draw_loading_indicator()`), `src/tui/event_loop.rs` (deferred execution block + squash merge receiver poll), `src/tui/event_loop/actions/deferred.rs` (`execute_deferred_action()`)

**Color Identity:** All accent colors use the `AZURE` constant (`#3399FF`, defined in `src/tui/util.rs`) instead of ANSI Cyan, aligning the visual identity with the "Azureal" name. Import via `use super::util::AZURE;` (TUI modules) or `use crate::tui::util::AZURE;` (non-TUI modules).

**Viewer Dual Purpose:**
- When file selected in FileTree → shows syntax-highlighted file content with line numbers
- When `.md`/`.markdown` selected in FileTree → renders prettified markdown via `render_markdown_for_viewer()` (headers with `█▓▒░` prefixes, bullets, numbered lists, blockquotes with `┃`, syntax-highlighted code blocks, box-drawn tables, inline bold/italic/code). No line numbers (gutter=0). Reverts to plain syntax-highlighted text in edit mode or when `viewer_edit_diff` is active.
- When image selected in FileTree → renders image via terminal graphics protocol (Kitty/Sixel/halfblock fallback) using `ratatui-image` crate. Image auto-fits viewport; no scroll/selection/edit mode. `Picker::from_query_stdio()` lazy-inits once to detect terminal capabilities. `StatefulProtocol` adapts to render area each frame.
- When diff selected in Session → shows diff detail (future)

**Viewer Tabs:** Up to 12 tabs across 2 rows (6 per row, fixed-width). `t` saves current file to a tab, `⌥t` opens tab dialog, `[`/`]` navigate, `x` closes. Tab bar renders inside the border at rows 1-2, overlaying empty padding lines so content shifts down. `tab_bar_rows()` returns 0/1/2 based on count; `viewport_height` reduced by tab rows for correct scroll clamping. 12-tab max enforced in `viewer_tab_current()` with status message on overflow.

**Syntax Highlighting:**
- Uses **tree-sitter** (AST-based parser) with `tree-sitter-highlight` for token classification and hardcoded capture-to-color mapping in `src/syntax.rs`
- 25 language grammars registered at init: Rust, Python, JavaScript, TypeScript, TSX, JSON, TOML, Bash, C, C++, Go, HTML, CSS, Java, Ruby, Lua, YAML, Markdown, Scala, R, Haskell, PHP, SQL, Perl (plain text fallback for Perl — no highlight queries in crate)
- Language detection: file extension lookup (`ext_to_lang` HashMap) for Viewer pane, code fence token lookup (`token_to_lang` HashMap) for Session code blocks, with extension fallback
- `SyntaxHighlighter` methods take `&mut self` (tree-sitter `Highlighter` reuses internal buffers)
- Two instances: one on `App` struct (main thread), one created in `RenderThread::spawn()` (background render thread)
- `highlight_impl()` uses disjoint field borrows: `&self.configs` (immutable) + `&mut self.highlighter` (mutable) — supports language injection callbacks
- Capture-to-color mapping via `HIGHLIGHT_NAMES` array (26 entries) → `capture_color(index)` function

Other features:
- Vim-style modal editing
- Diff viewer with syntax highlighting
- Help overlay with keybindings
- Mouse interaction: scroll panels, click to focus panes, click tab row/file tree to select, click input to position cursor, double-click to open files/expand dirs, drag to select text in Viewer/Session panes
- Preset prompts (⌥P): save up to 10 prompt templates; quick-select with 1-9,0 from picker OR directly from prompt mode with ⌥1-⌥9,⌥0 (skips picker); picker footer shows shortcut hint; add/edit/delete from picker (d=delete with y/n confirmation); available only in prompt mode; hint shown in prompt title bar. Dual-scope persistence: presets can be global (`~/.azureal/azufig.toml` `[presetprompts]`, shared across all projects) or project-local (`.azureal/azufig.toml` `[presetprompts]`); toggle scope with ⌃g in add/edit dialog; picker shows G/P badge per preset

Implementation: `src/tui/event_loop.rs` + `src/tui/event_loop/` (12 submodules: actions, agent_events, agent_processor, auto_rebase, coords, fast_draw, git_polling, housekeeping, input_thread, mouse, process_input, prompt) for event loop, `src/tui/run.rs` + `src/tui/run/` (3 submodules: splash, worktree_tabs, overlays) for rendering, `src/tui/render_thread.rs` for background session rendering, `src/app/state/` for state management (split into 10 focused submodules, `health` has 2 sub-submodules). `actions` itself is split into 6 sub-submodules: execute, navigation, escape, session_list, deferred, rcr.

**Mouse Click Architecture:**
- All pane `Rect`s cached on App struct during `ui()` draw: `pane_worktree_tabs`, `pane_worktrees`, `pane_viewer`, `pane_session`, `pane_todo`, `input_area`
- Pane hit-testing via `Rect::contains(Position::new(col, row))` — shared by both click and scroll handlers
- Worktree tab row uses `worktree_tab_hits: Vec<(u16, u16, Option<usize>)>` built during `draw_worktree_tabs()` — maps screen x-ranges to tab targets (None = main browse, Some(idx) = worktree index)
- FileTree uses the `pane_worktrees` rect area for click/scroll handling; entry index = `visual_row + file_tree_scroll`, with double-click detection via `last_click` field (same position within 500ms)
- Input click enters prompt mode and positions cursor via `click_to_input_cursor()` — uses `word_wrap_break_points()` to map screen coords to char index with word-boundary wrapping
- Overlays (help, branch_dialog, run_command_picker/dialog) are dismissed on any click outside

**Text Selection (Mouse Drag):**
- `MouseDown(Left)` converts screen coords to cache coords immediately, stores as `mouse_drag_start: Option<(usize, usize, u8)>` — `(cache_line_or_char, cache_col, pane_id)`. pane_id: 0=viewer, 1=session, 2=input, 3=edit-mode-viewer. Clears existing `viewer_selection` / `session_selection`.
- **Edit mode click:** When `viewer_edit_mode` is true and click lands in viewer pane, `screen_to_edit_pos()` maps screen coords → `(source_line, source_col)` by walking source lines and summing wrap counts. Sets `viewer_edit_cursor` and clears `viewer_edit_selection`. Drag anchor stored as pane_id=3.
- **Edit mode drag (pane_id=3):** Maps current drag position via `screen_to_edit_pos()`, sets `viewer_edit_selection = Some((anchor_line, anchor_col, drag_line, drag_col))` and moves cursor to drag end. Auto-scrolls when dragging above/below pane.
- `MouseDrag(Left)` calls `handle_mouse_drag()` which uses the cached anchor (pane_id from `mouse_drag_start`) and maps only the current cursor position from screen to cache coords via `screen_to_cache_pos()`. For input pane, uses `screen_to_input_char()` to map to char index.
- Anchor stored in cache coords so auto-scroll during drag doesn't shift the selection start
- Auto-scroll when dragging above/below pane content area
- Selection stored as `Option<(start_line, start_col, end_line, end_col)>` in cache-line indices (normalized so start <= end)
- Viewer selection rendered in `draw_viewer/selection.rs` via `apply_selection_to_line()` (already existed)
- Session selection rendered in `draw_output.rs` by calling `apply_selection_to_line()` after viewport build — `session_selection_cached` used as viewport cache invalidation key. **Content bounds clamping:** `compute_line_content_bounds()` analyzes each cache line's spans to find the selectable region, excluding bubble chrome (ORANGE `│ ` gutter, AZURE ` │` border, headers with colored bg, bottom borders, code fences). Selection highlighting clamps per-line to `(eff_sc, eff_ec)` within content bounds. Non-selectable lines (bounds `(0,0)`) are skipped entirely.
- `apply_selection_to_line()` is `pub(crate)` in `draw_viewer/selection.rs` (re-exported from `draw_viewer.rs`) — splits spans at selection boundaries, patches with `Rgb(60,60,100)` bg. Takes `gutter` param to skip line number column from highlighting (File mode computes from first span width; Diff/Session pass 0). O(spans_in_line) per viewport line, negligible cost.
- `⌘C` copies from whichever pane has active selection (viewer, session, or input). Session copy uses `extract_session_text()` (respects `compute_line_content_bounds()`, skips decoration lines, trims leading/trailing blanks). Viewer copy uses `extract_text_from_cache()` → `arboard::Clipboard`, stripping line number gutter (first span per line) so only file content is copied. In git mode, `⌘C` and `⌘A` are intercepted early in `handle_git_actions_input` (before `lookup_git_actions_action()`) since the git panel consumes all input. Git mode copy uses gutter=0 (diffs have no line numbers); falls back to copying `result_message` from the status box when no viewer selection exists.
- **Git status box selection:** Clicking the git status box sets `git_status_selected: bool` on App, highlighting the result message with `Rgb(60,60,100)` bg. `⌘A` selects the status box when viewer cache is empty. `⌘C` copies when `git_status_selected` is true. Cleared on panel close (`close_git_actions_panel`), non-nav keystrokes, and clicks on other panes.
- Selections cleared on: click, scroll, Tab, focus change
- **Fast-path exclusion:** `fast_draw_input()` and draw deferral are both skipped when `has_input_selection()` is true — fast-path writes raw text without selection styling

---

### Platform Support

Azureal compiles and runs on **macOS**, **Linux**, and **Windows**.

**Build requirements:** LLVM/Clang + CMake (for whisper-rs-sys). macOS: Xcode CLT. Linux: `libclang-dev cmake`. Windows: `winget install LLVM.LLVM Kitware.CMake Ninja-build.Ninja` + set `LIBCLANG_PATH`. Windows also requires NVIDIA CUDA Toolkit (`winget install Nvidia.CUDA`) for GPU-accelerated Whisper inference. The Ninja build system is required on Windows because CMake's default Visual Studio generator uses MSBuild, which invokes `nvcc --use-local-env` — this prevents CUDA from inheriting the Windows SDK include paths (`corecrt.h`). With `CMAKE_GENERATOR=Ninja`, CMake calls nvcc directly and the `INCLUDE`/`LIB` env vars propagate correctly. Set `INCLUDE`, `LIB` (MSVC + Windows SDK paths), and `CMAKE_GENERATOR=Ninja` in your environment, or build from a VS Developer Command Prompt with Ninja in PATH.

**Vendored dependencies** (`vendor/`):

- `whisper-rs` — Vendored from Codeberg (`vendor/whisper-rs/`). Three MSVC fixes in `sys/build.rs`: (1) `.layout_tests(false)` suppresses compile-time struct size assertions, (2) MSVC targets auto-skip bindgen entirely (bindgen generates opaque structs with only `_address` field on MSVC) and use the pre-built `src/bindings.rs` with layout assertion blocks stripped, (3) C enum type aliases (`ggml_*`/`whisper_*`) converted from `c_uint` to `c_int` during copy (MSVC uses signed int for C enums). All handled by `copy_bindings_without_layout_tests()`. Patched via `[patch.crates-io]` in `Cargo.toml`.

**Platform-conditional dependencies** (`Cargo.toml`):

- `whisper-rs` — Metal GPU acceleration on macOS, CUDA GPU acceleration on Windows, CPU-only on Linux. macOS variant adds `features = ["metal"]`, Windows variant adds `features = ["cuda"]`
- `crossterm` — `use-dev-tty` feature enabled on Unix only (reads `/dev/tty`); Windows uses Console API natively
- `libc` — Unix only, for `getrusage()` CPU time sampling
- `windows-sys` — Windows only, for `GetProcessTimes()` CPU time sampling

**Platform-conditional keybindings** (`src/tui/keybindings/bindings.rs`):

macOS `⌘` (Super) bindings get platform equivalents via `#[cfg(target_os = "macos")]` const key combos. Windows/Linux terminals cannot capture the Win/Super key. On Windows/Linux, destructive/modal actions use `Alt+` instead of `Ctrl+Shift+` because Windows Terminal intercepts `Ctrl+Shift+` combos and without the Kitty keyboard protocol the Shift modifier is dropped for alphabetic chars.

| Action | macOS | Windows/Linux |
|--------|-------|---------------|
| Copy selection | `⌘c` | `Ctrl+C` |
| Cancel agent | `⌃c` | `Alt+C` |
| Archive worktree | `⌘a` | `Alt+A` |
| Delete worktree | `⌘d` | `Alt+D` |
| Select all | `⌘a` | `Ctrl+A` |
| Save file | `⌘s` | `Ctrl+S` |
| Undo | `⌘z` | `Ctrl+Z` |
| Redo | `⌘⇧Z` | `Ctrl+Y` |
| STT (edit mode) | `⌃s` | `Alt+S` |

Display: `KeyCombo::display()` shows `⌃⌥⇧⌘` symbols on macOS, `Ctrl+Alt+Shift+` text labels on Windows/Linux.

**Runtime platform guards:**

- Shell detection (`src/app/terminal.rs`): On Windows, prefers `pwsh.exe` (PS7) → `powershell.exe` → `COMSPEC`/`cmd.exe` (verifies exit status, not just spawn success); on Unix uses `SHELL`/`/bin/bash`. PowerShell spawned with `-NoLogo`. `TERM=xterm-256color` set for all shells. Initial form feed (`0x0c`) skipped on Windows (Windows shells don't reprint prompt after clear). **Critical PTY init order:** `try_clone_reader()` and `take_writer()` must be called BEFORE `spawn_command()` — on Windows ConPTY, obtaining handles after spawn+slave-drop produces inconsistent pipe state. After spawn, `drop(pair.slave)` releases the slave so master reads unblock. The child process handle is stored in `App::terminal_child` / `SessionTerminal::child` to keep the process alive.
- Process killing (`src/app/state/ui.rs`, `claude.rs`): `kill` on Unix, `taskkill /PID /F` on Windows. Claude subprocess spawned with `.stdin(Stdio::null())` to prevent console stdin handle sharing on Windows (causes input event competition between TUI and child).
- macOS `.app` bundle (`src/main.rs`): `#[cfg(target_os = "macos")]` — Activity Monitor icon support
- Windows `.ico`/`.png` extraction + WT profile fragment (`src/main.rs`): `#[cfg(target_os = "windows")]` — extracts embedded `Azureal.ico` to `~/.azureal/` for Explorer/Alt+Tab, and `Azureal_toast.png` for toast notifications and the WT tab icon (PNG renders crisply; `.ico` is blurry). Writes a Windows Terminal profile fragment (`%LOCALAPPDATA%\Microsoft\Windows Terminal\Fragments\Azureal\azureal.json`) on every startup (not just when missing) referencing the PNG, so icon/exe path changes propagate automatically. `GetConsoleWindow()` returns null in WT (ConPTY has no window), so `WM_SETICON` cannot work.
- Windows exe icon embedding (`build.rs`): `#[cfg(target_os = "windows")]` — `winres` embeds `.ico` as Win32 resource for Explorer/Alt+Tab file icon
- Notification platform guards (`src/app/state/claude/process_lifecycle.rs`): `.sound_name("Glass")` gated to `#[cfg(target_os = "macos")]`; `.app_id("AZUREAL")` + `.icon()` gated to `#[cfg(target_os = "windows")]`
- Kitty keyboard protocol (`src/tui/run.rs` entry point): `PushKeyboardEnhancementFlags` (DISAMBIGUATE_ESCAPE_CODES + REPORT_EVENT_TYPES) gated to `#[cfg(not(target_os = "windows"))]` — conflicts with mouse capture on Windows Terminal.
- fast_draw (`src/tui/event_loop/fast_draw.rs`): `fast_draw_input()` gated to `#[cfg(target_os = "macos")]` — direct VT writes bypass ratatui's buffer. `fast_draw_session()` was removed (caused rendering artifacts: disappearing borders, duplicated events, stale content).
- Path canonicalization: All `std::fs::canonicalize()` calls replaced with `dunce::canonicalize()` to strip `\\?\` extended-length path prefix on Windows.
- Cross-platform session linking (`src/config.rs`): `find_foreign_project_dir()` + `link_project_dir()` create NTFS junctions (Windows, no elevation) or symlinks (Unix) to share session directories across platforms.
- Terminal title reassertion (`src/tui/event_loop.rs`): `#[cfg(target_os = "windows")]` — Claude CLI inherits the console and overwrites the title via `SetConsoleTitle()`. After every draw frame, `update_terminal_title()` is called unconditionally (not just while agents are running) so the title stays correct after agent exit too.
- Embedded terminal Enter key (`src/tui/input_terminal.rs`): Sends `\r` (carriage return) instead of `\n` (linefeed). PowerShell treats bare `\n` as line continuation.

**Already cross-platform** (no guards needed): `portable-pty` (ConPTY on Windows), `notify` (ReadDirectoryChangesW), `arboard`, `dirs`, `notify-rust`, `ratatui`/`crossterm`, `dunce`, all path handling via `PathBuf`.

---

## ⚠️ CRITICAL: CPU PERFORMANCE RULES ⚠️

**DO NOT REGRESS THESE OPTIMIZATIONS. CPU usage must stay <5% during scrolling.**

### 1. NEVER Create Expensive Objects in Render Path

```rust
// ❌ WRONG - Creates SyntaxHighlighter on EVERY FRAME (loads all 25 tree-sitter grammars)
fn render_edit_diff(...) {
    let highlighter = SyntaxHighlighter::new();  // CATASTROPHIC - 100ms+ per call
}

// ✅ CORRECT - Pass reference from App state
fn render_edit_diff(..., highlighter: &SyntaxHighlighter) {
    highlighter.highlight_file(...)  // Reuses pre-loaded syntax definitions
}
```

**Files:** `src/tui/render_events.rs` passes `&app.syntax_highlighter` to `render_edit_diff()`

### 2. CACHE Rendered Output

```rust
// ❌ WRONG - Re-renders ALL events on EVERY frame (O(n) per frame)
let all_lines = render_display_events(&app.display_events, ...);

// ✅ CORRECT - Cache rendered lines, only re-render when data changes
if app.rendered_lines_dirty || app.rendered_lines_width != width {
    app.rendered_lines_cache = render_display_events(...);
    app.rendered_lines_dirty = false;
}
let lines = app.rendered_lines_cache.iter().skip(scroll).take(height).cloned().collect();
```

**Files:** `src/tui/draw_output.rs` uses `app.rendered_lines_cache`; render cache is updated asynchronously by the background `RenderThread` — call `app.invalidate_render_cache()` when `display_events` changes to trigger a new render request

**Diff caching:** Same pattern for diff view - `app.diff_lines_cache` stores colorized diff output. Set `app.diff_lines_dirty = true` when `diff_text` changes. `src/tui/draw_output.rs` checks dirty flag before re-highlighting.

### 3. DECOUPLE Animation from Content Cache

**Critical: Animation must NOT invalidate the content cache.** The pulsating indicator only changes color - the content (markdown, tool calls) is unchanged.

```rust
// ❌ WRONG - Animation tick invalidates entire cache, re-renders 15k events every 250ms
let animation_changed = !app.pending_tool_calls.is_empty() && app.rendered_lines_tick != app.animation_tick;
if app.rendered_lines_dirty || animation_changed {
    app.rendered_lines_cache = render_display_events(...);  // EXPENSIVE: parses ALL events
}

// ✅ CORRECT - Cache content independently, patch status + animation in viewport only
if app.rendered_lines_dirty || app.rendered_lines_width != inner_width {
    let (lines, anim_indices) = render_display_events(...);  // Only when content changes
    app.rendered_lines_cache = lines;
    app.animation_line_indices = anim_indices;  // Track ALL tool indicator positions
}

// Patch tool status indicators in viewport slice (O(viewport) not O(all))
// animation_line_indices is Vec<(line_idx, span_idx, tool_use_id)> — tracks ALL tools,
// not just pending. Draw-time patching updates both text and color based on current
// pending_tool_calls / failed_tool_calls, so circles update immediately when a tool
// completes without waiting for a full re-render.
let pulse_color = pulse_colors[(app.animation_tick / 2) as usize % 4];
for (line_idx, span_idx, tool_use_id) in &app.animation_line_indices {
    if *line_idx >= scroll && *line_idx < scroll + viewport_height {
        if let Some(span) = lines[*line_idx - scroll].spans.get_mut(*span_idx) {
            if app.pending_tool_calls.contains(tool_use_id) {
                span.content = "○ ".into();
                span.style = span.style.fg(pulse_color);
            } else if app.failed_tool_calls.contains(tool_use_id) {
                span.content = "✗ ".into();
                span.style = span.style.fg(Color::Red);
            } else {
                span.content = "● ".into();
                span.style = span.style.fg(Color::Green);
            }
        }
    }
}
```

**Files:** `src/tui/draw_output.rs` patches status + colors in viewport; `src/tui/render_events.rs` returns `animation_line_indices`

**Draw-time status patching:** `animation_line_indices` tracks ALL tool indicator positions (not just pending). At draw time, each indicator is patched based on current `pending_tool_calls`/`failed_tool_calls` state. This means tool circles transition from ○→●/✗ immediately when a ToolResult arrives, without waiting for a re-render. Cache invalidation uses `tool_status_generation` (incremented in `handle_claude_output` on every pending/failed change) so the viewport rebuilds when status changes.

**Animation guard:** The animation patching loop is skipped entirely when `animation_line_indices` is empty (no tool calls rendered). Pulse animation only runs when at least one tool is still pending (checked via `pending_tool_calls`).

**Throttle values in `src/tui/event_loop.rs`:**
- `min_draw_interval = 33ms` (~30fps) for user interaction, **adaptive 200ms (~5fps) during idle streaming** — when Claude is streaming but the user isn't interacting, draw frequency drops 6x to reduce PTY escape-sequence volume. Reverts to 30fps immediately on any key event.
- `min_animation_interval = 250ms` (4fps pulsating indicators - viewport color patch only)
- `min_poll_interval = 500ms` (session file polling)
- `poll_ms = 16ms` when busy (render in-flight / Claude streaming), `100ms` when idle
- **Render submit throttle: 50ms** — `last_render_submit` in App state. Without this, every `poll_render_result()` completion immediately triggers another `submit_render_request()` (since `rendered_lines_dirty` is re-set by arriving events), cloning the full events array at ~60Hz. The 50ms floor batches streaming events into ~20 render cycles/sec.
- **Extended typing deferral (300ms window):** `typing_recently` (true when `last_key_time.elapsed() < 300ms`) suppresses `terminal.draw()` calls during active typing when fast-path is available. `fast_draw_input()` (~0.1ms) provides instant keystroke feedback; session pane updates wait for the next full ratatui draw cycle. Event loop profiler logs to `~/.azureal/event_loop_profile.log` for diagnostics.

### 14. True Single JSON Parse Per Claude Event

```rust
// ❌ WRONG - EventParser parses JSON, then handle_claude_output parses AGAIN
let events = self.event_parser.parse(&data);               // parse #1
let json = serde_json::from_str::<Value>(&data).ok();      // parse #2 (duplicate!)

// ✅ CORRECT - EventParser returns the parsed Value alongside events
let (events, parsed_json) = self.event_parser.parse(&data); // single parse
// parsed_json is reused for token extraction + display text
```

`EventParser::parse()` returns `(Vec<DisplayEvent>, Option<serde_json::Value>)` — the same JSON value used internally is also passed to the caller. `handle_claude_output` reuses it for token/model/context-window extraction with zero additional parsing.

**session_lines skip:** Once `rendered_lines_cache` has content, `display_text_from_json()` + `process_session_chunk()` are skipped entirely. They only feed the fallback raw output view (used before first render completes).

**Empty event batch skip:** Many stdout lines (progress, hook_started) produce 0 DisplayEvents. `display_events.extend()` + `invalidate_render_cache()` are skipped for these.

**Full render clone reduction:** The full render path clones only `display_events[deferred_start..]` instead of the entire Vec — avoids cloning early events that are never rendered.

**Reader thread optimization:** The stdout reader thread (`src/claude.rs`) only needs to extract `session_id` from the init event (happens once per session). Instead of full JSON parsing every line, it checks `line.contains("\"subtype\":\"init\"")` first — only parses JSON when the string matches.

**EventParser buffer optimization:** The parser collects all complete lines in one `drain()` call instead of re-allocating `self.buffer` on every newline (O(n) total instead of O(n²) per chunk).

**Dev profile optimization:** `Cargo.toml` sets `opt-level = 2` for `serde_json`, `serde`, `tree-sitter`, and `tree-sitter-highlight` packages in dev builds. These hot-path dependencies run 3-5x slower at opt-level 0, amplifying all parsing and highlighting costs in debug mode.

**Files:** `src/events/parser.rs` (parse returns JSON), `src/app/state/claude.rs::handle_claude_output()`, `src/app/util.rs` (`display_text_from_json`)

### 9. NEVER Use `.wrap()` on Pre-Wrapped Content

```rust
// ❌ WRONG - ratatui re-wraps every viewport line char-by-char during render()
let para = Paragraph::new(pre_wrapped_lines).wrap(Wrap { trim: false });

// ✅ CORRECT - content is already wrapped by wrap_text()/wrap_spans(), no re-wrapping needed
let para = Paragraph::new(pre_wrapped_lines);
```

Session pane content is pre-wrapped to `inner_width` by `wrap_text()` and `wrap_spans()` in `render_events.rs`. Adding `.wrap()` causes ratatui's `Paragraph::render()` to iterate every character of every span to compute line breaks that already exist — pure redundant O(viewport_chars) work per frame.

**Files:** `src/tui/draw_output.rs` renders Paragraph without `.wrap()`. If you add a new Paragraph that displays pre-wrapped content, do NOT add `.wrap()`.

### 13. Edit Mode: Cache Highlighting + Viewport-Only Rendering

```rust
// ❌ WRONG - Re-highlights entire file and builds all visual lines EVERY FRAME
let full_content = app.viewer_edit_content.join("\n");
let highlighted = app.syntax_highlighter.highlight_file(&full_content, &path_str);
// Then iterates ALL source lines to build all_lines...

// ✅ CORRECT - Cache highlighting, only re-run on content change; only process visible lines
let edit_ver = app.viewer_edit_version;  // monotonic counter, bumped on every mutation
if app.viewer_edit_highlight_ver != edit_ver {
    app.viewer_edit_highlight_cache = app.syntax_highlighter.highlight_file(...);
    app.viewer_edit_highlight_ver = edit_ver;
}
// Walk source lines to find visible range, only build Lines for viewport
```

**Impact:** AGENTS.md (~1000+ lines) caused 90%+ CPU in edit mode — tree-sitter was parsing the entire file every frame at 30fps. Now: highlight once on enter/edit (~50ms), then zero highlight cost per frame. Viewport-only line construction means O(viewport_height) not O(file_size) per frame.

**Cache invalidation:** `viewer_edit_highlight_ver` tracks `viewer_edit_version` — a monotonically increasing counter bumped in `push_undo()` and undo/redo. Cannot use `viewer_edit_undo.len()` because the undo stack caps at 100 entries; after 100 edits, push+trim keeps length at 100 so the cache key never changes. Scrolling, cursor movement, and selection don't bump version → cache hit → zero cost. Cleared on `exit_viewer_edit_mode()`.

**Cursor position:** Computed arithmetically by summing wrap counts for source lines before cursor. No `all_lines` array needed.

**Files:** `src/tui/draw_viewer/edit_mode.rs::draw_edit_mode()`, `src/app/state/app.rs` (cache fields), `src/app/state/viewer_edit.rs` (cache cleanup)

### 8. File Watching + Session File Polling (Notify + Deferred Parse + Incremental)

**Change detection** uses kernel-level filesystem notifications via the `notify` crate (kqueue on macOS, inotify on Linux, ReadDirectoryChangesW on Windows). A background `FileWatcher` thread (`src/watcher.rs`) owns a `notify::RecommendedWatcher` and forwards classified events to the main thread via mpsc channels — zero CPU between events, near-instant detection.

**Watch targets** (re-registered on session switch via `sync_file_watches()`):
- **Session JSONL file** — `NonRecursive` watch, sets `session_file_dirty = true` on change
- **Worktree directory** — `Recursive` watch, triggers debounced file tree refresh (500ms)

**Noise filtering** happens in the watcher thread: `/target/`, `/.git/`, `/node_modules/`, `.DS_Store`, `.swp`/`.swo`/`~` files are dropped before reaching the main thread. Events are coalesced (at most one `SessionFileChanged` + one `WorktreeChanged` per 200ms drain cycle).

**Graceful fallback:** If `notify` fails to initialize (`FileWatcher::spawn()` returns `None`) or the watcher thread errors at runtime (`WatcherFailed` event), the event loop falls back to the original stat()-based polling (500ms interval) seamlessly.

Three-phase parse+render pipeline for session files:

1. **Change detection** (notify-driven or fallback stat()): Sets `session_file_dirty = true`.
2. **Incremental parse** (`refresh_session_events()`): Seek to `session_file_parse_offset`, parse only new JSONL lines appended since last read. Rebuilds tool_call context from existing DisplayEvents via `IncrementalParserState`. Falls back to full re-parse if file shrank or user-message rewrite detected (parentUuid dedup).
3. **Incremental render** (`draw_output()`): If `rendered_events_count < display_events.len()` and width unchanged, renders only newly appended events and appends to `rendered_lines_cache`. Falls back to full re-render on width change or event count decrease (session switch).

```rust
// Watcher thread classifies filesystem events:
pub enum WatchEvent {
    SessionFileChanged,  // session JSONL modified
    WorktreeChanged,     // file created/deleted/modified in worktree
    WatcherFailed(String),
}

// Main thread drains events (non-blocking):
while let Some(evt) = watcher.try_recv() {
    match evt {
        SessionFileChanged => app.session_file_dirty = true,
        WorktreeChanged => {
            app.file_tree_refresh_pending = true;
            app.worktree_tabs_refresh_pending = true;
        },
        WatcherFailed(_) => { app.file_watcher = None; break; },
    }
}

// Incremental parse (only new bytes since last offset)
fn refresh_session_events(&mut self) {
    let parsed = parse_session_file_incremental(
        &path, self.session_file_parse_offset,
        &self.display_events, &self.pending_tool_calls, &self.failed_tool_calls,
    );
}
```

**Streaming vs Polling (Dual-Source Prevention):**
During active Claude streaming, events are added to `display_events` by the live process handler (`handle_claude_output()` in `claude.rs`). Session file polling is **skipped** during streaming (`poll_session_file()` returns early if `is_current_session_running()`). **Important:** stream-json stdout does NOT include `user` type events — only system/assistant/result/progress. User messages are pushed as real `DisplayEvent::UserMessage` events into `display_events` at prompt submit time (`add_user_message()` in `output.rs`), ensuring they render immediately and persist throughout the conversation. The `pending_user_message` field is kept only as a dedup marker — cleared by `load.rs` when the session file's authoritative `UserMessage` appears during re-parse. When Claude exits, `handle_claude_exited()` triggers a post-exit re-parse: **Store-backed sessions** (`current_session_id.is_some()`) do NOT reset `session_file_parse_offset` to 0 — only an incremental parse runs (finalizes pending tool calls without clobbering multi-turn `display_events`, since the JSONL only contains the current turn). **Legacy sessions** (no store, using `--resume`) reset to 0 for a full re-parse to reconcile live-streamed events with the authoritative JSONL. **RCR intercept:** if the exiting slot is the active RCR process, `handle_claude_exited()` sets `approval_pending = true` and returns early — skipping re-parse entirely to preserve the streaming output the user is viewing. **Guard:** the re-parse is also skipped if the exiting slot's session file doesn't exist in the current worktree's session directory (checked via `claude_session_file(worktree_path, sid)`). This prevents sessions spawned from a different working directory (e.g. merge conflict resolution spawned from main's repo root) from clobbering `display_events` with an unrelated old session file.

**Session Store (SQLite-backed Portable Sessions):**
All session data lives in `.azureal/sessions.azs` — a single SQLite database (DELETE journal mode) with S-numbered sessions (S1, S2, S3...). On prompt send, `build_context()` reconstructs prior conversation from stored events and injects it as `<azureal-session-context>` tags (replacing `--resume`). On Claude exit, the JSONL file is parsed, context tags stripped, and events appended to the store via `append_events()` which applies `compact_event()` before serialization — ToolResult content is truncated to match render display rules (Read: first+last line, Bash: last 2, Grep/default: first 3, Glob: count, Task: first 5), ToolCall input is stripped to the key field only (Edit preserved fully for diff rendering, Write summarized to line count + purpose). **Completion persistence:** When `append_events()` encounters a `Complete` event, it calls `mark_completed(session_id, duration_ms, cost_usd)` to persist completion metadata (success/failure, duration, cost) to the `sessions` table. This data is display-only — never injected into prompts. The session list UI shows completion badges (green checkmark for success, red X for failure) via `session_completion` HashMap on App state, populated from `SessionInfo` fields. Context-level compaction runs automatically at 400K chars (~100K tokens) via a background agent (using the currently selected model) that summarizes events since the last compaction — subsequent context injections use the compact summary as a prefix and only include events after the compaction boundary.

**Files:**
- `src/watcher.rs` - `FileWatcher` thread, `WatchEvent`/`WatchCommand` types, noise filtering
- `src/app/session_parser.rs` - `parse_session_file_incremental()`, `IncrementalParserState`
- `src/app/session_store.rs` - `SessionStore`, `SessionInfo`, `CompactionInfo`, `ContextPayload`, `open()`, `create_session()`, `rename_session()`, `delete_session()`, `list_sessions()`, `append_events()`, `load_events()`, `load_events_range()`, `compaction_boundary()`, `build_context()`, `store_compaction()`, `mark_completed()`
- `src/app/context_injection.rs` - `build_context_prompt()`, `strip_injected_context()`, `build_transcript()`, `build_compaction_prompt()`
- `src/app/state/load.rs` - `check_session_file()`, `poll_session_file()`, `refresh_session_events()`, `sync_file_watches()`
- `src/app/state/claude.rs` - `handle_claude_output()` (live events), `handle_claude_exited()` (full re-parse + store append)
- `src/app/state/session_names.rs` - `save_session_name()`, `load_all_session_names()` (store-only, numeric IDs)
- `src/tui/render_events.rs` - `render_display_events_incremental()`, `render_display_events_with_state()`
- `src/tui/draw_output.rs` - incremental render path selection, `pre_scan_events()`
- `src/tui/event_loop.rs` - watcher event drain, fallback polling, debounced file tree refresh

**App state for incremental tracking:**
- `file_watcher: Option<FileWatcher>` — background watcher thread handle (None = fallback to polling)
- `file_tree_refresh_pending: bool` — set by WorktreeChanged, cleared when background scan is spawned
- `file_tree_receiver: Option<Receiver<Vec<FileTreeEntry>>>` — background file tree scan result (spawned from event loop, polled with try_recv)
- `worktree_tabs_refresh_pending: bool` — set by WorktreeChanged, cleared when background refresh is spawned
- `worktree_refresh_receiver: Option<Receiver<Result<WorktreeRefreshResult>>>` — background worktree refresh result (git + FS I/O done off main thread)
- `worktree_last_notify: Instant` — timestamp of last worktree change (for 500ms debounce)
- `rendered_content_line_count: usize` — total line count of rendered cache (equals `rendered_lines_cache.len()`)
- `session_file_parse_offset: u64` — byte offset after last successful parse
- `rendered_events_count: usize` — how many events were rendered into current cache
- `rendered_events_start: usize` — start index for deferred render (>0 means early events skipped)

**Fallback triggers (reverts to full re-parse/re-render):**
- File shrank (shouldn't happen with append-only JSONL)
- User-message rewrite detected (parentUuid dedup → events reference earlier indices)
- Terminal width changed (need to re-wrap all text)
- Session switched (event count drops to 0)

**Safety guards in `refresh_session_events()`:**
- **Empty-parse guard:** If the re-parse returns empty events but we already had content and `end_offset == 0`, the existing `display_events` are preserved (file was likely temporarily unavailable). The next poll will retry.
- **Render counter reset on full re-parse:** When `session_file_parse_offset` was 0 (full re-parse, e.g. after Claude exit), `rendered_events_count`, `rendered_content_line_count`, and `rendered_events_start` are reset to 0. Without this, the incremental render path would use stale counts that reference the old event array, producing garbled output.

**Session file index stability (`load_worktrees()`):**
When `refresh_worktrees()` rebuilds `session_files` via `list_claude_sessions()` (sorted by mtime, newest first), a new session file can shift existing sessions to higher indices. `session_selected_file_idx` is preserved by UUID: before replacing the file list, the UUID at the current index is looked up in the new list, and the index is corrected. This prevents the viewed session from silently switching to a different session file after a worktree refresh.

### 10. Deferred Initial Render for Large Conversations

For conversations with 200+ events, only the last 200 events are rendered on initial load. The user starts at the bottom (`session_scroll = usize::MAX`) so they see recent messages instantly. Full render happens lazily when the user reaches scroll position 0 — both `scroll_session_up()` and `jump_to_prev_bubble()` set `rendered_lines_dirty = true` when they hit scroll 0 with `rendered_events_start > 0`, triggering deferred render expansion on the next event loop frame.

```rust
// Deferred rendering only triggers on INITIAL load (fresh session, never rendered).
// The guard `!expanding_deferred && rendered_events_start == 0 && rendered_events_count == 0`
// ensures that once expansion fires (user scrolled to top), the next render sees
// rendered_events_start > 0 (old value) and forces deferred_start = 0 (full render).
// Without this guard, the zeros written by expansion would re-trigger deferral,
// creating an infinite loop that prevented the user from ever seeing early events.
let deferred_start = if !expanding_deferred
    && app.rendered_events_start == 0
    && app.rendered_events_count == 0
    && event_count > DEFERRED_RENDER_TAIL
{
    event_count.saturating_sub(DEFERRED_RENDER_TAIL)
} else {
    0
};
render_display_events(&events[deferred_start..], ...);
app.rendered_events_start = deferred_start;

// When user scrolls to top and there are unrendered early events:
if app.rendered_events_start > 0 && app.session_scroll == 0 {
    // Expand to full render — sets expanding_deferred flag to prevent re-deferral
    app.rendered_events_start = 0;
    app.rendered_events_count = 0;
    app.rendered_lines_dirty = true;
}
```

**Message count denominator:** The session title `[x/y]` denominator counts `UserMessage` + `AssistantText` from the **full** `display_events` array — not from `message_bubble_positions` which only covers rendered events. This ensures the denominator shows the true total even when deferred rendering has skipped early events. The numerator uses `unrendered_offset = total - rendered_bubbles` so position numbering is correct before full render triggers.

**Files:** `src/tui/draw_output.rs` (DEFERRED_RENDER_TAIL const, deferred render logic, title denominator count)

### 11. NEVER Do File I/O in the DRAW Path (Render Thread Is Fine)

File I/O in `terminal.draw()` or any function called during frame rendering blocks the event loop. However, `render_edit_diff()` runs on the **background render thread** — file I/O there is safe because it doesn't block input or drawing.

`render_edit_diff()` reads the file once per Edit event to find the actual line number of the edit. It tries `new_string` first (post-edit state), then falls back to `old_string` (pre-edit state, for live preview mid-streaming when the edit hasn't been applied yet). Both give the correct position since they occupy the same location in the file at their respective points in time. Falls back to line 1 if the file can't be read or both strings are empty (pure deletion).

**Edit diff styling:** Removed lines (red) use dark grey text (`Rgb(100,100,100)`) on dim red bg — no syntax highlighting, deliberately darker than comment grey in syntax-highlighted green lines. Only added lines (green) get syntax highlighting. This keeps removed lines visually receded and reduces highlight calls to 1 per Edit event.

**Edit diff scroll correction:** `load_file_with_edit_diff()` in `ui.rs` sets a preliminary `viewer_scroll` from `find_edit_line()` (content line number), but the viewer cache has extra visual lines from word-wrapping and inserted old/deleted diff lines. A `viewer_scroll_to_diff` one-shot flag triggers correction in `draw_viewer.rs` during cache rebuild, using the actual visual line index where the diff highlight renders (`diff_visual_start`). Falls back to `viewer_line_numbers` mapping when the renderer can't find the highlight.

**Files:** `src/tui/render_tools/diff_render.rs` (`render_edit_diff()`)

### 4. SKIP Redraw When Nothing Changed

```rust
// ❌ WRONG - Always returns true, always redraws
pub fn scroll_session_up(&mut self, lines: usize) {
    self.session_scroll = self.session_scroll.saturating_sub(lines);
}

// ✅ CORRECT - Return whether position actually changed
pub fn scroll_session_up(&mut self, lines: usize) -> bool {
    let old = self.session_scroll;
    self.session_scroll = self.session_scroll.saturating_sub(lines);
    self.session_scroll != old  // false if already at top
}
```

**Files:** `src/app/state/scroll.rs` - all scroll functions return `bool`; `src/tui/event_loop.rs` uses return value

### 5. Event Loop Optimizations

- **Dedicated input reader thread:** A background thread (`input_thread::spawn_input_thread()`) continuously reads crossterm events from stdin and sends them to the main loop via `mpsc` channel. The main loop drains from the channel instead of calling `event::poll`/`event::read` directly. This ensures keystrokes are captured immediately even during `terminal.draw()` (~18ms) or other blocking operations — without the thread, keys that arrive during a draw sit in the kernel tty buffer and some terminal emulators drop them under heavy output load. The thread filters Release events early and exits when the receiver is dropped. Idle polling uses 50ms timeout to avoid burning CPU.
- **Event batching:** Drain ALL pending events from input channel before redrawing (one redraw per batch)
- **Motion discard:** Mouse motion events discarded instantly (zero processing)
- **Conditional polling:** Terminal rx only polled when `app.terminal_mode == true`
- **Cached terminal size:** Only updated on resize events, not every frame
- **Fast-path input rendering:** When keys arrive in Claude prompt mode (NOT terminal mode) with **single-line input** (no `\n`) and **no active selection**, `fast_draw_input()` writes the input box content directly to the terminal via crossterm (~0.1ms), completely bypassing `terminal.draw()` (~18ms). **Runs immediately after the event drain loop** — before Claude event processing, file watcher drain, file tree rebuild, worktree refresh, render submit, or any other housekeeping. This ordering is critical: blocking operations (file tree rebuild ~10-100ms, worktree refresh ~10-50ms) fire every 500ms when Claude modifies files; if fast_draw ran after them, keystrokes would visually lag by 20-150ms. The full `terminal.draw()` is deferred to the next quiet frame. `app.input_area` (cached from last full draw in `ui()`) provides the screen coordinates. **Must exclude terminal mode** — terminal uses `prompt_mode=true` for "type mode", but `fast_draw_input()` writes `app.input` (empty in terminal) over the input_area, wiping PTY display. **Must exclude multi-line input** — the input box resizes dynamically when newlines are added/removed, but `input_area` reflects the old height, causing cursor mispositioning. **Must exclude active selection** — `fast_draw_input()` writes raw text without selection highlighting; `has_input_selection()` check added to both fast-path and draw deferral conditions. **Reconciliation on eligibility loss:** `was_fast_path` is snapshotted before key event processing. If fast_draw was active but isn't after processing (e.g. prompt submit clears `prompt_mode`), one final `fast_draw_input()` runs to overwrite stale content on the physical terminal with padded spaces — without this, ratatui's diff sees no change between its stale buffer and the new empty-input buffer, leaving fast_draw's content stuck on screen.
- **Extended typing deferral (300ms window):** When typing single-line in Claude prompt mode with no selection, `terminal.draw()` is SKIPPED for the entire 300ms `typing_recently` window — not just the single key frame. `fast_draw_input()` (~0.1ms) provides visual feedback during the deferral; session pane updates wait for the next full draw. Events are always applied and redraws always triggered (no `suppress_redraw`); render results are always polled immediately (no `defer_render_poll`). Only the expensive full `terminal.draw()` is deferred. A `draw_pending` flag on App tracks deferred draws. **Terminal type mode, multi-line input, and active selection are NOT deferred** — they need immediate `terminal.draw()` calls (PTY has no fast-path; multi-line needs layout resize; selection needs full render for highlight styling).
- **Force full redraw on layout switch:** When the layout changes (e.g. git panel open/close), ratatui's diff may miss cells. `force_full_redraw: bool` on App is set by `open_git_actions_panel()` and `close_git_actions_panel()`. The event loop checks this flag before `terminal.draw()` and calls `terminal.clear()` first to reset ratatui's buffer, forcing a complete redraw.
- **Background worktree/git operations:** All blocking worktree operations (archive, unarchive, create, delete) and git panel operations (pull, push, rebase) run on background threads. Pattern: validate inputs + gather data synchronously → set `loading_indicator` → spawn thread with mpsc sender → store receiver on App (`background_op_receiver` for worktree/pull/push, `rebase_op_receiver` for rebase). Event loop polls both receivers alongside squash merge. `BackgroundOpProgress` carries phase string + optional `BackgroundOpOutcome` (Archived/Unarchived/Created/Deleted/GitResult/Failed). `BackgroundRebaseOutcome` is separate (Rebased/UpToDate/Conflict/Failed) because rebase needs conflict overlay handling. State cleanup (session maps, auto-rebase config) happens before spawn; post-op work (refresh_worktrees, select branch, set status) happens in the event loop on completion. `git_action_in_progress()` includes both receivers to block quit.
- **Pre-draw event drain with abort and fast-path:** Right before `terminal.draw()`, drain any key events that arrived since the top-of-loop drain (~0-5ms gap). If a key is found, the draw is ABORTED (loop continues without drawing). Keys caught here also get `fast_draw_input()` for immediate visual feedback — without this, they'd only appear on the next full draw (~33ms later).
- **Adaptive draw throttle:** 5fps (200ms interval) during idle streaming (Claude active, no user interaction), 30fps (33ms interval) during user interaction or idle. Reduces PTY escape-sequence volume 6x during streaming. Profiler confirmed: Terminal.app delays keyboard forwarding while processing escape sequences (~6ms per draw), so fewer draws = lower input distortion. Part of the three-part input blocking fix (see extended typing deferral above).
- **Adaptive poll timeout:** 16ms when busy (draw pending, render in-flight, Claude streaming, or background receivers pending), 100ms when idle. Ensures fast draw after typing stops without burning CPU when nothing is happening.
- **Background file tree + worktree refresh:** Instead of synchronous `load_file_tree()` (10-100ms FS walk) and `refresh_worktrees()` (10-50ms git + FS I/O) blocking the event loop, both are spawned on background threads. Inputs (paths, expanded dirs, hidden dirs for file tree; project path + main branch + worktrees dir for worktrees) are cloned and sent to the thread. Results arrive via `mpsc::try_recv()`. Old data stays visible until new results arrive — no flash of empty state. Synchronous callers (user-triggered expand/collapse, startup, worktree creation) still call the direct methods for instant feedback; only the event loop's debounced refresh uses background threads. Stale receivers are discarded when manual operations occur.
- **Background Claude event parsing (ClaudeProcessor):** Claude streaming JSON events (`serde_json::from_str`, 1-5ms each × up to 10/tick = 10-50ms) are parsed on a dedicated `claude-parser` background thread. The main event loop forwards raw `ClaudeEvent::Output` data to the processor via mpsc channel (after `is_viewing_slot()` guard) and polls pre-parsed results. Only cheap state updates (tool call tracking, todo parsing, token extraction, `display_events.extend()`) happen on the main thread. Non-Output events (Started, SessionId, Exited) are still handled directly — they're instant. Parser resets on session switch via `claude_processor_needs_reset` flag; stale results are drained to prevent old-session events from bleeding through.
- **Immediate render poll:** Render results are always polled immediately — no deferral during typing. Events are always applied, redraws always triggered. Only the expensive full `terminal.draw()` is deferred during the typing window.
- **Claude event cap per tick:** Raw Claude events are capped at 10 per tick (`MAX_CLAUDE_EVENTS_PER_TICK`). With background parsing, forwarding is just a channel send (~0.1ms each), so the cap is mostly a safety guard now. Parsed results from the background `ClaudeProcessor` are also capped at 10 per tick to prevent unbounded drain when the parser batches many results.
- **Auto-rebase deferred during streaming:** The 2-second auto-rebase check runs `git status --porcelain` on every eligible worktree (5-50ms per check). Skipped entirely when `claude_receivers` is non-empty — rebasing during active file modifications would fail with dirty working tree anyway. Resumes after all Claude sessions finish.
- **Health panel refresh deferred during streaming:** The debounced health panel rescan (`scan_god_files()` + `scan_documentation()`, 10-200ms synchronous filesystem walk) is skipped when `claude_receivers` is non-empty. The panel shows slightly stale data during streaming and refreshes once all Claude sessions finish.
- **Viewport cache:** Session pane caches the cloned viewport slice (`session_viewport_cache`). Only rebuilds when scroll position, content, or animation tick changes. On typing-only frames, serves from cache instead of re-cloning from the full `rendered_lines_cache`.
- **Background render thread:** Expensive session rendering (markdown parsing, syntax highlighting, text wrapping via `render_display_events`) runs on a dedicated background thread (`RenderThread`). The event loop sends render requests via `submit_render_request()` (non-blocking channel send) and polls for results via `poll_render_result()` (non-blocking channel recv). Input is NEVER blocked by rendering — the main thread only does cheap draw operations. Sequence numbers ensure stale results are discarded (latest-wins). The render thread drains to the latest request when multiple are queued, and uses zero CPU when idle (blocks on `mpsc::recv`). `draw_output()` has a width-mismatch fallback that re-renders if the terminal width changed since the request was submitted (rare, only on resize). `poll_render_result()` re-sets `session_scroll = usize::MAX` (follow-bottom sentinel) when the user was at/near the bottom of the OLD cache — this ensures newly appended content (e.g. pending user bubble, streaming events) is visible without requiring manual scroll-down. **Incremental renders are zero-clone:** For incremental renders (new events only), `submit_render_request()` sends only the new events + pre-scan state — NO clone of the existing `rendered_lines_cache`. The render thread produces only new lines with indices relative to 0. `poll_render_result()` extends the existing cache and offsets the new indices by the existing line count. Full renders (width change, scroll-to-top expansion) still replace the entire cache. This eliminated 1-5ms of heap allocation per incremental submit during streaming.

### 6. Pre-Format Expensive Data at Load Time

```rust
// ❌ WRONG - chrono::DateTime::from() on EVERY FRAME
fn draw_sidebar(...) {
    for file in files {
        let time_str = format_time(file.mtime);  // EXPENSIVE chrono call per-frame
    }
}

// ✅ CORRECT - Format once when loading, store String
pub fn list_claude_sessions(...) -> Vec<(String, PathBuf, String)> {
    sessions.into_iter()
        .map(|(id, path, mtime)| (id, path, format_time(mtime)))  // Format ONCE at load
        .collect()
}
```

**Rule:** Any data transformation (time formatting, string manipulation, parsing) must happen at load/update time, never in render functions.

**Files:** `src/config.rs::list_claude_sessions()` pre-formats time strings; `src/tui/draw_sidebar.rs` just displays them

### 7. Worktree Tab Row (Replaces Sidebar)

The worktree sidebar was replaced by a horizontal tab row at the top of the normal mode layout. The tab row rebuilds every frame (no caching needed — it's a single row of spans). `invalidate_sidebar()` is retained as a no-op for compatibility with existing callers.

**Files:**
- `src/tui/run/worktree_tabs.rs` — `draw_worktree_tabs()` renders the tab row with pagination
- `src/tui/draw_sidebar.rs` — only contains Git panel sidebar (Actions + Changed Files); normal sidebar code removed

### Performance Checklist for PRs

Before merging ANY change to render/event code:
- [ ] No `::new()` calls for expensive structs in render path
- [ ] No O(n) operations per frame (use caching for expensive computations)
- [ ] Animations throttled (not every frame)
- [ ] Scroll returns bool, caller checks before redraw
- [ ] Sidebar and file tree items cached (invalidated only on state change)
- [ ] Test: scroll aggressively, CPU must stay <5%

---

## CROSSTERM GOTCHAS

### BackTab arrives with SHIFT modifier on some terminals

Crossterm delivers Shift+Tab as `(KeyModifiers::SHIFT, KeyCode::BackTab)` on many terminals, but `KeyCode::BackTab` already *implies* Shift. If you define a binding as `KeyCombo::plain(KeyCode::BackTab)` (modifiers = NONE), it won't match when the terminal sends the SHIFT modifier alongside it.

```rust
// ❌ WRONG — only matches BackTab with zero modifiers
KeyCombo::plain(KeyCode::BackTab)  // won't fire on terminals that send SHIFT+BackTab

// ✅ CORRECT — KeyCombo::matches() strips SHIFT from BackTab before comparing
// (Fixed in types.rs — no binding changes needed, the match function handles it)
```

**Affected:** Any `BackTab` keybinding. Fixed in `KeyCombo::matches()` (`types.rs`).

---

### Ctrl+M and Shift+Enter are indistinguishable from Enter without Kitty protocol

Without the Kitty keyboard enhancement protocol, `Ctrl+M` and `Shift+Enter` both produce byte `0x0D` — identical to plain `Enter`. Crossterm decodes all three as `(KeyModifiers::NONE, KeyCode::Enter)`. The `PushKeyboardEnhancementFlags` call "succeeds" (it just writes bytes to stdout) even when the terminal ignores the escape sequence, so `kbd_enhanced` is unreliable as a feature detection signal. Terminals that DON'T support Kitty protocol include GNOME Terminal, xterm, and most SSH sessions.

```rust
// ❌ WRONG — only works with Kitty keyboard protocol
Keybinding::new(
    KeyCombo::ctrl(KeyCode::Char('m')),  // Ctrl+M = Enter without Kitty
    "Cycle model",
    Action::CycleModel,
)

// ✅ CORRECT — Alt+M always sends a distinct ESC+'m' sequence
Keybinding::with_alt(
    KeyCombo::ctrl(KeyCode::Char('m')),
    &ALT_CYCLE_MODEL,  // Alt+M fallback
    "Cycle model",
    Action::CycleModel,
)
```

**Rule:** Any binding using `Ctrl+<letter that maps to an ASCII control code>` or `Shift+Enter` MUST have an `Alt+<key>` alternative. Alt always sends a distinct `ESC` prefix regardless of terminal protocol support.

**Affected:** `CycleModel` (Ctrl+M → Alt+M fallback), `InsertNewline` (Shift+Enter → Alt+Enter fallback). Fixed in `bindings.rs`.

---

## WINDOWS GOTCHAS

### `notify-rust` with custom `app_id` silently drops toasts

`notify-rust`'s `.app_id("AZUREAL")` requires the AppUserModelID (AUMID) to be registered in the Windows registry via a Start Menu shortcut. Without that registration, the Action Center silently discards the toast — no error, no warning, nothing delivered.

```rust
// ❌ WRONG — AUMID "AZUREAL" is not registered, toast is silently dropped
notify_rust::Notification::new()
    .app_id("AZUREAL")
    .summary("AZUREAL")
    .body("Response complete")
    .show()?;

// ✅ CORRECT — shell out to PowerShell; its AUMID is pre-registered by Windows
// Use CREATE_NO_WINDOW (0x08000000) to suppress console flash
let ps_aumid = "{1AC14E77-02E7-4E5D-B744-2EB1AE5198B7}\\WindowsPowerShell\\v1.0\\powershell.exe";
// Pass WinRT toast XML to PowerShell via -Command
// Toast XML uses <image placement="appLogoOverride"> with Azureal_toast.png for crisp icon
// (.ico renders blurry in toasts — always use PNG for toast icons)
```

**Affected:** Any `notify-rust` call on Windows using a custom `.app_id()`. Fixed in `src/app/state/claude/process_lifecycle.rs` (`send_completion_notification()`).

---

## RATATUI GOTCHAS

### Paragraph doesn't clear unused cells

Ratatui's `Paragraph` only writes content characters to the buffer — cells beyond each line's width and rows beyond the last content line retain whatever was in the buffer from the **previous frame** (ratatui reuses buffers, it doesn't start fresh). This means switching from placeholder text to shorter content leaves ghost characters visible.

```rust
// ❌ WRONG — placeholder text bleeds through when diff lines are shorter
f.render_widget(Paragraph::new(display_lines).block(block), area);

// ✅ CORRECT — Clear first, then render
f.render_widget(Clear, area);
f.render_widget(Paragraph::new(display_lines).block(block), area);
```

**Affected:** Any pane that transitions between different content (placeholder → real content, short → long text). Fixed in `draw_git_viewer_selectable()`.

---

### Invalidate cached panes when exiting overlay modes

When a modal/overlay mode modifies a pane's visual state (e.g., scope mode adds green highlights to the file tree), exiting that mode must call `invalidate_file_tree()` (or the relevant invalidation) so the pane redraws without the overlay styling. Without it, the cached render persists until the user interacts with the pane.

```rust
// ❌ WRONG — file tree still shows scope highlights until cursor moves
app.god_file_filter_mode = false;
app.god_file_filter_dirs.clear();
app.focus = Focus::FileTree;

// ✅ CORRECT — invalidate forces redraw on next frame
app.god_file_filter_mode = false;
app.god_file_filter_dirs.clear();
app.invalidate_file_tree();
app.focus = Focus::FileTree;
```

**Affected:** Any cached pane whose rendering depends on modal state flags. Fixed in scope mode exit (`escape.rs`).

---

### DECSTBM scroll regions are FULL-WIDTH

DECSTBM (`\x1b[top;bottom r`) sets scroll margins for the **entire terminal width** — there is no column constraint. `ScrollUp(n)` within a DECSTBM region scrolls ALL columns on the affected rows, not just a rectangular sub-region. In a multi-column layout (file tree | viewer | session), using DECSTBM to scroll the session pane blanks the file tree and viewer columns on the same rows.

```
// ❌ WRONG — scrolls file tree + viewer columns too, leaving blank gaps
write!(stdout, "\x1b[{};{}r", session_top, session_bottom);  // DECSTBM
queue!(stdout, ScrollUp(n));  // scrolls ALL columns
write!(stdout, "\x1b[r");  // reset

// ✅ CORRECT — rewrite visible lines via cursor positioning (session column only)
for (i, line) in visible_lines.iter().enumerate() {
    queue!(stdout, cursor::MoveTo(session_left, row));
    // ... render spans with style, pad to session width ...
}
```

**Historical note:** `fast_draw_session()` (now removed) originally used DECSTBM, then switched to direct cell writes — both approaches caused rendering artifacts. DECSLRM (left/right margins) exists but is not widely supported (Terminal.app doesn't support it).

---

### Background Render Thread (Session Pane)

The session pane's expensive rendering pipeline (markdown parsing, syntax highlighting, text wrapping) runs on a dedicated background thread. This ensures the main event loop is never blocked by rendering, eliminating input freezing and character dropping during session updates.

**Architecture:**
- `RenderThread` owns its own `SyntaxHighlighter` (no cross-thread sharing needed)
- **Incremental renders clone only NEW events** — `pre_scan_events()` scans already-rendered events on the main thread (zero-cost reads, no allocation), then only `display_events[rendered_events_count..]` is cloned for the render thread. `PreScanState` carries the pre-computed flags. Full renders (width change, initial load) still clone all events but happen rarely.
- Communication via `mpsc` channels: `submit_tx` (main → render), `result_rx` (render → main)
- Sequence numbers (`u64`) ensure stale results are discarded — only the latest result is applied
- Render thread drains to the latest request when multiple are queued (skips intermediate states)
- Zero CPU when idle (blocks on `mpsc::recv`)

**Event loop integration:**
```rust
// Non-blocking send — never blocks the event loop
submit_render_request(&app);  // Sends cloned state to render thread

// Non-blocking poll — checks if a completed render is available
if let Some(result) = poll_render_result(&mut app) {
    if result.seq > app.render_seq_applied {
        app.rendered_lines_cache = result.lines;
        app.render_seq_applied = result.seq;
    }
}
```

**App state fields:**
- `render_thread: RenderThread` — handle to the background thread
- `render_seq_applied: u64` — sequence number of the last applied render result
- `render_in_flight: bool` — whether a render request is currently being processed
- `last_render_submit: Instant` — throttle: min 50ms between submits to batch streaming events

**Evolution:** Previous iterations moved rendering outside `terminal.draw()` (commit 8834050) and split render/draw into separate loop iterations (commit 5192228), but those were still synchronous. The background thread makes rendering fully asynchronous.

**Files:** `src/tui/render_thread.rs` (RenderThread struct, request/result types), `src/tui/draw_output.rs` (`submit_render_request()`, `poll_render_result()`), `src/tui/event_loop.rs` (submit/poll integration), `src/app/state/app.rs` (render_thread fields)

### Background File Tree + Worktree Refresh

The event loop's debounced file tree refresh and worktree tab refresh run on background threads to avoid blocking the main loop during Claude streaming. Pattern matches the render thread: clone inputs → spawn → poll result → apply.

**File tree** (`build_file_tree`): Clones `worktree_path`, `file_tree_expanded`, `file_tree_hidden_dirs`. Thread returns `Vec<FileTreeEntry>`. Applied: entries replaced, selection reset to 0, file tree invalidated.

**Worktree refresh** (`compute_worktree_refresh` in `src/app/state/load.rs`): Clones `project.path`, `main_branch`, `worktrees_dir`. Thread does all git subprocess calls + Claude session discovery. Returns `WorktreeRefreshResult` (main_worktree, worktrees, claude_session_ids, session_files). Applied via `apply_worktree_result()` which handles UUID-based selection preservation and branch-name selection stability.

**Stale result guard:** Synchronous callers (`load_file_tree()`, `toggle_file_tree_dir()`, `load_worktrees()`) set receiver to `None`, discarding any in-flight background result. Prevents race where background result overwrites a newer manual operation.

**Files:** `src/app/state/load.rs` (`compute_worktree_refresh()`, `apply_worktree_result()`), `src/app/state/helpers.rs` (`build_file_tree()`), `src/app/types.rs` (`WorktreeRefreshResult`), `src/tui/event_loop.rs` (spawn + poll)

**Startup sequence** (`src/tui/run.rs::run`): `draw_splash()` → `App::new()` → `app.load()` → `app.load_session_output()` → `event_loop::run_app()`. The splash screen renders immediately after terminal init (before any App state exists) so the user sees the AZUREAL logo with a dim spring azure butterfly outline (the app mascot) in the background while git discovery and session parsing run. Two-layer rendering: butterfly background using outlined wings (box-drawing chars + `░` fill, `║` body column, `╱╲` antennae) at `Color::Rgb(15, 45, 80)`, then logo + half-block acronym + "Loading project..." on top overwriting butterfly cells where they overlap. Both layers share the same vertical center point so butterfly wings extend above and below the text. Displayed for minimum 3 seconds. The first `ui()` draw in `run_app()` replaces the splash with the full layout.

### Vim-Style Input Mode

The input box uses vim-style modal editing:
- **Command mode** (red border): Keys are commands, not text input
- **Prompt mode** (yellow border): Keys are typed as Claude prompts

**Rationale:** Allows single-letter commands like 't' for terminal toggle without conflicting with text input. The red border in command mode provides immediate visual feedback that typing will execute commands, preventing accidental command execution.

Key mappings:
- `p` (global, except edit mode): Enter prompt mode and focus input (closes terminal/help if open)
- `T` (Shift+T, global, except edit mode): Toggle terminal pane
- `G` (Shift+G, global, except edit mode): Toggle Git panel

**CRITICAL: All keybinding guards are centralized in `lookup_action()`.** The skip logic in `lookup_action()` prevents single-key globals (`p`, `T`, `G`, `R`, `?`, `Tab`, `Shift+Tab`, `⌥r`) from firing during text input, edit mode, sidebar filter, or wizard. Terminal mode only blocks globals when focus is on `Focus::Input` (the terminal pane itself) — other panes can still trigger globals like `p` (enter prompt) even while the terminal is open. `⌘C` (copy) is skipped in edit mode so the edit handler owns clipboard. Tab/Shift+Tab skipped in edit mode, help overlay, and wizard. **NEVER add guard conditions in event_loop.rs or input handlers** — add them to the skip match in `lookup_action()` instead. **Every Shift+letter global binding MUST be in the skip list** or it will steal uppercase letter input in prompt mode.
- `Escape` / click another pane / `Tab` (in prompt mode): Return to command mode
- `Enter` (in prompt mode): Submit prompt and return to command mode. If Claude is already running, a single Enter cancels the current run and auto-sends the new prompt once the process exits (via `staged_prompt` mechanism — no second Enter needed)

Multi-line input is supported via Shift+Enter. The Kitty keyboard protocol is enabled on startup via `PushKeyboardEnhancementFlags` (DISAMBIGUATE + REPORT_EVENT_TYPES). We intentionally omit `REPORT_ALL_KEYS_AS_ESCAPE_CODES` because it causes Shift+letter to arrive as `(SHIFT, Char('1'))` instead of `(NONE, Char('!'))`, breaking secondary character input. With DISAMBIGUATE alone, Shift+Enter sends `CSI 13;2u` → `(SHIFT, Enter)`, which is sufficient. An `(ALT, Enter)` arm is kept as a safety net for Kitty-macOS edge cases. Release events are dropped; both Press and Repeat are processed (Repeat fires when a key is held down, enabling fast cursor movement with held arrow keys). The input field dynamically grows in height (up to 3/4 of terminal height) with proper cursor positioning for newlines and character-level wrapping. When content exceeds the visible area, the view scrolls to keep the cursor visible.

**CRITICAL: Uppercase letter keybinding matching.** Without `REPORT_ALL_KEYS`, shifted letters arrive inconsistently: `(NONE, Char('G'))`, `(SHIFT, Char('G'))`, or `(SHIFT, Char('g'))` depending on terminal. `KeyCombo::matches()` handles all three by accepting: `(SHIFT, any_case)` → uppercase match, or `(NONE, uppercase_only)` → match. Plain lowercase `(NONE, Char('g'))` is explicitly rejected to avoid `g` triggering a `Shift+G` binding. Always use `KeyCombo::shift(KeyCode::Char('T'))` for uppercase bindings — the match logic is centralized.

**Pre-wrapped input rendering:** The input Paragraph does NOT use ratatui's `.wrap()`. Instead, `build_wrapped_content()` pre-wraps text at word boundaries (one `Line` per visual row) and computes cursor position in the same pass. Word-wrap break points are computed by `word_wrap_break_points()` which prefers breaking at the last space before the width limit, falling back to hard char-boundary break when a single word exceeds the width. This guarantees cursor math and text layout always agree. All 6 locations that interact with input wrapping share `word_wrap_break_points()` from `draw_input.rs`: `build_wrapped_content()` (rendering + cursor), `fast_draw_input()` (fast-path rendering), `compute_cursor_row_fast()` (scroll offset), `click_to_input_cursor()` (mouse click), `screen_to_input_char()` (mouse drag), and `row_col_to_char_index()` (shared visual→char mapping). The `display_width()` helper computes unicode display width of char slices for accurate cursor column positioning.

**Theme-independent text color:** Input text is forced to `Color::White` regardless of terminal color scheme — applied in both `build_wrapped_content()` (`normal_style`) and `fast_draw_input()` (`SetForegroundColor(White)`). This ensures consistent visibility on light and dark terminal backgrounds.

Implementation: `prompt_mode: bool` in `App` struct, border color logic in `draw_input()` in `src/tui/draw_input.rs`.

### Terminal Pane

A PTY-based embedded terminal that acts as a portal to the user's actual shell:
- **Cyan border**: Terminal mode active
- Full shell emulation via `portable-pty` - runs in session's worktree
- Color support via `ansi-to-tui` conversion of ANSI escape sequences
- Proper cursor positioning via `vt100` terminal state parser
- Dynamic resizing to match pane dimensions
- Resizable height (5-40 lines)

Key mappings:
- `T` (Shift+T, global command mode): Toggle terminal; `t` (terminal command mode): Enter type mode
- `Esc` (terminal command mode): Close terminal
- `p` (terminal command mode or any mode with prompt tabbed away): Close terminal / refocus prompt
- All globals (e.g. `G`, `H`, `M`, `P`, `T`, `]`/`[`, `r`) work in terminal command mode
- `+/-` (terminal command mode): Increase/decrease terminal height
- `Esc` (terminal type mode): Exit type mode
- `⌥←`/`⌥→` or `⌃←`/`⌃→` (terminal type mode): Word navigation (sends `\x1bb`/`\x1bf` readline sequences)
- Click (terminal pane): Enter type mode and reposition cursor horizontally on the current prompt line
- Mouse drag (terminal pane): Select text with auto-scroll at edges; selection stored as `terminal_selection` in scrollback-adjusted absolute coordinates
- Mouse wheel (terminal pane): Scroll terminal history (clears selection)
- `⌘C`/`⌃C` with active `terminal_selection`: Copy selected terminal text to clipboard
- All other keystrokes in terminal type mode forward directly to PTY (clears selection)

Implementation:
- `terminal_pty`, `terminal_writer`, `terminal_rx`, `terminal_parser` in `App` struct
- `terminal_selection: Option<(usize, usize, usize, usize)>` — `(start_row, start_col, end_row, end_col)` in absolute scrollback coordinates
- `open_terminal()`, `close_terminal()`, `write_to_terminal()`, `poll_terminal()` in `src/app/terminal.rs`
- `draw_terminal()` in `src/tui/draw_terminal.rs` syncs vt100 parser dimensions with viewport, applies selection highlight via `apply_selection_to_line()`
- `copy_terminal_selection()` in `src/tui/event_loop/mouse.rs` — temporarily adjusts vt100 scrollback to extract text with `contents_between()`

### Centralized Keybindings

**ALL keybindings are defined once** in `src/tui/keybindings/` (5 submodules, module root re-exports everything). The `lookup_action()` function is the **SINGLE source of truth** for key → action resolution. Input handlers only receive keys that `lookup_action()` returned `None` for (text input, dialog nav, etc.). **Modal panels** (Health, Git, Projects, pickers, branch dialog) use per-modal lookup functions that resolve keys to the same `Action` enum — draw functions source hint labels from keybinding arrays via hint generators, never hardcoded strings.

**Architecture (5 submodules):**
- **`types.rs`** — `Action` enum (~110 variants incl CycleModel: navigation, editing, viewer tabs, file tree operations, modal-specific actions like `HealthSwitchTab`, `GitSquashMerge`, `GitAutoRebase`, `GitAutoResolveSettings`, `ProjectsAdd`, `BrowseMain`, `AzurealSwitchTab`, etc.), `KeyCombo` (key + modifier with display helpers), `Keybinding` (primary key, alternatives j/↓, description, action, `pair_with_next` for counterpart pairs), `HelpSection`
- **`bindings.rs`** — ~21 static arrays per context: `GLOBAL` (18 entries — core globals: `⌃q`, `⌃d`, cancel, copy, `⌃m`, `?`, `p`, `T`, `G` OpenGitActions, `H` OpenHealth, `M` BrowseMain, `P` OpenProjects, `]`/`[` worktree tabs, `r` RunCommand, `R` AddRunCommand, `Tab`/`⇧Tab`), `WORKTREES` (4 entries — leader sequence `W <key>` targets: `a` AddWorktree, `r` RenameWorktree, `x` ToggleArchive, `d` DeleteWorktree), `FILE_TREE` (17 entries), `VIEWER`, `EDIT_MODE`, `SESSION`, `INPUT`, `TERMINAL`, `HEALTH_SHARED` (9 entries), `HEALTH_GOD_FILES` (4 entries), `HEALTH_DOCS`, `GIT_ACTIONS` (27 entries — context-aware, includes BrowseMain), `PROJECTS_BROWSE`, `PICKER`, `BRANCH_DIALOG`, `AZUREAL_SHARED`, `AZUREAL_DEBUG`, `AZUREAL_ISSUES`, `AZUREAL_PRS`. Plus `ALT_*` static arrays for dual-key alternatives
- **`lookup.rs`** — `KeyContext` (captures guard state from App: focus, prompt_mode, edit_mode, terminal_mode, filter_active, help_open, stt_recording; built via `KeyContext::from_app(app)`), `lookup_action()` with guard logic inside (skip conditions prevent globals from firing during text input, edit mode, or filter — terminal type mode (`prompt_mode=true`) blocks single-letter globals; terminal command mode allows all globals; `EnterPromptMode` (`p`) has its own narrower skip — only blocked in edit mode or when prompt is already focused (so `p` refocuses when tabbed away); no guard duplication in event_loop.rs; when `stt_recording` is true, ToggleStt resolves from any focus/mode; `Focus::Worktrees` maps to `&WORKTREES` so `a`/`x`/`d` resolve directly when the worktrees panel is focused — leader sequence still works from any focus), `lookup_leader_action(mods, code)` resolves second key of `W` leader sequence against the WORKTREES binding array, plus 7 per-modal lookup functions: `lookup_health_action(tab, mods, code)`, `lookup_git_actions_action(focused_pane, is_on_main, mods, code)`, `lookup_azureal_action(tab, mods, code)`, `lookup_projects_action(mods, code)`, `lookup_picker_action(mods, code)`, `lookup_branch_dialog_action(mods, code)`
- **`hints.rs`** — `help_sections()`, title functions returning `(short_label, full_title, hints)` tuples: `prompt_type_title()`, `prompt_command_title()`, `terminal_type_title()`, `terminal_command_title()`, `terminal_scroll_title()`. Modal hint generators: `health_god_files_hints()`, `health_docs_hints()`, `git_actions_labels()`, `git_actions_footer()`, `projects_browse_hint_pairs()`, `picker_title()`, `dialog_footer_hint_pairs()`. Utility: `find_key_for_action()`, `find_key_pair()`. `split_title_hints()` packs as many hint segments as fit on the top border after the mode label, then puts remaining on the bottom border via ratatui's `.title_bottom()`
- **`platform.rs`** — `macos_opt_key()` maps macOS ⌥+letter unicode chars (26 letters + 10 digits) back to their original key for portable matching

The module root (`keybindings.rs`) re-exports all public items so existing `use super::keybindings::*` paths work unchanged.

Other details:
- `execute_action()` in `event_loop.rs` dispatches all actions to their side effects
- Global bindings shown in both the help panel (GLOBAL section) and the command box title (essential hints: prompt, terminal, git, health, run, cancel, quit, help); Terminal and Input bindings shown in their own title bars (not in help panel) via title functions
- Modal panels with visible footer hints (Health, Git, Projects) are excluded from the help panel — their keys are already self-documenting in the panel UI

**Resolution flow in `handle_key_event()` (event_loop.rs):**
1. **Leader continuation:** If `leader_state == WaitingForAction`, the key is resolved via `lookup_leader_action()` against the WORKTREES binding array. Any match dispatches the action and resets leader state; Esc cancels; unrecognized keys reset with a status message. Fires before all modals to complete mid-sequence
2. Modal overlays (help, wizard, projects, health, git, pickers, session list) intercept ALL input first — each modal uses its per-modal lookup function
3. Text input modals (`BranchDialog`, `file_tree_action`, `new_session_dialog_active`) bypass keybinding resolution entirely — routed directly to their handlers before `lookup_action()` to prevent global bindings (e.g., Shift+G → Git panel, Enter → OpenFile, Esc → focus change) from stealing keystrokes meant as literal text input or action confirmation
4. `KeyContext::from_app(app)` + `lookup_action()` resolves key → action for main views
5. If `stt_recording` is true, ToggleStt is resolved from any focus/mode (so recording can always be stopped even after Tab changes focus)
6. If action found → `execute_action()` dispatches it (except input-specific actions like Submit/InsertNewline/ToggleStt which fall through to handle_input_mode when `Focus::Input`)
7. **Leader entry:** `Shift+W` (not in prompt/terminal/edit mode) sets `leader_state = WaitingForAction` and shows `[W …]` in the status bar. Fires after modal dispatch but before focus-specific handlers
8. If `None` → focus-specific handler processes unresolved keys (text editing, dialog nav)

**Input handlers only handle unresolved keys:**
- `input_viewer.rs` — tab dialog, save dialog, discard dialog, edit mode text editing
- `input_output.rs` — session list overlay input
- `input_file_tree.rs` — clipboard mode (Copy/Move paste target), text-input actions (Add, Rename, Delete confirmation)
- `input_worktrees.rs` — 's' stop-tracking (only unresolved key handler for worktree tab row)
- `input_health.rs` — `lookup_health_action()` → Action match (tab switching, panel-level keys like scope, per-tab keys)
- `input_git_actions.rs` — Module root: `lookup_git_action()` → Action match dispatch; 5 submodules: `diff_viewer.rs` (file/commit diff loading), `operations.rs` (pull/push/rebase/squash-merge/commit/refresh + RebaseOutcome + auto-resolve union merge), `commit_overlay.rs` (commit message editing), `conflict_resolution.rs` (conflict overlay + RCR Claude spawn), `auto_resolve_overlay.rs` (auto-resolve file list settings)
- `input_projects.rs` — `lookup_projects_action()` → Action match (browse mode only; text input stays raw)
- `input_dialogs.rs` — `lookup_branch_dialog_action()`, `lookup_picker_action()` → Action matches; text input and number quick-select stay raw

**macOS ⌥+letter gotcha:** On macOS, `Option+letter` produces Unicode characters (e.g., `⌥c` → `ç`, `⌥r` → `®`), so crossterm sees `KeyCode::Char('ç')` with `KeyModifiers::NONE` — NOT `ALT + 'c'`. For keybindings that use `⌥+letter`, add the unicode char as an alternative via `with_alt()` and `ALT_MACOS_R` style statics (e.g., `⌥r` has `®` as alternative). `macos_opt_key()` maps all 26 unicode chars back to their letter for runtime lookups. `⌥+arrow` keys work fine since arrows don't produce Unicode. In text input modes, prefer `⌃+letter` (Ctrl) instead since those send real control codes. **Help panel display:** `display_keys()` filters out non-ASCII bare-char alternatives (®, π, †) so the help panel shows clean `⌥r` instead of `⌥r/®` — the unicode chars are internal matching details, not user-facing.

**input_cursor is a CHAR INDEX, not a byte offset.** `String::insert()` and `String::remove()` take byte offsets. Use `char_to_byte(char_idx)` to convert before calling them. Comparing `input_cursor` against `String::len()` (bytes) is wrong — use `.chars().count()` instead. See `src/app/input.rs`.

**Enforcement hooks:** `.claude/scripts/enforce-keybindings.sh` runs as a PreToolUse hook on every Edit/Write. Catches 3 violations: (1) raw `KeyCode::`/`KeyModifiers::` in `input_*.rs` (must use `lookup_*_action()`), (2) hardcoded key label strings in `draw_*.rs` without `keybindings::` import (must use hint generators), (3) new static binding arrays in `keybindings.rs` without companion lookup/hint functions. Configured in `.claude/settings.json`.

Implementation: `src/tui/keybindings.rs` (module root + re-exports), `src/tui/keybindings/types.rs` (KeyCombo, Action enum ~109 variants, Keybinding, HelpSection), `src/tui/keybindings/bindings.rs` (~21 static arrays), `src/tui/keybindings/lookup.rs` (KeyContext, lookup_action() + 7 per-modal lookup fns), `src/tui/keybindings/hints.rs` (help_sections(), title generators, hint generators, find_key_*), `src/tui/keybindings/platform.rs` (macos_opt_key), `src/tui/event_loop/actions.rs` (execute_action(), dispatch helpers, BrowseMain handler, Esc browse-mode exit), `src/tui/draw_dialogs.rs::draw_help_overlay()` (uses `keybindings::help_sections()`), `.claude/scripts/enforce-keybindings.sh` (PreToolUse enforcement hook)

### Wrap-Aware Edit Cursor

The viewer edit mode cursor navigates wrapped visual lines, not just source lines. Long lines wrap at `content_width = viewport_width - line_num_width - 3` characters. The wrap width is cached in `app.viewer_edit_content_width` (set by `draw_edit_mode()`).

**Word-boundary wrapping:** Both read-only and edit modes use `textwrap::wrap()` for word-boundary wrapping. The `word_wrap_breaks(text, max_width)` function returns `Vec<usize>` of char offsets where each visual row starts. All cursor math uses these break positions instead of fixed-width `col / cw` assumptions.

**Up/Down navigation:** `viewer_edit_up()` / `viewer_edit_down()` call `word_wrap_breaks()` to find which wrap row the cursor is on. Moving up from wrap_row > 0 stays on the same source line; from wrap_row 0 it jumps to the previous source line's last wrap row. Same logic in reverse for down. The visual column offset from the break position is preserved across wrap rows.

**Scroll-to-cursor:** `viewer_edit_scroll_to_cursor()` sums `word_wrap_breaks().len()` for all source lines before the cursor line, adds the cursor's wrap offset, and scrolls the viewport to keep that visual line visible.

**Mouse click/drag:** `screen_to_edit_pos()` maps screen coordinates to `(source_line, source_col)` by walking source lines and summing their wrap counts (via `word_wrap_breaks()`) until the clicked visual row is found. Click column mapped through break positions to get correct char offset. Stored as drag anchor with pane_id=3 for edit-mode drag selection.

**Display wrapping:** `wrap_spans_word()` wraps styled spans using word-boundary break positions from `word_wrap_breaks()`. Used by both read-only viewer and edit mode display. `word_wrap_breaks()` is `pub(crate)` in `draw_viewer/wrapping.rs` (re-exported from `draw_viewer.rs`) and duplicated privately in `viewer_edit.rs` (app module can't import from tui).

Implementation: `src/app/state/viewer_edit.rs` (cursor movement, scroll, local `word_wrap_breaks()`), `src/tui/event_loop/coords.rs` (`screen_to_edit_pos()`), `src/tui/event_loop/mouse.rs` (pane_id=3 drag handling), `src/tui/draw_viewer/wrapping.rs` (`word_wrap_breaks()`, `wrap_spans_word()`), `src/tui/draw_viewer/edit_mode.rs` (caches `content_width`)

### Stream-JSON Parsing

Claude output is received in `stream-json` format and parsed for clean display:
- User prompts shown as "You: <message>"
- Claude responses shown as "Claude: <text>"
- Tool calls shown as timeline nodes with tool name and primary parameter
- Tool results shown with tool-specific formatting (see below)
- Completion info shown as "[Done: Xs, $X.XXXX]"
- Hook output shown as "[Hook: <name>] <output>"
- Slash commands (`/compact`, `/crt`, etc.) shown as 3-line magenta banners
- Context compaction shown as "✓ Context compacted" green banner (post-compaction)

**Tool Status Indicators** (patched at draw time, updates immediately on tool completion):
| Indicator | Color | Meaning |
|-----------|-------|---------|
| ● | Green | Tool completed successfully |
| ○ | Pulsating white/gray | Tool in progress (waiting for result) |
| ✗ | Red | Tool failed (error detected in result) |

Status circles update in real-time as tools complete — `animation_line_indices` tracks ALL tool positions with their `tool_use_id`, and the viewport patching step checks current `pending_tool_calls`/`failed_tool_calls` to set the correct indicator text and color. `tool_status_generation` increments on every status change, invalidating the viewport cache.

Error detection uses the `is_error` field from Claude Code's stream-json `tool_result` blocks (authoritative). `DisplayEvent::ToolResult` carries `is_error: bool` populated from the raw JSON. For older session files lacking `is_error`, a conservative fallback heuristic checks: `<tool_use_error>`, first-line `"Error..."` prefix, `"ENOENT"`.

**Tool Result Display Formats:**
| Tool | Format | Description |
|------|--------|-------------|
| Read | Clickable link + first/last line | File path underlined and clickable; shows file boundaries with line count |
| Bash | Last 2 lines | Shows command results (usually at end) |
| Edit | Clickable link + inline diff (last 20) | File path underlined; click to view in Viewer with diff overlay. Last 20 Edit calls show inline diff: removed lines in grey on dim red bg (no syntax highlighting), added lines syntax-highlighted on dim green bg, with actual file line numbers |
| Write | Clickable link + purpose line | File path underlined and clickable; shows line count + first comment |
| Grep | First 3 matches | Preview of search results |
| Glob | Directory summary | File count grouped by directory |
| Task | Summary line | First line of agent response |
| WebFetch | Title + preview | Page title and first content line |
| WebSearch | First 3 results | Numbered search results |
| LSP | Result + context | Location and code context |

**Command Detection:**
User messages containing `<command-name>/xxx</command-name>` tags are parsed as slash commands and displayed prominently with centered 3-line banners in magenta.

**Compacting Detection:**
- "✓ Context compacted" (green, `Compacting` variant) - shown when the compaction summary text ("This session is being continued from a previous conversation...") is detected in a user message. This text appears AFTER compaction finishes — there is no "starting" event for auto-compaction.
- "✓ Context compacted" (green, `Compacted` variant) - shown when `<local-command-stdout>` contains "Compacted" (from `/compact` slash command output). **Unreachable in practice** since AZUREAL uses `-p` mode which doesn't support slash commands.
- "⏳ Session may be compacting conversation..." (yellow, `MayBeCompacting` variant) - shown when context usage ≥ 90% AND 20 seconds pass with no new events parsed from the session. Since there's no way to detect auto-compaction until after it completes, this inactivity heuristic warns the user why the session pane appears frozen. Banner is injected once per high-context period; cleared when new events arrive or context drops below 90%. The 20s inactivity timer resets in `register_claude()` so it starts from prompt submission, not from the previous response's last event.

**Filtered Messages:**
- Meta messages (`isMeta: true`) are hidden - internal Claude instructions
- `<local-command-caveat>` messages are hidden - tells Claude to ignore local command output
- `<task-notification>` messages are hidden - injected by Claude Code when background commands complete
- `<local-command-stdout>` content is hidden - raw output from local commands like `/memory`, `/status`
  - Exception: "Compacted" triggers the CONVERSATION COMPACTED banner before being filtered
- Rewound/edited user messages - when user rewinds to edit a message, only the corrected version is shown
  - Detection: Multiple user messages sharing the same `parentUuid` - keep only the most recent by timestamp

**Debug Output:**
`⌃D` opens a naming dialog, then dumps diagnostic output to `.azureal/debug-output[_name]`. Enter with empty name saves as `debug-output`; typing a name saves as `debug-output_<name>` (e.g., `debug-output_scroll_bug`). Esc cancels. A "Saving…" dialog is shown while the dump I/O runs (two-phase: draw dialog first, run dump next frame) so the app doesn't appear frozen on large sessions. All user/assistant message content, file paths, and rendered conversation text are **obfuscated** via deterministic word replacement (same word → same fake word) so the file can be safely attached to GitHub issues without exposing sensitive project details. Tool names, event types, parsing stats, and structural markers are preserved for diagnostic value. Contains: parsing stats, event type breakdown, last 5 events (obfuscated previews), and full rendered output (obfuscated).

**Markdown Rendering:**
Claude responses are parsed for markdown syntax and rendered with proper styling:
- `# H1`, `## H2`, `### H3` headers → styled with block chars (█, ▓, ▒) and colors, prefix removed
- `**bold**` → bold text without markers
- `*italic*` → italic text without markers
- `` `inline code` `` → yellow text on dark background
- ``` code blocks ``` → box-drawn borders with language label
- `| table | rows |` → box-drawing characters (│, ├, ┼, ┤), column widths clamped to fit bubble width (cells truncated with `…` when too wide)
- `- bullet` and `1. numbered` lists → indented with cyan bullets
- `> blockquotes` → gray vertical bar with italic text

Implementation: `parse_markdown_spans()`, `parse_table_row()`, `is_table_separator()` in `src/tui/markdown.rs`

**Hook Visibility - Multiple Extraction Methods:**
Claude Code hooks are captured from multiple sources in the session file:

1. **hook_progress events** (type: "progress", data.type: "hook_progress")
   - PreToolUse, PostToolUse hooks
   - Hook output extracted from `command` field's echo statements
   - Patterns: `echo 'message'` or `OUT='message'; ...; echo "$OUT"`
   - **Fallback**: If command format doesn't match, shows `[hookName]` to ensure visibility

2. **system-reminder tags** in assistant "thinking" blocks
   - UserPromptSubmit hooks appear here (Claude Code injects them into context)
   - Claude sees the injected system-reminder and it appears in thinking output
   - Format: `<system-reminder>HookName hook success: output</system-reminder>`
   - Extracted via `extract_hooks_from_content()` in `load_claude_session_events()`

3. **system-reminder tags** in user messages and tool results
   - Various hooks that appear in user message content or tool result content
   - Same extraction pattern as thinking blocks

4. **hook_response events** (SessionStart only)
   - Only emitted for SessionStart hooks in stream-json

5. **UserPromptSubmit hook positioning**
   - Claude Code doesn't execute shell commands for UserPromptSubmit hooks (only injects output into context)
   - System-reminder with hook content appears in assistant thinking blocks (not tool_results)
   - Azureal extracts UPS hooks from thinking blocks and assigns them timestamp = user_message_timestamp + 1ms
   - When events are sorted by timestamp, UPS hooks naturally appear right after their user message
   - UPS hooks from hooks.jsonl are skipped (duplicates with wrong timestamps)
   - UPS hooks display as dim gray lines: `› UserPromptSubmit: <output>`

6. **Compaction summary handling**
   - When loading a continued session, the summary message ("This session is being continued...") contains quoted `<system-reminder>` references from conversation history
   - These quoted references should NOT be treated as real hooks
   - Azureal skips hook extraction for the compaction summary and its immediately following tool results
   - Flag `in_compaction_summary` tracks this state and resets only when a real user prompt is encountered

**Hook Deduplication:**
- Consecutive-only deduplication (not global)
- Same hook can appear multiple times throughout conversation
- Only back-to-back identical hooks are filtered
- Hooks display next to their corresponding tool calls

**Supported hook types:** SessionStart, UserPromptSubmit, Stop, PreToolUse, PostToolUse, SubagentStop, PreCompact

Implementation: `extract_hooks_from_content()` in `src/app/session_parser.rs`, `parse_progress_event()` in `src/events/parser.rs`

### Token Usage Counter

Color-coded context window usage percentage displayed on the Session pane's right border title. Helps users predict when context compaction will occur.

**Data source:** Claude's JSONL session files already contain `message.usage` on every assistant event with `input_tokens`, `output_tokens`, `cache_read_input_tokens`, and `cache_creation_input_tokens`. No external tokenization library needed — exact counts from the API.

**Metric:** `input_tokens + cache_read_input_tokens + cache_creation_input_tokens` = effective context size. Each assistant event overwrites the previous — the last one reflects the most recent context window consumption.

**Color thresholds:**
| Range | Color | Meaning |
|-------|-------|---------|
| 0–59% | Green | Plenty of context remaining |
| 60–79% | Yellow | Context getting full |
| 80–100% | Red | Near compaction threshold |

**Context window detection (two-tier):**
1. **Authoritative (result event):** The `result` event's `modelUsage.<model_id>.contextWindow` field provides the exact context window from the API. Extracted in both `parse_result_event()` (session file) and `handle_claude_output()` (live stream). This is the definitive value.
2. **Heuristic fallback (assistant event):** `context_window_for_model()` in `src/app/session_parser.rs` maps model ID prefixes to their standard context window (currently 200k for all). Used as an early estimate before the first `result` event fires. Only sets `model_context_window` if it's still `None` (won't override the authoritative value).
3. **Auto-bump to 1M:** If actual token usage exceeds the detected window, the display logic in `draw_output.rs` bumps to 1M (1M beta detection).

Stored as `model_context_window: Option<u64>` on App state — `None` until first event is parsed.

**Data flow (session store sourced):**
1. **Live streaming:** `apply_parsed_output()` in `src/app/state/claude.rs` increments `chars_since_compaction` per event and calls `update_token_badge_live()` — badge updates in real-time during streaming
2. **Store append:** `store_append_from_jsonl()` in `src/app/state/claude.rs` parses JSONL → strips injected context → appends to SQLite store → calls `update_token_badge()`
3. **Compaction stored:** `poll_compaction_agents()` in `src/tui/event_loop/agent_events.rs` stores the compaction summary → calls `update_token_badge()` (chars drop, percentage resets)
4. **Load propagation:** `load_session_output()` and `refresh_session_events()` in `src/app/state/load.rs` call `update_token_badge()` which reads from the store. `load_session_output()` resets `chars_since_compaction = 0` and syncs from store for both live and historic session paths
5. **Badge cache:** `update_token_badge()` in `src/app/state/app/model.rs` queries `store.total_chars_since_compaction(session_id)`, syncs `chars_since_compaction` (live counter), and computes `(chars / COMPACTION_THRESHOLD) * 100%` (400k chars = 100%). `update_token_badge_live()` reads `chars_since_compaction` directly (no store query). Both precompute `(String, Color)` — draw path reads the cached value with zero computation
6. **Display:** `draw_output_pane()` in `src/tui/draw_output.rs` reads `token_badge_cache` and renders as right-aligned spans before PID/exit code

**Reset:** `token_badge_cache` cleared to `None` and `chars_since_compaction` reset to `0` on session switch (in `load_session_output()`). Badge hidden when no store/session available.

**Compaction inactivity watcher:** When context usage ≥ 90%, `update_token_badge()` sets `context_pct_high = true`. The event loop checks: if `context_pct_high && !compaction_banner_injected && is_active_slot_running() && last_session_event_time elapsed ≥ 30s`, it injects a `DisplayEvent::MayBeCompacting` banner. Uses `is_active_slot_running()` (not `!agent_receivers.is_empty()`) so background processes on other branches don't trigger the banner for the viewed session. When new events arrive (in `apply_parsed_output()` or `refresh_session_events()`), both `last_session_event_time` and `compaction_banner_injected` are reset. `load_session_output()` also resets both to prevent stale timers from triggering on session switch. When context drops below 90% (e.g. after compaction completes), `compaction_banner_injected` is cleared.

**Model persistence via session-store tags:** Selected model persists per-session using `DisplayEvent::ModelSwitch { model }` tags injected into the event stream. `cycle_model()` pushes a `ModelSwitch` event into `display_events` and appends it to the SQLite session store on every model change. On startup, `last_session_model()` scans the loaded event stream in reverse — `ModelSwitch` tags take priority (explicit user choice), then falls back to `Init` events (model from session start). `model_alias_from_init()` maps model strings to ALL_MODELS aliases (exact matches, Claude API names, legacy "codex" string, unknown gpt-* prefixes). Only defaults to opus when a session is brand-new/empty. `ModelSwitch` events are stripped from LLM context injection (`format_event()` returns `None`). In the render pipeline, `ModelSwitch` updates `current_model` (both in `render_events.rs` main loop and `render_submit.rs` pre-scan) so subsequent `AssistantText` bubble headers display the correct model name — but the tag itself produces no visible output.

Implementation: `token_badge_cache: Option<(String, Color)>`, `context_pct_high: bool`, `last_session_event_time: Instant`, `compaction_banner_injected: bool`, `chars_since_compaction: usize`, `compaction_spawn_deferred: bool`, `auto_continue_after_compaction: bool`, `selected_model: Option<String>`, `detected_model: Option<String>` in `src/app/state/app.rs`, `update_token_badge()` / `display_model_name()` / `cycle_model()` / `last_session_model()` methods + `model_alias_from_init()` / `default_model()` free functions in `src/app/state/app/model.rs`, `DisplayEvent::ModelSwitch` in `src/events/display.rs`, `total_chars_since_compaction()` + `COMPACTION_THRESHOLD` in `src/app/session_store.rs`, display in `src/tui/draw_output.rs`, inactivity check + compaction spawn + auto-continue in `src/tui/event_loop.rs`

### TodoWrite Sticky Widget

Claude's `TodoWrite` tool calls are parsed from session JSONL and rendered as a persistent checkbox widget at the bottom of the Session pane instead of inline generic tool call JSON. The widget stays visible as the user scrolls through conversation history and hides when all todos are completed. When a subagent (Task tool) is active, its TodoWrite calls render as indented subtasks directly beneath the parent todo item (the in-progress item when the Task spawned), tracked via `subagent_parent_idx`, and prefixed with `↳`. Subagent todos are cleared when the Task tool completes.

**Height cap and scrollbar:** The widget grows to fit its content but caps at 20 visual lines (including wrapped text). When content exceeds 20 lines, a scrollbar column appears on the rightmost border position (AZURE `█` thumb on `│` track) and the widget responds to mouse wheel scrolling. Scroll offset (`todo_scroll`) resets to 0 whenever todos are updated (new TodoWrite tool call). The `pane_todo` rect is cached during draw for mouse hit-testing — checked before `pane_session` in `apply_scroll_cached()` since the todo widget overlaps the session area.

**Status icons:**
| Icon | Color | Meaning |
|------|-------|---------|
| ✓ | Green | Completed |
| ● | Yellow (pulsating) | In progress |
| ○ | Dim gray | Pending |

In-progress items show their `activeForm` text (present tense, e.g., "Building project"), while pending/completed items show `content` (imperative, e.g., "Build project").

**Data flow:**
1. **Live stream:** `handle_claude_output()` in `src/app/state/claude.rs` detects `TodoWrite` ToolCall events and routes them: if an Agent/Task tool is active (`active_task_tool_ids` non-empty), todos go to `app.subagent_todos` and `subagent_parent_idx` is set to the index of the current in-progress item; otherwise to `app.current_todos`. Both `"Agent"` and `"Task"` tool calls are tracked via `active_task_tool_ids` — when the last one completes, subagent todos are cleared.
2. **Session load:** `extract_skill_tools_from_events()` in `src/app/state/load.rs` scans all display_events forward to find the latest TodoWrite and restore todo state
3. **Session switch:** `current_todos` cleared on session switch and rebuilt from new session's events
4. **Rendering:** `draw_todo_widget()` in `src/tui/draw_output/todo_widget.rs` splits the session area with `Layout::vertical()` — scrollable content above, sticky todo box below. Height capped at 22 rows (20 content + 2 borders); when content overflows, accepts `scroll` offset and renders a proportional scrollbar on the rightmost column via direct buffer writes

**Lifecycle:** Widget stays visible even after all items are completed (showing all checkmarks). It clears when the user submits their next prompt (`current_todos.clear()` in the Enter handler). This ensures the user sees the final completed state before it disappears.

**Inline suppression:** TodoWrite tool calls and their results are suppressed from the inline session stream (`render_display_events()` skips them). The sticky widget is the only representation.

Implementation: `TodoItem` struct + `TodoStatus` enum in `src/app/state/app.rs` (includes `subagent_todos`, `active_task_tool_ids`, `pane_todo`, `todo_scroll`, `todo_total_lines` fields), `parse_todos_from_input()` in `src/app/state/claude.rs`, `draw_todo_widget()` in `src/tui/draw_output/todo_widget.rs` (renders subtasks beneath parent item via `subagent_parent_idx` with `↳` prefix, scroll offset, scrollbar column), mouse scroll routing in `src/tui/event_loop/mouse.rs` (`pane_todo` hit-test before `pane_session`), suppression in `src/tui/render_events.rs`

### AskUserQuestion Options Box

Claude's `AskUserQuestion` tool calls are parsed from session JSONL and rendered as a numbered options box (similar to plan approval prompts) instead of raw JSON. The user responds by typing a number or custom text.

**Rendering:** A magenta-bordered box per question with the question header, numbered options (label + description), and an implicit "Other" option at the end. Multi-select questions are annotated. Rendered inline in the session stream when the tool result arrives (positioned after the result, before user response).

**Input handling:** When `awaiting_ask_user_question` is true, the user's response gets a hidden system context prefix (`build_ask_user_context()` in `src/tui/input_terminal.rs`) listing the questions and numbered options that were shown. This lets Claude interpret "1", "2", etc. as option selections. The context is invisible to the user — they just see their typed response.

**State tracking:**
- `awaiting_ask_user_question: bool` — set when AskUserQuestion ToolCall detected, cleared on user submit
- `ask_user_questions_cache: Option<serde_json::Value>` — cached input JSON for building context prefix
- `saw_ask_user_question` / `saw_user_after_ask` in render pipeline for conditional box display

**Session load:** `extract_skill_tools_from_events()` tracks whether the last AskUserQuestion was answered by scanning for a subsequent UserMessage. If unanswered, restores the awaiting state.

Implementation: `render_ask_user_question()` in `src/tui/render_events.rs`, `build_ask_user_context()` in `src/tui/input_terminal.rs`, state in `src/app/state/app.rs`

### Worktree Tab Row

The worktree sidebar was replaced by a horizontal tab row at the top of the normal mode layout. `[★ main]` tab is always first; clicking it or pressing `Shift+M` toggles main branch browse. `[`/`]` switch tabs globally from any pane. The tab row is not focusable — `Tab`/`Shift+Tab` cycle through FileTree → Viewer → Session → Input. Worktree actions (`w` add, `⌘a` archive, `⌘d` delete) are global keybindings.

**Tab styling:** Active tab uses AZURE bg + white fg + bold; `[M]` active uses yellow bg + black fg + bold; archived tabs use dim gray with `◇` prefix; unread tabs (session finished while viewing another worktree) use AZURE fg with `◐` prefix; inactive tabs use gray with status symbol prefix. No leading space before icons — trailing space only for separator padding. Auto-rebase indicator `R` (bold, color-coded) appended after label with +1 char width. Pagination via greedy tab packing with `N/M` page indicator.

Implementation: `draw_worktree_tabs()` in `src/tui/run/worktree_tabs.rs`, `worktree_tab_hits: Vec<(u16, u16, Option<usize>)>` in `src/app/state/app.rs`, mouse click handling in `src/tui/event_loop/mouse.rs`

### Speech-to-Text Input

Press `⌃s` in prompt mode or file edit mode to toggle speech recording. Audio is captured via cpal (CoreAudio on macOS), transcribed locally via whisper.cpp with GPU acceleration (Metal on macOS, CUDA on Windows), and inserted at the cursor position. In edit mode, text goes into the viewer edit buffer; in prompt mode, into the prompt input field. When recording is active, `⌃s` resolves from ANY focus/mode (via `stt_recording` in `KeyContext`) so the user can stop recording even after Tab clears prompt_mode.

**Architecture:**
- Background thread (`stt_loop`) blocks on `mpsc::recv()` when idle (zero CPU)
- Same pattern as RenderThread and Terminal PTY: mpsc channels, `try_recv()` polling in event loop
- `SttHandle` lazy-initialized on first `⌃s` press (no resources allocated until needed)
- `WhisperContext` lazy-loaded on first transcription and cached for reuse

**Audio pipeline:**
1. cpal callback pushes `f32` samples to `Arc<Mutex<Vec<f32>>>` (~10μs lock per callback)
2. Device sample rate captured from default input config (typically 44100 or 48000 Hz)
3. Stereo/multi-channel audio mixed down to mono in the callback
4. On stop: samples drained, resampled to 16kHz mono via linear interpolation
5. Whisper transcription with `Greedy { best_of: 1 }`, single-segment mode
6. Transcribed text inserted at cursor with smart space handling

**Visual feedback:**
- Recording: magenta border + `REC` prefix in input title
- Transcribing: magenta border + `...` prefix in input title
- Status bar shows progress messages (Recording..., Transcribing Xs of audio..., Loading Whisper model...)

**Model:** `~/.azureal/speech/ggml-small.en.bin` (~466MB). If missing, status bar shows download instructions:
```bash
mkdir -p ~/.azureal/speech && curl -L -o ~/.azureal/speech/ggml-small.en.bin \
  https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-small.en.bin
```

**Event loop integration:**
- `poll_stt()` called every iteration when `stt_handle` exists
- Events collected into Vec first (avoids borrow conflict: `try_recv` borrows handle, processing borrows `&mut self`)
- Short poll timeout (16ms) when `stt_recording || stt_transcribing`

Implementation: `src/stt.rs` (engine), `stt_handle`, `stt_recording`, `stt_transcribing` in `src/app/state/app.rs`, `toggle_stt()`, `poll_stt()`, `insert_stt_text()` methods, `⌃s` binding in `src/tui/keybindings.rs` (`EDIT_MODE` and `INPUT` arrays), `Action::ToggleStt` dispatched via `execute_action()` in `actions.rs` (edit mode) and raw match in `handle_input_mode()` (prompt mode), polling in `src/tui/event_loop.rs`, visual feedback in `src/tui/draw_input.rs` and `src/tui/draw_viewer.rs` (edit mode magenta border + REC indicator)

### Conversation Persistence

Each session maintains conversation history via the SQLite session store (`.azs`):
- New sessions created in the store via `store.create_session(branch)` when the user starts a new session
- Conversation context built from the store and injected into each prompt (no `--resume`)
- After each agent exit, JSONL is parsed → events appended to store → JSONL file deleted. A fallback cleanup in `handle_claude_exited()` independently resolves the JSONL path via `agent_session_ids` + worktree path and deletes it, catching cases where `store_append_from_display()` already consumed the `pid_session_target` entry (compaction, prompt supersede)
- History is portable: copy `.azureal/sessions.azs` to transfer all session data between machines

**Data Discovery:**
- **Project**: Discovered via `git rev-parse --git-common-dir` (parent = repo root), main branch detected from git
- **Worktrees**: Discovered from `git worktree list` (active) + `git branch | grep {BRANCH_PREFIX}/` (archived)
- **Sessions**: Loaded from SQLite store (`.azureal/sessions.azs`); live sessions read from JSONL during active streaming
- **Path encoding**: `encode_project_path()` in `config.rs` — matches Claude CLI's `OP()` function: `replace(/[^a-zA-Z0-9]/g, "-")`. Paths >200 chars get truncated + hash suffix. Startup migration (`migrate_project_dirs()`) renames old-encoding dirs to new encoding.
- **Live streaming**: During active Claude processes, the JSONL file is watched for changes and incrementally parsed for real-time display. After process exit, events are ingested into the store and the JSONL is deleted.

Implementation: `encode_project_path()`, `session_file()`, `migrate_project_dirs()` in `src/config.rs`, `load_worktrees()` in `src/app/state/load.rs`, `session_store.rs` for SQLite operations

### Worktree Health Panel

Tabbed modal overlay (`Shift+H` toggles open/close, global keybinding) housing multiple health-check systems. Green accent color (`Rgb(80,200,80)`, `GF_GREEN` constant) with QuadrantOutside border. Centered modal (55% × 70%, min 50×16). Bold title: `" Health: <worktree_name> "` (mirrors the Git panel's `" Git: <worktree_name> "` pattern — `worktree_name` field on `HealthPanel` struct, populated from `Worktree::name()` at open time). Reopens on the last-visited tab (`last_health_tab` in App state, defaults to God Files).

**Tab Bar:**
Row 0 inside border: `[ God Files ]  [ Documentation ]` — active tab bright green + bold, inactive dim gray. Tab key switches between tabs.

**God Files Tab:**
Scans the project for "god files" — source files exceeding 1000 lines of production code. For Rust files, `#[cfg(test)]` module blocks are excluded from line counts so test code doesn't inflate the measurement. Same checkbox list as the old standalone panel.

*Scanning:*
- **Source-root detection:** If well-known source directories exist under the project root (`src/`, `lib/`, `crates/`, `cmd/`, `pkg/`, `internal/`, `app/`, `core/`, `common/`, `modules/`, `services/`, `packages/`, `components/`, `Sources/`, `include/`, `source/`), ONLY those directories are scanned (plus top-level files). Falls back to full project root if none found.
- **Skip directories:** Hidden dirs, build artifacts, dependency caches, IDE dirs, non-source content (~55 common non-source directories). Case-insensitive matching.
- Source extensions (~60): Systems, JVM, .NET, Web, Scripting, Functional, Shell, Infra/Query, Schema.
- Threshold: >1000 LOC. Results sorted by line count descending.
- Synchronous scan — fast enough for typical projects.

*Keybindings (God Files tab):*
- `j/↓`, `k/↑` — navigate; `J/K` — page scroll (page size = `screen_height` minus chrome, NOT the embedded terminal pane's `terminal_height`); `⌥↑/⌥↓` — jump top/bottom
- Mouse wheel scrolls the list (modal intercepts all scroll events)
- `Space` — toggle check; `a` — toggle all
- `v` — view checked files as Viewer tabs (up to 12)
- `Enter`/`m` — modularize checked files
- `Tab` — switch to Documentation tab
- `Esc` — close panel

*Scope Mode (`s` — panel-level, accessible from any tab):*
`s` is a shared health panel keybinding (in `HEALTH_SHARED`, displayed as `s:scope` in the panel border top-right; `Tab:tab` mirrors it on the top-left). Opens the FileTree overlay in scope mode with green highlights on directories in the scan scope. Subdirectories of accepted dirs automatically inherit accepted status (bright green). Files inside scoped dirs dimmed green; everything else dimmed gray. Green double-line border with `" Health Scope (N dirs) "` title. Enter toggles dirs in/out of scope. Esc persists scope to project azufig.toml `[healthscope].dirs` (alias `[godfilescope]` for backward compatibility via `#[serde(alias = "godfilescope")]`), rescans both god files and documentation, and reopens the health panel with updated results. Scope auto-loaded on panel open.

*Module Style Selection:*
When checked files include `.rs` or `.py`, pressing Enter/m shows a **module style selector** dialog before spawning. The dialog lets users choose between dual-style module conventions:
- **Rust**: File-based root (`modulename.rs` + `modulename/`, modern) vs directory module (`modulename/mod.rs`, legacy)
- **Python**: Package (`__init__.py` directory) vs single-file modules

The choice is embedded in each file's modularization prompt. Languages without dual-style conventions (Go, Java, TypeScript, etc.) skip the dialog entirely. `Space` toggles between styles, `j/k` navigates between language rows (when both Rust and Python are checked), `Enter` confirms and spawns, `Esc` cancels back to the file list.

*Parallel Modularization:*
All checked files spawned simultaneously as concurrent agent processes on the current worktree. Each session named `[GFM] <filename>`. Model and backend derived from `selected_model` via `AgentProcess::spawn()` — respects the user's model switcher choice (Claude or Codex). Changes merge back to main via squash-merge.

**Documentation Tab:**
Scans all source files for documentation coverage — counts documentable items (`fn`, `struct`, `enum`, `trait`, `const`, `static`, `type`, `impl`, `mod`) and checks whether each has a preceding `///` or `//!` doc comment. Line-based heuristic, no AST parsing.

*Display:*
- `[DH] (Documentation Health)` session naming hint at top
- Overall score header: `Overall Documentation Score: XX.X%` color-coded (green ≥80%, yellow ≥50%, red <50%) with file count
- Per-file list sorted by coverage ascending (worst-documented first) with `[x]/[ ]` checkboxes
- Each row: checkbox, file path, coverage percentage, visual bar (`█░` blocks), documented/total ratio
- Selected row highlighted in green; checked count shown in header

*Keybindings (Documentation tab):*
- `j/↓`, `k/↑` — navigate; `J/K` — page scroll (page size = `screen_height` minus chrome, NOT the embedded terminal pane's `terminal_height`); `⌥↑/⌥↓` — jump top/bottom
- Mouse wheel scrolls the list (modal intercepts all scroll events)
- `Space` — toggle check on selected file
- `a` — check all non-100% files (toggle: if all non-100% checked, unchecks them)
- `v` — view checked files as Viewer tabs (up to 12)
- `Enter` — complete checked files (spawns concurrent [DH] Claude sessions, one per file)
- `Tab` — switch to God Files tab
- `Esc` — close panel

*[DH] Session Spawning:*
Checked files spawn concurrent agent sessions on the current worktree, each prefixed `[DH] filename`. Model and backend derived from `selected_model` via `AgentProcess::spawn()` — respects the user's model switcher choice (Claude or Codex). The prompt instructs the agent to add `///` and `//!` doc comments to all undocumented items without modifying executable code. Shows current documented/total ratio so the agent knows the starting coverage. Changes merge back to main via squash-merge.

*Auto-Refresh:*
When the health panel is open, file changes in the worktree trigger an automatic rescan (debounced 500ms alongside file tree refresh). `health_refresh_pending` flag set on `WorktreeChanged` events when `health_panel.is_some()`. `refresh_health_panel()` rescans god files + documentation while preserving tab, cursor positions, scroll offsets, and checked states (matched by `rel_path`). Cursor clamped to new list bounds after rescan.

Implementation: `src/app/state/health.rs` (module root: shared constants, open/close/refresh panel, scope persistence via `load_health_scope()` / `save_health_scope()`, `AzufigHealthScope` struct), `src/app/state/health/god_files.rs` (scan, scope mode, modularize, module style selector, view_checked), `src/app/state/health/documentation.rs` (doc scanner, DH session spawning, toggle/view), `src/tui/input_health.rs` (uses `lookup_health_action()` → Action match; `HealthScopeMode` action handled as panel-level, not tab-specific), `src/tui/draw_health.rs` (panel rendering with tab bar, `Tab:tab` label top-left + `s:scope` label top-right in panel border, footer hints from `keybindings::health_god_files_hints()` / `health_docs_hints()`), `src/app/types.rs` (GodFileEntry, HealthPanel, HealthTab, DocEntry), `src/tui/keybindings.rs` (HEALTH_SHARED (9 entries, includes scope) + HEALTH_GOD_FILES (4 entries) + HEALTH_DOCS arrays, `lookup_health_action()`, hint generators, `Shift+H` in GLOBAL)

### Git Panel

Reuses the existing 3-pane layout (`Shift+G` toggles open/close, global keybinding) — each pane detects git mode (`app.git_actions_panel.is_some()`) and renders git-specific content instead of its normal content. Accessible from any pane (skipped in prompt mode, edit mode, terminal mode, filter, wizard). Uses standard Double/Plain border types with Git brand colors: GIT_ORANGE (`#F05032`) when focused, GIT_BROWN (`#A0522D`) when unfocused.

**Layout geometry (differs from normal mode):** Git mode uses a completely separate layout branch in `run.rs::ui()`. Normal mode has the input spanning only the left two columns with the session pane extending full height on the right. Git mode uses a 3-zone vertical layout: a **1-row worktree tab bar** at the top, the **3-column panes** (Min 4 rows) in the middle, and a **full-width git status box** (3 rows) at the bottom. The git tab bar uses the exact same design as the normal tab bar (`draw_worktree_tabs`) — ★ main tab, status symbols (●/◇), archived styling, pagination, hit-test regions for mouse clicks — but with the git color palette: active worktree tab: `GIT_ORANGE` bg + white fg + bold; active ★ main tab: `GIT_ORANGE` bg + black fg + bold; inactive: `GIT_BROWN` fg; separators: `GIT_BROWN │`. Clicking ★ main opens the main branch's git view (also reachable via Shift+M). `[`/`]` cycling skips main, matching normal tab row behavior. `draw_git_worktree_tabs()` in `run.rs` renders it; `switch_git_panel_worktree()` in `input_git_actions.rs` handles single-tab cycling; `switch_git_panel_page()` handles page-level jumping.

**Pane mapping (git mode → normal pane):**

| Layout Zone | Git Mode Content | Notes |
|-------------|------------------|-------|
| Tab bar (top, 1 row) | Horizontal worktree tab strip (same design as normal tab row) | ★ main + all worktrees with status symbols; `GIT_ORANGE`/`GIT_BROWN` colors; clickable; `[`/`]` cycles worktrees (skips main, matching normal mode), `{`/`}` jumps pages; ★ main reachable via click or Shift+M |
| Sidebar (left) | Actions list (top) + Changed Files (bottom) — split vertically | "Actions" / "Changed Files (N, +X/-Y)" |
| Viewer (center) | File/commit diff with diff coloring | "Viewer" (or diff title) |
| Session (right) | Commit log | "Commits (N)" |
| Status box (bottom, 3 rows) | Full-width git status box with keybinding hints in title | " GIT  <footer hints>" |
| Status bar | Minimal: "Git: <worktree>" + CPU/PID badge | — |

Commit editor and conflict overlays render on top of the viewer pane from `run.rs::ui()` overlay section (not inline in the viewer). Uses GIT_ORANGE (#F05032) for overlay borders (commit editor, conflict resolver) and cursor highlights; GIT_BROWN (#A0522D) for diff header coloring and dim key hints.

- **Actions section** (sidebar top, auto-height): Context-aware git operations with single-key shortcuts, navigable with j/k. A `─` divider separates actions from toggles (auto-rebase, auto-resolve). Height computed dynamically from `git_actions_labels().len()` + extra rows (divider, auto-resolve, auto-rebase on feature) + 2 border. Focus: `panel.focused_pane == 0`.
- **Changed Files section** (sidebar bottom, fills remaining): Working tree changes with status chars, +/-N stats, underlined paths. Selecting a file auto-loads its diff in the viewer. Focus: `panel.focused_pane == 1`.
- **Viewer pane** (center): Shows file diffs or commit diffs with diff coloring (green additions, red deletions, cyan hunks, GIT_BROWN headers). Empty state shows "Select a file or commit to view its diff". Populates `viewer_lines_cache` so mouse drag selection, `⌘A` (select all), and `⌘C` (copy) work identically to the normal viewer — uses `viewer_scroll` for scrolling and `viewer_selection` for highlighting via `apply_selection_to_line()` with gutter=0. `⌘C` falls back to copying the status box `result_message` when no selection exists.
- **Commits pane** (right): Branch-scoped commit log — feature branches show only their own commits (`git log main..HEAD`), main shows full history. Unpushed commits show green, pushed show dim. `Git::get_commit_log()` uses `git rev-list --count @{u}..HEAD` for ahead count. Selecting a commit loads `git show <hash>` in the viewer. Auto-refreshes after commit/push operations. Focus: `panel.focused_pane == 2`.
- **Git status box** (full-width bottom): Title formatted via `split_title_hints()` — label `" GIT "` with keybinding hints in `(key:desc | key:desc)` format (same style as the prompt box). Content shows result messages (green=success, red=error). Always Double + GIT_ORANGE border.
- **Status bar**: Minimal — shows "Git: <worktree>" on left, CPU/PID badge on right.

**Context-Aware Actions (when actions section focused):**
Actions change based on whether the current worktree is the main/master branch or a feature branch. `is_on_main: bool` on `GitActionsPanel` determines which set is shown, set by comparing `worktree_name == main_branch` in `open_git_actions_panel()`.

*On main branch (5 actions):*
- `l` / Enter on index 0 — Pull (`exec_pull()`) — pulls latest changes from remote
- `c` / Enter on index 1 — Commit (see below)
- `Shift+P` / Enter on index 2 — Push to remote
- `z` / Enter on index 3 — Stash (`exec_stash()`) — `git stash push -u` (includes untracked)
- `Shift+Z` / Enter on index 4 — Stash pop (`exec_stash_pop()`) — `git stash pop`

*On feature branches (6 actions):*
- `m` / Enter on index 0 — Squash merge to main: runs on a **background thread** with progress phases shown via `loading_indicator`. Thread sends `SquashMergeProgress` updates through an `mpsc` channel (polled in event loop at 16ms). Phases: "Rebasing onto main..." → "Pushing rebased branch..." → "Merging into main..." → "Pushing to remote...". Final `SquashMergeOutcome` dispatches to success (PostMergeDialog), conflict (GitConflictOverlay), or error. Dirty-check runs synchronously before spawning. Pattern matches commit message generation (`GitCommitOverlay.generating` + `overlay.receiver`). RCR auto-continue path follows the same flow.
- `Shift+R` / Enter on index 1 — Rebase onto main (`exec_rebase()`) — manual rebase of feature branch onto main, then auto-pushes the rebased branch to its remote via `Git::push(&wt_path)`. On conflict, shows overlay with RCR option; after RCR acceptance the branch is also pushed.
- `c` / Enter on index 2 — Commit (see below)
- `Shift+P` / Enter on index 3 — Push to remote
- `z` / Enter on index 4 — Stash (`exec_stash()`) — `git stash push -u` (includes untracked)
- `Shift+Z` / Enter on index 5 — Stash pop (`exec_stash_pop()`) — `git stash pop`
- `r` — Refresh (re-fetches changed files and commit log; works on all pages including main)

**Post-action refresh:** Every git action outcome calls `refresh_changed_files()` + `refresh_commit_log()` — including error paths (rebase failed, squash merge failed), conflict paths (shows overlay but also refreshes data underneath), and "up to date" results. Switching worktree pages (`[`/`]`, `{`/`}`, `Shift+M`) calls `open_git_actions_panel()` which does a full rebuild from git, so navigated-to pages always show fresh data.

**Mutual exclusivity guards:** `lookup_git_actions_action()` takes `focused_pane: u8` (derives `actions_focused = focused_pane == 0` internally) and blocks `GitSquashMerge` and `GitRebase` when `is_on_main` is true (cannot squash-merge/rebase main into itself) and blocks `GitPull` when `is_on_main` is false (pull only available on main). Both also require `actions_focused`. `GitStash`, `GitStashPop`, `GitCommit`, `GitPush`, and `GitAutoResolveSettings` also require `actions_focused`.

**File list (when files pane focused, focused_pane==1):**
- Each file shows status char (M=yellow, A=green, D=red, R=cyan, ?=magenta untracked), path, right-aligned `+N/-N` stats (green for additions, red for deletions; orange override when row is selected). **Staged files** show underlined path with normal colors; **unstaged files** show strikethrough path in DarkGray with dimmed stats. Title shows total file count, staged count (e.g. `3✓`), and +/- stats.
- `j/k` — navigate files (auto-loads diff in viewer pane via `load_file_diff_inline()`); `Enter`/`d` — also loads diff inline
- `s` — toggle stage/unstage for selected file (purely UI — flips `staged` bool, no git commands). Same key as `GitAutoResolveSettings` but pane-gated: `s` fires auto-resolve in actions pane (focused_pane==0), toggle-stage in files pane (focused_pane==1)
- `Shift+S` — stage all / unstage all toggle (purely UI — flips all `staged` bools)
- `x` — discard changes for selected file. Shows inline confirmation (`Discard <path>? [y/n]`) in red bold, replacing the file's row. `y` confirms (`Git::discard_file` — `git restore` for tracked, `git clean -f` for untracked), `n`/`Esc` cancels. State: `panel.discard_confirm: Option<usize>` (file index)
- **Staging model:** All files default to `staged = true` on load. User unstages files they don't want committed. At commit time (`exec_commit_start`): if all files staged (default), `stage_all` for efficiency; if any unstaged, `unstage_all` first then `stage_file` for each staged file individually.
- Mouse wheel scrolls selection (moves `selected_file`, auto-loads diff)
- Scroll maintained via `file_scroll` field (written back from draw function each frame)
- `GitChangedFile.staged` defaults to `true` in `get_diff_files()` — staging is a UI concept, not read from git index
- **Bottom border** shows `s:stage | x:discard` hints via `git_files_pane_footer()` — styled with the same border color/modifier as the pane (orange+bold when focused, brown when not)

**Commits list (when commits pane focused, focused_pane==2):**
- Each commit shows hash (green=unpushed, gray=pushed) and subject line. Selected row highlighted in orange+bold.
- `j/k` — navigate commits (auto-loads `git show <hash>` in viewer via `load_commit_diff_inline()`); `Enter` — also loads diff inline
- Mouse wheel scrolls selection (moves `selected_commit`, auto-loads diff)
- `Git::get_commit_log(worktree_path, 200, main_branch)` loads commits on panel open — passes `Some(main_branch)` for feature branches (scopes to `main..HEAD`), `None` for main (full log). `refresh_commit_log()` called after all git operations (pull, push, commit, rebase, refresh); also refreshes all divergence counts
- **Bottom border divergence badges:** Compact right-aligned badges show `↑N ↓M main` (red bg when behind, green when only ahead; feature branches only) and `↑N ↓M remote` (yellow bg when behind, cyan when only ahead). Uses `Git::get_main_divergence()` and `Git::get_remote_divergence()` — both backed by `git rev-list --left-right --count`. Panel fields: `commits_behind_main`, `commits_ahead_main`, `commits_behind_remote`, `commits_ahead_remote`

**Global within panel:**
- `Tab` / `Shift+Tab` — cycle focus forward/backward: Actions → Files → Commits → Actions
- `[` / `]` — cycle to prev/next active worktree's git view without closing the panel; focused pane preserved; no-op with ≤1 active worktrees
- `{` / `}` — jump to prev/next tab bar page (first worktree on the target page becomes active); wraps around; no-op with ≤1 pages
- `R` — refresh changed files and commit log
- `Shift+J` / `PageDown` — page down in diff viewer
- `Shift+K` / `PageUp` — page up in diff viewer
- `⌘A` — select all viewer content (sets `viewer_selection` spanning entire `viewer_lines_cache`)
- `⌘C` — copy viewer selection to clipboard; falls back to copying status box `result_message` when no selection exists
- `Esc` — close panel and return to normal layout

**Commit overlay (renders inline in viewer pane):**
Pressing `c` stages all changes (`git add -A` + gitignore guard), gets `git diff --staged` + `git diff --staged --stat`, and spawns a one-shot background thread to generate a conventional commit message. The backend is derived from the selected model (`gpt-*` → Codex via `codex exec --ephemeral`, else → Claude via `claude -p`). **Fallback:** if the primary backend fails (e.g. usage limit hit), the thread automatically retries with the alternate backend and shows a fallback notice in `panel.result_message` (non-error). If both fail, the overlay closes with a combined error message. While generating (~3 sec), the viewer pane shows "Generating..." with a spinner. The commit editor fills the viewer pane area with the generated message in an editable text area (no longer a centered sub-dialog). `Enter` commits (deferred with "Committing..." loading indicator), `⌘P` commits + pushes (deferred with "Committing and pushing..." loading indicator), `Shift+Enter` inserts a newline, `Esc` cancels. Both commit actions use the `DeferredAction` two-phase pattern so the loading popup renders before the blocking git operation runs. Full text editing with word-wrap: type, backspace, delete, left/right arrows, up/down line navigation, home/end. Session persistence is disabled via `--no-session-persistence` so no .jsonl file is created. No streaming occurs — uses `std::process::Command` stdout capture. Markdown code fences are stripped from the output. State managed by `GitCommitOverlay` struct on `GitActionsPanel` (`commit_overlay: Option<GitCommitOverlay>`). Action count is context-dependent: `action_count(is_on_main)` returns 5 for main, 6 for feature branches. Confirm-index mapping: main=[0=pull, 1=commit, 2=push, 3=stash, 4=stash pop], feature=[0=squash-merge, 1=rebase, 2=commit, 3=push, 4=stash, 5=stash pop]. Commit message receiver polled in event loop with short-poll (250ms) while generating.

**Data flow:** On open, `open_git_actions_panel()` reads `current_worktree().worktree_path` and calls `Git::get_diff_files()` which combines `git diff HEAD --name-status` + `git diff HEAD --numstat` (working tree vs last commit) plus `git ls-files --others --exclude-standard` (untracked files), then filters all paths through `git check-ignore --stdin` to drop tracked-but-gitignored files (e.g., `.DS_Store`). Also calls `Git::get_commit_log(&wt_path, 200, main_branch)` to populate the commits pane (feature branches pass `Some(main_branch)` to scope to branch-only commits). Panel stores `worktree_path`, `repo_root` (project path, always on main), and `main_branch` locally to avoid reborrow conflicts during input handling. After operations that modify the working tree, `refresh_changed_files()` re-scans. After commit/push operations, `refresh_commit_log()` re-fetches the commit log.

**Data flow (rebase-before-merge):** `exec_squash_merge()` validates dirty state synchronously, then spawns a `std::thread::spawn` background thread. The thread sends `SquashMergeProgress { phase: String, outcome: Option<SquashMergeOutcome> }` updates via `mpsc::Sender`. The receiver is stored on `panel.squash_merge_receiver` and polled in the event loop at 16ms intervals. Phase updates set `app.loading_indicator`; final outcome dispatches to PostMergeDialog (success), GitConflictOverlay (conflict), or result_message (error). `git_action_in_progress()` returns true while `squash_merge_receiver.is_some()`, blocking quit. The thread first calls `exec_rebase_inner(&wt_path, &main_branch)` to rebase the feature branch onto main. Returns `RebaseOutcome` enum:
- `RebaseOutcome::Rebased` — rebase succeeded, proceed to merge
- `RebaseOutcome::UpToDate` — already up to date, proceed to merge
- `RebaseOutcome::Conflict { conflicted, auto_merged, raw_output }` — rebase left in progress (NOT aborted) so RCR can resolve. Opens conflict overlay.
- `RebaseOutcome::Failed(String)` — error, shows message

`exec_rebase_inner()` checks `merge-base HEAD <main>` vs `rev-parse <main>` for up-to-date detection. Before rebasing, stashes any dirty working tree on the feature branch (`git stash --include-untracked`) so files like `.DS_Store` or editor swap files don't cause "cannot rebase: You have unstaged changes". Stash is popped on all exit paths (success, failure, conflict). Then runs `git rebase --onto <main> <fork-point>` (where fork-point is the merge-base). The `--onto` form replays only branch-specific commits, preventing squash merge commits from other branches (inherited via prior rebases) from being replayed. Falls back to plain `git rebase <main>` if merge-base unavailable.

**Configurable auto-resolve via union merge:** On conflict, `parse_conflict_files()` extracts conflicted file paths. If ALL conflicted files are in the user-configurable auto-resolve list, `try_auto_resolve_conflicts()` resolves each via `union_merge_file()` — extracts 3 index stages (`:1:` base, `:2:` ours/main, `:3:` theirs/branch), runs `git merge-file --union` to produce a merged result keeping BOTH sides' changes (no conflict markers, no content loss), copies the result back to the working tree, and stages with `git add`. Loops through subsequent commits that also have auto-resolvable-only conflicts via `git rebase --continue`. Only stops for non-auto-resolve conflicts (hands off to RCR) or fatal errors.

The auto-resolve file list defaults to AGENTS.md, CHANGELOG.md, README.md, CLAUDE.md — loaded from azufig.toml `[git]` section with `auto-resolve/<filename> = "true"` keys via `azufig::load_auto_resolve_files()`. Users configure this interactively via the `[s] Auto-resolve files` overlay in the Git panel (press `s`): checklist with j/k navigation, Space to toggle, `a` to add new files, `d` to remove, Esc to save. Changes persisted to project azufig.toml immediately on close.

Non-auto-resolve conflicts: leaves rebase in progress (no auto-abort) so the conflict overlay can offer RCR resolution. Conflict parsing: `rsplit("Merge conflict in ")` from combined stdout+stderr, falls back to `Git::get_conflicted_files()` if parsing fails.

**Data flow (squash-merge-to-main after rebase):** After a successful rebase, `exec_squash_merge()` calls `Git::squash_merge_into_main(repo_root, branch)`. Returns `SquashMergeResult` enum:
- `SquashMergeResult::Success(String)` — clean merge, commit done, message returned
- `SquashMergeResult::Conflict { conflicted, auto_merged, raw_output }` — should rarely occur since rebase ensures linear history, but handled as safety net

Executes a multi-step cycle from the repo root:
0. Pre-flight: aborts stale merge/rebase state, removes stale `SQUASH_MSG`, resolves unmerged files via `git reset --hard HEAD` (catches ALL unmerged patterns: UU/AA/DD/AU/UA/DU/UD)
1. `git stash --include-untracked` — stashes any dirty working tree on main; checks both exit code and stdout to determine if stash occurred (prevents silent stash failures from leaving dirty state)
2. `git pull --ff-only` on main — non-fatal if offline, fatal if main has diverged
3. `git merge --squash <branch>` — collapses all branch commits into staged changes
4. `git log HEAD..branch --reverse --format="- %s"` — collects individual commit messages as bullet points (captured before step 3)
5. `git commit -m "feat: merge <branch> into main\n\n<commit log bullets>"` — commits with rich message preserving individual commit details, then pops stash. Checks BOTH stdout and stderr for "nothing to commit" (git writes this to stdout not stderr)

`get_main_branch()` dynamically detects main/master/HEAD. `exec_squash_merge()` blocks if the feature branch has uncommitted changes (must commit first). Push to remote is included in the squash merge flow (phase 4).

**Conflict resolution overlay (renders inline in viewer pane):**
When a rebase produces conflicts (either from manual `r` or pre-merge rebase in `exec_squash_merge()`), a `GitConflictOverlay` fills the viewer pane area with a red-bordered UI. The rebase is left in progress (NOT aborted) so RCR can resolve it. Contents:
- Red section: conflicted file list with count header (files with CONFLICT markers)
- Green section: auto-merged file list with count header (cleanly merged files)
- Two selectable action options with `▶` arrow indicator: `[y] Resolve with Claude` / `[n] Abort rebase`
- Footer hint bar: `j/k:navigate  Enter/y:resolve  n/Esc:abort`

Input handled by `handle_conflict_overlay()` (intercepted before commit overlay and normal panel dispatch):
- `j/k` or `↑/↓` — navigate between two options
- `Enter` or `y` — if selected=0: calls `spawn_conflict_claude()` to spawn a streaming Claude session
- `n` or `Esc` — calls `abort_rebase()` which runs `Git::rebase_abort()` on the worktree, pops the pre-rebase stash, calls `Git::cleanup_squash_merge_state()` on main, closes overlay, shows "Aborted" status

`spawn_conflict_claude()` follows the GFM/DH streaming session pattern (NOT one-shot):
1. Builds a rebase-specific prompt listing conflicted and auto-merged files with resolution instructions (read markers, edit files, `git add`, `git rebase --continue`, repeat if more conflicts, verify with `git status`)
2. `AgentProcess::spawn(wt_path, &prompt, None, selected_model)` — spawns interactive agent session in the feature branch worktree (where the rebase is happening), model and backend from `selected_model` (respects the user's model switcher choice)
3. `pending_session_names.push(("[RCR] <branch>", slot))` — names the session for display
4. `register_claude(branch, pid, rx)` — registers under the feature branch name so output appears in the current view immediately
5. Creates `RcrSession { branch, display_name, worktree_path, repo_root, slot_id, session_id: None, approval_pending: false, continue_with_merge }` → sets `app.rcr_session`
6. Sets `app.title_session_name = "[RCR] <display>"` (locked — `update_title_session_name()` early-returns during RCR)
7. Closes git panel, sets `focus = Focus::Session` so the user sees the session pane in RCR mode

**RCR (Rebase Conflict Resolution) mode:**
When `spawn_conflict_claude()` activates RCR, the session pane switches to green-themed borders and titles. The user can send follow-up prompts to Claude during/after resolution — prompts are routed to `rcr.worktree_path` (feature branch worktree where the rebase is in progress) with `--resume rcr.session_id`. Each follow-up spawns a new Claude process; `rcr.slot_id` is updated to the new PID. When the RCR Claude process exits, `handle_claude_exited()` intercepts: sets `rcr.approval_pending = true`, skips the normal re-parse (preserving streaming output), and returns early. A green-bordered approval dialog renders over the session pane:
- `y` / `Enter` — `accept_rcr()`: deletes session file from `~/.claude/projects/<worktree-encoded>/<session-id>.jsonl`, pops the pre-rebase stash on the worktree, clears RCR state, restores normal borders and title. If `continue_with_merge` is true (rebase was triggered by squash merge), also pops the main stash, auto-proceeds with `Git::squash_merge_into_main()` and shows PostMergeDialog on success. If false (manual rebase), just shows "Rebase complete — conflicts resolved for <branch>".
- `n` — aborts the rebase via `git rebase --abort` on the worktree, deletes session file, restores normal state
- `Esc` — dismisses dialog, status shows "Review the resolution, then press ⌃a to accept"
- `⌃a` — re-shows the approval dialog (available when RCR active, Claude not running, dialog not shown)

RCR state tracked by `RcrSession` struct on `App` (fields: `branch`, `display_name`, `worktree_path`, `repo_root`, `slot_id`, `session_id`, `approval_pending`, `continue_with_merge`). `continue_with_merge` flows from `GitConflictOverlay` through `spawn_conflict_claude()` to `RcrSession` — set true when the overlay was triggered by `exec_squash_merge()`, false for manual rebase. Session ID propagated via `set_claude_session_id()` when the RCR slot receives its session UUID.

Closing the git panel while conflict overlay is open auto-aborts the rebase via `Git::rebase_abort()` in `close_git_actions_panel()` and cleans up squash merge state on main via `Git::cleanup_squash_merge_state()` — no dirty rebase or merge state left behind.

`handle_git_actions_input()` takes `&ClaudeProcess` parameter (passed from `event_loop/actions.rs`) to enable conflict resolution spawning.

Implementation: `src/tui/input_git_actions.rs` (module root — dispatch via `lookup_git_action()` → Action match, `action_count(is_on_main)` returns 3/4, Cmd+C/A/scroll interception; takes `&ClaudeProcess` param; 5 submodules: `diff_viewer.rs` (`open_file_diff_inline()`, `load_file_diff_inline()`, `load_commit_diff_inline()`), `operations.rs` (`exec_squash_merge()` with rebase-before-merge, `exec_rebase()` + `exec_rebase_inner()`, `RebaseOutcome` enum, `union_merge_file()` for 3-way union merge, `try_auto_resolve_conflicts()`, `parse_conflict_files()`, `exec_commit_start()`, `exec_pull()`, `exec_push()`, `refresh_changed_files()`, `refresh_commit_log()`), `commit_overlay.rs` (`handle_commit_overlay()` — text editing, Enter to commit, ⌘P to commit+push), `conflict_resolution.rs` (`handle_conflict_overlay()`, `spawn_conflict_claude()`, `abort_rebase()`), `auto_resolve_overlay.rs` (`handle_auto_resolve_overlay()` — j/k nav, Space toggle, a add, d remove, Esc save+close); re-exports: `exec_rebase_inner`, `RebaseOutcome`, `refresh_changed_files`, `refresh_commit_log`; confirm index mapping: main=[0=pull, 1=commit, 2=push], feature=[0=squash-merge, 1=rebase, 2=commit, 3=push]), `src/tui/draw_git_actions.rs` (overlay renderers only: `draw_commit_editor()` renders inline in viewer area, `draw_conflict_inline()` renders inline in viewer area, `draw_auto_resolve_overlay()` renders inline in viewer area with GIT_ORANGE Double border; git pane content rendered by existing draw functions — `draw_sidebar.rs` for Actions+Files (includes `[s] Auto-resolve (N)` label), `draw_viewer.rs` for diffs, `draw_output.rs` for Commits; `draw_git_status_box()` in `run/overlays.rs` for the full-width status box; labels from `keybindings::git_actions_labels(is_on_main)`, footer from `keybindings::git_actions_footer()`), `src/tui/draw_output.rs` (green border override when `rcr_session.is_some()`, green center title color, `draw_rcr_approval()` dialog), `src/tui/run.rs` (git mode layout branch in `ui()`, delegates to `run/overlays.rs` and `run/worktree_tabs.rs` — separate layout geometry with full-width status box, `draw_git_status_box()` function; RCR approval dialog render after `draw_output`; auto-resolve overlay render after conflict overlay), `src/app/state/ui.rs` (open/close methods, `is_on_main` set from `worktree_name == main_branch`, initializes `focused_pane: 0`, `commits`, `selected_commit: 0`, `commit_scroll: 0`, `viewer_diff: None`, `viewer_diff_title: None`, `auto_resolve_files` from azufig, `auto_resolve_overlay: None` in panel constructor; `close_git_actions_panel()` auto-aborts rebase + cleans up squash merge state if conflict overlay open), `src/app/state/claude.rs` (RCR session ID tracking in `set_claude_session_id()`, RCR exit intercept in `handle_claude_exited()` — sets `approval_pending`, skips re-parse, returns early), `src/app/state/load.rs` (title guard: `update_title_session_name()` early-returns when `rcr_session.is_some()`; startup cleanup: `load()` untracks gitignored files + auto-aborts orphaned rebase state on all worktrees), `src/tui/input_terminal.rs` (RCR prompt routing: uses `rcr.worktree_path` as cwd, `rcr.session_id` for `--resume`, updates `rcr.slot_id`), `src/tui/event_loop/actions.rs` (RCR approval dialog intercept before modal checks: y/Enter → `accept_rcr()`, n → abort rebase + dismiss; `⌃a` re-shows dialog; `accept_rcr()` helper deletes session file + clears state, pops orphaned stash before re-calling squash merge when `continue_with_merge` is true; post-merge dialog archive/delete handlers clean up `auto-rebase` config keys; passes `claude_process` to `handle_git_actions_input`; `DeferredAction::GitCommit` and `GitCommitAndPush` call `refresh_commit_log()` after completion), `src/git/core.rs` (methods: `get_diff_files`, `get_file_diff`, `get_commit_log` (ahead count via `git rev-list --count @{u}..HEAD`), `get_commit_diff` (`git show <hash> --stat --patch`), `squash_merge_into_main` (returns `SquashMergeResult`), `has_unmerged_files` (checks `git status --porcelain` for ALL unmerged patterns: UU/AA/DD/AU/UA/DU/UD via byte matching), `cleanup_squash_merge_state` (resets hard if unmerged, pops orphaned stash, removes SQUASH_MSG — called from `close_git_actions_panel()` and `abort_rebase()`), `stage_all` (calls `untrack_gitignored_files` after `git add -A`), `untrack_gitignored_files` (`git ls-files -i --exclude-standard` → `git rm --cached`; also called from `load()` on startup), `ensure_worktrees_gitignored` (idempotent: reads `.gitignore`, appends any missing entries from `REQUIRED_GITIGNORE` (`worktrees/`, `.azureal/sessions/`), stages + commits; called from `load()` so new projects get correct gitignore on first open), `get_staged_diff`, `get_staged_stat`, `commit`, `pull`, `push`; `SquashMergeResult` enum), `src/git/rebase.rs` (3 functions: `is_rebase_in_progress`, `get_conflicted_files`, `rebase_abort`), `src/app/types.rs` (GitActionsPanel with `repo_root`, `is_on_main`, `focused_pane: u8` (0=Actions, 1=Files, 2=Commits), `commits: Vec<GitCommit>`, `selected_commit`, `commit_scroll`, `viewer_diff: Option<String>`, `viewer_diff_title: Option<String>`, `commit_overlay`, `conflict_overlay`, `auto_resolve_files: Vec<String>`, `auto_resolve_overlay: Option<AutoResolveOverlay>` fields; AutoResolveOverlay with `files: Vec<(String, bool)>`, `selected`, `adding`, `input_buffer`, `input_cursor`; GitCommit with `hash`, `full_hash`, `subject`, `is_pushed`; GitChangedFile, GitCommitOverlay, GitConflictOverlay with `continue_with_merge`, RcrSession with `worktree_path` and `continue_with_merge`), `src/tui/keybindings.rs` (GIT_ACTIONS array (21 entries), `lookup_git_actions_action()` takes `focused_pane: u8` + `is_on_main` params with mutual exclusivity guards for `GitSquashMerge`/`GitRebase`/`GitAutoRebase`/`GitAutoResolveSettings`, `git_actions_labels()` takes `is_on_main` param returning 3 or 4 actions, hint generators, `GitPull`/`GitPush`/`GitRebase`/`GitAutoRebase`/`GitAutoResolveSettings` actions, `l`/`r`/`a`/`s`/`Shift+P` bindings, `G` in GLOBAL), `src/azufig.rs` (`load_auto_resolve_files()` loads from `auto-resolve/<filename> = "true"` keys with default list, `save_auto_resolve_files()` clears and rewrites keys), `src/tui/event_loop.rs` (polls commit message receiver, short-poll when generating, `check_auto_rebase()` every 2 seconds with dirty worktree guard — loads `auto_resolve_files` from azufig for rebase)

### Rebase Support

Rebasing is integrated into the Git Actions panel as both a manual action (`r` key) and an automatic pre-merge step during squash merge. `src/git/rebase.rs` provides 3 functions: `is_rebase_in_progress()`, `get_conflicted_files()`, and `rebase_abort()`. The `pub(crate) exec_rebase_inner()` in `input_git_actions/operations.rs` handles the actual rebase execution with conflict detection, auto-resolve via union merge, and structured outcome reporting via the `pub(crate) RebaseOutcome` enum. Both are re-exported from the `input_git_actions` module root. Conflicts are resolved through the RCR flow (conflict overlay + Claude-assisted resolution).

**Push after rebase:** `Git::push()` detects diverged branches via `git rev-list --left-right --count HEAD...origin/<branch>` (ahead > 0 AND behind > 0). When diverged, skips `pull --rebase` (incompatible histories) and uses `--force-with-lease` instead of regular push. Status message appends "(force-pushed)" suffix. This handles the common case of pushing after a local rebase without manual intervention.

**Auto-rebase:** Press `a` in the Git panel (feature branches only) to toggle auto-rebase for that worktree. Persisted in `.azureal/azufig.toml` `[git]` section as `auto-rebase/<branch> = "true"`. In-memory state: `App.auto_rebase_enabled: HashSet<String>` loaded from `azufig::load_auto_rebase_branches()` on startup. Every 2 seconds, `check_auto_rebase()` in `event_loop.rs` iterates enabled worktrees, skipping any with: Claude running (`is_session_running`), active RCR, file being edited, git panel open **for that specific worktree** (other worktrees still rebase), or **dirty working tree** (`git status --porcelain` non-empty). Calls `exec_rebase_inner()` for ALL eligible worktrees in a single pass (not one-per-tick). On `Rebased`, auto-pushes via `Git::push()` and collects the branch name; on `Conflict`, switches to that worktree and opens the Git panel with conflict overlay (same RCR flow as manual rebase), then stops processing remaining trees; on `UpToDate`/`Failed`, silent. After the loop, all successfully rebased branches are shown in a single 3-second combined toast (`auto_rebase_success_until: Option<(Vec<String>, Instant)>`) with a titled border listing each branch (with `" → pushed"` suffix on success). The toast dialog uses `draw_auto_rebase_dialog(f, &[String], bool)` — one line per branch, positioned in the **top-right corner** (avoids overlapping the centered post-merge dialog), with green border on success. Auto-rebase is also triggered immediately (timer reset) after: successful pull on main (`git_polling.rs` detects "Pulled:" prefix in `GitResult`), squash merge success, and RCR completion with merge — ensuring worktrees rebase without waiting for the next 2-second tick. Sidebar shows colored `R` indicator per worktree: green (auto-rebase on, idle), orange (RCR active), blue (RCR approval pending), none (disabled). `accept_rcr()` and abort both set `sidebar_dirty = true` to update the `R` color. Config keys cleaned up on worktree archive/delete via post-merge dialog. Orphaned rebase state (`.git/rebase-merge/`) and detached HEADs repaired inside `load_worktrees()` itself (not just startup). Two cases: (1) **active rebase** (e.g. RCR in progress): reads branch name from `rebase-merge/head-name` or `rebase-apply/head-name` WITHOUT aborting — patches the `WorktreeInfo.branch` field so the worktree keeps its correct name; (2) **orphaned detached HEAD** (no rebase state): re-attaches via `git for-each-ref --points-at=HEAD` → `git checkout <branch>`, then re-fetches the worktree list. `handle_claude_output()` also has an RCR fallback: if the active `rcr_session.slot_id` matches the output slot, it displays regardless of `current_worktree().branch_name` matching — prevents output loss if the worktree's branch name is temporarily empty during rebase.

### Run Commands

User-defined shell commands that can be saved and executed from any pane. Commands are executed in the embedded terminal. Dual-scope persistence: commands can be **global** (`~/.azureal/azufig.toml` `[runcmds]`, shared across all projects) or **project-local** (`.azureal/azufig.toml` `[runcmds]`); toggle scope with `⌃s` in add/edit dialog; picker shows G/P badge per command.

**Keybindings (global — works from any pane):**
- `R` — Open picker (if multiple saved commands) or execute directly (if only 1)
- `⌥r` — Open dialog to create a new run command

**Picker overlay:**
- `j/k` / `↑/↓` — Navigate selection
- `1-9` — Quick-select by number
- `Enter` — Execute selected command
- `e` — Edit selected command
- `d` — Delete selected command (y/n confirmation)
- `a` — Add new command

**Dialog overlay:**
- `Tab` — In Name field: advance to Command/Prompt field. In Command/Prompt field: cycle between Command and Prompt modes.
- `⇧Tab` — Go back to Name field from Command/Prompt field
- `⌃s` — Toggle global/project scope (shown as [GLOBAL]/[PROJECT] badge in title bar)
- `Enter` — In Name field: advance. In Command mode: save. In Prompt mode: generate (spawns Claude session).
- `Esc` — Cancel

**Command vs Prompt mode:** The second field has a right-aligned title showing the current mode and Tab hint. In **Command** mode, user types a raw shell command directly. In **Prompt** mode, user types a natural-language description and Enter spawns a new Claude session on the current worktree that reads the description, determines the right shell command, and writes it to `.azureal/runcmds`. The session is named `[NewRunCmd] <name>` in `.azureal/sessions`. Run commands auto-reload when the `[NewRunCmd]` session exits (via `handle_claude_exited()` check on `title_session_name`).

**Storage:** Global commands in `~/.azureal/azufig.toml` `[runcmds]`, project-local in `.azureal/azufig.toml` `[runcmds]` — keys prefixed with 1-based position number: `N_name = "command"` (e.g., `1_Build = "cargo build"`). Prefix preserves quick-select number across restarts; stripped on load, re-written on save. Merged on load (globals first, then locals). Loaded on startup.

Implementation: Types in `src/app/types.rs` (RunCommand, RunCommandDialog, RunCommandPicker, CommandFieldMode), state methods in `src/app/state/ui.rs`, input handling + `spawn_run_command_prompt()` in `src/tui/input_dialogs.rs`, rendering in `src/tui/draw_dialogs.rs`, auto-reload in `src/app/state/claude.rs`

### Projects Panel

Persistent project management across azureal sessions. Projects are stored in `~/.azureal/azufig.toml` `[projects]` section (`DisplayName = "~/path"` pairs). Opened with `P` from Worktrees pane, or shown automatically on startup when not inside a git repo.

**Behavior:**
- When launched inside a git repo, auto-registers the repo in `projects` and loads normally. Display name derived from `git remote get-url origin` (repo name from SSH/HTTPS URL, `.git` stripped); folder name fallback if no remote. `Project::from_path()` reads display name via `project_display_name()` so title bar, sidebar, and terminal title all use it.
- When launched outside a git repo, shows the Projects panel full-screen with "Project not initialized. Press i to initialize or choose another project." message; clears on first keypress
- The sidebar no longer shows a project header row — project name appears in the Worktrees pane border title instead

**Panel Actions:**
- `Enter`: switch to selected project (validates git repo first — shows error if not a valid repo; kills all Claude processes on success, reloads sessions/files)
- `a`: add a new project by path (validates it's a git repo)
- `d`: delete selected project from list (does NOT delete the repo)
- `n`: rename the selected project's display name
- `i`: initialize a new git repo at a specified path (or cwd if blank); rejects paths that are already git repos
- `Esc`: close panel (only if a project is already loaded)
- `⌃Q`: quit azureal

**Project Switching (Parallel Projects):**
Projects run in parallel — switching preserves all state. When switching away, the current project's state is saved to a `ProjectSnapshot` (display_events, worktrees, file tree, viewer tabs, branch→slot mappings, unread sessions, terminals, run commands, presets). `save_current_terminal()` is called before snapshot creation to ensure the active worktree's shell session is captured. Claude processes continue running in background; the event loop drains all `agent_receivers` regardless of active project. Output events for non-active projects are silently dropped (session file captures them). When switching back, the snapshot is restored (including `display_events`) and `load_session_output()` rebuilds the display — live sessions pick up the restored events via `saved_display_events`, historic sessions load from SQLite. Stale slot entries (processes that exited while backgrounded) are cleaned up on restore.

**Background Process Exit Handling:**
`handle_claude_exited()` checks `slot_to_project` to determine if the exiting slot belongs to a background project. If so, `handle_background_exit()` updates the snapshot's `branch_slots`/`active_slot`, marks the session as unread in the snapshot, and shows a status message prefixed with the project name. Global maps (`running_sessions`, `agent_exit_codes`, `agent_session_ids`) are always updated regardless of active project.

**Activity Status Icons:**
The projects panel shows worktree activity status icons (same symbols as the worktree tab row) next to each project name. `project_status()` computes the aggregate status across all worktrees: for the active project it checks live worktree statuses; for background projects it checks saved snapshot worktrees against `running_sessions`. Priority: Running > Failed > Waiting > Pending > Completed > Stopped.

**Auto-pruning:** `load_projects()` validates every entry on load — directories that don't exist or aren't git repos are silently removed from `projects`. This prevents ghost entries after a repo's `.git` directory is deleted.

Implementation: `src/config.rs` (persistence: `load_projects()`, `save_projects()`, `register_project()`, `project_display_name()`, `repo_name_from_origin()`), `src/app/types.rs` (`ProjectsPanel`, `ProjectsPanelMode`), `src/tui/draw_projects.rs` (rendering), `src/tui/input_projects.rs` (key handling), `src/app/state/ui.rs` (`switch_project()`), `src/app/state/project_snapshot.rs` (`ProjectSnapshot`), `src/app/state/claude.rs` (`handle_background_exit()`), `src/app/state/app/queries.rs` (`project_status()`)

### Debug Dump

`⌃d` (global) opens a naming dialog for creating a debug dump — a snapshot of the current session's full state saved to `.azureal/debug-output-{name}.txt`. The dialog is a simple centered text input; Enter saves, Esc cancels.

**Flow:** `⌃d` → `Action::DumpDebug` → sets `app.debug_dump_naming = Some(String::new())` → dialog handler captures text input → Enter sets `app.debug_dump_saving = Some(name)` → event loop calls `app.dump_debug_output(&name)` → status bar shows confirmation.

**App State:** `debug_dump_naming: Option<String>` (naming dialog active when `Some`), `debug_dump_saving: Option<String>` (triggers dump on next frame).

Implementation: `src/app/state/load.rs` (`dump_debug_output()`), `src/tui/run/overlays.rs` (`draw_debug_dump_naming()`, `draw_debug_dump_saving()`), `src/tui/event_loop/actions.rs` (dialog handler), `src/tui/event_loop/actions/execute.rs` (`Action::DumpDebug`), `src/tui/event_loop.rs` (deferred saving)

### Creation Wizard

Unified "New..." dialog (`n` from Worktrees) with tabs for creating resources:

**Tabs:**
1. **Project** (placeholder) - future project creation
2. **Branch** (placeholder) - future branch creation
3. **Worktree** - create git worktree with Claude session
   - Name: becomes `{BRANCH_PREFIX}/{name}` branch
   - Prompt: initial message to Claude
4. **Session** - create new Claude session in existing worktree
   - Name (optional): custom name stored in `index.json`
   - Prompt: initial message to Claude
   - Worktree: select target from list

**Session Name Storage:**
SQLite store only — S-numbered sessions. Name defaults to "S{id}" (e.g. "S1", "S2"), overridden by `rename_session()`. Keyed by integer ID as string.

Implementation: `src/wizard.rs` (wizard state), `src/tui/draw_wizard.rs` (rendering), `src/tui/input_wizard.rs` (input handling), `src/app/state/session_names.rs` (name storage — SQLite store only)

### Completion Notifications

Cross-platform notification sent when any agent instance finishes its response. Fires for every session exit (not just the currently viewed one), so the user sees alerts even when working in another app.

**Notification format:**
- Title: `worktree:session_name`
- Body: "Compacting context" (mid-turn compaction), "Response complete" (exit 0), "Exited with error" (non-zero), or "Process terminated" (signal)
- Session name uses custom name from `sessions` if set, otherwise first 8 chars of UUID
- Branded Azureal icon on all platforms (not Finder/Terminal/generic)

**Platform-specific notification setup:**

*macOS:*
- Uses `notify-rust` crate with `set_application("com.xcorvisx.azureal")` for branded icon
- `.app` bundle auto-created at `~/.azureal/AZUREAL.app` on first launch — zero manual setup
- `.icns` icon embedded in binary via `include_bytes!()` and extracted to bundle on first run
- Binary copied into bundle (`Contents/MacOS/azureal`) — NOT symlinked, because `proc_pidpath()` resolves symlinks and Activity Monitor needs the real path inside the `.app` to show the custom icon
- On startup, process re-execs through the bundle copy via `Command::exec()` so `proc_pidpath()` returns the bundle path
- `TransformProcessType(psn, kProcessTransformToUIElementAppType)` registers the process with the macOS window server — without this, `NSRunningApplication` returns nil and Activity Monitor shows a generic icon despite correct `proc_pidpath()`
- `AZUREAL_REEXEC` env var prevents infinite re-exec loop; `already_in_bundle` check provides secondary guard
- Bundle ad-hoc codesigned after binary copy (source has linker ad-hoc signature that fails validation inside a `.app` bundle)
- Bundle registered with macOS Launch Services via `lsregister` on creation/update
- Activity Monitor shows "AZUREAL" as process name with branded icon
- Notification permissions auto-enabled on first launch by writing `ALLOW_NOTIFICATIONS|BANNERS|SOUND|BADGE|PREVIEW_ALWAYS` flags to `~/Library/Preferences/com.apple.ncprefs.plist` via Python's `plistlib` (the only reliable way to edit macOS binary plists). Marker file `~/.azureal/.notif_enabled` prevents overriding user's preference on subsequent launches
- `.sound_name("Glass")` for macOS notification sound (platform-gated via `#[cfg(target_os = "macos")]`)

*Windows:*
- `.ico` file (6 sizes: 256/128/64/48/32/16) embedded in binary via `include_bytes!()` and extracted to `~/.azureal/Azureal.ico` on startup; `Azureal_toast.png` also embedded via `include_bytes!()` and extracted to `~/.azureal/Azureal_toast.png` on first run — ensures the toast icon is always present without manual copying
- `build.rs` uses `winres` crate to embed the `.ico` as a Win32 resource — Explorer, pinned taskbar, and Alt+Tab show the icon for the `.exe` file itself
- At startup, writes a Windows Terminal profile fragment at `%LOCALAPPDATA%\Microsoft\Windows Terminal\Fragments\Azureal\azureal.json` — registers an "Azureal" profile with `Azureal_toast.png` (PNG, not `.ico`) as the tab icon for crisper rendering. Rewritten on every startup (not just when missing) so icon/exe path changes propagate automatically. (`GetConsoleWindow()` returns null in WT because ConPTY uses a hidden pseudo-console with no window handle.)
- Notifications shell out to PowerShell using WinRT toast APIs — `notify-rust`'s `.app_id("AZUREAL")` silently drops toasts because the AUMID isn't registered via a Start Menu shortcut. PowerShell's own AUMID (`{1AC14E77-02E7-4E5D-B744-2EB1AE5198B7}\WindowsPowerShell\v1.0\powershell.exe`) is always registered, so toasts reliably appear. `CREATE_NO_WINDOW` (0x08000000) prevents console flash.
- Toast XML uses `appLogoOverride` image placement with `~/.azureal/Azureal_toast.png` for a crisp branded icon — the `.ico` file renders blurry in Windows toasts, PNG renders crisply
- Windows uses its own default notification sound (no `.sound_name()`)

**Common details:**
- Binary mtime comparison detects when source binary changed (e.g., after `cargo install`) and re-copies (macOS bundle)
- Notification runs in a fire-and-forget background thread (never blocks event loop)
- Called from `handle_claude_exited()` BEFORE state cleanup (needs session info still available)
- For current session: uses cached `title_session_name`; for background sessions: looks up from `session_files` + `index.json` display names

Implementation: `src/app/state/claude/process_lifecycle.rs` (`send_completion_notification()`), `src/main.rs` (macOS bundle creation + re-exec, Windows ico extraction), `build.rs` (Windows icon embedding via winres)

# MANIFEST

```
azureal/
├── .azureal/                # Project-level azureal data (tracked in git)
│   ├── azufig.toml         # Project-local unified config (TOML): filetree options, sessions, healthscope (alias: godfilescope), local runcmds, local presetprompts
│   └── sessions.azs        # SQLite session store (.azs = obscure extension, internally standard SQLite with DELETE journal mode) — portable sessions with S-numbering
├── .claude/                 # Project-level Claude Code config
│   ├── settings.json        # Hook configuration (PreToolUse keybinding enforcement)
│   └── scripts/
│       └── enforce-keybindings.sh  # Catches raw KeyCode in input_*.rs, hardcoded labels in draw_*.rs, new arrays without companions in keybindings.rs
├── .project/               # Project management files
│   ├── edits/              # Edit history
│   │   └── edits.md        # Current edit log
│   └── fix.md              # Bug queue
├── refs/                   # Reference files
├── src/
│   ├── app.rs              # Module root (re-exports only)
│   ├── app/                # Application state module
│   │   ├── state.rs        # State module root (re-exports only)
│   │   ├── state/          # State submodules
│   │   │   ├── app.rs      # App module root: struct definition + new() + cache invalidation; declares submodules
│   │   │   ├── app/        # App submodules (file-based module root)
│   │   │   │   ├── cpu.rs       # CPU usage monitoring (get_cpu_time_micros + update_cpu_usage)
│   │   │   │   ├── deferred.rs  # DeferredAction enum (two-phase draw pattern)
│   │   │   │   ├── model.rs     # Backend-aware model selection (cycle_model, display_model_name, update_token_badge, last_session_model, model_alias_from_init) — injects ModelSwitch tags to session store
│   │   │   │   ├── queries.rs   # Session status queries + project/worktree accessors (current_worktree, is_session_running, set_status, open_table_popup)
│   │   │   │   ├── stt.rs       # Speech-to-text integration (toggle_stt, poll_stt, insert_stt_text)
│   │   │   │   └── todo.rs      # TodoItem + TodoStatus types from Claude's TodoWrite tool call
│   │   │   ├── load.rs     # Load module root: declares submodules, re-exports compute_worktree_refresh
│   │   │   ├── load/      # Load submodules (file-based module root)
│   │   │   │   ├── worktree_refresh.rs  # Worktree discovery, project loading, file tree init; main stored in main_worktree (not worktrees vec)
│   │   │   │   ├── session_output.rs    # Session content loading/switching, viewed_session_id, extract_skill_tools_from_events, format_uuid_short
│   │   │   │   ├── session_file.rs      # Session file monitoring: check_session_file, poll_session_file, refresh_session_events, sync_file_watches
│   │   │   │   └── debug_dump.rs        # Debug output with content obfuscation for bug reports (dump_debug_output)
│   │   │   ├── sessions.rs # Worktree navigation, session file selection, archive. start_new_session() creates session in SQLite store
│   │   │   ├── output.rs   # Session output processing
│   │   │   ├── scroll.rs   # Scroll operations
│   │   │   ├── claude.rs   # Claude module root: submodule declarations, parse_todos_from_input() free function, tests
│   │   │   ├── claude/    # Claude session handling submodules (file-based module root)
│   │   │   │   ├── event_handling.rs     # Live event processing: apply_parsed_output(), handle_claude_output(), is_viewing_slot(), apply_slot_turn_duration() — tool call tracking, todo state, compaction counters
│   │   │   │   ├── process_lifecycle.rs  # Process lifecycle: register_claude(), handle_claude_started/exited(), cancel_current_claude(), handle_background_exit(), send_completion_notification(), set/get_claude_session_id()
│   │   │   │   └── store_ops.rs          # Store persistence: store_append_from_display(), store_append_from_jsonl(), store_append_background(), parse_jsonl_for_store(), tool_status_from_events()
│   │   │   ├── file_browser.rs # File tree and viewer
│   │   │   ├── ui.rs       # Focus, dialogs, menus, wizard, enter_main_browse/exit_main_browse (clears in switch_project). switch_project() saves/restores session_store + pid_session_target + current_session_id via ProjectSnapshot
│   │   │   ├── viewer_edit.rs # Viewer edit mode: wrap-aware cursor, mouse click/drag, clipboard
│   │   │   ├── project_snapshot.rs # Per-project state snapshot for parallel switching — display_events, pid_session_target stores (session_id, worktree_path) tuples, current_session_id
│   │   │   ├── session_names.rs # Custom session name storage — store-only (SQLite S-numbered sessions), save_session_name()/load_all_session_names()
│   │   │   ├── health.rs    # Health module root: shared constants (SOURCE_EXTENSIONS, SKIP_DIRS, SOURCE_ROOTS), scope persistence (load_health_scope/save_health_scope, AzufigHealthScope), open/close panel, current_worktree_info(), health_scan_root() (worktree-aware scan root), translate_scope_dirs() (project→worktree path translation)
│   │   │   ├── health/     # Health submodules (file-based module root)
│   │   │   │   ├── god_files.rs     # God File System: scan, scope mode, parallel modularize, module style selector
│   │   │   │   └── documentation.rs # Doc coverage scanner, DH session spawning, doc toggle/view
│   │   │   └── helpers.rs  # Utility functions
│   │   ├── session_store.rs # SQLite-backed session store (.azs) — SessionStore wrapping rusqlite::Connection with DELETE journal mode, S-numbered sessions, event append/load, boundary-based compaction (compaction_boundary + load_events_range), context building for session resume, completion persistence (mark_completed). Tables: sessions (with completed/duration_ms/cost_usd columns), events, compactions, meta. Key types: SessionInfo, CompactionInfo, ContextPayload (pub(crate))
│   │   ├── context_injection.rs # Context injection for session resumption — builds conversation transcripts from stored events, wraps in <azureal-session-context> tags, strips injected context from parsed results. Key fns: build_context_prompt(), strip_injected_context(), build_transcript() (pub(crate))
│   │   ├── session_parser.rs # Claude session file parsing (pub(crate))
│   │   ├── codex_session_parser.rs # Codex session file parsing (pub(crate))
│   │   ├── terminal.rs     # PTY terminal management
│   │   ├── types.rs        # Enums (Focus, ViewMode, FileTreeAction, ProjectsPanel, GitActionsPanel with is_on_main + squash_merge_receiver + discard_confirm, GitChangedFile with staged field, GitCommitOverlay, GitConflictOverlay, RcrSession, PostMergeDialog, SquashMergeProgress, SquashMergeOutcome, WorktreeRefreshResult, dialogs)
│   │   ├── input.rs        # Input handling methods
│   │   └── util.rs         # ANSI stripping, JSON parsing
│   ├── tui.rs              # Module root (re-exports only)
│   ├── tui/                # Terminal UI module
│   │   ├── run.rs          # TUI module root: entry point (run()), main layout (ui()), submodule declarations
│   │   ├── run/            # Run submodules
│   │   │   ├── splash.rs       # Splash screen ASCII art rendering (butterfly + logo + acronym)
│   │   │   ├── worktree_tabs.rs # Worktree tab bar rendering (normal + git mode), pagination, hit-test regions
│   │   │   └── overlays.rs     # Popup overlays: auto-rebase, git status box, debug dump, loading indicator
│   │   ├── event_loop.rs   # Event loop module root (run_app + submodule declarations)
│   │   ├── event_loop/     # Event loop submodules
│   │   │   ├── actions.rs  # Key dispatch module root (handle_key_event, modal interception, re-exports)
│   │   │   ├── actions/    # Action dispatch submodules
│   │   │   │   ├── execute.rs      # execute_action (main Action→handler match), start_or_resume, jump_edit
│   │   │   │   ├── navigation.rs   # Focus-aware nav dispatch (down/up/left/right/page/top/bottom)
│   │   │   │   ├── escape.rs       # Context-dependent escape dispatch
│   │   │   │   ├── session_list.rs # Session list overlay open, finish load, JSONL message counting
│   │   │   │   ├── deferred.rs     # Deferred action execution (post-loading-indicator dispatch)
│   │   │   │   └── rcr.rs          # RCR acceptance (cleanup, merge continuation)
│   │   │   ├── agent_events.rs # Agent process event handling + staged prompt (Claude + Codex dispatch), compaction agent spawning with cross-backend fallback (spawn_compaction_agent), compaction polling with retry (poll_compaction_agents), CompactionJob struct, streaming JSON assistant text extraction (extract_assistant_text)
│   │   │   ├── agent_processor.rs # Background JSON parsing thread for agent streaming events
│   │   │   ├── auto_rebase.rs # Periodic auto-rebase checking for enabled worktrees
│   │   │   ├── coords.rs   # Screen-to-content coordinate mapping
│   │   │   ├── fast_draw.rs # Fast-path input (~0.1ms) + session rendering (~2-5ms direct cell writes)
│   │   │   ├── git_polling.rs # Background git operation polling (commit gen, squash merge, ops, rebase)
│   │   │   ├── housekeeping.rs # File watcher, session/tree/health refresh, STT, debug dump
│   │   │   ├── input_thread.rs # Dedicated stdin reader thread
│   │   │   ├── mouse.rs    # Click, drag, scroll, selection copy
│   │   │   ├── process_input.rs # Input event dispatch (key, mouse, resize routing)
│   │   │   └── prompt.rs   # Staged prompt sending and compaction lifecycle
│   │   ├── util.rs         # Display utilities (re-exports)
│   │   ├── colorize.rs     # Output colorization
│   │   ├── markdown.rs     # Markdown parsing
│   │   ├── render_markdown.rs # Markdown rendering (tables, headers, lists, quotes, code blocks) + viewer markdown
│   │   ├── render_events.rs # DisplayEvent rendering module root (thin orchestrator + submodule declarations)
│   │   ├── render_events/ # DisplayEvent rendering submodules (file-based module root)
│   │   │   ├── bubbles.rs     # User/assistant message bubbles and completion banners
│   │   │   ├── dialogs.rs     # Plan approval and user question prompts
│   │   │   ├── plan.rs        # Plan mode full-width display
│   │   │   ├── system.rs      # Session init, hooks, and commands
│   │   │   └── tool_call.rs   # Tool invocation rendering with status indicators
│   │   ├── render_thread.rs # Background render thread (PreScanState, RenderRequest/Result, sequence numbers)
│   │   ├── render_tools.rs # Tool result rendering (module root — re-exports from submodules)
│   │   │   ├── tool_params.rs  # Tool parameter extraction, display names, truncation
│   │   │   ├── tool_result.rs  # Tool result summarization rendering + write preview
│   │   │   ├── diff_parse.rs   # Diff/patch parsing types and extraction
│   │   │   └── diff_render.rs  # Diff rendering (edit, apply-patch, unified-diff)
│   │   ├── render_wrap.rs  # Text/span wrapping utilities
│   │   ├── draw_projects.rs # Projects panel modal (full-screen project selection/management)
│   │   ├── draw_sidebar.rs # Git panel sidebar (Actions + Changed Files) + FileTree overlay delegate
│   │   ├── file_icons.rs  # File tree icon mapping — Nerd Font glyphs (primary) + emoji fallback
│   │   ├── draw_file_tree.rs # FileTree pane rendering (always visible in left column)
│   │   ├── draw_viewer.rs  # Viewer pane module root (re-exports + main draw_viewer fn)
│   │   ├── draw_viewer/    # Viewer pane submodules
│   │   │   ├── wrapping.rs     # Text wrapping utilities (word_wrap_breaks, wrap_spans_word)
│   │   │   ├── selection.rs    # Selection highlighting (apply_selection_to_line, apply_selection_to_spans)
│   │   │   ├── edit_mode.rs    # Edit mode rendering (cursor, syntax hl, dashed border)
│   │   │   ├── dialogs.rs      # Save/discard confirmation dialogs
│   │   │   ├── tabs.rs         # Tab bar rendering and tab picker dialog
│   │   │   └── git_viewer.rs   # Git panel diff viewer (draw_git_viewer_selectable)
│   │   ├── draw_output.rs  # Session pane module root (re-exports + main draw_output fn)
│   │   ├── draw_output/    # Session pane submodules
│   │   │   ├── render_submit.rs  # Background render thread submit/poll (submit_render_request, poll_render_result)
│   │   │   ├── session_list.rs   # Session list overlay (filter, content search, name list)
│   │   │   ├── todo_widget.rs    # Sticky todo/tasks widget at bottom of session pane (20-line cap, scrollbar, mouse wheel)
│   │   │   ├── selection.rs      # Selectable content range calculation for cache lines
│   │   │   ├── dialogs.rs        # Session pane dialog overlays (new session, RCR approval, post-merge)
│   │   │   ├── git_commits.rs    # Git panel commit log rendering with divergence badges
│   │   │   ├── viewport.rs       # Viewport cache building with real-time overlays (tool status, selection, search)
│   │   │   └── session_chrome.rs # Session pane border/block construction (focus/RCR styling, PID badge, model indicator)
│   │   ├── draw_health.rs   # Worktree Health panel modal (tabbed: God Files + Documentation)
│   │   ├── draw_git_actions.rs # Git panel overlay renderers only (commit editor + conflict resolution)
│   │   ├── draw_*.rs       # Other rendering functions
│   │   ├── keybindings.rs  # Module root — re-exports all public items for backwards compatibility
│   │   ├── keybindings/    # SINGLE SOURCE OF TRUTH for all keybinding definitions
│   │   │   ├── types.rs    # Core types: KeyCombo, Action (~109 variants incl CycleModel), Keybinding, HelpSection
│   │   │   ├── bindings.rs # ~17 static arrays (GLOBAL, WORKTREES, GIT_ACTIONS, etc.) + alt key statics
│   │   │   ├── lookup.rs   # KeyContext, lookup_action() + 6 per-modal lookups (lookup_git_actions_action, lookup_health_action, etc.)
│   │   │   ├── hints.rs    # UI hint generators: help_sections(), prompt/terminal/health/git/projects title builders, find_key_for_action()
│   │   │   └── platform.rs # macOS ⌥+letter unicode remapping (macos_opt_key)
│   │   ├── input_projects.rs # Projects panel input (browse, add, delete, rename, init)
│   │   ├── input_file_tree.rs # FileTree: clipboard mode + text-input actions only (commands resolved upstream)
│   │   ├── input_viewer.rs # Viewer: tab/save/discard dialogs + edit mode text editing (commands resolved upstream)
│   │   ├── input_output.rs # Session: session find + session list overlay only (commands resolved upstream)
│   │   ├── input_health.rs  # Worktree Health panel input (tab switching, per-tab keys)
│   │   ├── input_git_actions.rs # Git panel module root (dispatch + re-exports)
│   │   ├── input_git_actions/  # Git panel input submodules
│   │   │   ├── diff_viewer.rs          # File/commit diff loading into inline viewer
│   │   │   ├── operations.rs           # Git ops (pull, push, rebase, squash-merge, commit, refresh, RebaseOutcome, auto-resolve union merge)
│   │   │   ├── commit_overlay.rs       # Commit message editing overlay
│   │   │   ├── conflict_resolution.rs  # Conflict overlay + RCR Claude spawn
│   │   │   └── auto_resolve_overlay.rs # Auto-resolve file list settings overlay
│   │   └── input_*.rs      # Other input handlers
│   ├── events.rs           # Module root (re-exports only)
│   ├── events/             # Stream-JSON events module
│   │   ├── types.rs        # Raw Claude Code event types
│   │   ├── display.rs      # DisplayEvent enum
│   │   ├── parser.rs       # Claude EventParser + tests
│   │   └── codex_parser.rs # Codex streaming parser (CodexEventParser → DisplayEvent)
│   ├── git.rs              # Module root (re-exports only)
│   ├── git/                # Git operations module
│   │   ├── core.rs         # Git struct, SquashMergeResult enum, WorktreeInfo, repo detection, branch info, status
│   │   ├── branch.rs       # Branch management (list_local_branches, list_remote_branches_cached, get_main_branch, get_current_branch)
│   │   ├── commit.rs       # Commit creation and log queries (commit, get_commit_log, get_commit_diff)
│   │   ├── diff.rs         # Diff operations (get_diff_files, get_file_diff, get_staged_diff, get_staged_stat)
│   │   ├── merge.rs        # Squash-merge with conflict detection (squash_merge_into_main, cleanup_squash_merge_state)
│   │   ├── rebase.rs       # Rebase operations (is_rebase_in_progress, get_conflicted_files, rebase_abort)
│   │   ├── remote.rs       # Push, pull, divergence queries (pull, push, get_main_divergence)
│   │   ├── staging.rs      # Stage, unstage, discard, gitignore cleanup (stage_all, untrack_gitignored_files, ensure_worktrees_gitignored)
│   │   └── worktree.rs     # Worktree create/delete/list
│   ├── cmd.rs              # CLI command handler routing (file-based module root)
│   ├── cmd/                # CLI command handler submodules
│   │   ├── session.rs      # Session list/show commands
│   │   └── project.rs      # Project info command
│   ├── azufig.rs           # Unified config: GlobalAzufig + ProjectAzufig structs (HashMap-based flat sections), load/save/update helpers (TOML I/O with bare-key post-processing), auto-rebase helpers (set_auto_rebase, load_auto_rebase_branches)
│   ├── backend.rs          # Backend enum (Claude/Codex) + AgentProcess wrapper enum
│   ├── claude.rs           # Claude CLI process management
│   ├── codex.rs            # Codex CLI process management
│   ├── cli.rs              # CLI argument parsing (clap definitions)
│   ├── config.rs           # Configuration (permissions, API key), session discovery (Claude + Codex), projects persistence (reads from azufig)
│   ├── main.rs             # Entry point
│   ├── models.rs           # Domain models (Worktree, WorktreeStatus, Project, RebaseResult, OutputType, DiffInfo)
│   ├── stt.rs              # Speech-to-text engine (cpal + whisper-rs + background thread)
│   ├── syntax.rs           # Syntax highlighting (SyntaxHighlighter: tree-sitter-based highlighting for Viewer + Session code blocks)
│   ├── watcher.rs          # Filesystem watcher (notify crate — kqueue/inotify/ReadDirectoryChangesW)
│   └── wizard.rs           # Session creation wizard
├── worktrees/              # Git worktrees for sessions
├── .github/
│   └── workflows/
│       └── release.yml     # GitHub Actions: multi-platform release builds (triggered by v* tags)
├── AGENTS.md               # This file
├── build.rs                # Build script — Windows: embeds Azureal.ico via winres
├── CHANGELOG.md            # Version history
├── Cargo.toml              # Rust dependencies
├── README.md               # User-facing documentation
└── resources/
    ├── Azureal.icns        # macOS app icon (embedded via include_bytes)
    ├── Azureal.ico         # Windows app icon (6 sizes: 256/128/64/48/32/16, embedded via winres + include_bytes)
    └── Azureal_toast.png   # Windows toast notification icon (embedded via include_bytes, extracted to ~/.azureal/ on first run — PNG renders crisply in toasts; .ico blurs)
```

# ROADMAP

## Phase 1: Core Functionality (Current)
- [x] TUI with worktrees/viewer/session/input panels
- [x] Git worktree creation and management
- [x] Claude CLI spawning with `-p` mode
- [x] Multi-session concurrent agents
- [x] Stream-JSON parsing for clean output
- [x] Conversation persistence via SQLite session store + context injection
- [x] Diff viewing with syntax highlighting
- [x] Squash merge to main (replaced rebase/merge)
- [x] Vim-style modal input (command/insert modes)
- [x] Embedded terminal pane for shell commands

## Phase 2: Enhanced UX
- [x] File viewer pane (3-pane layout: Worktrees, Viewer, Session; FileTree as overlay)
- [x] Session list overlay in Session pane (`s` toggle — browse current worktree's session files with message counts)
- [x] Token usage percentage on Session pane title
- [x] TodoWrite sticky widget (persistent checkbox list at bottom of Session pane)
- [x] AskUserQuestion options box (numbered options with context-aware response handling)
- [x] Squash merge to main (collapses all branch commits into single main commit, replaced rebase/merge/auto-rebase)
- [ ] Session templates
- [ ] Per-project configuration
- [ ] Theme customization
- [x] Input history persistence
- [x] Search/filter sessions (`/` in Worktrees pane)
- [x] Session search (`/` in Session pane — find text in current session, `n/N` to cycle matches)
- [x] Session list search (`/` name filter, `//` cross-session content search)
- [x] Speech-to-text input (`⌃s` in prompt mode and file edit mode)

## Phase 3: Advanced Features
- [x] God File System (scan >1000 LOC files, batch-modularize via concurrent Claude sessions)
- [x] Worktree Health Panel (tabbed modal: God Files tab + Documentation coverage tab, Shift+H global)
- [x] Rebase-before-merge flow with RCR conflict resolution
- [x] Auto-rebase toggle per worktree (sidebar `R` indicator, 2-second polling, conflict → RCR flow)
- [x] Git panel (reuses existing panes: Actions+Files in sidebar, diffs in viewer, Commits in session pane; full-width status box with prompt-style keybind hints)
- [x] Git panel worktree tab bar (mirrors normal tab row design: ★ main tab, status symbols, archived styling, mouse clicks; GIT_ORANGE/GIT_BROWN theme; paginated; `[`/`]` cycles worktrees (skips main), `{`/`}` jumps pages; ★ main via click/Shift+M; focused pane preserved)
- [x] Debug dump shortcut (⌃d: creates debug output snapshot with naming dialog)
- [x] Multi-backend support — OpenAI Codex CLI as second backend alongside Claude Code CLI. `AgentProcess` struct holds both backends, dispatches at spawn time based on selected model via `backend_for_model()`. Unified model pool (9 models: opus/sonnet/haiku + 6 Codex GPT models) — single Ctrl+M cycle through all. Codex streaming parser, Codex session file parser, Codex session discovery, backend-dispatched session loading. Both backends produce `DisplayEvent` for rendering — TUI layer unchanged.
- [ ] Session export/reporting
- [x] Cross-session context sharing (via SQLite session store + context injection — all 9 phases complete)
- [ ] Agent orchestration (one agent spawns tasks for others)
- [ ] Custom tool definitions per session

## Phase 4: Portable Sessions (Complete)
- [x] SQLite-backed session store (`.azureal/sessions.azs`) with DELETE journal mode, S-numbered sessions
- [x] Context injection module (build conversation transcripts, strip injected context)
- [x] Per-PID session target tracking (snapshot active session at prompt time)
- [x] Session store wired into session creation and session list overlay
- [x] Replace `--resume` with context injection in prompt flow
- [x] Post-exit flow (parse JSONL → strip context → append to store)
- [x] Compaction system (400K char threshold, background agent summarization using selected model)
- [x] Session completion persistence (completed/duration_ms/cost_usd columns, completion badges in session list)
- [x] Legacy cleanup (removed session_cache.rs, migration code, old polling fields, cache read/write paths)

# TESTING REQUIREMENTS

## Domain-Specific Guidelines

This is a TUI + CLI wrapper application with stateless architecture. Testing focuses on:

1. **Process Management**: Verify agent processes (Claude + Codex) spawn, communicate, and terminate correctly
2. **State Discovery**: Ensure app correctly discovers sessions from git worktrees and branches (both Claude and Codex session formats)
3. **Event Parsing**: Validate stream-json parsing handles all event types for both backends
4. **Concurrent Operations**: Test multiple sessions running agents simultaneously
5. **Error Recovery**: Verify graceful handling of agent exits and git errors

## Test Coverage (6583 tests)

| Module | File | Tests | What's Tested |
|--------|------|------:|---------------|
| config | `src/config.rs` | 104 | `encode_project_path` (20 -- ASCII, unicode, emoji, special chars +/@/spaces, consecutive specials, boundary 199/200/201 chars, deterministic hashing, empty/root), `radix_36` (11 -- 0, single digits, boundaries 35/36/1295/1296, powers of 36, u64::MAX, char validation), `display_path` (8 -- home dir, nested, outside home, root, spaces), `Config` serde (6 -- roundtrip, defaults, partial deserialize), `PermissionMode` serde (10 -- serialize/deserialize all variants, unknown/empty rejection, roundtrip, Copy/Clone/Debug), `ProjectEntry` (3 -- construction, clone, debug), `claude_executable()` (3 -- default, custom, empty), `codex_session_cwd` (6 -- valid/invalid-json/no-cwd/not-session-meta/empty-line/nested-payload), `codex_session_id_from_filename` (5 -- valid-rollout/no-suffix/short/invalid-hyphens/no-jsonl), `list_codex_sessions` (5 -- empty/nonexistent/cwd-mismatch/valid-sessions/multiple-date-dirs), `codex_session_file` (3 -- found/not-found/multiple-dirs), `find_latest_codex_session` (3 -- found/empty/no-match), `list_sessions`/`find_latest_session`/`session_file` backend dispatch (9 -- Claude/Codex dispatch for each function), `codex_executable()` (3 -- default, custom, empty) |
| models | `src/models.rs` | 68 | `strip_branch_prefix` (16 -- prefix/no-prefix/empty/nested/different/unicode/emoji/double-prefix/case-sensitive/dots/dashes/partial-match/whitespace/slash-only), `WorktreeStatus` (12 -- as_str/symbol/color all variants, PartialEq exhaustive 6x6, Eq, Copy, Clone, Debug, serde roundtrip, lowercase/non-empty validation), `Worktree` (17 -- name edge cases, construction variations, full 8-combination status truth table, clone, debug, serde roundtrip with/without None fields), `DiffInfo` (4 -- construction, empty, clone, serde roundtrip), `OutputType` (7 -- all variants, PartialEq exhaustive, Copy, Clone, Debug, serde roundtrip), `RebaseResult` (5 -- Aborted, Failed with/without message, clone, debug), `BRANCH_PREFIX` (1) |
| git/core | `src/git/core.rs` | 62 | Types (`Git`, `SquashMergeResult`, `WorktreeInfo`) + repo detection, branch listing, status. `SquashMergeResult` (11 -- Success construction/extraction/empty/multiline/pull-note/already-up-to-date, Conflict construction/fields/empty-vecs/many-files/paths-with-subdirs, variant discrimination Success-not-Conflict/Conflict-not-Success, match exhaustiveness), `WorktreeInfo` (17 -- construction with-branch/main/master/no-branch/detached, Clone/Clone-with-None, Debug format all-fields/None-branch, path components, long 40-char commit hash, empty commit, is_main true-for-path/false-for-feature/true-without-main-name), `Git` struct (2 -- exists, zero-sized), size_of checks (2), commit log parsing (4 -- 3-field/extra-tabs/too-few-fields/empty-subject), divergence parsing (3 -- zeros/only-behind/only-ahead), merge message (3 -- no-log/with-log/line-count), status unmerged detection (4 -- UU/AA/DD vs clean/added), CONFLICT path extraction (2), Auto-merging strip_prefix (1), real-repo smoke tests (4 -- staged_diff/staged_stat/commit_log/main_divergence) |
| git/commit | `src/git/commit.rs` | 0 | Commit operations: `get_staged_diff`, `get_staged_stat`, `get_commit_log`, `commit` (tests in core module) |
| git/diff | `src/git/diff.rs` | 0 | Diff operations: `get_diff`, `get_diff_files`, `get_file_diff`, `get_commit_diff` (tests in core module) |
| git/merge | `src/git/merge.rs` | 0 | Merge operations: `squash_merge_into_main`, `has_unmerged_files`, `cleanup_squash_merge_state` (tests in core module) |
| git/remote | `src/git/remote.rs` | 0 | Remote operations: `pull`, `push`, `rev_list_divergence`, `get_main_divergence`, `get_remote_divergence` (tests in core module) |
| git/staging | `src/git/staging.rs` | 0 | Staging operations: `stage_all`, `stage_file`, `discard_file`, `unstage_all`, `untrack_gitignored_files`, `ensure_worktrees_gitignored` (tests in core module) |
| git/branch | `src/git/branch.rs` | 52 | Remote branch local name extraction (8 -- simple/nested/upstream/no-slash/multiple-slashes/single-segment/double-segment/zero-segments), branch filter patterns (3 -- excludes-HEAD/requires-slash/excludes-empty), main/master filtering (3 -- excludes-exact/keeps-similar-names/case-sensitive), porcelain output parsing (4 -- trim-and-filter/empty/whitespace-only/single), branch dedup (4 -- no-local-equivalent/has-local-equivalent/exact-remote-ref/main-remote-skipped), checked_out tracking (3 -- collects/empty/contains-current), Git struct (2 -- accessible/zero-sized), Path compatibility (2 -- from-string/with-spaces), combined filter exhaustiveness (3), newline-free branch names (2), non-existent path smoke tests (3) |
| git/worktree | `src/git/worktree.rs` | 51 | `WorktreeInfo` construction (7 -- basic/main/detached/clone/debug/spaces/unicode), porcelain `list_worktrees` parsing (4 -- single/multiple/empty/no-prefix), detailed `list_worktrees_detailed` parsing (9 -- single-main/two-worktrees/master/detached/is_main-by-path/empty/three-worktrees/no-commit-skipped/is-main-by-path), prefix stripping (3 -- branch/HEAD/worktree line prefixes), remote branch extraction (2 -- simple/nested), slash detection (1), edge cases (3 -- spaces-in-path/unicode-in-path/clone-independence), size_of check (1), additional coverage (21 -- add/delete/toggle/navigation/state) |
| git/rebase | `src/git/rebase.rs` | 51 | `RebaseResult` (22 -- Aborted/Failed construction/extraction/empty-msg/not-other-variant, Clone Aborted/Failed/independence/three-level-chain, Debug Aborted/Failed/Failed-empty/deterministic, match exhaustiveness, size_of non-zero, multiline-error/unicode-error, Option/Result wrappers, array exhaustive matching, whitespace-only message, roundtrip fidelity), rebase state dir paths (3 -- rebase-merge/rebase-apply in main .git, worktree .git dir), conflicted file parsing (6 -- output/empty/single/subdirs/entry-no-newlines/clean-repo), `rebase_abort` error display (2), `is_rebase_in_progress` live cwd (1), Git struct (2 -- accessible/zero-sized) |
| app/util | `src/app/util.rs` | 81 | `strip_ansi_escapes` (23 -- plain, empty, CSI color/bold/cursor/erase, OSC title/hyperlink, mixed, multi-param, 256-color, RGB, nested, partial/just-ESC/ESC-at-end, consecutive, real terminal git-diff/cargo output, unicode/emoji preserved, only-escapes), `extract_tool_param` (26 -- all tool types Read/Write/Edit/Bash/Glob/Grep/Task/WebFetch/WebSearch/LSP + lowercase variants, NotebookEdit/TodoWrite, numeric/nested object values, LSP missing/partial fields, exact 60/61 char boundary truncation, unknown tool fallbacks for path/command/query/pattern), `display_text_from_json` (27 -- init with/without model/cwd, hook with/without output, user message with/without content/message, assistant text/tool_use/no-param/mixed/empty/unknown-block/multiple-text/content-not-array/missing-message, result with values/zero/missing/large, system missing subtype, unknown type, missing type), `parse_stream_json_for_display` (5 -- valid, invalid, whitespace, empty, just-whitespace) |
| azufig | `src/azufig.rs` | 67 | `is_bare_key` (14 -- simple, underscores/dashes, empty, spaces, special chars, single chars, all lowercase/uppercase/digit chars, numbers-only, underscores-only, dashes-only, invalid ASCII printable, multi-byte unicode, tab/newline, mixed valid/invalid), `strip_unnecessary_key_quotes` (14 -- bare key, preserves needed quotes, section headers, mixed, no quotes, indent, special key preserved, empty string, no trailing newline, multiple equals, value with quotes, nested sections, array values, digits-only key, dot key stays quoted, empty key stays quoted), `default_hidden` (1), struct defaults (5 -- AzufigFiletree, GlobalAzufig, ProjectAzufig, AzufigConfig, AzufigHealthScope), TOML round-trips (7 -- GlobalAzufig all fields, ProjectAzufig all fields, AzufigConfig all fields, AzufigFiletree, AzufigHealthScope, clone/debug for all structs), partial TOML deserialization (5 -- empty/partial Global, partial/filetree-only Project, partial Config), `DEFAULT_AUTO_RESOLVE` (4 -- contains all, specific entries, order, all-markdown), `AZUFIG_FILENAME` (1), `godfilescope` alias (1) |
| session_parser | `src/app/session_parser.rs` | 131 | `check_plan_approval` (14 -- empty/pending/resolved/multiple/all-resolved, non-resolving event types: ToolResult/AssistantText/Hook/Command/Compacting/Init/Complete/Filtered), `extract_hooks_from_content` (16 -- success/failed/ellipsis/multiple/unclosed/empty-name/empty-string/no-closing/nested/content-between-tags/multiline/unicode-name/colon-in-name/only-opening/tag-without-content/newline-escape/empty-output), `context_window_for_model` (11 -- opus/sonnet/haiku/claude3/unknown/empty/case-sensitive/partial-match/whitespace/numeric/future-model), `IncrementalParserState::from_events` (13 -- empty/captures-tool-calls/ignores-non-tool-events/slug/none-slug/duplicate-ids/100-tools/result-not-captured/complete-not-captured/init-not-captured/plan-not-captured/user-msg-parent-always-empty/mixed), `parse_session_file` (18 -- nonexistent/empty/invalid-json/mixed-valid-invalid/system-command/user-message/assistant-text/result/compaction/is-meta/caveat-filtered/stdout-compacted/command-name/tool-call-result/unknown-type/end-offset/blank-lines/token-usage/model-usage-context-window), agent suppression (3 -- Agent tool suppresses sub-agent prompt/clears after result/non-agent tool does not suppress), `ParseDiagnostics` (4 -- default/counts/no-message/no-content-array), incremental parse (2), progress events (6) |
| app/types | `src/app/types.rs` | 101 | All enum variant equality/Debug/Clone/Copy/Default for ViewerMode(4)/ViewMode(2)/Focus(7)/CommandFieldMode(2)/ProjectsPanelMode(4)/RustModuleStyle(2)/PythonModuleStyle(1)/HealthTab(2), BranchDialog (18 -- new empty/populated, is_checked_out exact/remote-prefix/multi-slash/no-match, selected_branch populated/empty/after-filter, select_next/prev/at-bounds/on-empty, filter_char narrows/case-insensitive, filter_backspace widens/on-empty, selected-resets-on-shrink, unicode filter; cursor_pos tracks byte offset for Left/Right arrow navigation, filter_char inserts at cursor, filter_backspace deletes before cursor), FileTreeEntry (4 -- clone/hidden/directory/debug), FileTreeAction (3 -- add-clone/copy-path/move-path), RunCommand (6 -- basic/global/string-types/clone/empty/special-chars), RunCommandDialog (3 -- new-defaults/edit-populates/edit-idx-zero), RunCommandPicker (1), PresetPrompt (4 -- basic/global/clone/unicode), PresetPromptPicker (1), PresetPromptDialog (3 -- new-defaults/edit-populates/edit-unicode-cursors), ProjectsPanel (20 -- new-defaults/with-entries, select_next/prev/empty, start_add/rename/rename-empty/init, cancel_input, input_char/at-cursor, backspace/at-start/empty, delete/at-end, cursor left/at-zero/right/at-end/home/end), ViewerTab (3 -- name/empty-name/clone), GitCommit (2 -- clone/debug), GitChangedFile (3 -- clone/added/deleted), GodFileEntry (1), DocEntry (3 -- clone/zero/full-coverage), GitConflictOverlay/PostMergeDialog/RcrSession/AutoResolveOverlay/ModuleStyleDialog/HealthPanel/GitActionsPanel field construction (7) |
| events/types | `src/events/types.rs` | 62 | `ClaudeCodeEvent` deserialization (10 -- system init/hook, user/assistant/result with all fields, minimal fields, extra fields ignored), `ContentBlock` variants (10 -- text/tool_use/tool_result with string/object/array/null/numeric content, empty input, nested input), `Usage` (5 -- defaults/all-zeros/large-tokens/partial-fields/roundtrip), `SystemEvent` (4 -- minimal/all-fields/hook-and-output/all-tools/empty-tools), round-trip serialization (4 -- system/user/result/assistant with full content+usage), type tag serialization (4 -- all variants), user message content (5 -- special-chars/newlines/unicode/100k-long/empty), deserialization errors (5 -- invalid-type/missing-type/invalid-content-block/missing-session-id/missing-message), `ContentBlock` serialization (3 -- text/tool_use/tool_result format), `AssistantMessage` (6 -- empty-content/multiple-text/multiple-tool-use/mixed-text-tool/minimal/with-usage) |
| events/parser | `src/events/parser.rs` | 78 | `EventParser` new/default (2), buffer behavior (8 -- no-newline/flush/multi-line/empty-lines/empty-string/returns-last-json/invalid-json/plain-text), `parse_system_event` (8 -- init full/missing-cwd/missing-model, non-init-non-hook ignored, hook empty-name/empty-output/name-fallback/hook-fallback/stdout-fallback), `parse_user_event` (12 -- string content, compaction summary, compacted/non-compacted stdout, caveat filtered, no-message/no-content, array tool_result with known/unknown tool, array tool_result with array/empty content, text blocks in array), agent suppression (4 -- Agent tool suppresses string user event, Task tool suppresses string user event, Agent tool suppresses text block user event, non-Agent tool does not suppress), `parse_assistant_event` (10 -- no-message/no-id/no-content-array, multiple text/tool_use, mixed text+tool, file_path/path fallback/no-path, unknown block type, empty content), `parse_result_event` (4 -- success/error/missing-session-id/defaults), `parse_progress_event` (8 -- non-hook/no-data/empty-name, echo single/double quote, OUT var single/double quote, fallback hookName, hookEvent fallback), hook/hook_result/hook_response types (5 -- all three types, no-name fallback, no-output), `parse_text_hook` (5 -- success/failed with output, success/failed no output, generic hook), `extract_hooks_from_content` static (5 -- success/failed/empty/multiple/unclosed), tool call tracking (2), user event with embedded hooks (1), whitespace handling (2 -- leading whitespace, incremental buffer) |
| state/viewer_edit | `src/app/state/viewer_edit.rs` | 110 | `word_wrap_breaks` (15 -- empty/zero-width/fits/exact/one-over/word-boundary/width-1/width-2/single-char/unicode/long-word-forced/multiple-spaces/first-always-zero/monotonically-increasing/huge-width), `viewer_edit_char` (7 -- empty-line/end/middle/unicode/dirty-flag/undo-push/multibyte-position/CJK/tab), `viewer_edit_backspace` (7 -- start-first-line/middle/end/joins-lines/join-unicode-cursor/single-char/unicode), `viewer_edit_delete` (5 -- end-last-line/middle/start/joins-next/empty-line-joins), `viewer_edit_enter` (6 -- end/start/splits/empty/dirty/unicode-split), cursor movement (12 -- left-start/normal/wrap-prev/wrap-multi, right-end/normal/wrap-next/wrap-multi, home/home-already/end/end-empty/end-unicode), up/down (5 -- up-first/up-simple/down-last/down-simple/down-clamps), `clamp_edit_cursor` (4 -- beyond-lines/beyond-col/valid/single-empty), undo/redo (7 -- restore/reapply/empty-noop/empty-redo-noop/clears-redo/multiple-undo/stack-cap), selection (16 -- start/extend/clear/has-none/has-zero-width/has-true/normalized-backward/normalized-forward/text-single/text-multi/text-zero/text-none/text-entire/text-backward/text-three-lines/select-all-multi/single/empty/unicode), delete-selection (4 -- single-line/multi-line/clears/zero-width-noop), scroll-to-cursor (3 -- at-top/below/above), selection-aware movement (6 -- left-no-extend/left-extend/right-no-extend/right-extend/up-extend/down-extend), roundtrip/edge (7 -- insert-undo-all/enter-backspace-roundtrip/delete-undo/version-increments-edit/undo/redo) |
| state/scroll | `src/app/state/scroll.rs` | 75 | `session_natural_bottom` (6 -- normal/exact/fewer/zero-lines/zero-viewport/single-line), `session_max_scroll` (3 -- normal/single/zero), `clamp_session_scroll` (5 -- sentinel/within-range/beyond-max/at-zero/empty-sentinel), `scroll_session_down` (8 -- one/from-sentinel/past-max/zero/reengage-sentinel/returns-true/large-small-content), `scroll_session_up` (7 -- one/from-sentinel/past-zero/zero/already-at-zero/to-zero-dirty/to-zero-no-dirty), `scroll_session_to_bottom` (2 -- from-zero/from-mid), `viewer_natural_bottom` (3 -- normal/fewer/zero), `viewer_max_scroll` (2 -- normal/zero), `clamp_viewer_scroll` (4 -- sentinel/within/beyond/at-zero), `scroll_viewer_down` (5 -- one/from-sentinel/past-max/zero/at-max), `scroll_viewer_up` (5 -- one/from-sentinel/past-zero/zero/at-zero), `scroll_viewer_to_bottom` (1), `jump_to_next_bubble` (8 -- user-only/include-assistant/no-more/from-sentinel/empty-list/skip-assistant/clamp-to-max/at-line-zero), `jump_to_prev_bubble` (7 -- from-end/user-only/none-goes-to-zero/from-sentinel/empty-list/triggers-dirty/no-dirty-without-deferred), combined scenarios (9 -- session-down-up-roundtrip/viewer-down-up-roundtrip/session-down-accumulates/session-up-accumulates/viewer-single-line/session-exact-viewport/page-down/page-up/bubble-walk-forward/bubble-walk-backward) |
| state/claude | `src/app/state/claude.rs` | 73 | `parse_todos_from_input` (64 -- real data, empty, missing fields, unknown status, missing content, null/bool/number/string/array root values, todos field wrong types, all status strings including case-sensitive/capitalized/empty/null, content null/number/bool/empty/unicode/special/very-long, activeForm null/number/bool/unicode, mixed valid/invalid entries, 50/100 item stress tests, order preservation, extra fields, whitespace, realistic payloads, code snippets), `register_claude`/`handle_claude_exited`/`apply_parsed_output`/`store_append_from_jsonl` (9 -- codex slot tracking, exit code clearing, full session reparse for Codex, incremental for Claude, duration persistence, live context counter+badge, compaction trigger at threshold) |
| state/app | `src/app/state/app.rs` | 122 | `App::new()` field defaults (60+ -- project/worktrees/focus/view_mode/session/terminal/viewer/render/parse_stats/tokens/todos/stt/file_tree/health/git/browsing/session_list/find/filter/model/backend), `invalidate_render_cache` (2 -- sets dirty, idempotent), `invalidate_file_tree` (1), `set_status`/`clear_status` (6 -- stores/overwrites/String/format/clears/noop), `current_project`/`current_worktree` (5 -- none defaults, selected index, browsing_main returns main, out of bounds), `is_session_running` (3 -- no slots, running slot, stopped slot), `is_current_session_running` (2 -- no worktree, true), `is_active_slot_running` (3 -- no worktree, running, stopped), `branch_for_slot` (3 -- found, not found, empty), `is_claude_session_running` (3 -- true, not running, no match), `display_model_name` (2 -- default opus when None, custom value), `backend_for_model` (3 -- Claude models, Codex models, unknown defaults Claude), `cycle_model` (5 -- opus→sonnet, haiku→gpt-5.4 crosses backend, last codex wraps to opus, full 9-model cycle with backend checks, unknown→sonnet), `ALL_MODELS` (3 -- has 9, first is default, Claude then Codex ordering), `update_token_badge` (6 -- no tokens, low/medium/high usage, default 200k window, drop below threshold), `TodoItem`/`TodoStatus` (6 -- equality, construction, clone, debug), `DeferredAction` (7 -- all variant construction), `cancel_all_claude` (1 -- clears all state), `git_action_in_progress` (4 -- default false, deferred commit/commit+push, non-git deferred), `get_cpu_time_micros` (1 -- returns nonzero) |
| state/ui | `src/app/state/ui.rs` | 73 | `focus_next` (9 -- all 5 pane transitions, full cycle, WorktreeCreation/BranchDialog stay, clears session_list), `focus_prev` (9 -- all 5 reverse transitions, full cycle, WorktreeCreation/BranchDialog stay, clears session_list), focus_next/prev inverses (2 -- roundtrip both directions), `toggle_help` (3 -- on/off/double), `exit_worktree_creation_mode` (1 -- sets focus+clears input+clears status), `open_branch_dialog` (2 -- empty branches sets status, with branches opens dialog), `close_branch_dialog` (1 -- clears and refocuses), `close_projects_panel` (1), `is_projects_panel_active` (2 -- false/true), `open_run_command_dialog` (1), `open_run_command_picker` (3 -- empty sets status, single executes, multiple opens picker), `open_preset_prompt_picker` (2 -- no presets opens dialog, with presets opens picker), `select_preset_prompt` (3 -- valid index populates input, invalid noop, sets status), `viewer_tab_current` (3 -- no content noop, adds tab, max 12 tabs), `toggle_viewer_tab_dialog` (1), `viewer_close_current_tab` (2 -- empty noop, last tab clears viewer), `load_tab_to_viewer` (1 -- restores state), `enter_main_browse`/`exit_main_browse` (2 -- no main sets status, exit restores), `git_action_in_progress_rcr` (1), `parse_ordered_key` (8 -- valid, large number, no underscore, non-numeric, zero, multiple underscores, empty after prefix, empty string), `load_ordered_map` (4 -- empty, single, preserves order, no prefix), `load_ordered_presets` (2 -- empty, sorted), `find_edit_line` (10 -- new string found, old string found, both empty, new preferred, significant lines, trimmed match, identifier fallback, no match, empty content, first line) |
| state/load | `src/app/state/load/` (4 submodules) | 54 | **worktree_refresh** (3): `load_file_tree` (2 -- clears when no worktree, nonexistent path), `refresh_worktrees` (1 -- no project ok). **session_output** (47): `format_uuid_short` (12 -- standard UUID, 8-char prefix, short prefix, long no dash, short no dash, empty, exactly 12/13 chars, dash at position 8, multiple dashes, dash only, dash at end), `viewed_session_id` (6 -- no data, correct id, second selection, out of bounds idx, no idx, empty branch), `extract_skill_tools_from_events` (10 -- no events, TodoWrite parses todos, cleared by user message, AskUserQuestion awaiting, answered clears awaiting, no ask clears cache, multiple TodoWrites uses last, resets scroll, ignores other tools, mixed event types), `load_session_output` state reset (10 -- resets session state, render caches, token state, historic flag, ask_user state, clickable paths, selected_event, pending_message, active_task_ids, render_seq), `load_session_output` preservation (9 -- preserves compaction flag, plan approval from events, with worktree no session, historic badge recompute, clears selected_event, pending_message, fresh parser, active_task_ids, render_seq). **session_file** (4): `check_session_file` (2 -- no path noop, nonexistent path noop), `poll_session_file` (2 -- not dirty returns false, reparses active Codex session from disk) |
| state/helpers | `src/app/state/helpers.rs` | 50 | `build_file_tree` (50 -- returns entries, top-level-only collapsed, dirs-before-files sort, hidden-after-non-hidden sort, expansion adds children, expanded children depth, non-expanded no children, skips target dir, skips node_modules, hidden_dirs filter single/multiple, empty dir, is_dir flag, is_hidden dot-prefix, non-dot not hidden, children of hidden inherit hidden, path is absolute, name matches filename, alphabetical within category, dirs alphabetical, nested expansion, partial expansion, nonexistent root empty, hidden dir no sibling effect, mixed dirs-and-files sort, hidden files after non-hidden files, hidden dirs after non-hidden dirs, entry count no expansion, expand src adds children, expand all dirs, hidden dir exact name match, FileTreeEntry fields/clone/debug, symlinks no crash, deeply nested 5-level, expanding nonexistent dir noop, unicode filenames, hidden via custom config, empty expanded set, empty hidden set, only hidden files, only dirs, only files, many files sorted, multiple hidden dirs, target filter applies to files, special chars in name, three-level depth, node_modules filter applies to files) |
| state/health | `src/app/state/health.rs` | 59 | `SOURCE_EXTENSIONS` (18 -- contains rust/python/javascript/typescript/go/c/cpp/java/kotlin/swift/shell/sql/vue/svelte/ruby/elixir/haskell, not data formats/images/markdown, not empty, no duplicates), `SKIP_DIRS` (14 -- contains git/target/node_modules/build-artifacts/ide/python-venvs/vendor/docs/examples/generated/third-party, no duplicates, not empty), `SOURCE_ROOTS` (8 -- contains src/lib/go-dirs/java-dirs/swift-sources/cpp-include, not empty, no duplicates), `collect_source_files` (19 -- finds .rs/.py/.js multiple types, ignores non-source extensions, skips hidden dirs/files, skips SKIP_DIRS, recurses into subdirs, deeply nested, empty dir, nonexistent dir, sorted output, case-insensitive skip dirs, no extension ignored, appends to existing vec, all source types, skips refs/vendor/coverage dirs, returns absolute paths) |
| state/health/god_files | `src/app/state/health/god_files.rs` | 64 | `count_source_lines` (11 -- no-test-module/test-module-at-end/nested-braces-in-test/non-rust-fallback/cfg-test-same-line-as-mod/no-test-all-counted/empty-file/nonexistent/excludes-large-test-block/scan-integration-not-flagged/scan-integration-still-flags), `build_modularize_prompt` (19 -- contains file path/line count/instructions/steps, Rust file-based/mod.rs/no-style, Rust style ignored for non-.rs, Python package/single-file/no-style, Python style ignored for non-.py, both styles only uses matching language, generic file no style section, zero lines, very large count, nested path, mentions re-export/backwards-compatibility/single-responsibility/not-util-or-helpers), `scan_dir_recursive` (14 -- finds god files >1000 LOC, ignores small files, threshold boundary 1000-not-god/1001-is-god, relative paths, skips hidden/target/node_modules dirs, ignores non-source extensions, checked defaults false, multiple files, empty dir, nonexistent, subdir scan), `scan_top_level_files` (6 -- finds god files, ignores small, does not recurse, skips dirs, skips non-source, nonexistent), `GodFileEntry` (3 -- construction/checked toggle/clone), `RustModuleStyle` (3 -- eq/copy/debug), `PythonModuleStyle` (3 -- eq/copy/debug), `ModuleStyleDialog` (2 -- construction/both-languages), `GOD_FILE_THRESHOLD` (1) |
| state/health/docs | `src/app/state/health/documentation.rs` | 56 | `scan_file_doc_coverage` (36 -- empty file, single documented/undocumented fn, pub fn documented/undocumented, struct documented/undocumented, pub struct, enum documented, pub enum undocumented, trait documented, pub trait, const documented/undocumented, static documented, type alias documented, impl block, mod/pub mod, async fn documented/undocumented, pub async fn, unsafe fn documented/undocumented, pub unsafe fn, mixed documented-and-not, all documented, none documented, module-level //! comment, multiline doc comment, attribute between doc and fn, blank line between doc and fn, skips use/extern/regular-comments/closing-braces, nonexistent file, only-comments file, only-use-statements, indented fn, pub static/const/type alias), `build_doc_health_prompt` (10 -- contains file path/coverage counts/percentage, zero total, all documented, none documented, mentions doc comments/no-code-modification/no-reformatting/read-file), `DocEntry` (5 -- construction/checked toggle/zero coverage/full coverage/clone) |
| state/file_browser | `src/app/state/file_browser.rs` | 64 | `is_image_extension` (15 -- png/jpg/jpeg/gif/bmp/webp/ico positive, case insensitive PNG, not-image rs/txt/svg/no-ext/pdf/mp4/empty-path), `copy_dir_recursive` (4 -- empty dir, with files, nested, preserves content), `file_tree_next` (5 -- from first, from middle, at end stays, from None selects first, empty tree from None), `file_tree_prev` (5 -- from last, from middle, at start stays, from None noop, single entry), `file_tree_first_sibling` (4 -- from last root, already at first, None selected, nested children), `file_tree_last_sibling` (4 -- from first root, already at last, None selected, nested children), `clear_viewer` (2 -- resets all state, already empty), `load_file_into_viewer` (2 -- no selection noop, dir selected noop), `load_file_by_path` (5 -- text file loads, nonexistent noop, empty file, resets scroll, clears image state), `file_tree_exec_add` (3 -- creates file, creates dir with trailing slash, no selection noop), `file_tree_exec_rename` (3 -- renames file, existing target error, no selection noop), `file_tree_exec_delete` (3 -- removes file, removes dir, no selection noop), `file_tree_exec_copy_to` (3 -- copies file, existing target error, expands target dir), `file_tree_exec_move_to` (3 -- moves file, existing target error, expands target dir), `toggle_file_tree_dir` (3 -- no selection noop, on file noop, single entry) |
| state/sessions | `src/app/state/sessions.rs` | 53 | `select_next_session` (6 -- from first, from middle, wraps from last, empty worktrees, from None, single worktree), `select_prev_session` (6 -- from last, from middle, wraps from first, empty worktrees, from None, single worktree), `select_first_session` (4 -- from end, already first, empty worktrees, from middle), `select_last_session` (4 -- from start, already last, empty worktrees, from middle), `select_session_file` (6 -- valid idx, out of bounds, unknown branch, first idx, empty list, last valid idx, overwrite previous), next/prev roundtrip (4 -- next-then-prev, prev-then-next, next wraps full cycle, prev wraps full cycle), two worktrees toggle (2 -- next toggles, prev toggles), guard tests (6 -- archive main blocked, delete main blocked, delete no worktree, archive no worktree, unarchive non-archived error, unarchive no selection), error cases (2 -- create worktree no project, delete no project), navigation stress (6 -- many worktrees next/prev 10-item, first/last large list, next/prev 5 times, across wrap boundary), state preservation (4 -- next/prev/first/last preserve worktrees vec), idempotent (2 -- first/last idempotent) |
| tui/input_terminal | `src/tui/input_terminal.rs` | 55 | `build_ask_user_context` (55 -- single/multi-select/multiple-questions/empty/missing-label, edge cases: null/bool/number/string/array root values, questions field wrong types, empty arrays, zero/null/missing options, multi-select variations, Q-prefixed numbering, option label wrong types/unicode/emoji/empty/very-long, question text null/number/unicode/newlines/whitespace, output structure bookends, independent numbering per question, footer text, mixed option types, deeply nested JSON) |
| tui/render_events | `src/tui/render_events.rs` | 77 | `render_ask_user_question` (18 -- multiple questions/no-empty-null-missing descriptions/labels, 5-option Other numbering, wide/narrow/zero width, unicode labels, long description wrapping), `render_init` (4 -- basic/empty-model/empty-cwd/line-count/unicode), `render_hook` (6 -- with-output/empty/whitespace/multiline/narrow/special-name), `render_command` (3 -- basic/line-count/empty), `render_user_message` (6 -- basic/bottom-border/empty/wraps/unicode/newlines/min-bubble), `render_complete` (5 -- success/failure/zero-duration/large-duration/zero-cost), `render_plan_approval` (5 -- all-options/header/borders/narrow/zero-width), `render_plan` (8 -- borders/name/empty/markdown-headers/bullets/numbered-list/code-block/blockquote/narrow), `render_display_events` integration (22 -- empty/init/dedup-init/user-message/assistant/hook/dedup-hooks/command/compacting/compacted/may-be-compacting/complete/filtered/pending/todowrite-skipped/tool-pending-animation/plan/compaction-summary/mixed-sequence/init-after-content-suppressed) |
| tui/file_icons | `src/tui/file_icons.rs` | 127 | Nerd font icons (80+ extensions, named files, directories), emoji fallbacks, dir collapsed/expanded, case insensitivity, unicode filenames |
| tui/keybindings | `src/tui/keybindings/` | 596 | `platform::macos_opt_key` (60+ Unicode-to-key mappings), `types::KeyCombo` (equality, modifiers, display), `types::Keybinding` (matching, alternatives), `types::Action` (clone, eq, debug), `types::HelpSection` fields, `lookup` (136 -- lookup_action global/context-specific/skip-guards for prompt/edit/help/terminal modes, lookup_health_action shared+god-files+docs tabs, lookup_git_actions_action focus/branch guards, lookup_projects_action, lookup_picker_action, lookup_branch_dialog_action, unknown keys return None), `hints` (116 -- help_sections count/titles/bindings, find_key_for_action known/unknown/empty, find_key_pair found/fallback, prompt_type_title label/hints content, prompt_command_title label/hints, terminal_type/command/scroll titles, health_god_files/docs hints, git_actions_labels main/feature counts/content, git_actions_footer, projects_browse_hint_pairs with/without project, picker_title, dialog_footer_hint_pairs), `bindings` (161 -- array lengths/nonempty, no duplicate primaries across 15 arrays, descriptions nonempty, specific binding verification for GLOBAL/FILE_TREE/VIEWER/EDIT_MODE/SESSION/INPUT/TERMINAL/GIT_ACTIONS/HEALTH/PROJECTS/PICKER/BRANCH_DIALOG, static alt array values, CMD_SHIFT constant, display_keys integration, matching integration) |
| tui/markdown | `src/tui/markdown.rs` | 68 | `parse_markdown_spans` (40+ -- plain, bold, italic, code, mixed, edge cases, unicode, long text, base style propagation), `is_table_separator` (12+ -- valid/invalid separators, alignment markers, edge cases) |
| tui/render_wrap | `src/tui/render_wrap.rs` | 51 | `wrap_text` (25+ -- empty, fits, wraps, newlines, width 1-3, unicode, long words), `wrap_spans` (25+ -- empty, style preservation, narrow widths, each-char-own-span, multi-span wrapping) |
| tui/colorize | `src/tui/colorize.rs` | 68 | `strip_ansi` (11 -- plain, ANSI codes, 256-color, RGB, unicode, consecutive), `detect_message_type` (17 -- user/assistant/other markers, ANSI wrapping, partial matches), `colorize_output` (40 -- user/assistant/tool/done/error/code/JSON/bullets/headers/paths/default) |
| tui/render_tools/tool_params | `src/tui/render_tools/tool_params.rs` | 68 | `tool_display_name` (12 -- Grep/Glob rename, Read/Write/Bash/Edit/Task/WebFetch passthrough, empty, unknown), `extract_tool_param` (42 -- Read/Write/Edit file_path+path fallback+empty+null, Bash command+long, Glob/Grep pattern, WebFetch url, WebSearch query, Agent/Task subagent_type+default+empty, LSP operation+file+empty, EnterPlanMode/ExitPlanMode, unknown tool fallback priority chain), `truncate_line` (14 -- exact-fit, under-max, over-max, max-1/max-0, empty, whitespace-trim, unicode, emoji, special chars) |
| tui/render_tools/tool_result | `src/tui/render_tools/tool_result.rs` | 33 | `render_tool_result` (23 -- Read empty/single/two/many lines, Bash single/multi/empty-lines/all-empty, Grep few/exact-3/more-than-3, Glob count, Agent/Task few/many, default few/many, failed-red/success-azure, system-reminder stripping, narrow-width truncation), `render_write_preview` (10 -- content with lines, comment detection for `//`/`#`/`/*`/`///`/`//!`, no-comment first-line fallback, no-content-field, empty content, checkmark) |
| tui/render_tools/diff_parse | `src/tui/render_tools/diff_parse.rs` | 4 | `extract_edit_preview_strings` (4 -- prefers explicit old/new fields, update patch, add patch, unified diff) |
| tui/render_tools/diff_render | `src/tui/render_tools/diff_render.rs` | 3 | `render_edit_diff` (3 -- patch shows diff lines, unified diff shows diff lines, patch skips header and hunk) |
| watcher | `src/watcher.rs` | 66 | `is_noisy_path` (38 -- NOISY_SEGMENTS: target/git/node_modules/DS_Store at root/nested/deep paths, NOISY_EXTENSIONS: swp/swo/swn, backup tilde: bare/deep/middle-of-path, clean paths: rust/toml/json/md/lock/hidden/txt/empty/root/relative, substring-not-segment: targeted/github/gitignore, unicode/spaces/dots/no-extension/special-chars/emoji, exact-segment-only, swp-without-extension), `classify_event` (28 -- error result ignored, EventKind filtering: Access/Any/Other ignored, session file detection: create/modify/remove/no-session-set, worktree clean paths: create/modify/remove, noisy paths filtered: target/git/swap/backup/node_modules, multi-path: session+worktree/all-noisy/mixed/deduplicated/empty-paths, flag preservation: saw_session/saw_worktree remain true) |
| events/display | `src/events/display.rs` | 52 | `DisplayEvent` construction (31 -- Init basic/empty/unicode/special-chars, Hook basic/empty-output/multiline, UserMessage basic/unicode/100k-long, Command basic/empty/special-chars, Compacting/Compacted/MayBeCompacting/Filtered unit variants, Plan basic/empty/50k-content, AssistantText basic/markdown, ToolCall with-path/without-path/empty-input/null-input, ToolResult basic/no-path/empty-content, Complete success/failure/zero-duration/large-values), Debug impl (9 -- Init/Compacting/Compacted/MayBeCompacting/Filtered/Hook/ToolCall/Complete variant names and fields), Clone impl (8 -- Init/Compacting/Filtered/ToolCall-independence/Complete/UserMessage/ToolResult), variant discrimination (4 -- all variants distinct, Compacting!=Compacted, MayBeCompacting!=Compacting, Filtered!=Compacted) |
| session_store | `src/app/session_store.rs` | 90 | `SessionStore::open` (DELETE journal mode, schema v2 creation, idempotent), `create_session` (auto-increment, worktree assignment), `rename_session`/`delete_session` (CRUD), `list_sessions` (filtered/unfiltered, event+message counts via JOIN, completion fields), `load_all_session_names` (S-number default, custom name), `append_events` (seq numbering, Filtered skipping, char_len, transaction, compaction, auto-mark_completed on Complete event), `mark_completed` (success/failure, duration_ms, cost_usd), `load_events`/`load_events_from` (ordering, JSON round-trip), `load_events_range` (subset/single-event/empty/order-preserved), `compaction_boundary` (fewer-than-keep/exactly-keep/returns-seq-before-3rd-to-last/respects-from-seq/from-seq-too-late), `count_events` (kind filtering), `message_count` (UserMessage+AssistantText), `total_chars_since_compaction` (with/without compaction), `store_compaction`/`latest_compaction`, `max_seq`, `build_context` (full/partial/empty), `event_kind` (all DisplayEvent variants), `event_char_len` (content extraction), `db_path`, `compact_event` — ToolResult.content (8 -- Read truncates-large/preserves-small, Bash keeps-last-two, Grep keeps-first-three, Glob shows-count, Task keeps-first-five, default keeps-first-three, strips-system-reminder), ToolCall.input (4 -- Write summarizes-content, Edit preserved, Bash strips-extras, Read strips-extras), passthrough (2 -- UserMessage/AssistantText unchanged), integration (3 -- append compacts ToolResult/ToolCall, boundary+range end-to-end) |
| context_injection | `src/app/context_injection.rs` | 37 | `build_context_prompt` (empty events no-op, with events wraps in tags, with compaction summary prefix, preserves user prompt), `strip_injected_context` (returns clean prompt, no tags returns original, handles missing close tag), `build_transcript` (formats UserMessage/AssistantText/ToolCall/ToolResult/Plan, skips Init/Hook/Complete/Filtered/Compacting/Compacted/MayBeCompacting/Command, pairs ToolCall+ToolResult), `format_event` (all variant coverage), `extract_key_param` (Read/Write/Edit/Bash/Glob/Grep/Agent/Task/WebFetch/WebSearch/LSP), `compact_result` (truncation at 500 chars), `build_compaction_prompt` (contains transcript tags, includes instruction keywords, empty payload valid, includes compaction summary) |
| tui/event_loop/coords | `src/tui/event_loop/coords.rs` | 64 | `screen_to_cache_pos` (22 -- top-left content/scroll-offset/column-offset/row+col/pane-offset/pane-offset+column, borders: left/top/both return None, below-content/past-cache/scroll-exceeds-cache/zero-cache return None, minimum 3x3 pane/too-small 2x2/height-1, large pane, last-valid-row/last-cache-line/exactly-at-cache-len, left-of-pane/above-pane), `compute_cursor_row_fast` (18 -- empty/single-char/end-of-line/single-newline/multiple-newlines/cursor-at-newline/just-after-newline, word-wrap: at-width/end-of-wrapped/three-wraps/width-1/exactly-fits, cursor clamping: beyond-len/way-beyond, unicode: CJK-chars/mixed-ascii-unicode, newline+wrap combined), `row_col_to_char_index` (24 -- empty/single-char-origin/col-1/middle/end/past-end, multiline: second-line/second-col/first-line/first-col/newline-skipped, wrapping: second-row/col-2/first-row-last-col, beyond-content: row-5/row-1-single, width-1: each-char-own-row, unicode: wide-chars/second-row, edge cases: zero-origin/spaces/only-newlines/single-newline/trailing-newline/large-width) |
| tui/draw_viewer/wrapping | `src/tui/draw_viewer/wrapping.rs` | 61 | `wrap_text` (20 -- empty/single-word/exact-width/short-word-wraps/two-words-fit/break-at-space/three-words/long-word-breaks/multiple-spaces/width-1/multiline/unicode/leading-spaces/exact-length/multi-line-result/sentence-reassembly/all-spaces/single-char/large-width), `word_wrap_breaks` (21 -- empty/zero-width/both-empty-zero/fits/exact/one-over/two-words/three-words/hard-break/very-long/width-1/first-always-zero/single-char/single-char-wide/monotonically-increasing/last-offset-within-text/spaces-at-boundary/trailing-space/leading-space/count-matches-wrap-text), `wrap_spans_word` (20 -- empty-vec/zero-width/single-short/exact-width/single-wraps/preserves-style/two-styles-no-wrap/style-split-at-boundary/hard-break/multiple-spans/content-preserved/single-char/empty-content/width-1/adjacent-merged/modifier-preserved/large-width/three-styles/wrap-preserves-all-styles/result-widths-within-max/bg-color-preserved) |
| tui/draw_viewer/selection | `src/tui/draw_viewer/selection.rs` | 50 | `apply_selection_to_spans` (27 -- no-selection-start-ge-end/no-selection-col-beyond-end/full-line/partial-start/partial-end/middle/preserves-text/multi-span-across-boundary/entire-multi-span/empty-vec/sel-start-equals-end/line-between-start-end/start-line-partial/end-line-partial/single-char/beyond-span-end/before-span/after-span/three-spans-middle/preserves-bg/unicode/on-start-line-only/many-small-spans/covers-only-second/covers-only-first/four-spans-middle-two/all-same-style), `apply_selection_to_line` (23 -- no-selection-start-ge-end/full-line-no-gutter/gutter-skips-leading/gutter-clamps-col/sel-end-zero/middle-fully-selected/start-line-with-gutter/end-line-with-gutter/partial-styled-spans/preserves-all-text/gutter-larger-than-line/col-beyond-len/empty-content/multi-span-gutter-preserved/start-col-at-gutter/sel-end-at-gutter/single-char/at-very-end/at-very-start/many-small-with-gutter/gutter-equals-line-len/middle-selection-with-gutter-styled) |
| tui/draw_viewer/tabs | `src/tui/draw_viewer/tabs.rs` | 51 | `tab_bar_rows` (34 -- zero-through-thirteen/hundred tabs, boundary 0-to-1/6-to-7, all-single-row-1-to-6, all-two-row-7-to-12, never-more-than-two 0-200, large-value-usize-MAX, viewport-arithmetic, monotonic-non-decreasing, slot_w arithmetic 80-wide/name_max derivation, narrow-terminal early-return, dialog width capped-at-40/narrow/height-with-few-tabs) |
| tui/draw_viewer/git_viewer | `src/tui/draw_viewer/git_viewer.rs` | 51 | Git viewer draw functions and diff rendering state tests |
| tui/draw_viewer/edit_mode | `src/tui/draw_viewer/edit_mode.rs` | 58 | Edit mode rendering, cursor positioning, line numbering |
| tui/draw_status | `src/tui/draw_status.rs` | 52 | Badge width arithmetic (UTF-8 pipe 3-byte), right-area x bound, saturating_sub underflow, color distinctness, focus-specific hint coverage (session/:search, viewer/Tab:switch, worktrees/G:git, file-tree/h-l:collapse) |
| tui/draw_sidebar | `src/tui/draw_sidebar.rs` | 50 | Scroll math, file list rendering, selection highlighting |
| tui/draw_output | `src/tui/draw_output.rs` | 50 | Output pane rendering, bubble layout, scroll indicators |
| tui/draw_input | `src/tui/draw_input.rs` | 50 | Input pane rendering, prompt/command field display |
| tui/draw_health | `src/tui/draw_health.rs` | 50 | Health panel rendering, tab switching, scope display |
| tui/draw_god_files | `src/tui/draw_god_files.rs` | 50 | God file list rendering, selection, scroll |
| tui/draw_git_actions | `src/tui/draw_git_actions.rs` | 50 | Git panel layout, action list rendering |
| tui/draw_terminal | `src/tui/draw_terminal.rs` | 50 | Terminal pane rendering, ANSI output display |
| tui/draw_file_tree | `src/tui/draw_file_tree.rs` | 57 | File tree rendering, icon mapping, expansion state |
| tui/draw_dialogs | `src/tui/draw_dialogs.rs` | 72 | Thin module root — re-exports from 6 submodules, holds all tests |
| tui/draw_dialogs/help_overlay | `src/tui/draw_dialogs/help_overlay.rs` | 0 | Help overlay with auto-sized multi-column layout (tests in parent module) |
| tui/draw_dialogs/preset_prompt | `src/tui/draw_dialogs/preset_prompt.rs` | 0 | Preset prompt picker and editor dialogs (tests in parent module) |
| tui/draw_dialogs/run_command | `src/tui/draw_dialogs/run_command.rs` | 0 | Run command picker and editor dialogs (tests in parent module) |
| tui/draw_dialogs/table_popup | `src/tui/draw_dialogs/table_popup.rs` | 0 | Full-width table popup overlay (tests in parent module) |
| tui/draw_dialogs/welcome_modal | `src/tui/draw_dialogs/welcome_modal.rs` | 0 | Welcome modal for projects with no worktrees (tests in parent module) |
| tui/draw_dialogs/worktree_dialogs | `src/tui/draw_dialogs/worktree_dialogs.rs` | 0 | Branch dialog, delete/rename worktree dialogs (tests in parent module) |
| tui/draw_projects | `src/tui/draw_projects.rs` | 62 | Projects panel rendering, list navigation |
| tui/draw_output/render_submit | `src/tui/draw_output/render_submit.rs` | 60 | Submit/poll render thread (submit_render_request incremental/deferred/full, expanding_deferred guard prevents re-deferral, poll_render_result seq gating, deferred_start calculation, large session 300-event expansion) |
| tui/draw_output/todo_widget | `src/tui/draw_output/todo_widget.rs` | 62 | Todo list widget rendering, status icons |
| tui/draw_output/session_list | `src/tui/draw_output/session_list.rs` | 69 | Session list rendering, selection highlight |
| tui/draw_output/selection | `src/tui/draw_output/selection.rs` | 0 | Content bounds calculation (tests in parent module) |
| tui/draw_output/dialogs | `src/tui/draw_output/dialogs.rs` | 0 | Dialog overlays: new session, RCR approval, post-merge (tests in parent module) |
| tui/draw_output/git_commits | `src/tui/draw_output/git_commits.rs` | 0 | Git commit log rendering (tests in parent module) |
| tui/draw_output/viewport | `src/tui/draw_output/viewport.rs` | 0 | Viewport cache with overlays (tests in parent module) |
| tui/draw_output/session_chrome | `src/tui/draw_output/session_chrome.rs` | 0 | Session block chrome construction (tests in parent module) |
| tui/event_loop | `src/tui/event_loop.rs` | 50 | Event loop dispatch, tick handling |
| tui/event_loop/actions | `src/tui/event_loop/actions.rs` | 50 | Action dispatch, guard checks |
| tui/event_loop/actions/execute | `src/tui/event_loop/actions/execute.rs` | 50 | Action execution, state mutations |
| tui/event_loop/actions/escape | `src/tui/event_loop/actions/escape.rs` | 51 | Escape key handling across all modal states |
| tui/event_loop/actions/navigation | `src/tui/event_loop/actions/navigation.rs` | 71 | Page size math (39 -- saturating_sub 0/1/2/100), Focus variant exhaustiveness, browsing_main guards, terminal+prompt guards, dispatch function pointer type checks, App::new() integration tests, go_to_top scroll-reset |
| tui/event_loop/actions/session_list | `src/tui/event_loop/actions/session_list.rs` | 14 | Session list open (SQLite store), finish load, navigation, selection, filtering |
| tui/event_loop/actions/deferred | `src/tui/event_loop/actions/deferred.rs` | 50 | `DeferredAction` execution (50 -- LoadSession with slashes/unicode, session_search_results clearing, field extraction all 7 variants, LoadFile/OpenHealthPanel/GitCommit/GitCommitAndPush/RescanHealthScope/SwitchProject state effects, idx=usize::MAX, 50-dir stress, deep nested path, repeated calls, no-panel guards) |
| tui/event_loop/actions/rcr | `src/tui/event_loop/actions/rcr.rs` | 51 | `RcrSession` clone/fields, `make_rcr` helper consistency, status non-empty after no-merge, `accept_rcr` session clearing for both merge paths, `PostMergeDialog` field construction/selected values, focus preservation, session_id set, approval_pending, multiple independent sessions, empty slot_id, absolute path assertions |
| tui/event_loop/agent_events | `src/tui/event_loop/agent_events.rs` | 65 | `handle_agent_event` output/started/session_id/exited routing, slot independence, exit code storage, staged prompt consumption/preservation, running session tracking, multiple session IDs, large PIDs, unicode session IDs, `extract_assistant_text` (valid/missing-content/empty-content/non-text/nested/no-message/wrong-type/not-json/empty-text/multiple-blocks), `poll_compaction_agents` (no-receivers-returns-false, completed-stores-summary, in-progress-accumulates, exit-no-store-cleans-up). Compaction model selection uses `app.selected_model` directly (no hardcoded per-backend mapping) |
| tui/event_loop/fast_draw | `src/tui/event_loop/fast_draw.rs` | 53 | `word_wrap_break_points` (newline/long-line/multiple/width-1), `display_width` (single/CJK/mixed), scroll offset edge cases, padded text, adjusted row underflow, Rect coordinate math, unicode_width fallback, input_area defaults, cursor row assignment, `ratatui_to_crossterm` color mapping (ANSI/RGB/indexed), `fast_draw_session` direct cell write output |
| tui/event_loop/mouse | `src/tui/event_loop/mouse.rs` | 75 | Mouse event handling, click regions, drag selection |
| tui/input_output | `src/tui/input_output.rs` | 56 | `recompute_session_find_matches` (6 -- clears-empty-query/resets-index/finds-matches/case-insensitive/no-match-empty-cache/multiple-same-line), `jump_next/prev_match` (8 -- empty-noop/advances/wraps-end/wraps-start/updates-scroll/invalidates-viewport), `handle_session_input` routing (5 -- find-active/show-list/n-forward/N-backward/Esc-clears) |
| tui/input_health | `src/tui/input_health.rs` | 50 | Health panel input handling, tab switching, scope editing |
| tui/input_projects | `src/tui/input_projects.rs` | 50 | Projects panel input, add/rename/delete flows |
| tui/input_worktrees | `src/tui/input_worktrees.rs` | 50 | Worktree tab input, creation/archive/delete flows |
| tui/input_viewer | `src/tui/input_viewer.rs` | 78 | Viewer input handling, edit mode, selection, copy |
| tui/input_file_tree | `src/tui/input_file_tree.rs` | 54 | File tree input, navigation, expand/collapse |
| tui/input_dialogs | `src/tui/input_dialogs.rs` | 66 | Dialog input handling, confirmation, text input |
| tui/input_git_actions | `src/tui/input_git_actions.rs` | 58 | Git panel input routing, key dispatch |
| tui/input_git_actions/operations | `src/tui/input_git_actions/operations.rs` | 57 | `parse_conflict_files` (5 -- mixed/auto-merged-only/whitespace-trimming), auto-resolve boundary checks, overlay field construction (GitConflictOverlay/GitCommitOverlay/GitChangedFile/PostMergeDialog), commit message markdown fence stripping, generator label shows selected model, push note formatting |
| tui/input_git_actions/commit_overlay | `src/tui/input_git_actions/commit_overlay.rs` | 53 | Cursor Left/Right boundary (at-0/at-end), backspace/delete no-ops, insert at beginning/empty, multibyte emoji, unicode insert, Shift+Enter newline, scroll field, modifier combinations (SUPER/CONTROL/SHIFT/NONE) |
| tui/input_git_actions/diff_viewer | `src/tui/input_git_actions/diff_viewer.rs` | 54 | Panel viewer_diff set/clear, diff title prefix/short-hash/subject, file/commit selection, commits_ahead/behind_main counters, result message success/error, full_hash format |
| tui/input_git_actions/auto_resolve_overlay | `src/tui/input_git_actions/auto_resolve_overlay.rs` | 51 | Auto-resolve overlay add/delete/toggle/navigation state tests |
| tui/input_git_actions/conflict_resolution | `src/tui/input_git_actions/conflict_resolution.rs` | 82 | Navigation/abort/spawn/prompt building/state preservation across all conflict resolution states |
| tui/render_markdown | `src/tui/render_markdown.rs` | 60 | Markdown-to-spans rendering + viewer markdown (10 tests: header/bullet/code block/blockquote/table/numbered list/mixed content/empty/width/no gutter) |
| tui/render_thread | `src/tui/render_thread.rs` | 50 | Render thread message passing, try_recv |
| tui/util | `src/tui/util.rs` | 50 | TUI utility functions, truncation, formatting |
| tui/run | `src/tui/run.rs` | 22 | TUI module root: declares submodules (splash, worktree_tabs, overlays), layout constraints, input height calculation, row wrapping |
| tui/run/splash | `src/tui/run/splash.rs` | 15 | Splash screen ASCII art rendering (butterfly + logo + acronym) |
| tui/run/worktree_tabs | `src/tui/run/worktree_tabs.rs` | 21 | Worktree tab bar rendering (normal + git mode), pagination, hit-test regions, rebase indicators, color constants |
| tui/run/overlays | `src/tui/run/overlays.rs` | 18 | Popup/dialog overlays: auto-rebase, git status box, debug dump naming/saving, loading indicator |
| backend | `src/backend.rs` | 16 | `Backend` default (Claude), Copy/Clone/Debug/PartialEq, display format, from_str_loose (5 -- claude/codex/openai/unknown/empty/case-insensitive), serde roundtrip (3 -- codex/claude/unknown fails), equality (1), clone (1), debug (1), `AgentProcess::new` (1 -- constructs both backends), spawn empty prompt (3 -- claude/codex/no-model all fail) |
| claude | `src/claude.rs` | 52 | Init-line substring detection, session_id extraction from JSON, missing session_id, output data with tabs/special chars, verbose default, no API key default, SessionId UUID format/unicode |
| codex | `src/codex.rs` | 12 | Thread-started session_id extraction, missing thread_id, non-thread.started events, output data forwarding, permission flag mapping (Ignore/Approve/Ask), executable default/custom |
| codex_parser | `src/events/codex_parser.rs` | 24 | `CodexEventParser` thread.started→Init, item.started command→ToolCall, item.completed command→ToolResult, item.completed file_change→ToolResult, item.completed reasoning→AssistantText, item.completed agent_message→AssistantText, turn.completed→Complete with usage, error/turn.failed→Complete(false), partial line buffering, multi-line output, unknown event types ignored |
| codex_session_parser | `src/app/codex_session_parser.rs` | 45 | `parse_codex_session_file` session_meta→Init, user message→UserMessage, assistant message→AssistantText, function_call→ToolCall (shell_command/apply_patch/custom_tool), function_call_output→ToolResult (matched by call_id), agent_reasoning→AssistantText, task_complete→Complete, turn_context→model extraction, incremental parsing with end_offset, empty/nonexistent/invalid-json files, array vs string content formats, workdir extraction from shell_command, file path extraction from apply_patch |
| cli | `src/cli.rs` | 51 | CLI argument parsing, subcommand routing |
| stt | `src/stt.rs` | 51 | Speech-to-text event handling |
| syntax | `src/syntax.rs` | 50 | Syntax highlighting, theme loading |
| install | `src/install.rs` | 6 | Self-install logic: PATH detection, binary copy, shell profile update |
| main | `src/main.rs` | 50 | Main entry point, CLI dispatch |
| app/input | `src/app/input.rs` | 80 | Input handling, key routing, mode switching |
| app/state/output | `src/app/state/output.rs` | 51 | Output state management, event buffering |
| app/state/session_names | `src/app/state/session_names.rs` | 40 | `save_session_name` (10 -- creates-via-store/updates-via-store/non-numeric-id-ignored/empty-name-clears/unicode-name/numeric-string-parsed/no-store-noop/multiple-sessions/overwrite-existing/preserves-others), `load_all_session_names` (10 -- empty-store/single-session/multiple-sessions/default-s-prefix/custom-name-overrides/no-store-returns-empty/after-rename/mixed-named-unnamed/special-chars/sequential-ids), integration (20 -- round-trip-save-load/rename-reflects-in-load/multiple-renames/load-after-create/empty-name/save-nonexistent-session/load-only-named/unicode-round-trip/many-sessions/id-format) |
| cmd/project | `src/cmd/project.rs` | 52 | Project CLI subcommands |
| cmd/session | `src/cmd/session.rs` | 59 | Session CLI subcommands |

### Test Categories

- **Pure function unit tests** -- Parsing, formatting, ANSI stripping, TOML key validation, branch prefix handling, config defaults, JSON deserialization/round-trips, markdown span parsing, text wrapping, output colorization, tool result rendering, type constructors/methods/trait impls, path noise classification, event classification, coordinate mapping, viewer text wrapping with styled spans, selection highlighting with gutter offsets, tab bar row calculation, UUID formatting, ordered key parsing, file tree building/sorting/filtering, source extension/skip dir constants, god file scanning/threshold/prompt generation, doc coverage scanning/heuristics, image extension detection, modularize prompt language-specific style embedding
- **State logic tests** -- Plan approval detection, worktree status derivation, incremental parser state, todo parsing, dialog navigation/filtering/input editing, App constructor defaults, focus cycling, unified model pool cycling (9 models, backend derived from selection), token badge computation, session/slot running queries, branch-for-slot lookup, viewer tab management, run command/preset prompt pickers, edit line finding, session output state reset, skill tool extraction from events, file tree navigation next/prev/first-sibling/last-sibling, file operations add/rename/delete/copy/move, session navigation next/prev/first/last with wrap-around, main branch archive/delete guards, collect_source_files recursive scanning with skip dirs, backend dispatch for session discovery
- Integration tests for git operations (worktree create/delete/list) — future
- Integration tests for session discovery from git state — future
- E2E tests for TUI event handling (would require mock terminal) — future

# REFERENCES

(None fetched yet)

---

## **CONFLICTS**

(None)

# USE

## Installation

### Pre-built Binaries (Self-Installing)

Download the latest binary from [Releases](https://github.com/xCORViSx/AZUREAL/releases) and run it. The binary automatically installs itself to your PATH on first run — no manual setup needed.

- **macOS/Linux:** Installs to `/usr/local/bin/azureal` (or `~/.local/bin/` if not writable). May prompt for `sudo`.
- **Windows:** Installs to `%USERPROFILE%\.azureal\bin\azureal.exe` and adds to user PATH.

After install, run `azureal` from any terminal.

### From Source

```bash
cargo install --path .
```

## Running

```bash
# Launch the TUI
azureal tui

# Or simply
azureal
```

## Keybindings

### Global (Command Mode)
| Key | Action |
|-----|--------|
| `p` | Enter prompt mode (focus input) |
| `T` | Toggle terminal pane |
| `G` | GitView panel |
| `H` | Health panel |
| `M` | Browse main branch |
| `P` | Projects panel |
| `r` | Run command |
| `R` | Add run command |
| `[`/`]` | Switch worktree tab (works from any pane, including main browse) |
| `j/k` | Navigate / scroll line |
| `J/K` | Page scroll (Viewer/Session/Terminal) |
| `Tab`/`Shift+Tab` | Cycle focus forward/backward (FileTree → Viewer → Session → Input) |
| `f` | Toggle file tree |
| `?` | Help |
| `⌘c` / `Ctrl+C` | Copy selection |
| `⌃c` / `Alt+C` | Cancel agent |
| `⌃m` / `Ctrl+M` | Cycle model (opus → sonnet → haiku) |
| `⌃q` / `Ctrl+Q` | Quit |

### Worktrees (`W` Leader Sequence)

Destructive worktree actions are behind a two-key leader sequence: press `Shift+W`, then the action key. The status bar shows `[W …]` while waiting. Press `Esc` to cancel. State tracked via `LeaderState` enum on App (`None` / `WaitingForAction`).

| Key | Action |
|-----|--------|
| `Wa` | Add worktree (open branch dialog) |
| `Wr` | Rename worktree (branch rename dialog) |
| `Wx` | Archive worktree |
| `Wd` | Delete worktree |

### FileTree Pane
| Key | Action |
|-----|--------|
| `j/k` | Navigate up/down |
| `⌥↑/⌥↓` (`Alt+↑/↓`) | Jump to first/last sibling in current folder |
| `Enter` | Open file in Viewer / Expand directory |
| `h/l` | Collapse/Expand directory |
| `⌥→/⌥j` (`Alt+→/j`) | Recursive expand directory and all subdirs |
| `⌥←/⌥k` (`Alt+←/k`) | Recursive collapse directory and all subdirs |
| `Space` | Toggle directory expand |
| `a` | Add file (trailing `/` creates directory) |
| `d` | Delete selected file/directory (y/N confirm) |
| `r` | Rename selected file/directory |
| `c` | Copy selected file/directory (clipboard-style: navigate to target dir, Enter to paste) |
| `m` | Move selected file/directory (clipboard-style: navigate to target dir, Enter to paste) |
| `O` | Options overlay (toggle visibility of `.git`, `.claude`, `.azureal` dirs) |
| `Esc` | Move focus to Session |

### Viewer Pane
| Key | Action |
|-----|--------|
| `j/k` | Scroll up/down |
| `J/K` | Page scroll (viewport minus 2 overlap) |
| `⌥↑/⌥↓` (`Alt+↑/↓`) | Jump to top/bottom |
| `⌥←/⌥→` (`Alt+←/→`) | Prev/next Edit (syncs Session scroll) |
| `⌘A` / `Ctrl+A` | Select all (then `⌘C`/`Ctrl+C` to copy) |
| `t` | Tab current file (save to tab list) |
| `⌥t` / `Alt+T` | Open tab dialog (browse/switch tabs) |
| `x` | Close current tab |
| `Esc` | Exit viewer (restores previous content if in Edit diff view) |

### Session Pane
| Key | Action |
|-----|--------|
| `j/k` | Scroll line |
| `↑/↓` | Jump to prev/next message (user + assistant) |
| `Shift+↑/↓` | Jump to prev/next user prompt only |
| `J/K` | Page scroll (viewport minus 2 overlap) |
| `⌥↑/⌥↓` (`Alt+↑/↓`) | Jump to top/bottom |
| `a` | Add session (clear state, enter prompt mode — also works from session list overlay) |
| `r` | Rename selected session (in session list overlay — centered dialog, Enter saves, Esc cancels) |
| `s` | Toggle Session list overlay (browse all session files) |
| `/` | Search text in current session (yellow highlights, `[N/M]` counter) |
| `n/N` | Next/prev search match (after `/` search confirmed with Enter) |
| `Esc` | Return to FileTree |

**Clickable File Paths:** Edit, Read, and Write tool file paths are underlined in orange and clickable. Clicking an Edit path opens the full file in the Viewer with the edit region highlighted (red background for deleted lines, green background for added lines) and sets the `selected_tool_diff` index so `⌥←/⌥→` cycling continues from that position. Clicking a Read or Write path opens the file plain in the Viewer. The clicked/cycled path is highlighted with inverted colors (orange background, black text) in the Session pane — highlight covers all wrapped continuation lines via `wrap_line_count` field in `ClickablePath`. Clicking a continuation line of a wrapped path also triggers the file open. Use `⌥←/⌥→` in the Viewer to cycle through edits (also syncs Session scroll and sets the highlight). The border title shows `[Edit N/M]` where N is the current edit-only position and M is the total number of Edit tool calls (excludes Read/Write). The last 20 Edit calls also show inline diff previews in the Session pane.

**Clickable Tables:** Markdown tables in assistant messages are clickable. Clicking anywhere on a table opens a centered popup overlay that re-renders the table at near-terminal width (terminal width minus 8 chars) so columns aren't truncated. The popup has an AZURE double border, "Table" title, scroll support (`j`/`k`/arrows), and dismiss (`Esc`/`q`/click outside). Table regions are tracked via `ClickableTable = (cache_line_start, cache_line_end, raw_markdown)` alongside `ClickablePath` through the background render pipeline — `render_assistant_text()` returns both rendered lines and table regions, which are offset to absolute cache positions and threaded through `RenderResult` → `poll_render_result()` → `app.clickable_tables`. The popup calls `render_table_for_popup()` which reuses `scan_tables()` + `render_table_row()` at the wider width. State is `app.table_popup: Option<TablePopup>`, cleared on session switch.

### Prompt Mode (Input Focused)

Prompt keybindings are displayed directly in the Input pane's title bar (not in the help panel). All title hints are dynamically sourced from the `INPUT` binding array via `find_key_for_action()` / `find_key_pair()` — changing a key in the array automatically updates the title. When the terminal is too narrow for the full title, `split_title_hints()` packs as many hint segments as fit on the top border, then overflow hints go on the bottom border in parentheses with the same style (color + bold) as the top title.

**Type mode title shows (macOS):** `(Esc:exit | Enter:submit | ⇧Enter:newline | ⌃c:cancel agent | ↑/↓:history | ⌥ ←/→ :word | ⌃w:del wrd | ⌃s:speech | ⌥p:presets)`
**Type mode title shows (Windows/Linux):** `(Esc:exit | Enter:submit | Shift+Enter:newline | Ctrl+c:cancel agent | ↑/↓:history | Alt+ ←/→ :word | Ctrl+w:del wrd | Ctrl+s:speech | Alt+p:presets)`
**Command mode title shows (macOS):** `(p:PROMPT | T:TERMINAL | G:Git | H:Health | M:main | w␣r:run | ⌃c:cancel agent | ⌃q:quit | ?:help)`
**Command mode title shows (Windows/Linux):** `(p:PROMPT | T:TERMINAL | G:Git | H:Health | M:main | w␣r:run | Alt+c:cancel agent | Ctrl+q:quit | ?:help)`

### Terminal Mode

Terminal keybindings are displayed directly in the terminal pane's title bar (not in the help panel). All title hints are dynamically sourced from the `TERMINAL` binding array via `find_key_for_action()` / `find_key_pair()` — changing a key in the array automatically updates the title.

**Command mode title shows (macOS):** `(t:type | p:prompt | Esc:close | j/k:scroll | J/K:page | ⌥↑/⌥↓:top/bottom | +/-:resize)`
**Command mode title shows (Windows/Linux):** `(t:type | p:prompt | Esc:close | j/k:scroll | J/K:page | Alt+↑/Alt+↓:top/bottom | +/-:resize)`
**Type mode title shows (macOS):** `(Esc:exit | ⌥←/→:word)`
**Type mode title shows (Windows/Linux):** `(Esc:exit | Alt+←/→:word)`
**Scroll mode title shows (macOS):** `[N↑] (j/k:scroll | J/K:page | ⌥↑:top | ⌥↓:bottom | t:type | Esc:close)`
**Scroll mode title shows (Windows/Linux):** `[N↑] (j/k:scroll | J/K:page | Alt+↑:top | Alt+↓:bottom | t:type | Esc:close)`
