# Native command parity ledger

Status: in progress. This ledger is the release gate for the pure-Rust Ruflo
CLI. A command is **not** complete because it appears in `--help`: it must
have a native implementation, source-oracle fixture coverage, and an
end-to-end behavior test where it has side effects.

## Authority and scope

The source of truth is the original V3 command registry:
`/mnt/datadisk/dev/ruflo/v3/@claude-flow/cli/src/commands/index.ts`.
It currently registers the top-level commands below, including lazy-loaded
commands. The native wrapper is also required to preserve the `ruflo` and
`claude-flow` executable aliases and their observable command behavior.

Current native baseline: only `--help`, `--version`, and `mcp start` execute.
All other names printed by the former native help output are unimplemented and
must remain marked incomplete here until the required evidence exists.

## Completion rule

For every row:

1. Capture the source command's help, exit, stdout/stderr, JSON, filesystem,
   cancellation, and error contracts.
2. Implement the Rust command without a Node runtime or API-key requirement
   for local Codex execution.
3. Add differential and focused end-to-end tests.
4. Mark the row complete only after the native and source fixtures agree.

## Command inventory

| Wave | Original top-level command | Status |
| --- | --- | --- |
| Core lifecycle | `init`, `start`, `status` | `init` and `status` have an initial native lifecycle implementation; source parity fixtures and `start` remain pending |
| Core agents | `agent`, `swarm`, `task`, `session` | Pending |
| Core state/transport | `memory`, `mcp`, `config`, `migrate`, `hooks`, `workflow` | Pending |
| Runtime operations | `hive-mind`, `process`, `daemon`, `version`, `doctor`, `completions` | Pending |
| Safety and intelligence | `neural`, `security`, `performance`, `policy`, `embeddings`, `guidance`, `route`, `analyze`, `progress`, `verify` | Pending |
| rUv integration | `ruvector`, `transport`, `claims`, `issues`, `providers`, `plugins` | Pending |
| Release and lifecycle | `deployment`, `update`, `appliance`, `appliance-advanced`, `transfer-store`, `cleanup`, `autopilot`, `benchmark`, `gaia-bench`, `metaharness`, `eject` | Pending |
| Product integrations | `funnel`, `settings`, `auth`, `proxy`, `advisor`, `spinner`, `announcements` | Pending |

## First delivery slice

The first executable parity slice is deliberately the deployment-critical
surface: `init`, `status`, `agent`, `swarm`, `task`, `session`, `memory`, and
`mcp`. It must support a real local Codex swarm: create durable project state,
spawn/track workers, assign/cancel tasks, resume a session, store/retrieve
shared memory, and run the standard stdio MCP endpoint.

Later waves may use existing native rUv components at their stable boundaries;
they must not reimplement RuVector, RVF, AgentDB, or Agentic Flow.

## Captured core subcommand contracts

The following source-oracle help output was captured from `ruflo@3.34.0` on
2026-08-07. Each entry remains pending until implemented and differentially
tested in Rust.

| Family | Original subcommands |
| --- | --- |
| `agent` | `spawn`, `list` (`ls`), `status`, `stop` (`kill`), `metrics`, `pool`, `health`, `logs`, `wasm-status`, `wasm-create`, `wasm-prompt`, `wasm-gallery`, `publish` |
| `swarm` | `init`, `start`, `status`, `stop`, `scale`, `coordinate`, `compress-message`, `pheromone`, `join` |
| `task` | `create` (`new`, `add`), `list` (`ls`), `status` (`info`, `get`), `cancel` (`abort`, `stop`), `assign`, `retry` (`rerun`) |
| `session` | `list` (`ls`), `save` (`create`, `checkpoint`), `restore` (`load`), `delete` (`rm`, `remove`), `export`, `import`, `current` |
