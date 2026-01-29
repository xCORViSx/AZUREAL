# Tool Result Display Options

5 different truncation strategies for each tool type. Pick one per tool.

---

## Read (File Contents)

### Option A: First 3 Lines
```
[Read] src/app.rs
│ //! Application state module
│ //!
│ //! Split into focused submodules:
│ ... (847 more lines)
```

### Option B: Line Count + Preview
```
[Read] src/app.rs → 850 lines
│ //! Application state module...
```

### Option C: Stats Only
```
[Read] src/app.rs ✓ 850 lines, 28KB
```

### Option D: First + Last Line
```
[Read] src/app.rs
│ //! Application state module
│ ...
│ } // end impl App
```

### Option E: Content Type Detection
```
[Read] src/app.rs → Rust module (850 lines)
│ mod input; mod terminal; mod types; mod util;
```

---

## Bash (Command Output)

### Option A: First 3 Lines
```
[Bash] cargo check
│ Checking azural v0.1.0
│ warning: unused import
│ warning: unused variable
│ ... (38 more lines)
```

### Option B: Exit Code + First Line
```
[Bash] cargo check → exit 0
│ Finished `dev` profile in 0.60s
```

### Option C: Exit Code Only
```
[Bash] cargo check ✓ exit 0
```

### Option D: Last 2 Lines (Results)
```
[Bash] cargo check → exit 0
│ ...
│ warning: 41 warnings generated
│ Finished `dev` profile in 0.60s
```

### Option E: Smart Summary
```
[Bash] cargo check ✓ 41 warnings, 0 errors (0.60s)
```

---

## Edit (File Modification)

### Option A: Diff Preview
```
[Edit] src/app.rs:120
│ - old_line_here
│ + new_line_here
│ ... (+3/-1 lines)
```

### Option B: Change Summary
```
[Edit] src/app.rs:120 → +15/-3 lines
```

### Option C: Confirmation Only
```
[Edit] src/app.rs ✓ modified
```

### Option D: Context Line
```
[Edit] src/app.rs:120
│ fn load_claude_session_events(...) → modified
```

### Option E: Before/After Snippet
```
[Edit] src/app.rs:120
│ before: let mut events = Vec::new();
│ after:  let mut events = Vec::with_capacity(100);
```

---

## Write (File Creation)

### Option A: First 2 Lines
```
[Write] src/new_file.rs
│ //! New module
│ use std::collections::HashMap;
│ ... (45 lines written)
```

### Option B: Stats Only
```
[Write] src/new_file.rs ✓ 45 lines, 1.2KB
```

### Option C: Confirmation Only
```
[Write] src/new_file.rs ✓ created
```

### Option D: File Type + Size
```
[Write] src/new_file.rs → Rust file, 45 lines
```

### Option E: Purpose Comment
```
[Write] src/new_file.rs ✓
│ //! New module for handling X
```

---

## Grep (Search Results)

### Option A: First 3 Matches
```
[Grep] "DisplayEvent" in src/
│ src/events.rs:132 - pub enum DisplayEvent {
│ src/app/mod.rs:26 - use crate::events::DisplayEvent;
│ src/tui/util.rs:9 - use crate::events::DisplayEvent;
│ ... (12 more matches)
```

### Option B: Match Count + Files
```
[Grep] "DisplayEvent" → 15 matches in 4 files
```

### Option C: File List Only
```
[Grep] "DisplayEvent" → events.rs, mod.rs, util.rs, draw_output.rs
```

### Option D: Count Per File
```
[Grep] "DisplayEvent"
│ src/events.rs (8) src/app/mod.rs (3) src/tui/util.rs (4)
```

### Option E: First Match + Count
```
[Grep] "DisplayEvent" → 15 matches
│ src/events.rs:132 - pub enum DisplayEvent {
```

---

## Glob (File Pattern Match)

### Option A: First 5 Files
```
[Glob] src/**/*.rs
│ src/main.rs
│ src/app/mod.rs
│ src/app/input.rs
│ src/tui/mod.rs
│ src/tui/util.rs
│ ... (23 more files)
```

