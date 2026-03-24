# Introduction

**AZUREAL** -- **A**synchronous **Z**oned **U**nified **R**untime **E**nvironment for **A**gentic **L**LMs -- is a Rust TUI application that wraps Claude Code CLI and OpenAI Codex CLI to enable multi-agent development workflows with git worktree isolation.

Built with [ratatui](https://ratatui.rs/), AZUREAL provides a terminal-native interface for orchestrating concurrent AI coding agents, each operating in its own git worktree with independent branches, sessions, and working directories. Instead of running one agent at a time in a single checkout, you can run several agents simultaneously across isolated feature branches, all managed from a single terminal window.

This page includes a small documentation-only mock edit so workflow tests can produce a visible, harmless diff.

Version **1.0.0** | MIT License | Author: xCORViSx

---

## Philosophy

AZUREAL is built around two core ideas:

### Mostly-Stateless Architecture

Runtime state is derived from git, not stored in custom databases or config files. Worktrees are discovered via `git worktree list`. Branches are discovered via `git branch`. The active backend (Claude or Codex) is derived from whichever model is currently selected. When you close the application and reopen it, the state reconstructs itself from your repository.

Persistent data is minimal by design:

- **Session store** -- a single SQLite file (`.azureal/sessions.azs`) holds all conversation history, portable across machines by copying one file.
- **Configuration** -- two small TOML files (`azufig.toml`), one global and one per-project, store preferences and registered projects.
- **Agent session files** -- temporary JSONL files that are parsed during live streaming, ingested into the SQLite store on exit, then deleted.

There is no daemon, no server process, and no external database to manage.

### Multi-Agent Concurrent Development

Traditional AI-assisted development is sequential: you prompt an agent, wait for it to finish, review the result, then prompt again. AZUREAL breaks this pattern by letting you run multiple agents in parallel, each in its own git worktree. While one agent refactors a module on `feat-a`, another can be writing tests on `feat-b`, and a third can be fixing a bug on `fix-c` -- all at the same time, with no interference between them.

Git worktrees provide the isolation guarantee. Each worktree has its own working directory, its own branch, and its own uncommitted changes. Agents cannot step on each other because they are literally working in different directories on different branches.

---

## Key Capabilities

### Multi-Worktree Sessions

Create, archive, rename, and delete git worktrees from within the TUI. Each worktree hosts independent agent sessions with PID-keyed slots, so you can even run multiple agents on the same branch concurrently.

### 3-Pane Layout

The default view divides the terminal into three panes: a file tree on the left, a file viewer in the center, and the session pane on the right. Pane proportions are tuned for readability, and Tab/Shift+Tab cycles focus between them.

### Embedded Terminal

A full shell per worktree with color support, click-to-position cursor, word navigation, mouse drag selection with auto-scroll, and mouse wheel scrolling. Toggle it with a single keystroke.

### Git Panel

A dedicated overlay for the full git workflow: staging, committing with AI-generated messages, squash merging, rebasing, conflict resolution with Claude-assisted RCR (Rebase Conflict Resolution), and pull/push operations.

### Health Panel

Scan your codebase for oversized "god files" and missing documentation, then spawn an agent to fix them directly from the panel.

### Speech-to-Text

Dictate prompts with a keybinding. Audio is transcribed locally via Whisper -- nothing leaves your machine.

### Projects Panel

Switch between multiple projects without leaving the application. Agent processes continue running in the background when you switch away, with activity status icons per project.

### Dual-Backend Support (Claude + Codex)

Cycle between Claude models (Opus, Sonnet, Haiku) and Codex models (GPT-5.4, GPT-5.3-codex, GPT-5.2-codex, and others) with a single keybinding. The backend is derived automatically from the selected model -- no manual configuration required. A single session can span prompts to both backends.

### Session Store with Context Injection

Conversations persist in a SQLite database with automatic compaction. Context is injected into each new prompt from the store, eliminating dependency on external session files for conversation continuity. The store is the single source of truth.

---

## Target Audience

AZUREAL is designed for developers who:

- Already use AI coding assistants (Claude Code, OpenAI Codex) and want to scale beyond single-agent, single-branch workflows.
- Work on projects large enough to benefit from concurrent feature development across multiple branches.
- Prefer terminal-native tools and keyboard-driven interfaces over GUI applications.
- Want git worktree isolation without the overhead of manually managing worktrees, sessions, and agent processes.

Familiarity with git (especially branching and worktrees) and at least one supported AI coding CLI is assumed throughout this manual.

---

## How This Manual Is Organized

The manual is divided into three sections:

### User Guide

The main body of the manual, covering everything you need to use AZUREAL day-to-day:

- **Getting Started** -- requirements, installation, and first launch.
- **The TUI Interface** -- layout, pane navigation, mouse support, and text selection.
- **Keybindings & Input Modes** -- vim-style modes, global keybindings, leader sequences, and the help overlay.
- **Git Worktrees** -- creating, managing, archiving, and browsing worktrees.
- **Agent Sessions** -- Claude and Codex backends, model switching, multi-agent concurrency, and session lifecycle.
- **Session Store & Persistence** -- the SQLite `.azs` format, context injection, compaction, and portability.
- **The Session Pane** -- markdown rendering, tool call display, session list, todo widget, context meter, and clickable elements.
- **The File Browser & Viewer** -- file tree, syntax highlighting, markdown preview, image viewing, diffs, tabs, and edit mode.
- **The Git Panel** -- staging, AI commit messages, squash merge, rebase, conflict resolution, and push/pull.
- **The Embedded Terminal** -- terminal modes, shell integration, and terminal-specific features.
- **Projects Panel** -- managing multiple projects, parallel operation, and background processes.

### Features

Standalone feature pages for capabilities that span multiple parts of the interface:

- Speech-to-text, run commands, preset prompts, health panel, completion notifications, and debug dump.

### Reference

Technical and configuration details:

- **Configuration** -- global and project-level `azufig.toml` settings.
- **Architecture & Internals** -- event loop, render pipeline, performance characteristics, and file watcher.
- **Platform Support** -- macOS, Linux, and Windows specifics.
- **CLI Reference** -- command-line flags and subcommands.
- **Complete Keybinding Reference** -- every keybinding in one table.

---

> This manual tracks AZUREAL v1.0.0 — the first stable release.
