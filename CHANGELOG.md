# Changelog

All notable changes to Azureal will be documented in this file.

## [Unreleased]

### Fixed
- **Git panel shows gitignored files** — Tracked-but-gitignored files (e.g., `.DS_Store`) appeared in the Git Actions changed files list because `git diff HEAD` still reports them. `get_diff_files()` now filters all paths through `git check-ignore --stdin` to drop gitignored entries.
- **Scope overlay title says "God File Scope" instead of "Health Scope"** — The scope mode overlay in the file tree still showed the old "God File Scope" title after the scope feature was promoted to panel-level (shared across God Files and Documentation tabs). Updated to "Health Scope" to match the new general-purpose scope semantics.
- **Compaction summary renders as raw user message instead of banner** — When Claude auto-compacts context, the multi-page compaction summary ("This session is being continued...") was displayed as a regular user prompt bubble instead of the compact `⏳ Compacting context...` banner. Added compaction detection to the live stream `EventParser`, `add_user_message()`, and as a render-path safety net in `render_events.rs`. Also filters `<local-command-stdout>` and `<local-command-caveat>` in the live stream parser (previously only handled in the session file parser).
- **Convo pane stops scrolling before reaching the top of conversation** — Navigating upward via `↑` (bubble jump) or regular scroll in large conversations would stop at the deferred-render boundary instead of expanding to show earlier messages. Root cause: deferred render (last 200 events on initial load) only expanded when `output_scroll == 0`, but neither `scroll_output_up()` nor `jump_to_prev_bubble()` set `rendered_lines_dirty` when reaching scroll 0 — so `submit_render_request()` never triggered the expansion. Fix: both functions now set `rendered_lines_dirty = true` when hitting scroll 0 with deferred events pending.
- **User messages disappear from convo pane during streaming** — User prompts would vanish after sending: they either never appeared, disappeared when Claude's first response arrived, or only reappeared after the agent exited or the app restarted. Root cause: `pending_user_message` was a render-layer bandaid cleared on first assistant/tool event, leaving a gap where the message was invisible (stream-json stdout doesn't emit user events, and session file polling is blocked during streaming). Fix: `add_user_message()` now pushes a real `DisplayEvent::UserMessage` into `display_events` at prompt submit time so it renders immediately and persists throughout the conversation. `pending_user_message` retained only as a dedup marker for the full re-parse on Claude exit. Removed the early-clear logic from `claude.rs` and the pending bubble trim/append machinery from `render_submit.rs`.
- **Convo pane Up/Down navigation jumps to extremes instead of stepping incrementally** — Plain Up/Down was only jumping between user prompts (skipping assistant messages), causing apparent jumps from "1/160" to "160/160" when there were many assistant messages between user prompts. Swapped the semantics: plain `↑/↓` now steps through ALL message bubbles (user + assistant) incrementally, `Shift+↑/↓` jumps between user prompts only. Also scrolls 2 lines above bubble headers to show spacer context, and re-engages auto-follow sentinel when hitting the bottom instead of scrolling to `max_scroll`.
- **Archive worktree blocked on main branch** — Pressing `a` when the main branch is selected now shows "Cannot archive main branch" instead of destroying the primary worktree. Guard added in `archive_current_worktree()`.
- **Git merge direction fixed** — `merge_from_main()` renamed to `merge_into_main()` and rewritten to merge the feature branch INTO main by running from the repo root (which is always checked out on main). Previously merged main into the feature branch. `GitActionsPanel` now stores `repo_root` (project path) for this operation. Keybinding label changed from "Merge from main" to "Merge to main".
- **STT toggle in edit mode** — `⌃s` (speech-to-text) in viewer edit mode now dispatched via `execute_action()` instead of a raw handler in `input_viewer.rs`. The raw handler never fired because `lookup_action()` intercepted the key first. Now works correctly from both edit mode and prompt mode.

### Added
- **Debug build indicator** — The CPU|PID badge in the status bar now renders in AZURE (`#3399FF`) for debug builds and DarkGray for release builds, providing a quick visual indicator of the active build profile. Single `cfg!(debug_assertions)` check in `draw_status.rs`.
- **Git Commit overlay** — Press `c` in the Git Actions panel (`Shift+G`) to commit. Stages all changes (`git add -A`), gets `git diff --staged`, spawns `claude -p` one-shot in a background thread to generate a conventional commit message (~3 sec, "Generating..." shown while working). Displays an editable overlay dialog on top of the Git panel with the generated message. `Enter` commits, `⌘P` commits + pushes (both show loading indicator via deferred action pattern), `Shift+Enter` inserts a newline, `Esc` cancels. Full text editing with word-wrap: type, backspace, delete, left/right arrows, up/down line navigation, home/end. Session persistence disabled via `--no-session-persistence` (no stale session files); markdown code fences stripped from output. No streaming — just `std::process::Command` stdout capture.
- **Auto-rebase on Claude exit** — `A` (Shift+A) in the Git panel toggles automatic rebase when a Claude session finishes. Toggling Yes opens a scope picker: "This worktree" or "All worktrees". Toggling No is immediate. Action row shows `[Yes]`/`[No]` badge. State persisted to each **worktree's own** `.azureal/azufig.toml` `[git]` section (`auto-rebase = "yes"/"no"`). "All worktrees" iterates every active worktree and writes to each config individually — no sentinel keys, direct per-worktree edits. On Claude exit, `auto_rebase_on_exit()` reads the worktree-local config and runs `Git::rebase_onto_main()` (skipped on main branch). Result shown as status message.
- **Generic loading indicators** — Centered AZURE-bordered popup shown during all blocking I/O operations so the UI never appears frozen. Uses a `DeferredAction` enum + two-phase deferred draw pattern: set loading state → `terminal.draw()` renders popup → event loop executes expensive action on next frame → triggers redraw with result. Five operations converted: session load (`"Loading session…"`), file open (`"Loading <filename>…"`), health panel open (`"Scanning project health…"`), project switch (`"Switching project…"`), health scope rescan (`"Rescanning health scope…"`). Reusable `draw_loading_indicator()` function and `execute_deferred_action()` dispatcher.
- **Health panel page scroll + mouse wheel** — `J/K` (Shift+J/K) now pages through both God Files and Documentation lists by one viewport height. Mouse wheel scrolls the active tab's list — the health modal intercepts all scroll events while open so scrolling works regardless of cursor position. Page size derived from actual terminal window height (`screen_height`), not the embedded terminal pane height (`terminal_height`).
- **Todo widget height cap + scrollbar** — Tasks widget at bottom of Convo pane now caps at 20 visual lines (including wrapped text). When content overflows, a proportional scrollbar column (AZURE `█` thumb on `│` track) appears on the rightmost position and the widget responds to mouse wheel scrolling. Scroll offset resets when todos are updated. `pane_todo` rect cached for mouse hit-testing, checked before `pane_convo` in scroll dispatch.
- **FileTree Options overlay** — Press `O` in the FileTree to switch the pane into an options view with QuadrantOutside AZURE border and "Filetree Options" title. Five toggleable filters: `worktrees`, `.git`, `.claude`, `.azureal`, `.DS_Store` (all hidden by default). `j/k` navigate, `Space`/`Enter` toggle visibility, `Esc`/`O` close. Hidden names stored in `file_tree_hidden_dirs: HashSet<String>` replacing the old `hide_dot_git: bool`. Tree rebuilds immediately on toggle. **State persisted to project azufig.toml** — toggles survive restarts.
- **Unified `azufig.toml` config** — All persistent state consolidated from 6+ individual files into two TOML files named `azufig.toml`. **Global** (`~/.azureal/azufig.toml`): app config, registered projects, global run commands, global preset prompts. **Project-local** (`.azureal/azufig.toml`): filetree hidden dirs, custom session names, health scan scope, project-local run commands, project-local preset prompts. Clean flat format: every section uses single-bracket `[section]` with `key = "value"` pairs (e.g., `ProjectName = "~/path"`, `CmdName = "command"`). Keys matching the TOML bare-key pattern (`A-Za-z0-9_-`) are written unquoted; keys with spaces or special chars stay quoted. `#[serde(default)]` for forward-compatibility. Write pattern: load-modify-save to prevent clobbering unrelated sections.

### Changed
- **Health scope promoted to panel-level** — The scope feature (`s` key) moved from God Files tab to the shared health panel border (top-right `s:scope` label, mirrored by `Tab:tab` on top-left). Now accessible from any tab (God Files, Documentation). `[godfilescope]` section renamed to `[healthscope]` in azufig.toml (old name auto-migrated via `#[serde(alias = "godfilescope")]`). Internal renames: `load_god_file_scope` → `load_health_scope`, `save_god_file_scope` → `save_health_scope`, `AzufigGodFileScope` → `AzufigHealthScope`, `RescanGodFileScope` → `RescanHealthScope`. `s` binding moved from `HEALTH_GOD_FILES` (now 4 entries) to `HEALTH_SHARED` (now 9 entries). `HealthScopeMode` action handler moved from God Files tab-only to panel-level in `input_health.rs`.
- **Debug dump naming dialog** — `⌃d` now opens a naming dialog before saving. Enter with empty name saves as `debug-output`; typing a name saves as `debug-output_<name>` (e.g., `debug-output_scroll_bug`). Esc cancels. Centered AZURE-bordered popup with live filename preview. A "Saving…" indicator is shown while the dump I/O runs (two-phase deferred draw) so the app doesn't appear frozen on large sessions.
- **Command box title `⌃d:debug` → `⌃d:dump debug output`** — More descriptive hint label in command mode title bar.
- **Run command & preset prompt ordering persisted** — `[runcmds]` and `[presetprompts]` keys in azufig.toml now include a 1-based position prefix (`N_Name = "value"`) so quick-select numbers (1-9, 0) survive restarts. Prefix stripped on load, re-written on save. Keys without a prefix (from older configs) sort to the end.
- **Hide .git toggle moved from Git panel to FileTree Options** — The `H:hide .git ✓/✗` toggle and top-right indicator removed from the Git panel. Directory visibility is now managed via the FileTree Options overlay (`O`) which supports `.git`, `.claude`, and `.azureal` directories instead of just `.git`.
- **All state files consolidated into azufig.toml** — `~/.azureal/projects`, `~/.azureal/config`, `~/.azureal/runcmds`, `~/.azureal/presetprompts` → `~/.azureal/azufig.toml`. `.azureal/sessions`, `.azureal/runcmds`, `.azureal/presetprompts`, `.azureal/godfilescope` → `.azureal/azufig.toml` (now `[healthscope]` section). Old files preserved for manual cleanup after migration.

