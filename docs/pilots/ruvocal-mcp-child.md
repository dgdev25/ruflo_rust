# ruVocal opt-in native MCP-child pilot

Status: passed in an isolated temporary project.

The accepted ruVocal integration ADR specifies Ruflo as a stdio MCP child:
the bridge discovers tools with `tools/list`, prefixes them as `ruflo__*` for
the UI, strips that prefix for `tools/call`, and exchanges newline-delimited
JSON-RPC frames. The checked-out ruVocal tree contains that ADR and its shipped
stdio kernel, but not the bridge entrypoint; this is therefore a protocol pilot,
not a deployment claim.

`tests/mcp_stdio.rs::ruvocal_opt_in_mcp_child_pilot_routes_namespaced_memory_tools`
starts the native `ruflo mcp start` child, discovers `memory_store`, routes
`ruflo__memory_store` after prefix stripping, and searches the persisted result.
The scenario is isolated from user data and requires no model provider or API
key.

Evidence: `/mnt/datadisk/repos/rUvnet/RuVector/ui/ruvocal/docs/adr/ADR-033-RUVECTOR-RUFLO-MCP-INTEGRATION.md`,
`/mnt/datadisk/repos/rUvnet/RuVector/ui/ruvocal/mcp-bridge/mcp-stdio-kernel.js`,
and `tests/fixtures/consumers/ruvocal/mcp-child.json`.
