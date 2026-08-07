# RVF Interop Fixtures

## Status

Task 9 implementation evidence as of 2026-08-07.

## Verified inputs

- `agentdb-compatible.rvf` is generated inside `tests/rvf_interop.rs` through `RvfPersistencePort::create_agentdb`, then reopened through the pinned upstream `rvf-adapter-agentdb` API.
- `agentdb-compact.rvf` is generated inside `tests/rvf_interop.rs`, compacted through the same facade, then reopened to prove the query surface still works.
- `agentic-flow/swarm.rvf` is generated inside `tests/rvf_interop.rs` through `RvfPersistencePort::create_agentic_flow`, then reopened through the pinned upstream `rvf-adapter-agentic-flow` API to prove the store itself persists.

## Verified behavior

- Stable AgentDB result ordering is proven with equal-distance vectors. Upstream `rvf-runtime` sorts `(distance, id)` deterministically, and the facade preserves that ordering.
- AgentDB reopen after facade-driven compaction is proven for the basic search path.
- Agentic Flow reopen is proven only at the file/status level: `total_vectors` survives closing and reopening.

## Explicit blockers

- Unknown-segment preservation is not asserted here. The upstream runtime has tests for that behavior, but the currently available typed adapter APIs do not expose a way to create or round-trip an intentionally unknown segment without hand-encoding RVF bytes, which this task forbids.
- Agentic Flow shared-memory lookup/search after reopen is not asserted here. The current upstream `rvf-adapter-agentic-flow` `open()` path does not rebuild `entry_index` or `key_index`, so persistent retrieval semantics cannot be proven from the available API without patching upstream first.

## Upstream pins

- `rvf-runtime` from `https://github.com/ruvnet/RuVector.git` at `597be6a753472f0521fe2def097116e717ed4332`
- `rvf-adapter-agentdb` from the same immutable revision
- `rvf-adapter-agentic-flow` from the same immutable revision
