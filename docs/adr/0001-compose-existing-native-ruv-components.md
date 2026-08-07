# 0001 — Compose existing native rUv components

## Status

Accepted (2026-08-07)

## Context

Ruflo must become a pure-Rust native application while remaining compatible with rUvNet. rUvNet already contains maintained native implementations for RVF, vector storage, indexing, quantisation, witnesses, and AgentDB/Agentic Flow adapters.

## Decision

Build Ruflo as a thin native composition and compatibility layer over version-pinned rUv Rust crates. Ruflo owns its CLI, MCP, orchestration, compatibility, and policy boundaries; it does not copy, fork, or reimplement rUv vector/RVF applications.

## Consequences

- Preserves a single source of truth for RVF and vector semantics.
- Requires adapter boundaries and cross-version interoperability fixtures.
- Limits Ruflo to contracts it owns.

## Alternatives

- Vendor or fork upstream crates — rejected because it duplicates maintained rUv work and creates drift.
- Reimplement RVF/RuVector in Ruflo — rejected because it violates the project constraint and risks format divergence.
