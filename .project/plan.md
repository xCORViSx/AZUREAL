# Plan: Syntax-highlighted code blocks in Session pane

## Status: COMPLETE

## Summary

Added syntax highlighting to code blocks in assistant messages in the Session pane. Previously all code block content rendered as plain `Color::Yellow`. Now uses the same syntect-based highlighting as the Viewer pane, resolved from the code fence language tag.

## Changes

### `src/syntax.rs`
- Added `highlight_code_block(content, lang)` — resolves syntax via `find_syntax_by_token` then `find_syntax_for_file("file.{ext}")` fallback
- Extracted shared `highlight_with_syntax(content, syntax_ref)` to deduplicate `highlight_file` and `highlight_code_block`
- Added 12 tests for token-based language matching

### `src/tui/render_markdown.rs`
- `render_assistant_text` now takes `&SyntaxHighlighter` parameter
- Code block lines are collected during iteration, then batch-highlighted via `emit_code_block()` on close
- Lines that exceed bubble width fall back to single-color wrapping
- Added 4 tests verifying syntax-colored spans in code blocks

### `src/tui/render_events.rs`
- Updated caller to pass `syntax_highlighter` through to `render_assistant_text`
