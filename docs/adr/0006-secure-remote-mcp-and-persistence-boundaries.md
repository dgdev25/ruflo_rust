# 0006 — Secure remote MCP and persistence boundaries

## Status

Accepted (2026-08-07)

## Context

Stateless HTTP MCP introduces a remotely reachable surface for powerful Ruflo tools. Capability policy alone is insufficient without caller identity, server-side authorisation, resource governance, and protected persistence.

## Decision

Disable remote MCP by default. When enabled, require authenticated caller identity and validate audience, issuer, expiry, and capability context server-side per tool. Enforce request-body, concurrency, execution-time, and rate limits; use owner-only permissions, integrity validation, configurable encryption-key handling, and secret-safe audit diagnostics for persisted state.

## Consequences

- Prevents unauthenticated remote tool use and limits denial-of-service exposure.
- Adds authentication, policy, audit, and key-handling implementation work.
- Keeps local stdio use independent from remote deployment configuration.

## Alternatives

- Expose HTTP MCP without authentication — rejected because privileged tools would be reachable remotely.
- Apply policy only in the client — rejected because clients cannot enforce server-side authority.
