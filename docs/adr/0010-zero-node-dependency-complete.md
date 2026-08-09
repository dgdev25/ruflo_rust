# ADR-0010: Zero Node dependency — native rebuild complete

- **Status:** Accepted + Implemented
- **Date:** 2026-08-09
- **Builds on:** ADR-0001 (compose native rUv components), ADR-0005
  (enforce native-only plugin/hook execution), ADR-0008 (native swarm),
  ADR-0009 (RVF HNSW sufficient)

## Context

ADR-0001 and ADR-0005 set the direction: ruflo-rust composes native rUv
components (RuVector, RVF) and executes plugins/hooks natively. ADR-0008
removed Node from swarm execution. The zero-node-dependency plan (Aug 2026)
extended this to eliminate Node from EVERY remaining surface.

## Decision

The native Rust CLI now has **zero Node.js runtime dependencies** for the
implemented surface. This ADR records the completion and reconciles the
prior ADRs whose claims described a partially-native world.

## What changed (reconciliation)

| Prior ADR | Claim | Current state |
|-----------|-------|---------------|
| 0001 | Compose native rUv components | ✅ Fully realized — RuVector RVF is the vector DB (no hnswlib-node, no Pinecone) |
| 0005 | Native-only plugin/hook execution | ✅ Implemented — no Node plugin/hook path remains |
| 0007 | Codex-only dual-run (Node bridge) | **Superseded** by 0008 (2026-08-08) |
| 0008 | Native Rust swarm (claude+codex, no Node) | ✅ Accepted + Implemented, extended with pheromone feedback + work-stealing |
| 0009 | RVF HNSW sufficient (no DiskANN) | ✅ Accepted — closes the optional DiskANN task |

## Native equivalents delivered (this build)

- **Embeddings:** `ort` (ONNX Runtime) + all-MiniLM-L6-v2 — real 384-dim
  inference. Hash-vectorizer fallback when the model is absent. (No
  onnxruntime-node, no @claude-flow/transformers.)
- **Vector DB:** RuVector `.rvf` HNSW via `rvf_adapter` — k-NN search,
  ingest, semantic_id binding, backend-tagged to prevent hash↔ONNX mismatch.
  (No hnswlib-node.)
- **AST:** tree-sitter (rust/typescript/python/go/java/c) — real parser.
  (No ruvector ast-analyzer Node module.)
- **Graph:** petgraph (MinCut, Louvain, SCC, Dijkstra). Pure Rust.
- **Learning:** SONA MLP with full backpropagation + EWC++ Fisher
  consolidation. Thompson-sampling bandit router. Pure Rust.
- **Swarm:** native subprocess spawning (claude/codex), pheromone-adaptive
  topology, work-stealing, global AI budget circuit breaker.
- **MCP:** 343 tools, 306/306 TS name overlap, 0 missing.
- **IPFS:** gateway download + native CIDv1 computation.
- **Auth:** RFC 7636 PKCE (S256, test-vector-verified) + token exchange.
- **Windows:** cross-builds to a real PE32+ `.exe` (onnx feature-gated off
  for windows-gnu where ort ships no prebuilt).

## Remaining caveat

The 48 ported `services.rs` modules manage persisted state with full parity
for the CLI surface; live behavioral parity (subprocess pools, network
workers) is implemented for the highest-value services (headless executor,
git worktree, repo supervisor, flywheel ledger, global budget) and continues
to be extended. The `agentic-flow` and `ruvllm` MCP buckets degrade with a
documented reason when their external binary/endpoint is absent — they do
not silently fall back to Node.
