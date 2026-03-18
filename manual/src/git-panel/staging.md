# Changed Files & Staging

The Changed Files section occupies the bottom half of the git panel's left
sidebar. It lists every file with uncommitted changes and provides controls for
staging, unstaging, and discarding those changes. AZUREAL uses a **UI-only
staging model** -- staging state is tracked entirely within the interface, not
read from or written to the git index until commit time.

---

## File Status Indicators

Each file in the list is prefixed with a single-character status indicator,
color-coded by change type:

| Char | Meaning | Color |
|------|---------|-------|
| `M` | Modified | Yellow |
| `A` | Added (new tracked file) | Green |
| `D` | Deleted | Red |
| `R` | Renamed | Cyan |
| `?` | Untracked | Magenta |

These characters and colors match the conventions of `git status --short`,
making the display immediately readable for anyone familiar with git.

---

## Staged vs. Unstaged Appearance

The visual treatment of each file entry changes based on its staging state:

### Staged Files

- File path rendered in **normal text** with an underline
- Status character uses its normal color (from the table above)
- Diff stats (`+N / -M`) shown in normal intensity

### Unstaged Files

- File path rendered in **strikethrough** with DarkGray color
- Status character dimmed
- Diff stats dimmed

This visual distinction makes it possible to see at a glance which files will
be included in the next commit and which will be left out.

---

## Default Staging Behavior

All files default to **staged** when the changed files list loads. This matches
the most common intent: you typically want to commit everything. The staging
controls exist for the cases where you need to exclude specific files from a
commit.

Because staging is UI-only, it has no effect on the actual git index. AZUREAL
does not run `git add` or `git reset` when you toggle staging. The staging
state is applied at commit time, when AZUREAL stages exactly the files marked
as staged in the UI before running `git commit`.

---

## Staging Controls

| Key | Context | Action |
|-----|---------|--------|
| `s` | Changed Files focused | Toggle stage/unstage for the selected file |
| `Shift+S` | Changed Files focused | Stage or unstage all files at once |

Pressing `s` on a staged file unstages it (strikethrough + dim). Pressing `s`
again restages it (underline + normal). `Shift+S` is a bulk toggle: if any
files are unstaged, it stages all; if all are staged, it unstages all.

---

## Discarding Changes

| Key | Context | Action |
|-----|---------|--------|
| `x` | Changed Files focused | Discard changes to the selected file |

Pressing `x` prompts for inline confirmation directly in the file list entry.
The prompt displays `y/n` next to the file name. Pressing `y` confirms the
discard; pressing `n` or any other key cancels.

The discard mechanism depends on the file type:

| File Type | Git Command |
|-----------|-------------|
| Tracked (modified/deleted) | `git restore <path>` |
| Untracked | `git clean -f <path>` |

Discarding is irreversible -- there is no undo. The inline confirmation exists
specifically to prevent accidental data loss.

---

## Title Bar

The Changed Files section title summarizes the current state:

```text
Files (12, 10✓, +84 / -31)
```

The components are:

| Component | Meaning |
|-----------|---------|
| `12` | Total number of changed files |
| `10✓` | Number of files currently staged |
| `+84 / -31` | Aggregate lines added and removed across all files |

This gives you a quick summary without needing to scan every file in the list.
