# File Actions

The File Tree supports direct file and directory manipulation through a set of
single-key actions. All file actions operate on the currently highlighted entry
in the tree and are available in command mode when the File Tree has focus.

---

## Action Summary

| Key | Action | Description |
|-----|--------|-------------|
| `a` | Add | Create a new file or directory |
| `d` | Delete | Delete the highlighted entry |
| `r` | Rename | Rename the highlighted entry |
| `c` | Copy | Copy the highlighted entry to another location |
| `m` | Move | Move the highlighted entry to another location |

---

## Add (`a`)

Pressing `a` opens an inline text input at the bottom of the File Tree pane.
Type the name of the new file or directory to create.

**Creating a file:** Type a filename (e.g., `utils.rs`) and press `Enter`. The
file is created as an empty file inside the directory that currently has focus.
If the cursor is on a file, the new file is created in that file's parent
directory.

**Creating a directory:** Append a trailing `/` to the name (e.g., `helpers/`)
and press `Enter`. The directory is created, and the tree refreshes to show it.

Press `Esc` to cancel without creating anything.

---

## Delete (`d`)

Pressing `d` on a highlighted file or directory triggers a **confirmation
prompt**:

```text
Delete "filename"? (y/N)
```

The default is **No** -- pressing `Enter` without typing `y` cancels the
deletion. You must explicitly press `y` to confirm. This prevents accidental
data loss from a stray keypress.

Deleting a directory removes it recursively, including all of its contents.

---

## Rename (`r`)

Pressing `r` opens an inline text input **pre-filled with the current name** of
the highlighted entry. Edit the name as needed and press `Enter` to confirm the
rename.

The rename operates within the same directory -- it changes the entry's name but
does not move it. To move a file to a different directory, use the Move action
instead.

Press `Esc` to cancel without renaming.

---

## Copy (`c`)

Copy is a two-step operation:

### Step 1: Grab the Source

Press `c` on the entry you want to copy. The entry is marked as the **copy
source** with a visual indicator: a solid border around the name rendered in
magenta.

```text
┃filename.rs┃
```

The solid-border magenta highlight persists as you navigate the tree, reminding
you that a copy operation is in progress.

### Step 2: Paste at Target

Navigate to the target directory and press `Enter` to paste. The source file
(or directory) is copied into the target directory, preserving the original name.
If a file with the same name already exists at the target, the operation fails
with a status message.

**Recursive directory copy** is supported. Copying a directory duplicates its
entire contents, including all subdirectories and files.

### Cancelling

Press `Esc` at any point to cancel the copy operation. The source highlight is
removed and the tree returns to normal.

---

## Move (`m`)

Move works identically to Copy in its two-step flow, with two differences:

1. The visual indicator uses a **dashed border** in magenta instead of a solid
   border:

   ```text
   ╎filename.rs╎
   ```

2. After pasting at the target, the **original entry is removed** from its
   source location. Move is effectively a copy-then-delete.

### Step 1: Grab the Source

Press `m` on the entry you want to move. The dashed-border magenta highlight
appears.

### Step 2: Paste at Target

Navigate to the target directory and press `Enter`. The entry is moved to the
new location.

### Cancelling

Press `Esc` to cancel the move. The source highlight is removed and no
filesystem changes occur.

---

## Visual Summary

| Operation | Border Style | Border Color |
|-----------|-------------|--------------|
| Copy source | Solid (`┃name┃`) | Magenta |
| Move source | Dashed (`╎name╎`) | Magenta |

The distinct border styles make it possible to tell at a glance whether you are
in a copy or move operation.

---

## Error Handling

All file actions report errors via the status bar. Common error scenarios:

| Scenario | Message |
|----------|---------|
| Name already exists (add/rename) | File already exists |
| Target already exists (copy/move) | Target already exists |
| Permission denied | Permission denied |
| Delete confirmation declined | (no message, action cancelled silently) |

Failed operations leave the filesystem unchanged.
