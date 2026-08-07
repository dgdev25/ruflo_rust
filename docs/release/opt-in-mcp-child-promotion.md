# Opt-in native MCP-child promotion

Status: staging-verified for the bounded stdio MCP-child integration; not a
general replacement release.

## Evidence

- Native consumer pilot: `tests/mcp_stdio.rs::ruvocal_opt_in_mcp_child_pilot_routes_namespaced_memory_tools`.
- Consumer contract: `tests/fixtures/consumers/ruvocal/mcp-child.json` and
  `docs/pilots/ruvocal-mcp-child.md`.
- Real bridge-entrypoint staging smoke (2026-08-07): the checked-out ruVocal
  bridge was started with `RUFLO_MCP_COMMAND` pointing at the native `ruflo`
  binary. It discovered four `ruflo__*` tools, completed a namespaced
  `memory_store`/`memory_search` round trip through `/mcp`, and then proved
  that `ENABLE_RUFLO=false` removes every Ruflo group and starts no child.
- Release and supply-chain evidence: the repository's local locked verification
  suite (`fmt`, tests, Clippy, fixture, consumer-inventory, and supply-chain
  checks). GitHub Actions are intentionally absent from this repository.
- The run passed Linux x86_64 and aarch64, macOS x86_64 and aarch64, and
  Windows x86_64. Each runner built the release binaries, generated an SBOM,
  and ran its native platform smoke gate. The supply-chain job also passed.

## Approved opt-in

An integrator may configure the bridge to execute a reviewed native binary
directly, preserving the existing stdio child protocol:

```text
ENABLE_RUFLO=true
RUFLO_MCP_COMMAND=/opt/ruflo/bin/ruflo
```

Without `RUFLO_MCP_COMMAND`, the bridge retains its legacy default:
`npx -y ruflo mcp start`. The command override is an executable path, not a
shell command; the bridge supplies the fixed argument vector `mcp start`.

For ruVocal, retain its accepted bridge behavior: `tools/list` discovery,
`ruflo__` namespace prefixing at the HTTP boundary, prefix stripping for
`tools/call`, newline-delimited JSON-RPC to the child, and the existing
`ENABLE_RUFLO` opt-in/kill switch. No API key is required by this native
child or its compatibility tests.

## Boundaries and rollback

- This promotion covers the tested MCP memory child contract only. It does not
  claim native worker scheduling, `dual run` worker execution, arbitrary Node
  plugin execution, or TypeScript in-process import compatibility.
- Deferred native capabilities continue to return deterministic unsupported
  responses rather than silently falling back to Node behavior.
- To roll back, unset `RUFLO_MCP_COMMAND` to restore the legacy npm child, or
  set `ENABLE_RUFLO=false`; its built-in tools remain available and no Ruflo
  child is launched.
- Keep the deployment opt-in until its target rUvNet consumer has adopted the
  reviewed bridge override and recorded its own release evidence.
