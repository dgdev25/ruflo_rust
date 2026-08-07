# Native Ruflo rebuild backlog

This is the execution view of the approved rebuild plan. The TypeScript Ruflo
implementation remains the behavioral oracle; no item is complete without its
named compatibility evidence and focused verification.

## Current state

| Task | Status | Exit evidence |
| --- | --- | --- |
| 1. Workspace and shared contract types | Complete | `86b422f`; workspace contract tests pass. |
| 2. Differential fixture harness | Complete | `efb56e5`; fixture schema, approved pure-Rust capture, provenance, and redaction checks pass. |
| 3. Consumer inventory and P0 contract matrix | Complete | `50935ee`; nine P0 rows are source-grounded and matrix validation passes. |
| 4. Native CLI façade and aliases | Complete | `03fc0d9`; both native aliases pass version/help and MCP placeholder contract tests. |
| 5. Capability/configuration/policy layer | Complete | `6ed4b55`; precedence, manifest, denial, and resource-limit tests pass. |
| 6. Shared stdio MCP dispatcher | Complete | `1eb3994`; JSON-RPC protocol, dispatcher, errors, and denial tests pass. |
| 7. Runtime lifecycle handles | Complete | `2963195`; task cancellation and lifecycle invariants pass across the workspace suite. |
| 8. Persistence ports and migration safety | Complete | `9ab4c98`; rollback, locking, root validation, and backup permission tests pass. |
| 9. RVF adapter facade | Complete | `8d54f45`; public immutable RuVector pin and AgentDB/Agentic Flow interop tests pass; unavailable typed unknown-segment round trip is explicitly tracked with upstream evidence. |

## Remaining execution order

### 2. Differential compatibility fixture harness — complete

- Add fixtures for `--version`, `--help`, and MCP `tools/list`.
- Implement typed fixture parsing with argv/stdin/exit/stdout/stderr/environment/platform fields.
- Make capture explicit, redacted, non-overwriting by default, and reject home paths and secrets.
- Verify: `cargo test --test differential_cli` and `bash scripts/verify-fixtures.sh`.

### 3. Consumer inventory and P0 contract matrix — complete

- Inventory every rUvNet CLI, MCP, plugin, persistence, RVF/RVFA, and platform-hook consumer.
- Record consumer, invocation, fixture, owner, compatibility wave, and blocker/status.
- P0 includes aliases, version/help, MCP discovery/calls, memory round trips, migrations, policy denials, and hooks.
- Verify: matrix completeness test and `scripts/inventory-consumers.sh --check`.

### 4. Native CLI façade and aliases — complete

- Create shared parser plus thin `ruflo` and `claude-flow` Rust binaries.
- Match oracle fixtures for version/help/MCP start; initialize no models or adapters on version fast path.
- Send errors to stderr with stable nonzero exits.
- Verify both aliases against differential CLI fixtures.

### 5. Capability/configuration/policy layer — complete

- Implement precedence: CLI > environment > project config > defaults.
- Generate a supported/migrating/unsupported capability manifest.
- Enforce allow/deny, request size, execution duration, and concurrency before dispatch.
- Verify a deny rule removes a tool from both discovery and invocation.

### 6. Shared stdio MCP dispatcher — complete

- Implement `tools/list` and `tools/call` on one dispatcher with stable error mapping and correlation IDs.
- Use a Rust MCP library only where its stdio behavior satisfies fixtures.
- Guarantee JSON-RPC-only stdout and stderr-only diagnostics.
- Verify stdio round trips, schema equivalence, and denial filtering.

### 7. Runtime lifecycle handles — complete

- Define opaque agent, task, swarm, and workflow identifiers.
- Implement explicit lifecycle transitions, deterministic cancellation, and auditable terminal handles.
- Add topology analysis only once Wave 0 fixtures demand it.
- Verify invalid transitions, duplicate cancellation, and unknown-ID errors.

### 8. Persistence ports and safe legacy migration — complete

- Provide open/migrate/backup/commit/rollback port semantics.
- Use project-scoped locks, same-filesystem owner-only backups, markers, validation, atomic commit, and rollback metadata.
- Never expose database values, keys, or paths outside project root.
- Verify success, lock conflict, rollback, and permissions with legacy fixtures.

### 9. RVF adapter facade — complete

- Pin and verify existing `rvf-runtime`, AgentDB, and Agentic Flow adapter revisions.
- Translate objects only; never hand-encode RVF, indexes, vectors, quantization, or witnesses.
- Verify AgentDB/Agentic Flow fixture interop and stable search order; record any typed-API gap in the fixture evidence rather than hand-encoding RVF.

### 10. Native hook and plugin action boundary

- Accept only versioned declarative manifests and enum-based native actions.
- Canonicalize project-relative working directories and invoke only allowlisted binaries with structured arguments.
- Reject JavaScript executable plugins with `UnsupportedInWave`.
- Verify injection, path escape, timeout, concurrency, and allowlist behavior.

### 11. Stateless HTTP MCP — Wave 2, feature-gated

- Reuse the stdio dispatcher and exact tool schemas; do not create a second MCP implementation.
- Implement only after confirming the chosen Rust MCP SDK supports the required stateless API.
- Require authenticated identity with issuer/audience/expiry/per-tool authorization; apply size/rate/timeout/concurrency limits.
- Verify no server session ID, explicit handles, identical policy denials, and `401` without identity.

### 12. Platform hooks and release matrix

- Test Linux x86_64/aarch64, macOS x86_64/aarch64, and Windows x86_64 on native runners.
- Verify aliases, JSON-RPC cleanliness, paths, locks, cancellation, hook rendering, signatures, and SBOM presence.
- Add local POSIX and PowerShell release smoke scripts.

### 13. Supply-chain, SBOM, reproducibility gates

- Configure Cargo-deny license/source/advisory policy and `cargo audit`.
- Generate a locked CycloneDX or SPDX SBOM with digest.
- Fail on non-approved licenses, unapproved registries, or undocumented advisory exceptions.

### 14. Wave-promotion evidence gates

- Block Wave 2/3 promotion unless named consumer fixtures, security tests, native platform results, migration tests, and dependency review all pass.
- Require an ADR before selecting a new long-lived integration.
- Verify incomplete evidence keeps the capability manifest at `unsupported`.

## Operating rules

- Every task starts from a failing focused test, ends with a focused test run, a source-only commit, and a push to `origin/main`.
- Never add `.claude-flow/`, `.codex/`, `.ruvnet-brain/`, `.swarm/`, AgentDB/RVF files, or `target/` to Git.
- Store verified behavioral discoveries in AgentDB under the `ruflo-rust-rebuild` namespace/tier; do not record claims as facts without source or runtime evidence.
- Reuse existing native rUv components at their stable boundaries; do not rebuild them.
