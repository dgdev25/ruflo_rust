# Native Ruflo Rebuild — Design Specification

**Status:** Approved  
**Date:** 2026-08-07  
**Requirements:** REQ-001 through REQ-013 in [PRD](../../PRD.md)

## 1. Decision summary

Build a clean-room, pure-Rust Ruflo in this repository. It preserves observable rUvNet consumer contracts in compatibility waves while composing existing native rUv components rather than recreating them. Wave 1 is a local native CLI plus stdio MCP server. Wave 2 adds a stateless HTTP MCP transport behind the same dispatcher. The runtime requires no Node.js.

## 2. Scope and non-goals

The initial product covers native binaries, CLI aliases, stdio MCP, compatibility fixtures, configuration, capability reporting, persistence compatibility, and the core agent/task/swarm/memory contract. It targets Linux x86_64/aarch64, macOS x86_64/aarch64, and Windows x86_64.

This product does not recreate RuVector, RVF, AgentDB, Agentic Flow, or any other existing rUv Rust application. It does not execute arbitrary JavaScript plugins, claim untested behavioural parity, require Node.js, or require paid hosted services. These constraints satisfy REQ-001, REQ-006, REQ-007, REQ-010, and REQ-013.

## 3. Architecture

The workspace is split by stable responsibility:

```text
Consumer contracts
  CLI aliases and configuration ─┐
  MCP stdio / HTTP transports ───┼──> compatibility facade
                                 │          │
                                 │          ▼
                                 │      runtime domain
                                 │   agents/tasks/workflows
                                 │          │
                                 │          ├── persistence ports
                                 │          │     ├── legacy state compatibility
                                 │          │     └── migration/backup/rollback
                                 │          │
                                 └──────────┴──> rUv RVF/RuVector adapters
                                                   AgentDB / Agentic Flow state
```

### Compatibility facade

The facade owns executable aliases, command parsing, help/version/error behaviour, configuration and environment precedence, MCP schemas, capability manifest, and logging boundaries. It receives no direct access to storage implementation details. It satisfies REQ-001 through REQ-004, REQ-008, and REQ-012.

### Runtime domain

The runtime owns explicit IDs and handles for agents, tasks, swarms, workflows, cancellations, and timeouts. It never relies on hidden MCP transport sessions. It returns typed domain errors that the facade maps to stable CLI and MCP responses. This satisfies REQ-004 and REQ-008.

### Persistence ports

Persistence ports isolate legacy project-state compatibility from new RVF interchange. Existing persisted-data semantics are discovered and proven with fixtures before migration is enabled. Each migration creates a backup, uses a lock, records a reversible migration marker, and validates the postcondition before committing. This satisfies REQ-005.

### rUv adapters

The runtime composes version-pinned RVF/RuVector crates and existing AgentDB, Agentic Flow, and Claude Flow RVF adapters. These dependencies own vector storage, indexing, quantisation, cryptographic witnesses, and interchange layouts. Ruflo only translates its domain objects to their supported interfaces. This satisfies REQ-006.

### MCP transports

The initial stdio adapter is the compatibility transport. It emits newline-delimited JSON-RPC only on stdout and diagnostics only on stderr. The Wave 2 stateless HTTP adapter calls the same dispatcher. It carries protocol/client context per request, uses explicit state handles, and is disabled by default. It must not cause a second tool implementation or a second schema set. This satisfies REQ-003, REQ-004, and REQ-011.

## 4. Compatibility contract and rollout

Wave 0 builds the test harness before feature implementation. It inventories real rUvNet consumers and freezes CLI output, exit code, stdout/stderr, MCP request/response shape, tool discovery, config precedence, on-disk layout, locking, migrations, cancellation, and timeout behaviour.

Wave 1 ships the native `ruflo` and `claude-flow` aliases, stdio MCP, core memory/task/agent/swarm functions, legacy persistence compatibility, capability reporting, and supported-platform binaries. Every advertised Wave 1 capability needs a differential fixture and a consumer-driven test.

Wave 2 ships workflow execution, hooks, declarative plugin-manifest support, and stateless HTTP MCP. A Wave 2 feature remains disabled or explicitly unsupported until its fixtures pass. HTTP requests use the current stateless MCP model; stdio remains available for local legacy consumers.

Wave 3 contains federation, advanced learning and security functions, appliance work, and remaining plugin migrations. It is intentionally unbounded only by evidence: no area becomes compatible merely because it has an implementation.

Every release writes a machine-readable capability manifest. Deferred calls return a stable unsupported error containing the capability name, release wave, and migration path. This satisfies REQ-002, REQ-003, REQ-008, REQ-010, and REQ-012.

## 5. Plugin, hook, and security policy

Plugin manifests and declarative assets are validated against a versioned native contract. Existing executable JavaScript plugin code is neither embedded nor silently interpreted. A consumer receives one of: native support, a documented migration contract, or a deterministic unsupported response.

