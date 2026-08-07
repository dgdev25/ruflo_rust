# ruVocal bridge-entrypoint native staging smoke

Date: 2026-08-07

This staging smoke exercised the actual Node bridge entrypoint from
`/mnt/datadisk/dev/ruflo/ruflo/src/ruvocal/mcp-bridge/index.js`, not merely a
protocol simulator. The bridge was copied to a temporary directory with its
locked dependencies installed. It listened only on `127.0.0.1`.

## Native-child run

The bridge received:

```text
MCP_GROUP_INTELLIGENCE=false
RUFLO_MCP_COMMAND=/mnt/datadisk/dev/ruflo_rustv1/target/debug/ruflo
```

It started exactly one Ruflo backend, performed MCP initialization and
`tools/list`, and published four namespaced tools:

- `ruflo__agent_spawn`
- `ruflo__memory_store`
- `ruflo__memory_retrieve`
- `ruflo__memory_search`

Calls to `ruflo__memory_store` and `ruflo__memory_search` through the bridge's
`/mcp` HTTP endpoint completed a durable native-memory round trip. No model
provider or API key was used.

## Rollback run

The same entrypoint was restarted with:

```text
MCP_GROUP_INTELLIGENCE=false
ENABLE_RUFLO=false
RUFLO_MCP_COMMAND=/mnt/datadisk/dev/ruflo_rustv1/target/debug/ruflo
```

The health endpoint reported zero external tools and no Ruflo backend; the MCP
tool list contained only the three built-in bridge tools. This proves the
rollback does not depend on process-path changes or a failing child startup.

## Deployment guardrails

- Bind the bridge to loopback or configure its existing authentication before
  exposing it on a network.
- Use an absolute, reviewed native-binary path for `RUFLO_MCP_COMMAND`.
- First deploy with `ENABLE_RUFLO=true` and only the intended `MCP_GROUP_*`
  groups enabled.
- Roll back by setting `ENABLE_RUFLO=false`; do not remove persisted memory as
  part of rollback.
