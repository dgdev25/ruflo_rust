# RVF Interop Fixtures

## Status

Task 9 implementation evidence as of 2026-08-07.

## Verified inputs

- `agentdb-stable-order.json` drives creation and reopening through the pinned upstream `rvf-adapter-agentdb` API.
- `agentdb-compaction.json` drives compaction and reopen through the same facade.
- `agentic-flow-reopen.json` drives creation and reopen through the pinned upstream `rvf-adapter-agentic-flow` API.

## Verified behavior

- Stable AgentDB result ordering is proven with equal-distance vectors. Upstream `rvf-runtime` sorts `(distance, id)` deterministically, and the facade preserves that ordering.
- AgentDB reopen after facade-driven compaction is proven for the basic search path.
- Agentic Flow reopen is proven only at the file/status level: `total_vectors` survives closing and reopening.

The generic key/value memory compatibility path remains in `.swarm/memory.db`
because existing Ruflo consumers rely on its SQLite `memory_entries` contract.
The native MCP facade uses that table for exact retrieval and keyword fallback;
the RVF adapters remain the vector/semantic persistence boundary.

## Explicit blockers

- Unknown-segment preservation is not asserted here. The upstream runtime has tests for that behavior, but the currently available typed adapter APIs do not expose a way to create or round-trip an intentionally unknown segment without hand-encoding RVF bytes, which this task forbids.
- Agentic Flow shared-memory lookup/search after reopen is not asserted here. The current upstream `rvf-adapter-agentic-flow` `open()` path does not rebuild `entry_index` or `key_index`, so persistent retrieval semantics cannot be proven from the available API without patching upstream first.

## Upstream pins

- `rvf-runtime` from `https://github.com/ruvnet/RuVector.git` at `597be6a753472f0521fe2def097116e717ed4332`
- `rvf-adapter-agentdb` from the same immutable revision
- `rvf-adapter-agentic-flow` from the same immutable revision
