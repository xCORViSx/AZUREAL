# Markdown Preview

When the Viewer opens a `.md` or `.markdown` file, it switches from plain
syntax-highlighted source to a **prettified markdown preview**. This mode
renders markdown elements with styled formatting, making documentation files
readable without leaving the terminal.

---

## Rendered Elements

### Headers

Headers are rendered with a single block prefix character that varies by level:

```text
█ H1 Heading       (AZURE, bold, underlined)
▓ H2 Heading       (AZURE, bold)
▒ H3 Heading       (Green, bold)
░ H4-H6 Heading    (Green)
```

The descending block density (`FULL` > `DARK SHADE` > `MEDIUM SHADE` >
`LIGHT SHADE`) creates visual weight that distinguishes heading levels at a
glance. Header text is rendered bold (H1-H3) with H1 additionally underlined.

### Bullets and Lists

- **Unordered lists** render with standard bullet characters, indented per
  nesting level.
- **Ordered lists** render with their original numbering preserved.

Nested lists maintain correct indentation relative to their parent.

### Blockquotes

Blockquotes are prefixed with a vertical bar on each line:

```text
┃ This is a blockquote.
┃ It can span multiple lines.
```

The `BOX DRAWINGS HEAVY VERTICAL` character provides a clean visual margin.
Nested blockquotes stack the bar prefix (one bar per nesting level).

### Code Blocks

Fenced code blocks are rendered with **syntax highlighting** using the
tree-sitter grammar identified by the code fence token. A `` ```rust `` block
is highlighted as Rust, `` ```python `` as Python, and so on. Blocks without
a fence token or with an unrecognized token render as plain text.

Code blocks are visually distinct from the surrounding prose through indentation
and background contrast.

### Tables

Tables are rendered with **box-drawing characters** for borders:

```text
┌──────────┬───────────┐
│ Column A │ Column B  │
├──────────┼───────────┤
│ value 1  │ value 2   │
│ value 3  │ value 4   │
└──────────┴───────────┘
```

Column widths are calculated from the content. Header rows are visually
separated from data rows by a horizontal rule.

### Inline Styling

| Markdown | Rendering |
|----------|-----------|
| `**bold**` | Bold text |
| `*italic*` | Italic text |
| `` `code` `` | Styled inline code span |

Inline styles nest correctly -- `***bold italic***` produces bold italic text.

---

## Line Numbers

Markdown preview mode renders with **no line numbers**. The gutter width is set
to zero, giving the rendered content the full width of the Viewer pane. This
matches the reading-oriented intent of markdown preview -- line numbers are
useful for source code, not for documentation.

---

## Edit Mode Interaction

When you press `e` to enter [Edit Mode](./edit-mode.md) on a markdown file, the
preview is replaced with the **raw markdown source** rendered with standard
syntax highlighting (using the Markdown tree-sitter grammar). This shows the
actual markup characters -- `#`, `*`, `` ` ``, `|`, and so on -- so you can
edit them directly.

Exiting edit mode (via `Esc`) returns to the prettified preview, re-rendering
the file with any changes you made.
