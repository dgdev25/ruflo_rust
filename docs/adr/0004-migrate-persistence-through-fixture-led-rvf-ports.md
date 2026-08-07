# 0004 — Migrate persistence through fixture-led RVF ports

## Status

Accepted (2026-08-07)

## Context

Existing projects have persisted state contracts while rUv provides RVF adapters for AgentDB and Agentic Flow. Replacing storage formats without fixtures risks silent incompatibility or data loss.

## Decision

Preserve legacy persistence through a port with fixture-led migration to adapter-backed RVF interchange. Every migration creates a backup, takes a lock, records a reversible marker, validates the result, and supports rollback.

## Consequences

- Existing user data is protected by a reversible path.
- RVF becomes an interoperability boundary only where consumer evidence proves it.
- Migration logic and fixtures add implementation work before default enablement.

## Alternatives

- Make RVF the universal store immediately — rejected because legacy contracts may differ.
- Retain only legacy storage indefinitely — rejected because it blocks native rUv interoperability.