### Refactored
- **`Session` struct renamed to `Worktree` across 31 source files** — Domain model renamed to accurately reflect that the left-panel entries are git worktrees (not Claude sessions). `Session` → `Worktree`, `SessionStatus` → `WorktreeStatus`, `SessionAction` → `WorktreeAction`, `App.sessions` → `App.worktrees`, `current_session()` → `current_worktree()`, `load_sessions` → `load_worktrees`, `refresh_sessions` → `refresh_worktrees`, `archive_current_session` → `archive_current_worktree`, `rebase_current_session` → `rebase_current_worktree`, `discover_sessions` → `discover_worktrees`, `find_session` → `find_worktree` (CLI cmd). Claude session concepts intentionally NOT renamed: `session_file_path`, `session_parser`, `session_names`, `session_files`, `session_list`, `claude_session_ids`, and the CLI `session` subcommand — these correctly refer to Claude conversation sessions, not git worktrees.
- **Split `god_files.rs` into `health` module** — The monolithic 946-line `src/app/state/god_files.rs` (god file scanning + doc coverage + DH sessions + scope mode + modularization) split into a file-based module root with 2 submodules: `src/app/state/health.rs` (shared constants: ~60 source extensions, ~55 skip dirs, ~15 source roots; open/close panel; scope persistence), `health/god_files.rs` (god file scanning, scope mode, modularize with module style selector), `health/documentation.rs` (doc coverage scanner, DH session spawning, toggle/view). `state.rs` declares `mod health` instead of `mod god_files`.
- **Converted `src/cli/mod.rs` → `src/cli.rs` and `src/cmd/mod.rs` → `src/cmd.rs`** — Both legacy `mod.rs` module roots migrated to modern file-based roots. `cmd/` submodules (`session.rs`, `project.rs`) remain in place; `cli/` had no submodules so the directory was removed entirely. Zero code changes needed — Rust resolves the new layout automatically.
- **Centralized ALL modal panel keybindings into `keybindings.rs`** — Health, Git, Projects, pickers, context menu, and branch dialog panels now use per-modal lookup functions (`lookup_health_action()`, `lookup_git_action()`, `lookup_projects_action()`, `lookup_picker_action()`, `lookup_context_menu_action()`, `lookup_branch_dialog_action()`) instead of raw `match (key.modifiers, key.code)`. ~20 new Action variants added (`HealthSwitchTab`, `GitRebase`, `ProjectsAdd`, `PickerQuickDelete`, `BranchDialogOpen`, etc.). Draw functions source footer hints and labels from hint generators (`health_god_files_hints()`, `git_actions_labels()`, `projects_browse_hint_pairs()`, etc.) instead of hardcoded strings. Text input, number quick-select (1-9), and confirm-delete (y/n) stay raw. Changing a modal keybinding now requires a single edit in `keybindings.rs` — input handler, draw function, and help panel all update automatically.

