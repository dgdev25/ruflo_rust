# 0007 — Make native dual-run Codex-only, opt-in, and worktree-confined

## Status

**Superseded** by ADR-0008 (2026-08-08). Native swarm execution now supports
both `claude` and `codex` worker subprocesses directly from Rust — no Node
runtime involved. The Codex-only restriction is lifted.

## Context

Ruflo's TypeScript dual-mode orchestrator calls local `codex exec` workers and
also contains a separate Claude worker boundary. The Rust rebuild must preserve
the Codex CLI workflow without asking for provider credentials, running a
shell, or allowing concurrent writers to alter the same checkout.

## Decision

Native `claude-flow-codex dual run` accepts explicit `codex:<role>:<prompt>`
worker specifications only when `[swarm.automation] enabled = true` is present
in the project automation configuration. The scheduler creates one Git
worktree per worker and retains a record for explicit integration or cleanup.
It runs the local Codex executable as a direct argument vector, caps requested
limits to configuration, writes shared task context through the native SQLite
compatibility store, and removes secret-like variables from the worker
environment.

`claude` worker specifications are rejected with a stable explicit message.
They are not emulated and they do not fall back to a Node process.

## Consequences

- Codex CLI authentication stays local to Codex; the Ruflo binary has no API-key
  configuration or dependency.
- Each writer gets an isolated checkout and recoverable worktree registry
  record, preventing concurrent checkout collisions.
- The scheduler has executable proof for opt-in enforcement, worktree creation,
  direct `codex exec` argument construction, and environment redaction.
- Full mixed Claude/Codex orchestration remains a later contract wave rather
  than a misleading compatibility claim.

## Supersession note (2026-08-08)

ADR-0008 implements native Rust swarm execution that spawns both `claude` and
`codex` worker subprocesses directly, with no Node runtime. The Codex-only
restriction, the opt-in gate, and the worktree-confinement requirement of this
ADR are superseded. `swarm start --objective <X> --workers N` now forks N real
agent processes (claude default, `--agent codex` for codex), coordinates them
in parallel, and collects results — all native Rust.
