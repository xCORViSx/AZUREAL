# Getting Started

This section walks you through everything needed to go from zero to a running
AZUREAL instance: system requirements, installation, and what to expect the
first time you launch the application.

## What is AZUREAL?

AZUREAL (Asynchronous Zoned Unified Runtime Environment for Agentic LLMs) is a
terminal-based interface that wraps **Claude Code CLI** and **OpenAI Codex CLI**
into a single multi-agent development environment. Each feature branch gets its
own **git worktree** with isolated agent sessions, so you can run multiple AI
assistants concurrently across different parts of your project without any
cross-contamination.

The core workflow loop looks like this:

1. Open your project in AZUREAL.
2. Create a worktree for a feature branch.
3. Prompt an agent (Claude or Codex) to work in that worktree.
4. Switch to another worktree and prompt a different agent in parallel.
5. Review changes, commit, squash-merge back to main.

All of this happens inside a single terminal window with keyboard-driven
navigation, a built-in file browser, git staging panel, embedded terminal, and
persistent session history.

## In This Section

- **[Requirements](getting-started/requirements.md)** -- What you need
  installed before AZUREAL will build and run.
- **[Installation](getting-started/installation.md)** -- How to get the binary
  onto your system.
- **[First Launch](getting-started/first-launch.md)** -- What happens when you
  run `azureal` for the first time.