### Option B: Count Only
```
[Glob] src/**/*.rs → 28 files
```

### Option C: Directory Summary
```
[Glob] src/**/*.rs → 28 files
│ src/ (4) src/app/ (5) src/tui/ (12) src/git/ (4) src/cmd/ (3)
```

### Option D: First + Last
```
[Glob] src/**/*.rs → 28 files
│ src/main.rs ... src/wizard.rs
```

### Option E: Tree Preview
```
[Glob] src/**/*.rs → 28 files
│ src/{main,app/mod,tui/mod,git/mod,...}.rs
```

---

## Task (Subagent)

### Option A: Summary Line
```
[Task] Explore → "Found 3 files handling authentication"
```

### Option B: Status Only
```
[Task] Explore ✓ completed
```

### Option C: First 2 Lines of Response
```
[Task] Explore
│ Found authentication handlers in:
│ - src/auth/mod.rs (main entry)
│ ...
```

### Option D: Key Finding
```
[Task] Explore → src/auth/mod.rs, src/auth/jwt.rs, src/auth/session.rs
```

### Option E: Duration + Summary
```
[Task] Explore (4.2s) → 3 files identified
```

---

## WebFetch (URL Content)

### Option A: Title + Preview
```
[WebFetch] docs.rs/ratatui
│ "Ratatui - Terminal UI Library"
│ A Rust library for building rich terminal...
```

### Option B: Title Only
```
[WebFetch] docs.rs/ratatui → "Ratatui - Terminal UI Library"
```

### Option C: Status Only
```
[WebFetch] docs.rs/ratatui ✓ fetched
```

### Option D: Size Info
```
[WebFetch] docs.rs/ratatui → 45KB, 1200 lines
```

### Option E: Key Sections
```
[WebFetch] docs.rs/ratatui
│ Sections: Installation, Quick Start, Widgets, Examples
```

---

## WebSearch (Search Results)

### Option A: First 3 Results
```
[WebSearch] "rust async patterns"
│ 1. Asynchronous Programming in Rust - rust-lang.org
│ 2. Async/Await Primer - tokio.rs
│ 3. Understanding Futures - fasterthanli.me
│ ... (7 more results)
```

### Option B: Count + Top Result
```
[WebSearch] "rust async patterns" → 10 results
│ 1. Asynchronous Programming in Rust - rust-lang.org
```

### Option C: Count Only
```
[WebSearch] "rust async patterns" → 10 results
```

### Option D: Domains Only
```
[WebSearch] "rust async patterns"
│ rust-lang.org, tokio.rs, fasterthanli.me, stackoverflow.com
```

### Option E: Categorized
```
[WebSearch] "rust async patterns" → 10 results
│ Official (2) Tutorials (4) Q&A (4)
```

---

## LSP (Code Intelligence)

### Option A: Result + Context
```
[LSP:goToDefinition] DisplayEvent
│ → src/events.rs:132
│ pub enum DisplayEvent {
```

### Option B: Location Only
```
[LSP:goToDefinition] DisplayEvent → src/events.rs:132
```

### Option C: Symbol Info
```
[LSP:hover] DisplayEvent
│ enum DisplayEvent (14 variants)
```

### Option D: Reference Count
```
[LSP:findReferences] DisplayEvent → 23 references in 8 files
```

### Option E: Compact
```
[LSP] goToDefinition → events.rs:132
```

---

## My Recommendations

| Tool | Recommendation | Rationale |
|------|---------------|-----------|
| Read | B (Line Count + Preview) | Know what was read without flooding |
| Bash | D (Last 2 Lines) | Results/errors are usually at the end |
| Edit | A (Diff Preview) | See what actually changed |
| Write | B (Stats Only) | Just need confirmation |
| Grep | A (First 3 Matches) | See actual matches found |
| Glob | C (Directory Summary) | Understand file distribution |
| Task | C (First 2 Lines) | See agent's actual response |
| WebFetch | A (Title + Preview) | Know what content was retrieved |
| WebSearch | A (First 3 Results) | See what sources were found |
| LSP | B (Location Only) | Quick reference |
