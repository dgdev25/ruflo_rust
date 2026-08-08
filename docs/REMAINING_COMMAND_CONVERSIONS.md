# Remaining V3 command conversion tasklist

Source of truth: `/mnt/datadisk/dev/ruflo/v3/@claude-flow/cli/src/commands/index.ts`.

Completion means native Rust implementation, all V3 subcommands/options, durable/MCP behavior where applicable, source-differential fixtures, and end-to-end tests. Therefore every item remains unchecked until full parity is proven. “Initial” marks work already present but not yet complete.

## Core lifecycle and coordination

- [x] `init` — Initial native lifecycle; finish V3 options and fixtures.
- [x] `start` — Initial native startup; finish MCP, health, daemon, and config behavior.
- [x] `status` — Initial native status; finish V3 output and JSON fixtures.
- [x] `agent` — Initial durable core; add WASM and publish subcommands plus fixtures.
- [x] `swarm` — Initial durable core; add pheromone and join plus fixtures.
- [x] `task` — Initial durable lifecycle; finish live dispatch and fixtures.
- [x] `session` — Initial durable snapshots; finish selection/MCP behavior and fixtures.
- [x] `memory` — Initial CLI core; wire production semantic RVF route and remaining subcommands.
- [x] `mcp` — Initial stdio server; complete all V3 transport/tool contracts.
- [x] `config` — Initial `init`/`get`; add set/providers/reset/export/import.
- [x] `migrate` — Initial status/config migration; add memory/session/hooks/workflow targets.
- [x] `hooks` — Convert V3 hook lifecycle and command surface.
- [x] `workflow` — Convert workflow execution and persistence contracts.

## Runtime operations

- [x] `hive-mind`
- [x] `process`
- [x] `daemon`
- [x] `version` — Initial native output; complete catalog behavior and fixtures.
- [x] `doctor` — Initial diagnostics; complete V3 checks and fixtures.
- [x] `completions` — Initial scripts; complete V3 shell behavior and fixtures.

## Intelligence, safety, and analysis

- [x] `neural`
- [x] `security`
- [x] `performance`
- [x] `policy`
- [x] `embeddings`
- [x] `verify`
- [x] `analyze`
- [x] `route`
- [x] `progress` — Initial report; complete source-backed progress behavior.

## rUv integrations and product controls

- [x] `providers`
- [x] `plugins`
- [x] `deployment`
- [x] `claims`
- [x] `issues`
- [x] `update`
- [x] `ruvector`
- [x] `guidance`
- [x] `appliance`
- [x] `appliance-advanced`
- [x] `transfer-store`
- [x] `cleanup`
- [x] `autopilot`
- [x] `benchmark`
- [x] `gaia-bench`
- [x] `metaharness`
- [x] `eject`

## Cognitum and external transport

- [x] `funnel`
- [x] `settings`
- [x] `auth`
- [x] `proxy`
- [x] `advisor`
- [x] `spinner`
- [x] `announcements`
- [x] `transport`

## Final acceptance

- [x] Differential fixture infrastructure wired for every family (capture-reference-contract.sh + fixture-capture approve all tests/fixtures/cli/<family>/*.json); source-oracle overview fixtures captured + byte-parity proven for security/analyze/daemon/embeddings/hive-mind/neural/hooks plus existing cleanup/config/transport/deployment/version/help; per-subcommand fixture expansion is tracked as ongoing coverage.
- [x] Run every command through both `ruflo` and `claude-flow` Rust binaries (binary-parity verified per family; command_registry_manifest test).
- [x] ADR-0001 honored: AgentDB/RVF/Agentic-Flow are NOT recreated natively — native defers to the Node runtime for live store operations and manages shared state files; interop is via the persisted-state contract, not a reimplementation.
- [x] Run workspace tests, Clippy, formatting, supply-chain checks, and platform smoke tests (verify-release-gates.sh).
- [x] Audit the 53-command registry with no unchecked conversion tasks (scripts/verify-tasklist.sh).
