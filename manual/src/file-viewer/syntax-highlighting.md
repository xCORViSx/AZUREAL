# Syntax Highlighting

AZUREAL uses **tree-sitter** for all syntax highlighting. Unlike regex-based
highlighters, tree-sitter parses source code into a full abstract syntax tree
(AST), producing accurate highlighting that correctly handles nested structures,
multi-line strings, and language-specific edge cases.

---

## Supported Languages

Twenty-four language grammars are bundled:

| Language | Extensions |
|----------|-----------|
| Rust | `.rs` |
| Python | `.py`, `.pyw`, `.pyi` |
| JavaScript | `.js`, `.mjs`, `.cjs`, `.jsx` |
| TypeScript | `.ts`, `.mts`, `.cts` |
| TSX | `.tsx` |
| JSON | `.json`, `.jsonc` |
| TOML | `.toml` |
| Bash | `.sh`, `.bash`, `.zsh` |
| C | `.c`, `.h` |
| C++ | `.cpp`, `.hpp`, `.cc`, `.cxx`, `.hxx`, `.hh` |
| Go | `.go` |
| HTML | `.html`, `.htm` |
| CSS | `.css` |
| Java | `.java` |
| Ruby | `.rb` |
| Lua | `.lua` |
| YAML | `.yml`, `.yaml` |
| Markdown | `.md`, `.markdown` |
| Scala | `.scala`, `.sc` |
| R | `.r`, `.R` |
| Haskell | `.hs` |
| PHP | `.php` |
| SQL | `.sql` |
| Perl | `.pl`, `.pm` |

Perl is registered with an empty highlight query, so `.pl` and `.pm` files are
recognized but rendered as plain text without syntax coloring.

Files with unrecognized extensions are displayed as plain text without
highlighting.

---

## Language Detection

Language detection uses two strategies depending on context:

### File Viewer

When a file is opened in the Viewer, the language is determined by **file
extension lookup**. The file's extension is matched against the table above.
This is a direct map with no ambiguity -- each extension maps to exactly one
grammar.

### Session Code Blocks

When rendering fenced code blocks in the Session pane (agent responses), the
language is determined by the **code fence token** -- the string after the
opening triple backticks. For example, `` ```rust `` selects the Rust grammar,
`` ```python `` selects Python, and so on. Unrecognized or missing fence tokens
result in plain-text rendering.

---

## Capture-to-Color Mappings

Tree-sitter grammars produce **captures** -- named tokens like `keyword`,
`string`, `comment`, and `function`. AZUREAL maps 26 capture names to specific
colors:

| Capture | Color |
|---------|-------|
| `attribute` | Light blue / Lavender |
| `comment` | Gray (dim) |
| `constant` | AZURE (blue) |
| `constant.builtin` | Yellow |
| `constructor` | AZURE (blue) |
| `embedded` | White |
| `escape` | Magenta |
| `function` | Blue |
| `function.builtin` | Blue |
| `function.method` | Blue |
| `keyword` | Magenta |
| `label` | AZURE (blue) |
| `number` | Yellow |
| `operator` | White |
| `property` | White |
| `punctuation` | White |
| `punctuation.bracket` | White |
| `punctuation.delimiter` | White |
| `string` | Green |
| `string.special` | Green |
| `tag` | AZURE (blue) |
| `type` | Yellow |
| `type.builtin` | Yellow |
| `variable` | White |
| `variable.builtin` | Magenta |
| `variable.parameter` | Orange |

These mappings produce a consistent visual language across all supported
grammars. Keywords and control flow are always in the magenta family, strings
are always green, and comments are always dimmed -- regardless of the source
language.

---

## Dual Instances

AZUREAL runs two independent tree-sitter instances:

1. **App instance (main thread)** -- Used for highlighting file content in the
   Viewer and for edit-mode syntax cache updates. This instance runs on the main
   application thread and processes files as they are opened or edited.

2. **Render instance (background thread)** -- Used for highlighting code blocks
   in Session pane rendering. Because session content can contain many code
   blocks across multiple languages, this instance runs on the dedicated render
   thread to avoid blocking the main event loop.

Both instances share the same grammar set and color mappings. They are fully
independent in memory, so there is no synchronization overhead between them.

---

## Performance

Tree-sitter parsing is incremental. When a file is edited, only the changed
region of the AST is re-parsed rather than the entire file. This makes syntax
highlighting in [Edit Mode](./edit-mode.md) fast even for large files.

In the Viewer (read-only mode), the entire file is parsed once on load and the
highlight result is cached. In edit mode, the syntax cache is invalidated and
regenerated per edit version -- not per frame -- so rapid typing does not cause
redundant re-parses.
