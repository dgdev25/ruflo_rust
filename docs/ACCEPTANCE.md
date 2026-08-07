# Native Codex acceptance and rollout

The TypeScript `claude-flow-codex` executable is the behavioral oracle for
this phase. Fixture capture is limited to deterministic commands that neither
create project files nor launch a Codex or Claude worker.

## Acceptance ledger

- [x] Inventory the live Codex integration and identify its native boundaries.
- [x] Verify that the local oracle executable is available and capture replay
  fixtures for `--version`, `dual templates`, `dual run` with no worker
  configuration, and `dual run --help`.
- [x] Add a native `claude-flow-codex` façade that exactly replays the safe
  fixture contract.
- [x] Capture redacted source-oracle fixtures for each replayed command and
  add differential tests for the Rust façade.
- [x] Close the P0 `tools/call` evidence gap with a replayed reduced-schema
  fixture for deterministic `memory_search` dispatch.
- [x] Close the native P0 policy-denial evidence gap with a replayed denied
  `memory_search` JSON-RPC fixture.
- [x] Bind P0 migration/RVF adapter interop to checked-in synthetic scenarios
  for AgentDB stable ordering, compaction, and Agentic Flow reopening.
- [x] Close the P0 durable-memory gap with fixture-proven SQLite-compatible
  `memory_store`/retrieve/keyword-search persistence at `.swarm/memory.db`.
- [x] Implement the native Codex-only `dual run` scheduler behind explicit
  automation policy and isolated-worktree boundaries; it launches `codex exec`
  only after a caller supplies a `codex` worker invocation.
- [x] Add reduced-schema parity fixtures for invalid worker specifications and
  provider-free loop status/stop/dry-run lifecycle without executing a model
  provider.
- [x] Capture and implement the separate `dual status` shared-memory view
  without delegating to `npx ruflo@latest`.
- [x] Stage the actual ruVocal bridge entrypoint against the opt-in native MCP
  child, including namespace discovery, a durable memory round trip, and the
  `ENABLE_RUFLO=false` rollback.
- [x] Publish the opt-in native MCP-child promotion report with checked local
  verification evidence; GitHub Actions are intentionally absent.

## Observed oracle contract

The installed executable identifies as `claude-flow-codex` version `3.0.1`.
Its dual-mode command exposes the `feature`, `security`, and `refactor`
templates. A missing worker configuration prints guidance and exits without
starting a worker. Worker-bearing invocations use a distinct process boundary:
the TypeScript orchestrator ultimately calls `codex exec --sandbox
workspace-write --skip-git-repo-check <prompt>` (or read-only for explicitly
read-only workers).

This boundary keeps the pure-Rust rebuild free of provider credentials: the
native scheduler owns validation, policy, state, and cancellation; Codex CLI
remains the explicitly invoked local worker executable.

## Native scheduler boundary

The scheduler reads `[swarm.automation]` from `.agents/config.toml` (or
`.codex/config.toml`) and is disabled unless `enabled = true`. It creates a
registered Git worktree per worker, records the shared task context in native
SQLite memory, caps requested concurrency and timeout to the project policy,
and calls the local Codex CLI through a direct process argument vector:
`codex exec --sandbox <read-only|workspace-write> --skip-git-repo-check
<prompt>`. It does not invoke a shell, forward secret-like environment
variables, or require an API key.

The current native scheduler intentionally supports only `codex` worker specs.
`claude` workers return an explicit error until their independently governed
native process boundary has compatible fixtures and policy evidence.

The installed `claude-flow-codex` 3.0.1 release observed on 2026-08-07 did not
enforce the source tree's newer automation preflight. Its one-worker workflow
was therefore captured only with a harmless fake `codex` executable that
records arguments and exits successfully; its Node daemon child was then
terminated. The checked-in reduced-schema fixture and native process test prove
the real launch vector and prompt clauses without invoking a model. The
source-defined opt-in policy remains the authoritative safety contract for the
native release wave.