### Added
- **Documentation Health [DH] session spawning** — Documentation tab now mirrors the God Files tab workflow: `Space` to check files, `a` to check all non-100% files (toggles), `v` to view checked in Viewer, `Enter` to spawn concurrent `[DH]` Claude sessions that add missing doc comments. Each session gets a prompt with the file's current coverage ratio and instructions to add `///`/`//!` comments without modifying executable code. Session naming hint shown at top of tab.
- **Module style selector for GFM** — When modularizing checked files that include `.rs` or `.py`, a pre-spawn dialog asks which module convention to use. **Rust**: file-based root (`modulename.rs` + `modulename/`, modern) vs directory module (`modulename/mod.rs`, legacy). **Python**: package (`__init__.py`) vs single-file modules. The choice is embedded in each file's Claude prompt. Languages without dual-style conventions (Go, Java, TypeScript, etc.) skip the dialog and spawn immediately. `Space` toggles styles, `j/k` navigates rows, `Enter` confirms, `Esc` cancels.
- **Keybinding enforcement hooks** — Project-level `.claude/scripts/enforce-keybindings.sh` runs as a PreToolUse hook on every Edit/Write. Catches 3 violation types: (1) raw `KeyCode::`/`KeyModifiers::` in `input_*.rs` → must use `lookup_*_action()`, (2) hardcoded key label strings in `draw_*.rs` without keybindings:: import → must use hint generators, (3) new static binding arrays in `keybindings.rs` → reminds to create companion lookup function, hint generators, and update help_sections(). Configured in `.claude/settings.json`.
- **Worktree Health Panel** — `Shift+H` (global) opens a tabbed modal with two health-check tabs. **God Files tab**: existing god file scanner (files >1000 LOC) with check/scope/modularize/view actions. **Documentation tab**: scans all source files for doc-comment coverage (`///` and `//!`), showing per-file coverage percentage with visual bars sorted worst-first; overall score color-coded (green/yellow/red); `Enter` opens file in Viewer. `Tab` key switches between tabs.
- **Documentation coverage scanner** — Line-based heuristic counts documentable items (`fn`, `struct`, `enum`, `trait`, `const`, `static`, `type`, `impl`, `mod`) and checks for preceding `///` or `//!` doc comments. Per-file and overall coverage percentages computed. Reuses God Files' source-root detection, skip dirs, and scope persistence.
- **Concurrent Claude sessions per worktree** — Multiple Claude processes can now run simultaneously on the same branch. Session state maps (`claude_receivers`, `running_sessions`, `claude_exit_codes`, `claude_session_ids`) are now keyed by **PID string** instead of branch name. New `branch_slots` and `active_slot` maps track which PIDs belong to each branch and which one's output is displayed. Sending a prompt while Claude is running spawns a second process instead of cancelling the first. `⌃c` cancels only the active slot. When the active slot exits, auto-switches to the next remaining slot.
- **Parallel GFM spawning** — God File Modularization now spawns ALL checked files simultaneously as concurrent Claude sessions, replacing the sequential queue (`god_file_queue` removed). Each file gets its own PID slot; the newest spawn becomes the active slot.
- **Session list status dots** — Each session in the session list overlay (`s`) now shows a status dot: green `●` when a Claude process is actively running that session, dim gray `○` when idle. Mirrors the worktree sidebar dots but per-session instead of per-worktree.
- **Preset prompt quick-select from prompt mode** — `⌥1`-`⌥9` and `⌥0` directly load preset prompts without opening the picker first. Same number mapping as the picker (1-9 for presets 0-8, 0 for preset 9). Picker footer shows hint about this shortcut.
- **Run command dual-scope persistence** — Run commands can now be **global** (`~/.azureal/runcmds`, shared across all projects) or **project-local** (`.azureal/runcmds`). Toggle scope with `⌃s` in add/edit dialog. Picker shows G/P badge per command.
- **Run command delete with confirmation** — Delete key changed from `x` to `d` with y/n confirmation prompt in the title bar (matches preset prompts UX).
- **Title hint overflow to bottom border** — When the Input or Terminal pane is too narrow for all keybinding hints, `split_title_hints()` packs as many hint segments as fit on the top border after the mode label, then puts remaining segments on the bottom border in dim gray via ratatui's `.title_bottom()`. No content shifting or padding needed.
- **Image viewer** — Opening an image file (PNG, JPG, GIF, BMP, WebP, ICO) from FileTree renders it in the Viewer pane via terminal graphics protocol. Uses `ratatui-image` crate with auto-detection: Kitty protocol on Ghostty/Kitty, Sixel on iTerm2, unicode halfblock fallback on all other terminals. Image auto-fits the viewport; no scroll, selection, or edit mode. `Picker` lazy-inits once to detect terminal capabilities.
- **Nerd Font file tree icons** — File tree now uses Nerd Font glyphs as primary icons (~60 file types with language-brand colors: Rust orange, Python blue, JS yellow, etc.). Checks filename first (Dockerfile, Makefile, LICENSE, Cargo.toml, etc.) then extension. Auto-detected on startup via terminal cursor probe (`detect_nerd_font()` prints a PUA glyph and measures cursor advance via DSR). Emoji fallback used automatically when Nerd Font not detected; status bar shows informational message.
- **Hide .git toggle in Git panel** — Top-right corner of the Git panel shows `H:hide .git ✓` (green) or `H:hide .git ✗` (red). Press `H` (Shift+H) to toggle `.git` directory visibility in the file tree. Hidden by default. Rebuilds file tree immediately on toggle.
- **God File view action** — Press `v` in the God File panel to open all checked files as Viewer tabs. Fills available tab slots up to the 12-tab maximum; skips files already open in a tab. Closes the panel and focuses the Viewer on the last opened file. Status message reports how many files were opened, already tabbed, or skipped due to tab capacity.
- **God File source-root detection** — Scanner now detects well-known source directories (`src/`, `lib/`, `crates/`, `cmd/`, `pkg/`, `internal/`, `app/`, `packages/`, etc.) and only scans inside those + top-level files. Prevents non-source folders (refs, assets, data, examples, docs, etc.) from polluting results with thousands of irrelevant hits. Falls back to full-project scan when no recognized source roots exist. Skip list expanded from 11 to ~55 directories covering build artifacts, dependency caches, IDE dirs, and common non-source content.
- **God File scanner: ~60 language extensions** — Expanded from 22 to ~60 source file extensions covering systems (Rust, Go, C/C++, Obj-C, Swift, Zig, Nim, Crystal, V, D), JVM (Java, Kotlin, Scala, Groovy), .NET (C#, F#, VB), web (JS/TS variants, Vue, Svelte, Astro), scripting (Python, Ruby, Perl, PHP, Lua, R, Julia, Dart), functional (Haskell, OCaml, Clojure, Elixir, Erlang, Elm, Gleam, Racket), shell (sh/bash/zsh/fish), infra (SQL, Terraform, HCL), and schema (protobuf, Thrift, GraphQL).
- **God File scope mode with persistence** — Press `s` in the God File panel to open the FileTree in scope mode, showing which directories are included in the scan scope. Scoped directories highlighted bright green with green icons; files inside scope dimmed green; everything else dimmed gray. Green double-line border with "God File Scope (N dirs)" title and "Enter:toggle  Esc:save & rescan" bottom hint. Press Enter on any directory to add/remove it from the scan scope. Esc **persists scope to `.azureal/godfilescope`** (one absolute path per line) and rescans with the modified scope. Persisted scope auto-loaded on next panel open — no need to re-enter scope mode unless changing it. File modification actions (add, delete, rename, copy, move) disabled during scope mode. Subdirectories of accepted dirs automatically inherit accepted status (bright green) — acceptance propagates down the tree.

### Changed
- **Space key displays as "Space" in help panel** — `KeyCombo::display()` now renders `KeyCode::Char(' ')` as the word "Space" instead of a literal invisible space character.
- **Help panel trimmed** — Removed Health (shared + God Files + Documentation), Git, and Projects sections from the help overlay since all their keybindings are already visible in their own panel footers.
- **Health panel remembers last tab** — Reopens on whichever tab (God Files or Documentation) was active when closed. Stored in `App::last_health_tab`.
- **Documentation tab Enter label** — Changed from "spawn" to "complete" in footer hints and binding descriptions.
- **Health panel footer hints green** — Footer action hints now render in `GF_GREEN` (matching border) instead of dim gray.
- **God File panel → Worktree Health Panel** — Standalone God File panel refactored into a tab within the new Worktree Health Panel. Keybinding moved from `g` (Worktrees-only) to `Shift+H` (global). Panel title changed from "God Files" to "Worktree Health". All existing God File functionality preserved as the first tab.
- **God File panel color: azure → green** — Border, title, selected row highlight, and checkboxes all changed from AZURE (#3399FF) to GF_GREEN (`Rgb(80,200,80)`) — matching the scope mode overlay green for a cohesive God File visual identity.
- **Azufig files renamed to `.toml` extension** — `azufig` → `azufig.toml` for both global (`~/.azureal/azufig.toml`) and project-local (`.azureal/azufig.toml`). Auto-migration: on first load, old extensionless `azufig` files are renamed to `azufig.toml` if the new file doesn't exist yet. Proper TOML extension improves editor syntax highlighting and file type recognition.
- **God File scope mode keybinding: `f` → `s`** — Renamed from "filter" to "scope" for clarity. Scope now persists to project azufig.toml `[godfilescope]` and auto-loads on panel open.
- **Viewer tab bar: 2-row fixed-width layout** — Tab bar now supports up to 12 tabs across 2 rows (6 per row). Each tab has a fixed width (`inner_width / 6`) for uniform appearance. Viewport height automatically adjusts for tab bar rows; content padding prevents tab bar from overlaying real content. Max 12 tabs enforced with status message on overflow.
- **Terminal toggle changed from `t` to `T` (Shift+T)** — frees lowercase `t` for viewer tab operations without needing guard logic. Command pane title now shows `T:TERMINAL | G:Git` alongside other global hints.
- **Viewer tab dialog changed from `T` to `⌥t`** — avoids conflict with the new global `T` terminal toggle. `⌥t` also closes the dialog (toggle).
- Command pane title `G:GIT` changed to `G:Git` for consistent casing
- FileTree overlay title now shows **worktree name**: `Filetree (worktree_name)` instead of plain `Filetree`
- `G` (Git Actions) is now a **global keybinding** — opens from any pane, not just Worktrees (skipped in prompt mode, edit mode, terminal mode, filter, context menu, wizard)
- `G` now **toggles** Git Actions panel (close if open, open if closed) instead of only opening
- Git Actions panel secondary text (headers, key hints, separators, footer, stat slashes) changed from gray to **Git brown** (`#A0522D` sienna, `GIT_BROWN` constant) for a warmer Git-themed color scheme
- Git Actions panel diff counts (`+N/-N`) now **color-coded**: green for additions, red for deletions (orange override when row is selected); header totals also green/red
- Git Actions panel changed files now shows **working tree changes** (staged + unstaged vs HEAD) instead of committed divergence from main branch — shows what you're actively working on, including untracked files (`?` in magenta)
- Git Actions panel border changed from Rounded to QuadrantOutside (`▛▀▜▌ ▐▙▄▟`) for a distinct chunky look
- **Extensionless config files** — All app-created files are now extensionless plain documents: `sessions` (was `.toml`), `runcmds` (was `.json`), `presetprompts` (was `.json`), `projects` (was `.txt`), `debug_output` (was `.txt`), `config` (was `.toml`). Internal format unchanged — just no file extensions. **Note:** All these files (except `debug_output`) are now consolidated into `azufig.toml` files.
- **Removed stash/stash pop from Git Actions** — Worktree-based workflow eliminates the need for stash (each branch has its own working directory). Reduced action count from 7 to 5; removed `Git::stash()` and `Git::stash_pop()` methods, `s`/`S` keybindings, and Enter-on-index 5/6 paths.
- **Git Actions panel renamed to Git** — Panel title changed from "Git Actions: branch" to bold "Git: branch" for brevity.
- **God File panel changed from full-screen blackout to overlay** — Now renders as a centered modal on top of normal panes, same size as the Git panel (55% × 70%, min 50×16). Border changed from Rounded to QuadrantOutside with bold azure title.
- **3-pane layout changed from fixed to percentage-based** — Worktrees (15%), Viewer (50%), Convo (35%) replace the old fixed-width layout (40px / remaining / 80px). Panes now maintain consistent relative proportions across all terminal sizes. Convo content (bubbles, tool results, markdown) reflows dynamically to fit the actual pane width — messages wrap into more lines on narrower terminals instead of overflowing.
- **8 redundant actions removed** — Deleted `EnterInputMode` (`i`), `OpenContextMenu` (`Space`), `SelectNextProject`/`SelectPrevProject` (`J/K` in Worktrees), `ViewDiff` (`d`), `RebaseOntoMain` (`R` in Worktrees), `SwitchToOutput` (`o`), and `RebaseStatus` (`R` in Convo) — actions that were either no-ops, redundant with Git panel operations, or unnecessary. Action enum variants, keybindings, handler match arms, and the `rebase_current()` helper function all deleted. Also removed 4 duplicate `⌘⇧J/K` viewer bindings.
- **Status bar cleaned up** — Removed stale ViewMode indicator ("Output") that added no information (help hints already change per mode). Fixed redundant `main (main)` display — branch parenthetical now hidden when identical to display name. Updated all help hint strings to match current keybindings (removed references to deleted actions like `Space:actions`, `d:diff`, `R:rebase`, `o:output`).

### Fixed
- **Help panel: removed macOS unicode chars from Option-key bindings** — `display_keys()` now filters out non-ASCII unicode alternatives (®, π, †) produced by macOS ⌥+letter. Help shows clean `⌥r`, `⌥p`, `⌥t` instead of `⌥r/®`, `⌥p/π`, `⌥t/†`.
- God File Modularization (`g` → modularize) now **auto-switches the convo pane** to the main worktree session — previously the convo title and output stayed on whatever session was open before, requiring manual session list navigation to see GFM output. Also applies to queue advancement (next file auto-switches too).
- `Shift+G` (Git Actions panel) now works — `KeyCombo::matches()` fixed systemically to handle the Kitty protocol quirk where uppercase chars arrive as `(NONE, Char('G'))` not `(SHIFT, Char('G'))`. All future `KeyCombo::shift(Char('X'))` bindings are now immune to this bug.
- **Lowercase `t` no longer triggers terminal toggle** — `KeyCombo::matches()` was matching `(NONE, 't')` against `shift('T')` because `'t'.to_ascii_uppercase() == 'T'`. Now rejects plain lowercase: only `(NONE, 'T')` or `(SHIFT, *)` match uppercase bindings.
- **Preset prompt dialogs stuck in Esc loop** — When both the preset picker and add/edit dialog were open simultaneously, pressing Esc on the picker revealed the dialog underneath, and pressing Esc on the dialog reopened the picker (infinite cycle). Fixed by: (1) removing the "reopen picker on Esc" behavior from the dialog, (2) checking dialog input before picker input (dialog is on top), (3) drawing dialog on top of picker instead of exclusively.
- **Bottom border title styling** — Overflow hints on the bottom border of Input/Terminal panes were hardcoded dim gray and lacked parentheses. Now matches the top title: same border color, bold when focused, wrapped in parentheses.
- **Input text forced white** — Prompt input text now renders as explicit white (`Color::White`) regardless of terminal color scheme. Applied to both `draw_input.rs` (ratatui `normal_style`) and `fast_draw_input()` (crossterm `SetForegroundColor`), ensuring consistent visibility on light and dark terminal backgrounds.

### Refactored
- Modularized `draw_output.rs` (1002→443 lines) into file-based module root with 4 submodules:
  - `draw_output/render_submit.rs` (201 lines) — background render thread submit/poll coordination
  - `draw_output/session_list.rs` (248 lines) — session list overlay with filter and content search
  - `draw_output/todo_widget.rs` (81 lines) — sticky task progress widget
  - `draw_output/rebase_view.rs` (108 lines) — git rebase status display
  - Module root keeps `draw_output()` function + re-exports public API for backwards compatibility

### Added
- Startup splash screen: 2x-scale block-character "AZUREAL" logo in AZURE blue with half-block acronym subtitle, dim spring azure butterfly outline as background mascot, and "Loading project..." subtitle, shown for minimum 3 seconds while git/session I/O runs (replaces black screen)
- Convo pane search (`/`): find text in current session's rendered output
  - Yellow match highlighting with bright current match, `[N/M]` counter in search bar
  - `n`/`N` to cycle through matches after confirming with Enter
  - `Esc` clears search and highlights
- Session list name filter (`/` in session list): filter sessions by worktree name, session name, or UUID
  - Case-insensitive matching with live-updating filtered list
  - Title shows `[x/y of total]` match count
- Cross-session content search (`//` in session list): full-text search across current worktree's JSONL files
  - Activates with double-slash (second `/` while filter is empty)
  - Searches begin at 3+ characters, capped at 100 results, skips files >5MB
  - Shows session name + matched line preview; Enter loads that session
- macOS completion notifications: system notification fires when any Claude instance exits
  - Title shows `worktree:session_name`; body shows completion status; branded Azureal icon
  - `.app` bundle auto-created at `~/.azureal/AZUREAL.app` with embedded `.icns` (via `include_bytes!()`)
  - Process re-execs through bundle copy + `TransformProcessType` so Activity Monitor shows AZUREAL icon
  - Notification permissions auto-enabled on first launch (no System Settings visit needed)
  - Uses `notify-rust` crate with `set_application()` in background thread, non-blocking
  - Works per-instance: multiple Claude sessions each trigger their own notification
  - Zero manual setup: `cargo install --path .` then `azureal` — bundle auto-creates on first launch
- Preset prompts feature (`⌥P`): save up to 10 prompt templates, quick-select with `1-9,0`, add/edit/delete (`d` with y/n confirmation) from picker; selected preset populates input box and enters prompt mode. Available only from prompt mode (shown in title bar). Dual-scope persistence: presets can be global (`~/.azureal/presetprompts`, shared across projects) or project-local (`.azureal/presetprompts`); toggle with `⌃g` in add/edit dialog; picker shows G/P scope badge.
- Git Actions panel (`Shift+G`): centered modal overlay with Git brand orange (#F05032) borders, showing 5 git operations (rebase, merge, fetch, pull, push) with single-key shortcuts and changed files list with per-file `+N/-N` stats. Tab toggles between actions and file sections; Enter on a file opens its diff in the Viewer pane. 6 git methods: `get_diff_files`, `get_file_diff`, `fetch`, `pull`, `push`, `merge_from_main`.

### Changed
- Worktree sidebar simplified to flat list: removed session file dropdowns (chevrons) and expand/collapse keybindings (`l/h`, `Left/Right`); navigation is now `j/k` to move between worktrees, `Enter` to switch. Session switching moved to Convo pane's session list (`s` key). Removed `worktrees_expanded` state, `session_file_next/prev/first/last`, `expand/collapse/toggle_worktree`, `is_current_worktree_expanded` methods, and `SidebarRowAction::WorktreeFile` variant.
- Convo pane auto-follow: scrolling down to the bottom now re-engages follow-bottom auto-scroll (previously only ⌥↓ did this)
- Projects panel: launching app in a non-git directory shows "Project not initialized. Press i to initialize or choose another project." in red; clears on first keypress
- Session list overlay shows "Loading sessions…" dialog while message counts compute — two-phase open ensures UI never appears frozen on large session files
- Session list overlay now scoped to current worktree only (was all worktrees)
  - Removed redundant worktree name column from each row
  - Border title shows worktree name + position counter
- Auto-registered projects now derive display name from git remote origin URL (repo name) instead of folder name; falls back to folder name if no remote exists
- God File System: scan project for source files >1000 LOC and batch-modularize them
  - Press `g` in Worktrees pane to open full-screen scanner panel
  - Shows all oversized source files sorted by line count (worst offenders first)
  - Checkable list: `Space` to check, `a` to toggle all, `Enter`/`m` to modularize
  - Spawns sequential Claude sessions on main worktree — one file at a time, auto-advances when each completes
  - Sessions named `[GFM] <filename>` (GFM = God File Modularize) in `.azureal/sessions`
  - Panel shows explanation line before file list: "Sessions will be prefixed [GFM] (God File Modularize)"
  - Scans 22 source extensions, skips build/dependency directories
- Help panel: counterpart keybindings (up/down, next/prev, expand/collapse) merged onto single lines with `·` separator — halves the row count for navigation bindings
- Help panel (`?`) now uses double border (`═║`) matching focused pane style
- Dashed double-line border in Viewer edit mode when file has unsaved changes
  - Normal `═║` double border rendered first, then every other cell blanked by checking for `═`/`║` symbols — title text and corners preserved automatically
  - Creates `═ ═ ═` / `║ ║` gap pattern across all four edges
  - `[modified]` indicator displayed as right-aligned title (ratatui fills gap with border chars automatically)

### Changed
- Speech input keybinding: `⌃S` (renamed from "Voice input", was `⌃V` which conflicted with terminal paste)
- Whisper model directory: `~/.azureal/speech/` (renamed from `voice/`)
- `⌃C` renamed from "Cancel Claude response" to "Cancel agent"
- Removed `ClearInput` action (`⌃U`/`⌃C`) — use `⌘A` + `Delete` instead
- Prompt clipboard operations now use `⌘` only (removed redundant `⌃C/X/V/A` variants that conflicted with cancel/speech)
- Command mode title bar: PROMPT and TERMINAL labels now uppercase for visibility
- Delete word keybinding: `⌃W` primary, `⌃Backspace` alternative (was `⌥Backspace` which Alacritty strips to plain Backspace due to Kitty protocol bugs; `⌃W` is standard Unix delete-word and works universally)

### Refactored
- Modularized `event_loop.rs` (1660→330 lines) into 5 focused submodules using file-based module root pattern:
  - `event_loop/coords.rs` (174 lines) — screen-to-content coordinate mapping
  - `event_loop/mouse.rs` (342 lines) — click, drag, scroll, and selection copy
  - `event_loop/actions.rs` (732 lines) — key dispatch, navigation, and escape handling
  - `event_loop/fast_draw.rs` (87 lines) — fast-path input rendering bypass
  - `event_loop/claude_events.rs` (62 lines) — Claude process event handling

### Optimized
- Session list overlay (`s`) opens instantly — message count scanning replaced serde_json parsing with fast `contains()` string checks (zero false positives in Claude's compact JSON). Counts cached by file size so unchanged files skip I/O entirely on subsequent opens.

### Fixed
- `Ctrl+Q` now quits from the Projects panel — was being swallowed because the panel intercepted all keys before the global keybinding system could process the quit shortcut
- Projects panel no longer shows stale entries — `load_projects()` now validates each entry on load and prunes directories that don't exist or lost their `.git` (writes cleaned list back to `projects` automatically)
- Convo title `[x/y]` denominator now shows true total message count for large sessions — was showing ~40-50 instead of hundreds because deferred rendering (last 200 events on initial load) meant `message_bubble_positions` only covered the rendered tail. Denominator now counts `UserMessage` + `AssistantText` from the full `display_events` array; numerator uses offset arithmetic so positioning stays correct before full render triggers on scroll-to-top.
- Session list `[N msgs]` count now matches convo title `[x/y]` — was inflated due to three issues: (1) used wrong type string `"human"` instead of `"user"`, (2) counted every assistant JSONL line with `"stop_reason"` (95.5% are null), (3) didn't skip non-bubble user events (isMeta, `<local-command-caveat>`, `<local-command-stdout>`, `<command-name>`, compaction summaries) or deduplicate by parentUuid. Now uses fast string scanning matching the session parser's filtering logic.
- Clicking out of prompt input or Tab-cycling away now exits back to command mode (was staying in prompt mode with yellow border)
- Projects panel init (`i`) now rejects paths that are already git repos — shows "Already a git repo — use 'a' to add it" instead of re-initializing
- Projects panel now validates git repo before switching — selecting a non-git directory shows an error ("Not a git repository" or "Directory does not exist") instead of blindly opening it as a broken project
- Session list overlay (`s` key) now toggles off with `s` or `Esc` — `ToggleSessionList` action was always calling `open_session_list()` without checking if already open, and `dispatch_escape()` for Output focus went straight to Worktrees without closing the overlay first
- Session list overlay now responds to Up/Down arrow keys — keybinding system was intercepting them as JumpNextBubble/JumpPrevBubble before the session list handler could process them; session list now bypasses keybinding system as a modal overlay
- Shift+Tab from Viewer now lands on FileTree when the overlay is open (was always closing the overlay and jumping to Worktrees)
- Global `t` keybinding (terminal toggle) no longer fires in viewer edit mode — guard was missing `!viewer_edit_mode`, so typing `t` opened the terminal instead of inserting the character
- Global `Tab`/`Shift+Tab` (focus cycling) no longer fires in viewer edit mode — Tab inserts 4 spaces in edit mode but the global handler ran first and cycled focus away
- Global `⌘C` was swallowing copy in viewer edit mode — handler checked `viewer_selection` (read-only) but not `viewer_edit_selection`, then returned early; edit mode's `viewer_edit_copy()` never fired
- Viewer edit mode now uses word-boundary wrapping (matching read-only mode) — both modes use `textwrap::wrap()` with `word_wrap_breaks()` for consistent text reflow
  - Cursor navigation (up/down), scroll-to-cursor, and mouse click-to-cursor all updated to use word-boundary break positions instead of fixed-width char-boundary math
  - `wrap_spans_word()` replaces both `wrap_spans()` and `wrap_spans_hard()` — one wrapping function for all viewer modes
- Icon not showing on GitHub — `azural_icon.png` renamed to `azureal_icon.png` to match README reference
- `f` key now toggles FileTree overlay off (was only handled in worktrees input, not file tree input)
- Convo pane auto-scroll now properly follows bottom until user scrolls up, then stays put
  - `usize::MAX` sentinel was being resolved to a concrete value during draw, destroying follow-bottom state
  - Draw path now computes concrete scroll locally without writing it back — sentinel survives across frames
  - Scrolling up breaks follow; `⌥↓` resumes it
  - Removed forced scroll-to-bottom from `handle_claude_output()`, `add_output()`, and `refresh_session_events()`
- Convo pane bubbles too narrow — render width formula `(terminal - 80) / 2` was a leftover from the old 50/50 split layout; now passes the fixed 80-column pane width directly, giving bubbles proper ~52 char width instead of being crushed to minimum 40
- Edit cycling (`⌥←`/`⌥→`) now works after clicking an edit tool file path in the Convo pane
  - Clicking an edit path now sets `selected_tool_diff` so cycling knows the starting position (was `None`, always jumping to first edit)
- `[Edit N/M]` viewer title now counts only Edit tool calls (was counting all clickable paths including Read/Write)
- Edit cycling (`⌥←`/`⌥→`) now highlights the file path in the Convo pane with inverted orange colors (was only set on mouse click, not keyboard cycling)
- Clicked/cycled file path highlight now covers all wrapped continuation lines, not just the first line
  - `ClickablePath` tuple extended with `wrap_line_count` field
  - Clicking a continuation line of a wrapped path also triggers the file open
- Debug dump output file no longer mentions "obfuscated" in headers or status bar message
- File actions in FileTree pane: `a` (add file/dir), `d` (delete), `r` (rename), `c` (copy), `m` (move)
  - Inline action bar at bottom of FileTree pane with text input (or y/N confirmation for delete)
  - Add: trailing `/` creates directory; files created in selected dir or alongside selected file
  - Rename: pre-filled with current name
  - Copy/Move: clipboard-style paste workflow — press `c`/`m` to grab source file, navigate tree to target directory, press `Enter` to paste; source highlighted with `┃name┃` (copy, solid) or `╎name╎` (move, dashed) in magenta
  - `⌃u` clears input, `Esc` cancels, `Enter` confirms
  - Recursive directory copy support
- `⌃c` prompt title label changed from "cancel" to "cancel response" for clarity
- Wrapped file path highlight in Convo pane no longer extends past the path text
  - Continuation lines were highlighting from column 0 (including indent) to full line width
  - Now highlights from the path start column to end of actual path text only
- Viewer edit mode display freeze after ~100 edits — highlight cache used undo stack length as invalidation key, but the stack caps at 100 entries; replaced with monotonically increasing `viewer_edit_version` counter
- 100%+ CPU spike on prompt submit — three fixes applied:
  1. **Backpressure**: skip `submit_render_request()` when `render_in_flight` is true; dirty flag stays set and fires on next `poll_render_result()` completion
  2. **Single JSON parse**: `handle_claude_output()` was parsing each event 3x (EventParser, token extraction, display text); now token extraction and display text share one parse via `display_text_from_json()`
  3. **Render throttle**: 50ms minimum interval between render submits prevents rapid clone+submit cycles after each render completes; batches ~60Hz streaming events into ~20 render cycles/sec
- FileTree copy/move paste now selects the pasted file and auto-expands the target directory
- FileTree action bar text now wraps to multiple lines when wider than the pane (was clipping)
  - Wraps at word boundaries so key hints like `Enter:paste` and `Esc:cancel` stay together
- Edit diff preview showed line 1 during live streaming — `render_edit_diff()` searched for `new_string` in the file to find the actual line number, but during live streaming the edit hasn't been applied yet so `old_string` is still in the file. Now tries `new_string` first (post-edit), falls back to `old_string` (live preview mid-edit).

### Refactored
- **Consolidated ALL keybindings into `keybindings.rs` as single source of truth**
  - Added `KeyContext` struct capturing all guard state (focus, prompt_mode, edit_mode, terminal_mode, filter_active, has_context_menu, wizard_active, help_open) — replaces 6 scattered boolean parameters
  - Rewrote `lookup_action()` with all guard/skip logic centralized inside — guards defined ONCE next to bindings, never duplicated in event_loop.rs or input handlers
  - Added `execute_action()` dispatcher in `event_loop.rs` — central dispatch for all action side effects
  - Added new `Action` variants: `ToggleFileTree`, `ReturnToWorktrees`, `ToggleSessionList`, `SelectAll`, `ViewerTabCurrent`, `ViewerOpenTabDialog`, `ViewerNextTab`, `ViewerPrevTab`, `ViewerCloseTab`
  - Gutted `input_viewer.rs` — removed all 43 lines of hardcoded read-only bindings (tabs, PageUp/Down, Home/End, SelectAll, Cmd+Shift+J/K); only tab/save/discard dialogs + edit mode text editing remain
  - Gutted `input_output.rs` — removed all navigation dispatch (~100 lines); only session list overlay + rebase mode remain
  - Gutted `input_file_tree.rs` — removed all command bindings; only clipboard mode + text-input actions remain
  - Gutted `input_worktrees.rs` — removed all 170+ lines of hardcoded commands; only sidebar filter text input + stop-tracking remain
  - Removed `lookup_action_legacy()` wrapper, `is_nav_*` helper functions, `CTRL_ALT_CMD` constant, `CloseViewer` action (Escape handles it)
  - **Root cause of repeated keybinding bugs fixed**: guards for viewer_edit_mode, prompt_mode, etc. were scattered across event_loop.rs's hardcoded global handlers; each new binding required manually adding guards in multiple places
- Fixed session/worktree naming inconsistencies across 14 source files
  - Fields: `selected_session` → `selected_worktree`, `pane_sessions` → `pane_worktrees`, `sessions_expanded` → `worktrees_expanded`, `session_terminals` → `worktree_terminals`
  - Methods: `expand_session()` → `expand_worktree()`, `collapse_session()` → `collapse_worktree()`, `toggle_session_expanded()` → `toggle_worktree_expanded()`, `is_current_session_expanded()` → `is_current_worktree_expanded()`
  - Enum variants: `SidebarRowAction::Session` → `Worktree`, `SidebarRowAction::SessionFile` → `WorktreeFile`
  - "Session" now exclusively refers to Claude conversation files; "worktree" refers to git worktree directories

### Changed
- Layout refactored from 4-pane to 3-pane: Worktrees (40) | Viewer (remaining) | Convo (80 fixed)
  - FileTree removed as permanent pane — now a toggle overlay (`f`) inside the Worktrees pane
  - Press `f` on a selected worktree to browse its filesystem; `w` or `Esc` to return to worktree list
  - Convo pane width changed from 50% remaining to fixed 80 columns for consistent readability
- Session list overlay added to Convo pane (`s` to toggle)
  - Full-pane browser showing all session files across all worktrees
  - Each row shows: status symbol (colored), worktree name, session name/UUID, last modified time, `[N msgs]` count
  - Message counts computed via lightweight JSONL line scan, cached per session_id
  - `j/k` navigate, `J/K` page, `Enter` loads session, `s`/`Esc` closes
- Focus cycling (Tab/Shift+Tab) simplified: Worktrees → Viewer → Convo → Input (FileTree removed from cycle)
  - Tab/Shift+Tab now closes any open overlay (FileTree or Session list) before cycling
- AZURE accent color lightened from #007FFF to #3399FF for better readability on dark backgrounds
- Edit cycling in Viewer rebound from `f`/`b` to `⌥←`/`⌥→` to avoid key conflicts
- Edit cycling now only jumps through Edit tool entries (skips Read/Write paths)
- Command box title now shows `p:prompt`, `⌃d:dump debug output`, and `Tab/⇧Tab:focus` (both directions); Global section commented out from help panel
- Voice input (`⌃s`) now listed in Edit Mode section of help panel

### Added
- OS terminal title: shows `AZUREAL` on startup, `AZUREAL @ <project> : <branch>` when session selected
  - Updates dynamically on session switch and project switch
  - Reset to empty on exit
- Prompt mode for New Run Command dialog
  - Tab cycles between Command and Prompt modes when the second field is focused
  - In Prompt mode, Enter spawns a Claude session on the main branch that generates the shell command
  - Claude reads/writes `.azureal/runcmds` based on the user's natural-language description
  - Session named `[NewRunCmd] <name>` in `.azureal/sessions`
  - Run commands auto-reload when the `[NewRunCmd]` session exits
- Projects panel: persistent project management via `~/.azureal/projects`
  - Auto-registers git repos on startup; shown full-screen when launched outside a git repo
  - `P` from Worktrees pane opens panel; supports add, delete, rename, and git init
  - Project switching kills all Claude processes and reloads all session state
  - Sidebar project header replaced with project name in Worktrees pane border title
- Session title on Convo pane border: centered `[session name]` between left title and right badges
  - Custom session names from `.azureal/sessions` shown when available
  - Raw UUIDs truncated as `[xxxxxxxx-…]` (first 8 chars)
  - Ellipsis when name would overlap adjacent titles
  - Title cached on session switch (zero file I/O in render path)
- Kernel-level file watching via `notify` crate (replaces 500ms stat() polling)
  - Session file changes detected instantly via kqueue (macOS) / inotify (Linux)
  - File tree auto-refreshes when worktree files change (500ms debounce for rapid creates)
  - Background `FileWatcher` thread follows RenderThread/SttHandle pattern (mpsc channels, non-blocking)
  - Graceful fallback to stat() polling if notify fails to initialize
  - Noisy paths filtered: `target/`, `.git/`, `node_modules/`, editor swap files

### Optimized
- Edit mode rendering: cached syntax highlighting + viewport-only line construction
  - Syntax highlighting cached in `viewer_edit_highlight_cache`, only re-run when content changes (tracked via undo stack depth)
  - Only visible source lines are processed per frame (O(viewport) not O(file_size))
  - Cursor position computed arithmetically instead of walking all visual lines
  - AGENTS.md (~1000+ lines) dropped from 90%+ CPU to <5% in edit mode
- Deferred initial render: large conversations (200+ events) only render the tail on initial load
  - User starts at bottom, sees recent messages instantly (no 10s+ wait)
  - Full render happens lazily when scrolling to top
- Edit diff render no longer reads files from disk (was O(file_size) per Edit event)
  - Eliminated `std::fs::read_to_string()` + substring search per Edit tool call
  - Uses relative line numbers instead — convo panel is a summary view
- Edit diff syntax highlighting reduced from 3→2 calls per event
  - Reuses base syntect parse, applies background colors via cheap span iteration
- Incremental JSONL parsing: seeks to last byte offset, parses only newly appended lines
  - Rebuilds tool_call context from existing DisplayEvents via `IncrementalParserState`
  - Falls back to full re-parse if file shrank or user-message rewrite detected
- Incremental rendering: appends only new display events to cached rendered lines
  - Skips full re-render when width unchanged and events only grew
  - Pre-scans existing events to establish state flags for correct continuation
- Fast path in `wrap_text()`: skips textwrap entirely when text fits in one line
- Reduced clones in render pipeline: borrow file_path, reference-compare hooks before cloning
- Removed redundant `.wrap(Wrap { trim: false })` from Convo Paragraph
  - Content is pre-wrapped by `wrap_text()`/`wrap_spans()` — ratatui was re-wrapping every viewport line char-by-char per frame
- Animation patching loop now skipped when no tools are pending (avoids pulse computation on every scroll frame)
- Eliminated CPU spike on prompt send with 5 targeted fixes:
  1. **Mega-clone elimination**: incremental renders now clone only NEW events (from `rendered_events_count` onwards) instead of the entire `display_events` array. `pre_scan_events()` computes state flags on the main thread (zero-cost reads), sent to render thread via `PreScanState`.
  2. **EventParser O(n²) → O(n)**: buffer reallocation per newline replaced with single `drain()` of all complete lines
  3. **Reader thread**: full JSON parse per line replaced with `contains("\"subtype\":\"init\"")` string search (init happens once per session)
  4. **Dev profile opt-level=2** for `serde_json`, `serde`, `syntect` — 3-5x faster than opt-level 0 in debug builds
  5. **process_output_chunk**: `clone()+clear()` replaced with `std::mem::take()` (zero allocation)
  6. **True single JSON parse**: `EventParser::parse()` now returns `(Vec<DisplayEvent>, Option<Value>)` — `handle_claude_output` reuses the returned Value for token extraction instead of parsing a second time
  7. **Skip fallback output_lines**: `display_text_from_json()` + `process_output_chunk()` skipped once rendered cache exists (fallback view never read during normal streaming)
  8. **Full render clone reduction**: full render path clones only `display_events[deferred_start..]` instead of entire Vec
  9. **Empty batch skip**: progress/hook_started lines producing 0 events skip `display_events.extend()` + `invalidate_render_cache()`
- Background render thread for convo pane (`src/tui/render_thread.rs`)
  - Markdown parsing, syntax highlighting, and text wrapping run on a dedicated thread
  - RenderThread owns its own SyntaxHighlighter (no cross-thread sharing)
  - Requests carry cloned data so threads work independently
  - Sequence numbers ensure stale results are discarded (latest-wins)
  - Render thread drains to latest request when multiple are queued
  - Zero CPU when idle (blocks on `mpsc::recv`)
  - `update_convo_cache()` replaced with non-blocking `submit_render_request()` + `poll_render_result()`
  - Render cache cloned (not taken) for incremental requests — convo stays visible during background render
- Pre-draw event drain: keys typed during processing/render-poll are caught before `terminal.draw()`
- Adaptive poll timeout: 16ms when busy (render in-flight / Claude streaming), 100ms when idle
- Deferred draw: when keys arrive, `terminal.draw()` is SKIPPED entirely
  - `terminal.draw()` measured at ~18ms per call — during which event loop is blocked
  - Draw happens on next quiet iteration (no key events, ~16ms later) — imperceptible delay
  - Pre-draw drain aborts if a last-moment key arrives, preventing even that 18ms block
  - `draw_pending` flag on App tracks deferred draws; poll timeout drops to 16ms while pending
  - Throttle floor at 33ms (~30fps) prevents CPU burn on rapid background updates
- Token usage badge cached as `(String, Color)` — only recomputed when new token data is parsed
  - `update_token_badge()` called from load, refresh, and live stream paths
  - Draw path reads cached value with zero computation (was recomputing percentage every frame)
- Fast-path direct input rendering: `fast_draw_input()` writes input box content directly
  via crossterm (~0.1ms) when typing in prompt mode, bypassing `terminal.draw()` entirely
  - `app.input_area` cached from last full draw provides screen coordinates
  - Word-wrap and scroll-offset aware cursor positioning
  - Unicode display-width aware padding to overwrite stale content
  - Ratatui's next full draw naturally reconciles (no buffer invalidation needed)

### Fixed
- Tables in convo pane now wrap to fit bubble width instead of clipping at viewport edge
  - Column widths proportionally shrunk when total table width exceeds available bubble width
  - Cell text truncated with `…` ellipsis when it overflows its clamped column width
  - Overhead budget calculated as `3 + 3*ncols + sum(col_widths)` for gutter, borders, and padding
- Holding arrow keys now repeats cursor movement (was only moving once because `KeyEventKind::Repeat` events were dropped)
- Function name syntax highlighting changed from ANSI Blue to light blue (`rgb(100, 160, 255)`) — ANSI Blue was nearly invisible on dark terminal backgrounds
- Tasks widget now wraps long item text instead of clipping at pane edge; height accounts for wrapped lines
- Subtask todos now render directly beneath their parent item (tracked via `subagent_parent_idx`) instead of appended at the end of the todo list
- Messages disappearing after mid-conversation prompt send — force session file re-parse before auto-sending staged prompt

### Added
- Status bar badge (bottom-right): CPU usage % and PID for the current azureal instance
  - CPU% sampled via `getrusage(RUSAGE_SELF)` delta every ~1s (zero overhead between samples)
- Subagent subtask display in tasks panel
  - When a Task (subagent) tool is active, its TodoWrite calls appear as indented subtasks below the main agent's todos, prefixed with ↳
  - `active_task_tool_ids` tracks active Task tool calls; TodoWrite events routed to `subagent_todos` while any Task is in-flight
  - Subagent todos auto-clear when the last Task tool completes

### Changed
- All accent colors changed from ANSI Cyan to Azure (#3399FF, lightened from original #007FFF) to align with the "Azureal" name
  - `AZURE` constant defined in `src/tui/util.rs`, imported across 14 source files
  - Replaces every `Color::Cyan` usage: UI borders, titles, sidebar, dialogs, syntax highlighting, markdown, tool calls, user bubbles, file tree, status bar, and diff hunks
- Input box title in command mode renamed from "PROMPT" to "COMMAND" and now shows global keybindings (prompt, terminal, help, Tab/⇧Tab focus, cancel, quit, restart, dump debug output); Global section commented out from help panel
- Sending a prompt mid-conversation now cancels Claude and sends the new prompt in one Enter press (previously required Enter to cancel, then Enter again to send)
- Input box now wraps at word boundaries instead of character boundaries
  - Prefers breaking at last space before width limit; falls back to char break for long words
  - `word_wrap_break_points()` and `display_width()` centralized in `draw_input.rs` (pub(crate))
  - All 6 input wrapping consumers updated: `build_wrapped_content()`, `fast_draw_input()`, `compute_cursor_row_fast()`, `click_to_input_cursor()`, `screen_to_input_char()`, `row_col_to_char_index()`
  - Mouse click/drag and cursor positioning all use identical word-wrap logic
- `p` key now refocuses prompt input when prompt mode is already active but focus is on another pane (previously only worked to enter prompt mode from command mode)
- Simplified scroll system — removed half-page (`⌃d`/`⌃u`) and full-page (`f`/`b`) scroll bindings. `J`/`K` now does full-page scroll across all panes.
- Removed `g`/`G` keybindings for scroll-to-top/scroll-to-bottom. `⌥↑`/`⌥↓` is now the only way to jump to top/bottom across all panes (Convo, Viewer, Terminal, FileTree, Worktrees).
- `input_output.rs` now uses centralized `lookup_action()` from `keybindings.rs` — all input handlers are now fully centralized.
- Added `PageDown`, `PageUp` actions to keybindings enum for output pane coverage.
- Debug dump keybinding changed from `D` to `⌃D` and now obfuscates all sensitive content before writing to `.azureal/debug_output`
  - User/assistant messages, file paths, and rendered output replaced with deterministic fake words
  - Tool names, event types, parsing stats, and structural markers preserved for diagnostics
  - Users can safely attach the dump to GitHub issues without exposing project details
- Convo pane now extends full height (down to status bar), no longer shares height with Input/Terminal
  - Input/Terminal pane now spans only the first 3 panes (Sessions, FileTree, Viewer)
  - Gives Convo pane more vertical space for reading conversation history
  - Mouse scroll dispatch updated for asymmetric layout
- Terminal keybindings moved from help panel to terminal pane title bar
  - Command mode title: `(t:type | p:prompt | Esc:close | j/k:scroll | J/K:page | ⌥↑/⌥↓:top/bottom | +/-:resize)`
  - Type mode title: `(Esc:exit)`
  - Scroll mode title: `[N↑] (j/k:scroll | ... | Esc:close)`
  - Help panel (`?`) no longer has a Terminal section
  - All title hints dynamically sourced from `TERMINAL` binding array (single source of truth)
- Input keybindings moved from help panel to prompt input pane title bar
  - Type mode title: `(Esc:exit | Enter:submit | ⌃c:cancel | ↑/↓:history | ⌥←/→:word | ⌃w:del wrd | ⌃u:clear)`
  - Command mode title: `(p:type | t:terminal)`
  - Help panel (`?`) no longer has an Input section
  - All title hints dynamically sourced from `INPUT` binding array (single source of truth)
- Multi-line prompt input via Shift+Enter
  - Inserts a newline at cursor position; Enter alone still submits
  - Kitty keyboard protocol enabled on startup (DISAMBIGUATE + REPORT_EVENT_TYPES)
  - `REPORT_ALL_KEYS` intentionally omitted — it broke Shift+letter secondary characters (!, @, #, etc.)
  - Input field grows dynamically up to 3/4 of terminal height, then scrolls
  - Cursor positioning accounts for both newlines and word-wrapping
  - Selection highlighting works correctly across line boundaries

### Fixed
- Backspace line-join in edit mode used `.len()` (byte count) instead of `.chars().count()` for cursor positioning — caused cursor to land at wrong position on lines containing multi-byte UTF-8 characters
- Clicking a file in the file tree while another file was in edit mode left edit mode active on the new file — now exits edit mode cleanly before loading
- Edit mode cursor/selection misalignment — cursor didn't match selection highlight end because `textwrap::wrap()` breaks at word boundaries while cursor math assumed fixed-width char boundaries. Now uses `word_wrap_breaks()` to compute actual break positions for all cursor math

### Added
- Wrap-aware cursor navigation in file edit mode — Up/Down arrows now move through wrapped visual lines instead of jumping entire source lines
  - Long lines that wrap into multiple visual rows can be navigated row-by-row
  - Visual column position preserved when moving between wrap segments
  - Scroll-to-cursor accounts for wrap counts so viewport always follows cursor
- Mouse click-to-cursor in file edit mode — clicking positions the edit cursor at the clicked character
  - `screen_to_edit_pos()` maps screen coordinates through line wrapping to find source line and column
  - Works correctly on wrapped continuation lines (not just the first visual row)
- Mouse drag selection in file edit mode — click and drag creates text selections
  - Drag anchor stored as source coordinates (pane_id=3) so auto-scroll doesn't shift selection start
  - Auto-scrolls viewport when dragging above/below pane
- `⌥↑`/`⌥↓` jump-to-top/bottom across all panes (defined in centralized keybindings.rs)
  - Convo pane: scroll to top/bottom of conversation
  - Viewer pane: scroll to top/bottom of file
  - FileTree pane: jump to first/last sibling within the current folder
  - Worktrees pane: jump to first/last worktree
  - Terminal pane: scroll to top/bottom
- Speech-to-text voice input in prompt mode (`⌃s` to toggle)
  - Microphone capture via cpal (CoreAudio on macOS)
  - Local transcription via whisper.cpp with Metal GPU acceleration
  - Background thread with zero CPU when idle (blocks on `mpsc::recv`)
  - Lazy initialization: STT thread only spawned on first `⌃s` press
  - Whisper model lazy-loaded on first transcription and cached
  - Magenta border + REC/... indicator during recording/transcription
  - Transcribed text inserted at cursor with smart space handling
  - Model: `~/.azureal/models/ggml-base.en.bin` (~142MB, user-downloaded)
  - Also works in file edit mode (`⌃s` inserts at viewer edit cursor)
  - Viewer pane shows magenta border + REC/... indicator during edit mode recording
- TodoWrite sticky widget at bottom of Convo pane
  - Persistent checkbox list showing Claude's task progress during streaming and on session load
  - Status icons: ✓ (completed, green), ● (in_progress, yellow pulsing), ○ (pending, dim)
  - In-progress items show `activeForm` text; pending/completed show `content`
  - Widget stays visible after all items completed (shows checkmarks); clears on next user prompt
  - TodoWrite tool calls and results suppressed from inline convo stream
- Hierarchical session search/filter in Worktrees pane
  - Press `/` to activate filter bar at top of sidebar
  - Searches across project name, worktree names, session file UUIDs, and custom session names simultaneously
  - Matching items shown with parent hierarchy preserved (e.g., matching session UUID shows under its worktree and project)
  - Session files eagerly loaded at startup so UUIDs are searchable without manual expansion
  - Worktrees auto-expand to reveal matching session files
  - Esc clears filter, Enter accepts, Backspace removes chars
  - Selection auto-snaps to first match; j/k skip filtered-out sessions
  - Match count shown as `N/total` in filter bar
  - Global keys (p, t, ?, D) suppressed while filter is active
- AskUserQuestion rendered as numbered options box (instead of raw JSON)
  - Magenta-bordered box with question header, numbered options (label + description), and implicit "Other"
  - User responds with a number or custom text; hidden system context prefix ensures Claude interprets correctly
  - State restored on session load if the last AskUserQuestion was unanswered
- Token usage percentage counter on Convo pane title border
  - Color-coded badge: green (<60%), yellow (60-80%), red (>80%) of context window
  - Extracted from Claude's `message.usage` in JSONL session files (exact API counts, no estimation)
  - Context window from `result` event's `modelUsage.*.contextWindow` (authoritative API value)
  - Heuristic fallback from `message.model` for mid-turn display before result event arrives
  - Auto-detects 1M beta context when token usage exceeds standard 200k window
  - Updates in real-time during live streaming and from session file polls
  - Helps predict when context compaction will occur
  - Displayed alongside PID/exit code in the right-aligned title
- `⌘A` select-all in Viewer pane (read-only mode)
  - Selects entire viewer cache from first to last line, then `⌘C` to copy
  - Complements existing `⌘A` in edit mode and input pane
  - Copied text excludes line number gutter — only file content is copied
  - Selection highlight skips line number column (gutter stays unhighlighted)
- Full mouse click interaction for all panes
  - Click any pane to focus it (border highlights with double border)
  - Click sessions/session files in sidebar to select them
  - Click file tree entries to select; double-click to open files or expand/collapse directories
  - Click input pane to enter prompt mode and position cursor at click location
  - Click outside overlays (help, context menu, wizard, branch dialog, run command picker/dialog) to dismiss
  - Pane hit-testing via cached `Rect::contains()` — shared by both click and scroll handlers
  - Sidebar uses `SidebarRowAction` row map built alongside sidebar cache for O(1) click-to-item lookup
  - Scroll handler refactored to use cached pane rects (was duplicating layout math)
- Text selection via mouse drag in Convo and Viewer panes
  - Click-drag to select text with `Rgb(60,60,100)` highlight background
  - Screen-to-cache coordinate mapping via `screen_to_cache_pos()` helper
  - Auto-scroll when dragging above/below pane content area
  - `⌘C` copies selected text from any pane (viewer, convo, or input) to system clipboard
  - Selections cleared on click, scroll, Tab, or focus change
  - Convo viewport cache invalidated on selection change (no extra cost when no selection)
  - Reuses existing `apply_selection_to_line()` from Viewer (made `pub(crate)`)

### Fixed
- TodoWrite sticky widget now clears when user sends the next prompt
  - `extract_skill_tools_from_events()` was setting todos from the last TodoWrite but never clearing when a UserMessage appeared after it
  - Now tracks `saw_user_after_todo` flag to clear stale todos on session file re-parse
- Uppercase keybindings (`D` debug dump, `R` rebase, `T` tab dialog) now work correctly
  - Without `REPORT_ALL_KEYS`, shifted letters arrive as `(NONE, Char('D'))` not `(SHIFT, Char('D'))`
  - All three handlers were matching on `SHIFT` modifier which never fires in our Kitty protocol config
- Run command dialog redesigned with proper bordered text fields
  - Name and Command fields now have ALL borders with labels on top (were using partial borders with misplaced labels)
  - Enter in name field advances to command field; Enter in command field saves
  - Hint line at bottom shows Tab:switch, Enter:next/save, Esc:cancel
- `⌥r` (add run command) now works on macOS
  - macOS ⌥+letter produces unicode chars (⌥r→®, ⌥c→ç, etc.), not ALT modifier
  - Added `macos_opt_key()` lookup in keybindings.rs mapping all 26 ⌥+letter chars back to original letters
  - All future ⌥+letter bindings should use `macos_opt_key(c) == Some('x')` pattern
- File tree entries now highlight when clicked
  - Missing `invalidate_file_tree()` call after setting selection from mouse click — cache was never rebuilt
- Shift+Arrow selection highlight now visible in input pane
  - `fast_draw_input()` was writing raw text without selection styling, overwriting what `build_wrapped_content()` renders
  - Fast-path and draw deferral now skipped when `has_input_selection()` is true
- Mouse drag selection now works in input pane
  - `handle_mouse_drag()` only handled viewer and convo panes — added input handling with `screen_to_input_char()`
  - pane_id=2 in `mouse_drag_start` tuple for input pane targeting
- Scroll-during-drag no longer loses prior selection
  - `mouse_drag_start` changed from screen coords `(u16, u16)` to cache coords `(usize, usize, u8)` with pane_id
  - Anchor position computed once on MouseDown; only the end position is re-converted on each Drag event
  - Auto-scroll no longer shifts the anchor because it's stored in cache space, not screen space
- Edit diff inline previews now show actual file line numbers instead of always starting at 1
  - Reads file on background render thread; tries `new_string` first (post-edit), falls back to `old_string` (live preview mid-edit)
  - Falls back to line 1 if file can't be read or both strings are empty (pure deletion)
- Edit diff removed (red) lines no longer have syntax highlighting
  - Removed lines now show dark grey text (`Rgb(100,100,100)`) on dim red background — darker than comment grey in syntax-highlighted green lines
  - Only added (green) lines get syntax highlighting on dim green background
  - Reduces highlight calls from 2→1 per Edit event
- Convo messages no longer duplicated during active Claude sessions
  - During streaming, events came from BOTH the live process (`handle_claude_output`) AND session file polling (`refresh_session_events`)
  - Session file polling now skipped when Claude is actively streaming to the current session
  - On Claude exit, a full re-parse from the session file reconciles live-streamed events with the authoritative JSONL (which has hook extraction, rewrite handling, etc.)
- User prompt no longer shows twice (or more) during live streaming
  - `pending_user_message` (the "You:" bubble shown immediately on submit) is now cleared when the first assistant/tool event arrives in the stream
  - stream-json stdout does NOT include `user` events — previous approach waited for a `UserMessage` that never arrived, leaving the pending bubble stuck for the entire session
  - Incremental renders accumulated duplicate pending bubbles: fixed by tracking `rendered_content_line_count` and trimming the stale bubble before submitting incremental requests
  - Stale bubble immediately trimmed from `rendered_lines_cache` on clear (no waiting for background render)
  - `poll_render_result()` re-sets the follow-bottom sentinel when the user was at/near the old bottom
- Multi-line input cursor no longer mispositioned after Shift+Enter
  - Root cause: ratatui's `.wrap(Wrap { trim: false })` uses word-level wrapping, but cursor computation used character-level wrapping — text rendered at different positions than cursor expected
  - Fix: removed `.wrap()` from input Paragraph entirely; `build_wrapped_content()` now pre-wraps at character boundaries (one Line per visual row) and computes cursor position in the same pass
  - `fast_draw_input()` also skipped for multi-line input (cached `input_area` has wrong height after newlines change)
  - Draw deferral disabled for multi-line so the input box resizes immediately
- Terminal typing no longer blanks the PTY display
  - `fast_draw_input()` was firing in terminal type mode (which sets `prompt_mode=true`), writing empty `app.input` over the terminal area
  - Deferred draw was also skipping `terminal.draw()` on terminal keystrokes, but PTY output has no fast-path — it needs ratatui to render
  - Fast-path now excludes terminal mode; draw deferral only applies to Claude prompt typing
- Input no longer freezes or drops characters while convo pane is updating
  - Background redraws (Claude streaming, animations) throttled to 10fps; key events always draw immediately
  - Expensive convo rendering (markdown/syntax/wrapping) now runs on a dedicated background thread (`RenderThread`)
  - Main event loop sends non-blocking render requests and polls for results — input is never blocked
  - Previous iterations: moved rendering outside `terminal.draw()` lock, then split render/draw into separate loop iterations, now fully asynchronous via background thread
  - Convo viewport cached — avoids cloning full rendered_lines_cache on typing-only frames
- Prompt input keybindings now actually work: ⌥c (clear), ↑/↓ (history), word nav
  - INPUT binding array previously declared ⌃z/⌃x for word nav, which conflicted with clipboard cut/undo
  - Word nav now uses standard macOS ⌥←/⌥→ (and ⌃←/⌃→) matching the actual handler
  - Added missing handlers for ⌃u (clear input), ↑ (history prev), ↓ (history next)
  - Prompt history browses UserMessage entries from the session conversation
- Prompt input no longer crashes on multi-byte characters (e.g., `ç` from ⌥+c)
  - `input_cursor` was used as both char index and byte offset — `String::insert()`/`remove()` need byte offsets
  - Added `char_to_byte()` conversion; all String operations now use byte offsets derived from char index
  - Also fixed `input_right()` and `input_end()` comparing char index against `String::len()` (bytes)
- User prompts no longer appear twice in the Convo pane
  - `pending_user_message` dedup was limited to last 5 events; Claude's rapid output (hooks, tools, text) pushed the matching `UserMessage` beyond that window
  - Now scans backward to the most recent `UserMessage` regardless of distance from tail
- Session dropdown in Worktrees pane now shows custom names from `.azureal/sessions` instead of truncated UUIDs
- `KeyCombo::display()` now preserves character case — previously uppercased all chars (e.g., `r` showed as `R`)
- `KeyCombo::display()` no longer shows `⇧` prefix for uppercase char keys (J, K, G, R show as-is)
- Quit simplified to `⌃q` (was `⌃⌥⌘c`), Restart to `⌃r` (was `⌃⌥⌘r`)
- `⌃c` now cancels Claude response only (was also quit)

### Added
- PID and exit code now shown in the convo pane's top border (right corner)
  - Green `PID:12345` while Claude is running
  - Switches to exit code on process exit: green `exit:0` for success, red `exit:N` for non-zero
  - Border line characters (─/═) fill the gap between title and PID — no spaces
  - PID removed from status bar messages (now visible in border instead)
- Run command system: save, pick, edit, delete, and execute shell commands from Worktrees pane
  - `r` opens picker (or executes directly if only 1 command saved)
  - `⌥r` opens dialog to add a new run command
  - `R` now performs rebase onto main (moved from `r`)
  - Picker: `j/k` navigate, `1-9` quick-select, `e` edit, `x` delete, `a` add
  - Commands persisted in `.azureal/runcmds`, loaded on startup
- Unified "New..." dialog with tabs for creating different resources
  - `n` from Worktrees pane opens tabbed dialog
  - Tab 1: Project (placeholder)
  - Tab 2: Branch (placeholder)
  - Tab 3: Worktree - existing worktree creation functionality
  - Tab 4: Session - create new Claude session with optional custom name
    - Custom names stored in `.azureal/sessions`
    - Leave name blank to use Claude's auto-generated UUID
    - Select target worktree for the session
  - `←`/`→` to switch tabs (except during text input)
- Clipboard operations for both Viewer edit mode and Prompt input (system clipboard)
  - `⌘C` - Copy to system clipboard
  - `⌘X` - Cut to system clipboard
  - `⌘V` - Paste from system clipboard
  - `⌘A` - Select all
  - `Shift+Arrow` - Extend selection
  - Selection highlighted with blue background
  - Typing/backspace/delete replaces selection
  - Works with external apps (copy from browser, paste in azureal, etc.)
- Hidden files/directories now shown in FileTree (previously filtered out)
  - Sorted after non-hidden items within each category (dirs/files)
  - Displayed in dimmed colors: gray for files, muted cyan for directories
  - Children of hidden directories inherit dimmed styling
  - Still excludes `target/` and `node_modules/` (too noisy)

### Fixed
- `.azureal/` directory no longer created eagerly on startup
  - Global config uses `~/.azureal/` (created only when needed)
  - Project data uses `.azureal/` in git root (created only when writing data)
  - Prevents unwanted `.azureal/` directories appearing in every git repo you run azureal from
- Centralized keybindings module (`src/tui/keybindings.rs`)
  - All keybindings defined once, used by both input handlers and help dialog
  - `Action` enum for all possible actions
  - `Keybinding` struct with primary + alternatives (e.g., j/↓ for same action)
  - `lookup_action()` for input handler dispatch
  - `help_sections()` auto-generates help dialog content
  - Adding/changing a keybinding now updates help automatically

### Changed
- Keybinding updates for terminal and prompt navigation:
  - `Esc` now closes terminal (was `t`)
  - `t` enters terminal type mode (was `i`)
  - `p` in terminal command mode closes terminal and enters Claude prompt
  - `p` is now global: closes help panel and enters prompt from anywhere
  - Terminal title shows context-aware hints: `t:type | p:prompt | Esc:close` in command mode, `Esc:exit` in type mode
  - Prompt title shows `⌃C:cancel response` in type mode

### Optimized
- Session file polling now uses lightweight file size check + dirty flag pattern
  - `check_session_file()` only reads file metadata (no parsing)
  - `poll_session_file()` defers parsing until idle via dirty flag
  - `refresh_session_events()` is a lightweight path that skips terminal/file tree reload

### Added
- Run commands feature: save and execute shell commands/scripts
  - `r` - Execute run command (picker if multiple, direct if one)
  - `⌥r` - Add new run command (name + command fields)
  - Picker dialog supports `j/k` nav, `1-9` quick select, `e` edit, `x` delete
  - Commands persisted to `.azureal/runcmds`
- Debug output now triggered manually via `⌃⌥⌘D` (Ctrl+Opt+Cmd+D)
  - Saves session parsing diagnostics to `.azureal/debug_output`
  - Removed `--out`/`-D` flag and `cargo rd` alias
- Viewer tabs: `t` to tab current file, `T` for tab dialog, `[`/`]` to switch
- Clickable file paths for Read, Write, and Edit tools in Convo pane
  - File paths are underlined in orange and can be clicked to open in Viewer
  - Edit tool clicks show file with diff overlay highlighting changes
  - Read/Write tool clicks open file plain without diff overlay
  - Clicked path shows inverted color highlight (orange bg, black fg) in Convo
- 4-pane TUI layout: Sessions (40 cols), FileTree (40 cols), Viewer (50%), Convo (50%)
  - FileTree shows directory structure for selected session's worktree
  - Viewer displays file content with syntax highlighting and line numbers
  - Viewer dual-purpose: file preview from FileTree OR diff detail from Convo
  - Tab cycles through all 4 panes plus Input
- FileTree navigation with j/k, Enter to open, Space/l to expand, h to collapse
- Viewer scroll with j/k (line), J/K (page), ⌥↑/⌥↓ (top/bottom)
- Per-session terminal persistence: each session maintains its own PTY shell session

### Changed
- Message bubble containment: all content constrained to bubble + 10 max width
  - User/Claude message text wraps within bubble width
  - Tool calls, results, hooks, diffs constrained to bubble + 10
- Tool command lines show full parameters (no "..." truncation on commands)

### Fixed
- **Critical performance fix**: `SyntaxHighlighter::new()` was being called inside `render_edit_diff` on EVERY render frame, loading the entire syntect SyntaxSet each time. Now reuses single instance from App state.
- **Render caching**: Convo pane now caches rendered lines instead of re-rendering all events on every frame. Cache invalidated only when display_events actually change. Eliminates O(n) rendering on scroll/navigation.
- **Scroll optimization**: Scroll functions now return whether position changed; skip redraw when at boundaries (no wasted frames when already at top/bottom)
- **Animation throttling**: Pulsating tool indicators now update at 4fps instead of every frame; scroll throttled to 10fps (was 20fps)
- Session file polling throttled from 100ms to 500ms to reduce parsing overhead on large sessions
- Removed debug dump on every redraw (was causing disk I/O on every frame in debug builds)
- Tool results show summarized output constrained to width:
  - Read: first + last line with line count
  - Bash: last 2 non-empty lines
  - Grep: first 3 matches
  - Glob: file count
  - Task: first 5 lines
- Modularized large source files using file-based module roots:
  - Module root files (`app.rs`, `git.rs`, `events.rs`, `tui.rs`) now contain only mod declarations and re-exports
  - Created `app/state.rs` for App struct and core methods (extracted from app.rs)
  - Created `app/session_parser.rs` for Claude session file parsing
  - Created `git/core.rs` for Git struct and core operations
  - Created `events/types.rs`, `events/display.rs`, `events/parser.rs` (split from events.rs)
  - Created `tui/run.rs` for TUI entry point and main layout
  - Split `tui/util.rs` into `colorize.rs`, `markdown.rs`, `render_events.rs`, `render_tools.rs`
- Replaced SQLite database (`azureal.db`) with JSON config (`azureal.json`) for minimal footprint
  - Session outputs now read exclusively from Claude's JSONL session files
  - One-time automatic migration from SQLite if old database exists
  - Human-readable JSON format for debugging and manual inspection

### Added
- Tool progress animation: Pulsating indicator (`◐`) while running, green (`●`) on success, red (`✗`) on failure
  - Visual feedback during tool execution matching Claude Code CLI style

### Fixed
- Tool error status now works when loading from session file (not just live streaming)
  - Errors detected by content patterns: "error:", "failed", "ENOENT", "permission denied", non-zero exit codes
  - Failed tools show red `✗` indicator instead of green `●`
- Pending tool status now tracked when loading from session file
  - Tools with `tool_use` but no `tool_result` yet show pulsating `◐` indicator
- Compaction summary no longer shows raw text blob as hooks
  - Summary message contains quoted `<system-reminder>` tags from conversation history
  - Compaction messages skipped during hook extraction (detected by summary format)
- UserPromptSubmit (UPS) hooks now appear directly after user prompts
  - UPS hooks extracted from assistant "thinking" blocks where Claude Code injects them
  - Hooks assigned timestamp = user_message_timestamp + 1ms for correct sort order
  - When events are sorted by timestamp, UPS hooks now appear immediately after their user message
  - UPS hooks from hooks.jsonl are skipped (duplicates of session file hooks with wrong timestamps)
  - Previously UPS hooks appeared after tool activity instead of after user prompts
- Command display: Slash commands (`/compact`, `/crt`, etc.) shown as prominent 3-line centered magenta banners
- Compacting indicator: "COMPACTING CONVERSATION" yellow banner when compaction starts
- Compacted indicator: "CONVERSATION COMPACTED" green banner when compaction completes
- Filtered out internal Claude messages: `<local-command-caveat>`, `<local-command-stdout>`, meta messages
- Rewound message deduplication: When user rewinds to edit a message, only the corrected version is shown
  - Detects by `parentUuid` - multiple user messages sharing the same parent, keeps only the most recent
- Debug dump (debug builds only): Auto-writes `.azureal/debug_output` on session load
  - Shows rendered output exactly as it appears in the TUI (with styling annotations)
  - Only enabled in debug builds (`cargo run`), not release builds
- Markdown rendering in Claude response output:
  - Headers (`#`, `##`, `###`) styled with block characters and colors
  - Bold (`**text**`) rendered without markers
  - Italic (`*text*`) rendered without markers
  - Inline code (`` `code` ``) with dark background
  - Code blocks (``` ```) with language label and box-drawn borders
  - Tables with `|` converted to box-drawing characters
  - Bullet and numbered lists properly indented
  - Blockquotes with vertical bar styling
- Hooks file watching - azureal polls `<project>/.azureal/hooks.jsonl` for entries from ALL hook types
  - File-based IPC workaround for Claude Code's stream-json limitation
  - Works with `~/.claude/scripts/log-hook.sh` helper script
  - All hooks (PreToolUse, PostToolUse, UserPromptSubmit, etc.) now display in output pane
- Live session output - azureal continuously polls the Claude session file for changes
  - Output pane updates in real-time as you chat with Claude in another terminal
  - No need to switch sessions to see new messages
- PTY-based embedded terminal pane - press `t` to toggle a full shell terminal
  - Acts as a portal to the user's actual terminal within Azureal
  - Full color support with ANSI escape sequences via `ansi-to-tui`
  - Proper cursor positioning and terminal emulation via `vt100` parser
  - Dynamic resizing to match pane dimensions
- Multi-session concurrent Claude agents - each session can run its own Claude process
- Conversation continuity via `--resume <session_id>` flag
- Clean output display parsing stream-json format:
  - User prompts shown as "You: <message>"
  - Claude responses shown as "Claude: <text>"
  - Tool calls shown as timeline nodes with tool name and primary parameter
  - Tool results with tool-specific formatting:
    - Read: first + last line with line count
    - Bash: last 2 lines (results usually at end)
    - Edit: complete diff output with actual file line numbers, red/green coloring for changed lines only, gray context for unchanged
    - Write: first comment/purpose line + line count
    - Grep: first 3 matches with overflow indicator
    - Glob: file count grouped by directory
    - Task: summary line from agent
    - WebFetch: page title + preview
    - WebSearch: first 3 numbered results
    - LSP: location + code context
  - Completion info shown as "[Done: Xs, $X.XXXX]"
- Mouse scroll support - scroll panels based on cursor position (independent of keyboard focus, Shift+drag for text selection)
- iMessage-style output formatting:
  - User messages: right-aligned cyan
  - Claude messages: left-aligned orange
  - Two blank lines between transitions
  - Timeline-style tool use display with parameter preview
### Changed
- Conversation data now read from Claude's session files with auto-discovery
  - Auto-discovers Claude session files by scanning `~/.claude/projects/<encoded-path>/`
  - Links most recent session file to azureal session automatically
  - Hooks loaded from `<project>/.azureal/hooks.jsonl` and merged by timestamp
  - Fallback to database when no Claude session files exist

### Optimized
- Event loop CPU usage reduced from 60-90% to <20% during mouse interaction:
  - Event batching: all pending events drained before redrawing
  - Scroll throttling: 20fps max for scroll, immediate for key events
  - Cached terminal size: only updates on resize events
  - Mouse motion events discarded instantly (zero processing)
  - Conditional terminal polling: only when terminal mode active

### Changed
- Storage moved from system-level (`~/.azureal/`) to project-level (`.azureal/` in git root)
  - Database, hooks.jsonl, and config are now per-project
  - Eliminates cross-project hook pollution
  - Falls back to `~/.azureal/` if not in a git repository
- Updated all dependencies to latest versions:
  - ratatui: 0.29 → 0.30
  - crossterm: 0.28 → 0.29
  - ansi-to-tui: 7 → 8
  - portable-pty: 0.8 → 0.9
  - vt100: 0.15 → 0.16
- Prompt echo format changed from "> " to "You: " for consistency

### Changed
- Sessions now load scrolled to bottom (most recent messages visible)
  - Initial load, session switch, and 'o' key all scroll to bottom
  - Use `⌥↑` to scroll to top if needed

### Fixed
- Output pane now loads conversation history on startup (was empty until switching sessions)
  - Added `load_session_output()` call after `app.load()` in startup sequence
- All hook types now display in output pane (UserPromptSubmit, PreToolUse, PostToolUse, etc.)
  - Parses `hook_progress` events from Claude Code's session data
  - Extracts hook output from echo commands in hook definitions
  - Parses hook output from system-reminder tags in user messages AND tool results
  - Previously only SessionStart hooks were visible
- Tool results now display in realtime during Claude's response (not just after completion)
  - EventParser now tracks tool calls by ID to match with tool_result blocks
  - Previously tool results only appeared after switching away from output pane and back
- Edit tool now shows actual diff with red/green highlighted backgrounds and real file line numbers
  - Extracts `old_string`/`new_string` from ToolCall input (not ToolResult which only has success message)
  - Reads file to find where edit occurred, displays actual line numbers (not relative 1,2,3...)
  - Only changed lines are highlighted - unchanged lines show in gray as context
  - Removed lines: white text on red background
  - Added lines: black text on green background
  - Diff displayed inline with the tool call for immediate visibility
- Write tool now shows line count + purpose line from ToolCall input (not empty result message)
  - Extracts `content` from ToolCall input to count lines and find first comment/purpose line
  - Displays inline with the tool call for immediate visibility
- Tool results now strip `<system-reminder>` blocks that Claude Code appends
  - Removes entire block (tags + content) so malware disclaimers don't appear in output
- Read tool now shows last non-empty line (skips trailing empty lines like `60→`)
- Init event no longer appears mid-conversation - only first Init shown
- Hook deduplication now consecutive-only (not global) - hooks appear throughout conversation
  - Previously: each unique (name, output) pair shown only once at first occurrence
  - Now: same hook can appear multiple times, only consecutive identical hooks deduplicated
  - Hooks now display next to their corresponding tool calls instead of clustering at beginning
- UserPromptSubmit hooks now extracted from session file via system-reminder tags
  - Added `extract_hooks_from_content` to `load_claude_session_events`
  - Parses hooks from user message content and tool result content
  - Extracts hooks from meta messages (`isMeta: true`) before skipping them for display
- Hook time-filtering now uses `now()` as upper bound instead of last event timestamp
  - Previously hooks after last session event were filtered out (5s buffer too small)
  - Now all hooks from session start to present are included
- Polling parser now properly captures ToolCall and ToolResult events for parallel tool calls
  - Fixed missing ToolResults when Claude makes multiple tool calls at once
  - Tool calls tracked by ID to match results to their corresponding calls
- Hooks now persist across session switches - saved to database with OutputType::Hook
- Hook logging runs in background (`&`) to ensure execution even if Claude Code terminates early
- Hook output no longer truncated to 50 characters - full first line now displays
- Event parser now captures ALL content blocks from assistant messages (was only capturing first)
- Fixed raw JSON appearing in output when tool_result content contained "Hook" text - now parses JSON before checking text patterns
- Fixed empty UserMessage boxes appearing for tool_result events with no text content
- Improved hook event parsing to only show hooks with actual output (skips hook_started events)
- Identified Claude Code 2.1.19 bug breaking `-p --resume` with tool calls
- Documented workaround: use Claude Code ≤ 2.1.17
- Resolved hook visibility limitation via file-based IPC workaround (all hooks now display)
