# Consumer Inventory

Date: 2026-08-07

This inventory is grounded in the live source trees scanned for Task 3:

- `/mnt/datadisk/dev/ruflo`
- `/mnt/datadisk/repos/rUvnet`

The goal here is not to claim parity. It is to identify the real consumer-facing contracts that a native Ruflo rebuild must either reproduce with fixtures or leave explicitly unsupported.

## P0 Surfaces

| Consumer | Invocation Type | Contract Surface | Grounding Paths | Notes |
| --- | --- | --- | --- | --- |
| `ruflo` npm wrapper | CLI alias | `ruflo` binary registration plus fast `--version` wrapper path | `/mnt/datadisk/dev/ruflo/ruflo/package.json`; `/mnt/datadisk/dev/ruflo/ruflo/bin/ruflo.js` | The wrapper emits `ruflo v<version>` before heavy imports. |
| `claude-flow` root package | CLI alias | `claude-flow` root binary registration | `/mnt/datadisk/dev/ruflo/package.json` | The root package still publishes `claude-flow` as the primary bin. |
| `@claude-flow/cli` stdio entrypoint | CLI and stdio MCP | auto-detected stdio mode, `tools/list`, `tools/call`, and JSON-RPC newline framing | `/mnt/datadisk/dev/ruflo/v3/@claude-flow/cli/bin/cli.js`; `/mnt/datadisk/dev/ruflo/v3/@claude-flow/cli/src/mcp-server.ts` | This is the highest-signal consumer surface for Wave 1 compatibility. |
| `start-mcp` launchers | CLI wrapper scripts | explicit MCP startup on POSIX and Windows | `/mnt/datadisk/dev/ruflo/v3/scripts/start-mcp.sh`; `/mnt/datadisk/dev/ruflo/v3/scripts/start-mcp.cmd` | The scripts expose transport, host, port, and log-level contracts. |
| Memory tools and hooks learning | MCP tools plus persisted state | `memory/store`, `memory/search`, `memory/list`, and `.swarm/memory.db` persistence | `/mnt/datadisk/dev/ruflo/v3/mcp/tools/memory-tools.ts`; `/mnt/datadisk/dev/ruflo/v3/mcp/tools/hooks-tools.ts`; `/mnt/datadisk/dev/ruflo/.github/workflows/v3-ci.yml` | Hooks persist ReasoningBank state into `.swarm/memory.db`; the workflow smoke guards legacy DB behavior. |
| Security deny flows | CLI hook commands | dangerous-command denial and sensitive-file denial | `/mnt/datadisk/repos/rUvnet/claude-flow/tests/docker-regression/scripts/test-security.sh` | This is the clearest concrete consumer for policy-denial behavior in the upstream tree. |
| Plugin hook manifest | Platform hook manifest | Claude Code hook matchers and shell command shape | `/mnt/datadisk/dev/ruflo/plugin/hooks/hooks.json`; `tests/fixtures/consumers/platform-hooks/posix.json` | The native rebuild freezes the POSIX hook contract as a tokenized `ruflo mcp start` fixture instead of a shell pipeline. |
| Windows hook parity smokes | Cross-platform hook behavior | generated hook commands and runtime execution on Windows, macOS, Linux | `/mnt/datadisk/dev/ruflo/.github/workflows/v3-ci.yml`; `tests/fixtures/consumers/platform-hooks/windows.json`; `.github/workflows/compatibility.yml` | CI is the evidence route for native runner parity across the five supported targets. |
| RVFA appliance tests | Migration and appliance format | RVFA magic/version/header validation | `/mnt/datadisk/repos/rUvnet/claude-flow/v3/__tests__/appliance/rvfa-format.test.ts` | This is RVFA-specific, not a blanket proof for all migration paths. |
| RuVector RVF pipeline | Native RVF composition | RVF container construction and segment ordering | `/mnt/datadisk/repos/rUvnet/RuVector/crates/mcp-brain-server/src/pipeline.rs` | This is the upstream native RVF boundary the rebuild should compose with rather than reimplement. |

## Inventory Notes

- Alias and help contracts are split deliberately. The checked-in fixtures currently prove the `ruflo` wrapper, not the `claude-flow` alias.
- MCP discovery has a checked-in fixture already. MCP execution (`tools/call`) is visibly implemented upstream but still lacks a checked-in native rebuild fixture.
- Memory, migration, RVF, policy denial, and platform hook rows are present in the matrix even when they are blocked. Unknown or unproven surfaces are not marked supported.
- The POSIX hook manifest in `plugin/hooks/hooks.json` explicitly documents that it remains Windows-incompatible, so the native rebuild carries separate checked-in POSIX and Windows hook fixtures and proves them through the compatibility workflow.
