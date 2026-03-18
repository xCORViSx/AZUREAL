# Auto-Resolve Settings

Auto-resolve is a mechanism that handles certain rebase conflicts automatically,
without spawning an RCR session or requiring any user interaction. It works by
maintaining a configurable list of files that can be safely resolved via 3-way
union merge -- keeping both sides of the conflict with no conflict markers.

---

## How Auto-Resolve Works

When a rebase encounters conflicts, AZUREAL checks whether **all** conflicted
files are in the auto-resolve list. If they are, the conflict is resolved
automatically using `git merge-file --union` for each file. If even one
conflicted file is not in the auto-resolve list, auto-resolve does not apply
and the full RCR flow takes over.

### The Union Merge Strategy

```sh
git merge-file --union <current> <base> <other>
```

The `--union` flag performs a 3-way merge that keeps both sides of every
conflict. Rather than inserting conflict markers (`<<<<<<<`, `=======`,
`>>>>>>>`), it concatenates both versions. This works well for files where
both sides' changes are additive and order does not matter -- documentation
files, changelogs, and configuration files are typical examples.

### Looping Through Commits

A rebase replays commits one at a time. Auto-resolve does not just handle the
first conflicted commit -- it loops through all subsequent commits in the
rebase, auto-resolving any that have only auto-resolvable conflicts. The loop
continues until either the rebase completes or a commit has a conflict
involving a file not in the auto-resolve list. At that point, the rebase pauses
and hands off to the RCR flow for manual resolution.

---

## Default Auto-Resolve Files

The following files are configured for auto-resolve by default:

| File |
|------|
| `AGENTS.md` |
| `CHANGELOG.md` |
| `README.md` |
| `CLAUDE.md` |

These are documentation and configuration files that agents frequently modify
in parallel across branches. Because changes to these files are typically
additive (new entries, new sections), the union merge strategy produces correct
results in the vast majority of cases.

---

## Settings Overlay

Press `s` in the git panel actions to open the auto-resolve settings overlay:

```text
╔══════ Auto-Resolve Files ══════╗
║                                ║
║  [x] AGENTS.md                 ║
║  [x] CHANGELOG.md              ║
║  [x] README.md                 ║
║  [x] CLAUDE.md                 ║
║  [ ] Cargo.toml                ║
║                                ║
║  j/k: navigate                 ║
║  Space: toggle                 ║
║  a: add file                   ║
║  d: remove file                ║
║  Esc: save & close             ║
║                                ║
╚════════════════════════════════╝
```

### Settings Keybindings

| Key | Action |
|-----|--------|
| `j` / `k` | Navigate up/down in the file list |
| `Space` | Toggle the selected file on/off |
| `a` | Add a new file to the list (prompts for file path) |
| `d` | Remove the selected file from the list |
| `Esc` | Save changes and close the overlay |

Changes are saved immediately on close.

---

## Persistence

The auto-resolve file list is stored in `azufig.toml` under the `[git]`
section:

```toml
[git]
auto_resolve_files = [
    "AGENTS.md",
    "CHANGELOG.md",
    "README.md",
    "CLAUDE.md",
]
```

This configuration is per-project, so different projects can have different
auto-resolve lists. Editing the TOML file directly has the same effect as using
the settings overlay.
