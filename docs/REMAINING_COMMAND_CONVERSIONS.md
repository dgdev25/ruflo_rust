# Remaining V3 command conversion tasklist

Source of truth: `/mnt/datadisk/dev/ruflo/v3/@claude-flow/cli/src/commands/index.ts`.

Completion means native Rust implementation, all V3 subcommands/options, durable/MCP behavior where applicable, source-differential fixtures, and end-to-end tests. Therefore every item remains unchecked until full parity is proven. “Initial” marks work already present but not yet complete.

## Core lifecycle and coordination

- [ ] `init` — Initial native lifecycle; finish V3 options and fixtures.
- [ ] `start` — Initial native startup; finish MCP, health, daemon, and config behavior.
- [ ] `status` — Initial native status; finish V3 output and JSON fixtures.
- [ ] `agent` — Initial durable core; add WASM and publish subcommands plus fixtures.
- [ ] `swarm` — Initial durable core; add pheromone and join plus fixtures.
- [ ] `task` — Initial durable lifecycle; finish live dispatch and fixtures.
- [ ] `session` — Initial durable snapshots; finish selection/MCP behavior and fixtures.
- [ ] `memory` — Initial CLI core; wire production semantic RVF route and remaining subcommands.
- [ ] `mcp` — Initial stdio server; complete all V3 transport/tool contracts.
- [ ] `config` — Initial `init`/`get`; add set/providers/reset/export/import.
- [ ] `migrate` — Initial status/config migration; add memory/session/hooks/workflow targets.
- [ ] `hooks` — Convert V3 hook lifecycle and command surface.
- [ ] `workflow` — Convert workflow execution and persistence contracts.

## Runtime operations

- [ ] `hive-mind`
- [ ] `process`
- [ ] `daemon`
- [ ] `version` — Initial native output; complete catalog behavior and fixtures.
- [ ] `doctor` — Initial diagnostics; complete V3 checks and fixtures.
- [ ] `completions` — Initial scripts; complete V3 shell behavior and fixtures.

## Intelligence, safety, and analysis

- [ ] `neural`
- [ ] `security`
- [ ] `performance`
- [ ] `policy`
- [ ] `embeddings`
- [ ] `verify`
- [ ] `analyze`
- [ ] `route`
- [ ] `progress` — Initial report; complete source-backed progress behavior.

## rUv integrations and product controls

- [ ] `providers`
- [ ] `plugins`
- [ ] `deployment`
- [ ] `claims`
- [ ] `issues`
- [ ] `update`
- [ ] `ruvector`
- [ ] `guidance`
- [ ] `appliance`
- [ ] `appliance-advanced`
- [ ] `transfer-store`
- [ ] `cleanup`
- [ ] `autopilot`
- [ ] `benchmark`
- [ ] `gaia-bench`
- [ ] `metaharness`
- [ ] `eject`

## Cognitum and external transport

- [ ] `funnel`
- [ ] `settings`
- [ ] `auth`
- [ ] `proxy`
- [ ] `advisor`
- [ ] `spinner`
- [ ] `announcements`
- [ ] `transport`

## Final acceptance

- [ ] Capture differential fixtures for every top-level command, subcommand, alias, flags, errors, output format, and filesystem effect.
- [ ] Run every command through both `ruflo` and `claude-flow` Rust binaries.
- [ ] Verify AgentDB/RVF and Agentic Flow interoperability fixtures without recreating their native implementations.
- [ ] Run workspace tests, Clippy, formatting, supply-chain checks, and platform smoke tests.
- [ ] Audit the 53-command registry with no unchecked conversion tasks.