Tool policy is checked before discovery and invocation. The runtime supports allowlists, denylists, and curated profiles; a denial wins. HTTP is disabled by default; when enabled, it requires authenticated caller identity and server-side per-tool authorization that validates audience, issuer, expiry, and capability context. This satisfies REQ-009, REQ-010, and REQ-014.

Hooks and plugins execute only explicit, allowlisted native actions. Arguments are structured and validated, the working directory and environment are bounded, and untrusted text is never interpolated into a shell command. Per-tool request-body, concurrency, execution-time, and rate limits apply before work begins. Persisted state, migration backups, and audit records use owner-only filesystem permissions, integrity validation, and configurable encryption-key handling; diagnostics never expose secrets. This satisfies REQ-015, REQ-016, and REQ-017.

## 6. Technology and dependency policy

The MSRV is Rust 1.87 because the chosen native RVF workspace requires it. The expected core uses `rmcp` 3.1.1 for MCP (stdio first), `clap` 4.6.6 for CLI compatibility, `figment` 0.10.19 for layered typed configuration, and `tokio` 1.53.1 for asynchronous runtime. `petgraph` 0.8.3 is adopted only if the Wave 0 workflow contract requires graph analysis rather than a smaller domain representation.

The existing `rvf-runtime` 0.3.2 and the AgentDB/Agentic Flow/Claude Flow RVF adapters are upstream dependencies, pinned to a verified revision or release. Contract fixtures use `assert_cmd` 2.2.2 and `insta` 1.48.0. The named registry versions were checked against OSV with no version-specific known advisories. All direct dependencies are permissively licensed; the committed lockfile will be checked by advisory, licence, and source policy tools in CI.

The stateless HTTP implementation remains feature-gated until the selected Rust MCP implementation is production-ready for the required specification revision. No managed cloud, paid API, or external database is a design dependency. This satisfies REQ-007, REQ-009, and REQ-011.

## 7. Verification strategy

The compatibility suite contains golden CLI fixtures, MCP schemas and round trips, curated persistence fixtures, and real rUvNet consumer tests. Paired MCP operations are tested end-to-end so a write through one tool is discoverable through its paired reader.

The release matrix runs on each target architecture/operating system with native runners. Required gates are formatting, linting, unit and integration tests, differential fixtures, RVF/RVFA interoperability tests, lockfile audit, licence/source policy checks, SBOM generation, reproducible-artifact checks, and platform smoke tests for paths, locks, process signals, hooks, stdout discipline, authorization denial, resource limits, and secret-safe diagnostics. This satisfies REQ-002, REQ-003, REQ-005, REQ-007, REQ-009, and REQ-014 through REQ-017.

## 8. Error handling and observability

The domain defines stable categories: invalid input, unauthenticated, unauthorized, unknown capability, unsupported wave, rate limited, timeout/cancelled, lock conflict, migration failure, and upstream-adapter failure. The facade maps them consistently to CLI exit codes and MCP error objects. It attaches a correlation ID to every request and writes audit events without leaking secret values. A failed migration leaves its pre-migration backup and an actionable recovery instruction. This satisfies REQ-005, REQ-008, REQ-009, and REQ-014 through REQ-017.

## 9. Reusable code and prior art

| Candidate | Licence | Decision |
|---|---|---|
| RVF runtime and native adapters | MIT OR Apache-2.0 | Adopt for RVF storage/interchange, AgentDB memory, Agentic Flow coordination state, and Claude Flow storage support. |
| RuVector native crates | MIT | Adopt selectively through RVF boundaries; do not recreate vector/index/quantisation algorithms. |
| `rmcp` | Apache-2.0 | Adopt for MCP server implementation; use stdio first and gate stateless HTTP support. |
| `clap`, `figment`, `tokio`, `petgraph` | MIT or MIT OR Apache-2.0 | Adopt as plumbing only, with locked versions and contract-first boundaries. |
| `assert_cmd`, `insta` | MIT OR Apache-2.0 / Apache-2.0 | Adopt for black-box CLI and snapshot compatibility fixtures. |

## 10. Coordination

Reuse research and design critique ran through a Ruflo hierarchical swarm. The Ruflo worker registry initialized correctly, but direct model execution was unavailable because no provider key was configured; three native read-only research passes supplied the fallback evidence. This affects research orchestration only, not the product design.

## 11. Decisions to record as ADRs after review

- Native compositional architecture over reimplementation of rUv Rust components.
- Contract-first compatibility waves and capability manifest.
- One dispatcher with stdio MCP in Wave 1 and stateless HTTP MCP in Wave 2.
- RVF adapter integration with fixture-led legacy persistence migration.
- Native plugin migration boundary with no embedded JavaScript execution.
- Remote MCP identity and server-side capability authorization.
- Native hook/plugin execution and resource-governance boundary.
