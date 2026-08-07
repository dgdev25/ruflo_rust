# Product Requirements Document

## Summary

Rebuild Ruflo as a clean-room, pure-Rust native application that preserves the observable contracts used by the rUvNet ecosystem. The rebuild replaces the Node.js runtime with native executables while composing existing rUv Rust components for RVF, RuVector, AgentDB, and Agentic Flow interoperability rather than recreating them.

## Problem

Ruflo's current implementation exposes a broad CLI, MCP, memory, orchestration, plugin, and hook surface through a Node.js runtime. rUvNet consumers depend on those observable contracts, while the ecosystem already contains native Rust implementations for key vector, RVF, and adapter functionality. The rebuild must eliminate the Node.js runtime without breaking proven consumer workflows or duplicating maintained rUv components.

## Personas

- rUvNet maintainer — upgrades Ruflo without breaking dependent repositories and runtime integrations.
- Agent-platform operator — runs local or remote MCP tooling with predictable policies, state, and diagnostics.
- Plugin and integration author — migrates a consumer or plugin to the native compatibility contract with clear capability status.
- Developer using agent tooling — installs one native binary and relies on stable CLI, hooks, memory, and MCP behaviour.

## Goals & success metrics

- Ship one native executable with no Node.js runtime dependency for 100% of Wave 1 workflows.
- Pass 100% of approved P0 consumer-driven compatibility fixtures on all five release targets: Linux x86_64/aarch64, macOS x86_64/aarch64, and Windows x86_64.
- Keep 100% of MCP stdio protocol output on stdout valid JSON-RPC; diagnostics are emitted only on stderr.
- Demonstrate successful read/write/migrate/rollback compatibility for 100% of curated legacy persistence fixtures before enabling migration by default.
- Publish a capability manifest for 100% of shipped releases and return a stable unsupported response for 100% of deferred capabilities.

## Functional requirements

- REQ-001 (P0): The product shall provide native `ruflo` and `claude-flow` executable aliases without requiring Node.js at runtime.
- REQ-002 (P0): The product shall preserve the approved CLI command, help, version, exit-code, stdout, and stderr contracts through black-box fixtures.
- REQ-003 (P0): The product shall provide a stdio MCP server preserving required JSON-RPC framing, discovery, tool naming, and request-response schemas for approved Wave 1 tool families.
- REQ-004 (P0): The product shall expose a single compatibility dispatcher shared by all MCP transports so tool schemas and execution semantics do not diverge.
- REQ-005 (P0): The product shall preserve approved legacy project-memory and task-state contracts, including safe migration, backup, rollback, locking, and conflict handling.
- REQ-006 (P0): The product shall integrate existing rUv RVF/RuVector and AgentDB/Agentic Flow adapter interfaces without reimplementing their storage, vector, indexing, quantisation, or witness mechanics.
- REQ-007 (P0): The product shall run Wave 1 compatibility fixtures on Linux x86_64/aarch64, macOS x86_64/aarch64, and Windows x86_64 release targets.
- REQ-008 (P0): The product shall return typed, stable errors for invalid input, unknown capability, unsupported wave, timeout/cancellation, locking conflict, migration failure, and upstream adapter failure.
- REQ-009 (P0): The product shall enforce configurable MCP tool capability policy, input validation, path confinement, timeouts, and secret-safe diagnostics.
- REQ-010 (P1): The product shall support workflow execution, hooks, and declarative plugin-manifest compatibility after their consumer contracts are proven.
- REQ-011 (P1): The product shall provide a stateless HTTP MCP transport, disabled by default, using the shared compatibility dispatcher and explicit request handles for durable state.
- REQ-012 (P1): The product shall publish a machine-readable capability manifest that identifies supported, migrated, and unsupported contracts per release.
- REQ-013 (P2): The product shall add federation, advanced learning/security, appliance, and remaining plugin capabilities only after consumer-driven compatibility evidence is available.
- REQ-014 (P0): When enabled, stateless HTTP MCP shall require authenticated caller identity, enforce per-tool authorization server-side, and reject requests with invalid audience, issuer, expiry, or capability context.
- REQ-015 (P0): The product shall execute only allowlisted native hook/plugin actions with validated arguments, bounded environment and working-directory access, and no shell interpolation of untrusted input.
- REQ-016 (P0): The product shall enforce configurable request-body, concurrency, execution-time, and rate limits before invoking expensive or mutating tools.
- REQ-017 (P0): The product shall protect persisted state, migration backups, and audit records with owner-only filesystem permissions, integrity validation, and configurable encryption-key handling without logging secret material.

## Non-goals / out of scope

- Reimplementing any existing rUv Rust application, crate, adapter, RVF format, vector algorithm, or RuVector feature.
- Executing arbitrary legacy JavaScript plugins within the native executable.
- Claiming behavioural compatibility for a contract without a passing consumer fixture.
- Requiring a paid third-party service or managed cloud platform for operation.
- Making stateless HTTP MCP the only transport before existing stdio consumers have a migration path.

## Constraints

- The runtime is pure Rust and must not depend on Node.js at runtime.
- Existing rUvNet Rust components are integrated as version-pinned dependencies or adapters, not copied or forked.
- The initial release targets Linux x86_64/aarch64, macOS x86_64/aarch64, and Windows x86_64.
- Rust MSRV is 1.87 or higher, matching the required native RVF workspace.
- New dependencies must use an allowed permissive licence, pass version-specific advisory checks, and remain lockfile-audited.
- Stdio MCP compatibility remains the first transport requirement; stateless HTTP MCP is a Wave 2 capability and is disabled unless configured.

## Risks & mitigations

- Hidden consumer contracts may exceed documented command names; mitigate with Wave 0 consumer inventory and differential fixtures.
- Legacy JavaScript imports or plugins cannot be transparently native; mitigate with explicit migration, capability reporting, or deterministic rejection.
- Existing persistence semantics may not match RVF assumptions; mitigate with fixture-led migration, backup, rollback, and reversible opt-in.
- Cross-platform process, locking, path, and signal differences may break hooks; mitigate with native OS test runners and platform-specific black-box smoke tests.
- Upstream crate/API drift may break interoperability; mitigate with adapter trait boundaries, version pins, lockfiles, and golden cross-language fixtures.

## Open questions

- Agent — non-blocking: choose the exact production-ready Rust implementation/version for stateless HTTP MCP when Wave 2 is planned; retain stdio-only Wave 1 if its Rust support remains beta.

## Readiness gate

- [x] Every P0 requirement is testable as written.
- [x] Success metrics have numeric targets.
- [x] Non-goals section is non-empty.
- [x] No open question is marked blocking.
- [x] User has confirmed the REQ list (anything missing? anything wrong?).

Verdict: READY
