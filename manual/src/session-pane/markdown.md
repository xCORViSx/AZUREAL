# Markdown Rendering

Assistant messages are rendered as styled markdown rather than raw text. The
rendering pipeline runs on a background thread -- the main event loop submits
render requests and polls for completed results, so the draw function itself is
cheap and never blocks on parsing or syntax highlighting.

---

## Headers

Three heading levels are supported, each styled with a block character prefix
and a distinct color:

| Syntax | Block Char | Style |
|--------|-----------|-------|
| `# H1` | `█` (full block) | Bold, bright color |
| `## H2` | `▓` (dark shade) | Bold |
| `### H3` | `▒` (medium shade) | Bold |

The block character renders at the start of the line, visually anchoring the
heading hierarchy. Deeper headings (`####` and beyond) are not given special
styling -- they render as plain text.

---

## Inline Formatting

| Syntax | Rendering |
|--------|-----------|
| `**bold**` | Bold modifier applied |
| `*italic*` | Italic modifier applied |
| `` `inline code` `` | Yellow foreground on a dark background |

Inline formatting is parsed via the `parse_markdown_segments` function, which
tokenizes text into styled segments. Bold, italic, and code spans can appear
within any paragraph, list item, or blockquote.

---

## Code Blocks

Fenced code blocks (triple backtick) receive full treatment:

```text
┌─ rust
│ fn main() {
│     println!("Hello");
│ }
└─
```

**Structure:**

- The opening fence is replaced with a top border (`┌─`) and a language label
  in AZURE (`#3399FF`).
- Each code line is prefixed with a vertical bar (`│`) in dark gray, forming a
  visual gutter.
- The closing fence is replaced with a bottom border (`└─`).
- Content inside the block is syntax-highlighted using a tree-sitter-based
  highlighter that supports approximately 25 languages. The language is
  inferred from the info string after the opening backticks.

**Gutter color:** The left gutter bar uses an accent color (orange by default)
to visually distinguish code blocks from surrounding prose.

---

## Tables

Markdown tables are rendered with box-drawing characters instead of plain pipe
characters:

```text
│ Column A │ Column B │ Column C │
├──────────┼──────────┼──────────┤
│ value 1  │ value 2  │ value 3  │
```

**Box-drawing characters used:**

| Character | Position |
|-----------|----------|
| `│` | Vertical cell borders |
| `├` | Left junction (separator row) |
| `┼` | Cross junction (separator row) |
| `┤` | Right junction (separator row) |

**Column width clamping:** Column widths are pre-scanned and calculated to fit
within the bubble width. When the total table width would exceed the available
space, columns are proportionally narrowed. Cell content that overflows a
clamped column is truncated with an ellipsis (`...`).

**Separator rows:** Markdown separator rows (`|---|---|`) are replaced with
box-drawn separator lines using the junction characters above.

**Click to expand:** Rendered tables are registered as clickable regions. When
the user clicks a table, it opens in a full-width popup for easier reading.
See [Clickable Elements](./clickable-elements.md).

---

## Lists

Both bullet and numbered lists are supported:

- **Bullet lists** (`- item`) render with a cyan bullet character, indented
  from the left margin.
- **Numbered lists** (`1. item`) render with the number preserved, also
  indented with cyan styling.

Nested lists maintain their indentation level. List items can contain inline
formatting (bold, italic, code spans).

---

## Blockquotes

Lines starting with `>` render as blockquotes:

- A gray vertical bar appears on the left edge.
- The quoted text renders in italic.
- The visual style clearly separates quoted content from the assistant's own
  text.

---

## File Paths in Text

When the assistant mentions file paths in its prose (not inside tool calls),
the renderer detects them and styles them as underlined orange links. These
paths become clickable -- clicking one opens the file in the Viewer pane. See
[Clickable Elements](./clickable-elements.md) for details.

---

## Verification Paragraphs

Text beginning with `Verification:` (or bold/italic variants like
`**Verification:**`) is detected and styled distinctly. This handles the common
pattern where Claude outputs a verification section summarizing what it checked
or confirmed.

---

## Rendering Pipeline

The markdown renderer never runs on the main thread during draw. Instead:

1. The event loop calls `submit_render_request()` with the current display
   events and panel width.
2. A background render thread parses markdown, runs syntax highlighting, wraps
   text, and produces a line cache.
3. The event loop polls `poll_render_result()` each tick.
4. The draw function reads from the pre-built cache -- just a slice clone and
   viewport overlay.

When the session pane is resized, the cache width is compared to the new inner
width. A mismatch marks the cache as dirty, triggering a new render request on
the next loop iteration. The draw function never renders synchronously -- it
uses whatever cache exists, even if stale by one frame.
