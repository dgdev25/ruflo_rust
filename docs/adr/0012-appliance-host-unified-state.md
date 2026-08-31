# 0012 — Appliance host uses one SQLite store and resident job slots

## Status

Accepted (2026-08-14)

## Context

The native CLI stored agents, daemon intent, queues, and spend in many JSON
files. An always-on appliance needs one durable authority, a supervisor that
stays up, and a fail-closed spend gate on every AI spawn. REQ-013 requires
consumer fixtures before claiming Wave 3 appliance support.

## Decision

- Project operational state (agents, jobs, daemon kv) lives in `.swarm/memory.db`
  beside memory entries.
- Global spend lives in `$RUFLO_AI_BUDGET_DIR/ai-budget.db`.
- The supervisor registers resident-idle slots and claims jobs from that store.
  LLM work stays one-shot (ADR-0008).
- Swarm, hive/headless, and daemon budget pause share `SpendLedger`.
- The container image packs the native binary, `config/appliance/cloud.yaml`,
  and a checksummed RVFA host record. Capability `appliance-supervisor-host`
  stays unproven until the consumer fixture in this change passes in CI.

## Consequences

- JSON agent files are a migration input, not the source of truth.
- Completing a tagged five-target image is still a release-evidence task.
