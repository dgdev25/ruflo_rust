# 0008 — Native Rust swarm execution (claude + codex, no Node)

## Status

Accepted + Implemented (2026-08-08). Supersedes ADR-0007.

## Context

ADR-0007 deferred full agent orchestration to the Node runtime and restricted
native dual-run to Codex-only. The rebuild's goal is a self-contained native
binary that runs multi-agent swarms without Node. Both `claude` (Claude Code
2.x) and `codex` (codex-cli) CLIs are available as local subprocesses.

## Decision

Native Rust swarm execution spawns real agent-worker subprocesses directly:

- `swarm start --objective "<goal>" --workers N [--agent claude|codex]`
- Each worker is a `claude --print` or `codex exec` subprocess forked via
  `std::process::Command` — no Node, no MCP bridge, no JS runtime.
- Workers run concurrently (one OS thread per worker), each with the objective
  + a per-worker role/slice prompt.
- Worker working directory = the project root (shared checkout). A `--worktree`
  flag optionally creates one git worktree per worker for isolation.
- Each worker's stdout/stderr/exit is captured and recorded: into the swarm
  state file (`.claude-flow/swarm.json`) and into ruflo memory
  (`ruflo memory store`) keyed by worker ID.
- Environment is sanitized: `*_KEY`, `*_TOKEN`, `*_SECRET` vars matching common
  credential patterns are stripped from the worker env unless `--keep-env`.
- `--dry-run` prints the worker plan without spawning.

## Consequences

- Swarms run end-to-end in pure Rust — no Node dependency for agent execution.
- Both claude and codex workers supported; `--agent` selects.
- The CLI binary is the swarm coordinator: it records state, spawns workers,
  collects results, persists to memory. Real parallel multi-agent execution.
- ADR-0007's Codex-only / opt-in / worktree-required restrictions are lifted.
