# 0003 — Use one dispatcher for stdio and stateless MCP

## Status

Accepted (2026-08-07)

## Context

Existing rUvNet integrations invoke Ruflo as a stdio MCP child process. New MCP infrastructure benefits from a stateless HTTP transport, but a transport-only migration would break current consumers.

## Decision

Use one MCP compatibility dispatcher with a stdio adapter in Wave 1 and a disabled-by-default stateless HTTP adapter in Wave 2. Both adapters share tool schemas, policy, and execution semantics; durable state uses explicit handles rather than hidden transport sessions.

## Consequences

- Preserves existing local MCP compatibility while enabling scalable remote deployment later.
- Adds a transport test matrix but avoids duplicated tool implementations.
- HTTP needs independent authentication and authorisation controls.

## Alternatives

- Stdio only — rejected because it prevents the planned scalable remote surface.
- HTTP only — rejected because it breaks accepted rUvNet stdio integration.
- Separate MCP servers — rejected because schemas and behaviour would drift.
