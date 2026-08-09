# ADR-0009: RuVector RVF (HNSW) is sufficient — DiskANN not adopted

- **Status:** Accepted
- **Date:** 2026-08-09
- **Supersedes:** T12 (optional DiskANN) of the zero-node-dependency plan

## Context

The zero-node-dependency plan listed an optional Task 12: "DiskANN vector
index — assess need vs RVF HNSW, implement or close with ADR." DiskANN is a
disk-backed approximate-nearest-neighbor index designed for vector corpora
too large to fit in RAM (hundreds of millions to billions of vectors).

The native build uses RuVector's `.rvf` (RVF) HNSW store via
`ruflo-storage::rvf_adapter` for all semantic search: `memory_search`,
`embeddings search`, `agentdb_*`, and the registry.

## Decision

**Do NOT adopt DiskANN. RuVector RVF (in-RAM HNSW) is sufficient for ruflo's
scale.**

## Evidence

ruflo's vector stores are bounded by the number of memory entries / patterns
a single project accumulates:

| Store | Typical size | At 384-dim f32 (1.5KB/vec) |
|-------|-------------|----------------------------|
| `memory` namespace | hundreds – low thousands | < 5 MB |
| `patterns` (transfer-store) | dozens – hundreds | < 1 MB |
| `agentdb` router decisions | hundreds | < 1 MB |

Even an outlier project with 100k memory entries is ~150 MB — well within
RAM. DiskANN's disk-I/O-per-query cost is unjustified at this scale; in-RAM
HNSW returns sub-millisecond k-NN with zero disk seeks.

## Consequences

- **Positive:** No new dependency. RVF already integrated (rvf_adapter, git-pinned).
  Simpler build, faster queries, no disk-index lifecycle to manage.
- **Negative:** If a future use case needs billion-scale vectors (e.g. a shared
  global pattern registry across all ruflo installs), DiskANN or a sharded RVF
  tier would be revisited. That is not a current requirement.
- **Reconciliation:** T12 is closed. The `onnx` feature + RVF backend-tagging
  (ADR-implicit in #33) cover the cross-runtime compatibility concern instead.

## Implementation note

`SqliteMemoryStore::search_semantic` + `ingest_semantic` open the RVF store
sibling to the SQLite db and run real HNSW k-NN. The store is backend-tagged
(`memory.rvf.backend`) to prevent hash↔ONNX vector-mismatch. No DiskANN code
is introduced.
