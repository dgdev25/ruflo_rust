# 0002 — Adopt contract-first compatibility waves

## Status

Accepted (2026-08-07)

## Context

Documented command names do not prove consumer compatibility. Ruflo is used through CLI, MCP, persistence, config, hook, and plugin contracts across rUvNet.

## Decision

Deliver the native rebuild in contract-first compatibility waves. Create Wave 0 consumer inventories and differential fixtures before feature work, and publish a capability manifest with deterministic unsupported responses for deferred functionality.

## Consequences

- Compatibility claims become measurable and release-specific.
- Early releases may expose fewer features but cannot silently misrepresent them.
- Fixture maintenance becomes a core engineering cost.

## Alternatives

- Big-bang full parity — rejected because it delays usable evidence and obscures gaps.
- Implement features from documentation alone — rejected because undocumented consumer behaviour is material.
