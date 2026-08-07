# 0005 — Enforce native-only plugin and hook execution

## Status

Accepted (2026-08-07)

## Context

The native binary cannot safely or faithfully execute arbitrary legacy JavaScript plugin code. Plugin and hook compatibility is a high-privilege execution boundary.

## Decision

Support declarative plugin manifests through a versioned native contract. Execute only allowlisted native hook/plugin actions with structured validated arguments, bounded environment and working-directory access, and no shell interpolation. Legacy executable plugins receive a migration path or deterministic unsupported response.

## Consequences

- Preserves safety and the pure-Rust runtime constraint.
- Requires ecosystem plugin migration and explicit capability reporting.
- Avoids pretending that manifest compatibility implies executable parity.

## Alternatives

- Embed a JavaScript runtime — rejected because it violates the native runtime constraint.
- Execute arbitrary commands from manifests — rejected because it enables command injection and privilege escalation.
