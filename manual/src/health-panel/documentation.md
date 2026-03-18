# Documentation Health

The Documentation tab measures doc-comment coverage across your codebase. It
scans every source file for documentable items and checks whether each one has a
preceding doc comment. The result is a per-file and overall coverage score that
tells you where documentation is missing.

---

## What Gets Scanned

The scanner identifies the following **documentable items** using a line-based
heuristic (no AST parsing):

| Item Kind |
|-----------|
| `fn` |
| `struct` |
| `enum` |
| `trait` |
| `const` |
| `static` |
| `type` |
| `impl` |
| `mod` |

For each item found, the scanner checks whether the lines immediately preceding
it contain a `///` or `//!` doc comment. If a doc comment is present, the item is
counted as documented. If not, it is counted as undocumented.

This is a line-based heuristic, not a full AST analysis. It works well for
standard Rust code formatting but may produce inaccurate results for unusual
layouts.

---

## Overall Score

The top of the Documentation tab shows an **overall documentation coverage
score** as a percentage. The score header is color-coded:

| Score Range | Color |
|-------------|-------|
| 80% and above | Green |
| 50% to 79% | Yellow |
| Below 50% | Red |

---

## Per-File Breakdown

Below the overall score, each scanned file is listed individually with its own
coverage percentage. Files are sorted by **coverage ascending** -- the files with
the worst documentation appear first, making it easy to identify where attention
is most needed.

---

## Keybindings

| Key | Action |
|-----|--------|
| `j` / `k` | Navigate up / down one entry |
| `Space` | Toggle checkbox on the highlighted file |
| `a` | Check all files that are not already at 100% coverage |
| `v` | View the highlighted file in the file viewer |
| `Enter` | Spawn documentation sessions for all checked files |

---

## Documentation Sessions

When you press **`Enter`** with one or more files checked, AZUREAL spawns agent
sessions to add missing documentation. Each file gets its own session tagged with
`[DH]` (Documentation Health).

All `[DH]` sessions run **concurrently**. The agent prompt instructs Claude to
add doc comments to all undocumented items in the file **without modifying any
existing code**. Only documentation is added -- no refactoring, no formatting
changes, no logic changes.

You can track progress in the session pane, where each `[DH]` session appears as
a separate conversation.

---

## Quick Reference

```text
Shift+H       Open Health Panel
Tab           Switch to God Files tab
j/k           Navigate results
Space         Toggle checkbox
a             Check all non-100% files
v             View file in viewer
Enter         Spawn [DH] sessions for checked files
s             Enter Scope Mode
```
