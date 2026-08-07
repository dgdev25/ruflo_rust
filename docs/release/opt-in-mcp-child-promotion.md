# Opt-in native MCP-child promotion

Status: promotion-ready for the bounded stdio MCP-child pilot; not a general
replacement release.

## Evidence

- Native consumer pilot: `tests/mcp_stdio.rs::ruvocal_opt_in_mcp_child_pilot_routes_namespaced_memory_tools`.
- Consumer contract: `tests/fixtures/consumers/ruvocal/mcp-child.json` and
  `docs/pilots/ruvocal-mcp-child.md`.
- Five-platform release and supply-chain evidence: GitHub Actions
  [run 31210691011](https://github.com/dgdev25/ruflo_rustv1/actions/runs/31210691011),
  commit `184ebfc`.
- The run passed Linux x86_64 and aarch64, macOS x86_64 and aarch64, and
  Windows x86_64. Each runner built the release binaries, generated an SBOM,
  and ran its native platform smoke gate. The supply-chain job also passed.

## Approved opt-in

An integrator may place the tested native `ruflo` binary ahead of the existing
Ruflo command on the bridge process `PATH`, then start it as the existing
stdio child command:

```text
ruflo mcp start
```

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
- To roll back, restore the prior command resolution for the bridge process or
  set `ENABLE_RUFLO=false`; its built-in tools remain available.
- Keep the deployment opt-in until a real bridge-entrypoint integration test
  can supplement the checked-out source's accepted protocol evidence.
