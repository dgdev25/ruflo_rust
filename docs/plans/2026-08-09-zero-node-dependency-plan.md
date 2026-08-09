# Zero Node Dependency Plan — ruflo-rust full native implementation

**Created:** 2026-08-09
**Goal:** Eliminate ALL Node/JS runtime dependencies. Every TS feature gets a pure-Rust equivalent.
**Method:** GLM code → Codex Sol review → fix → repeat (per task)

---

## Current state

- 53 command families: ✅ native dispatch
- 48 services: ✅ state management
- 33 MCP tools: ✅ (from 4)
- Native swarm: ✅ claude + codex workers
- 9/9 release gates: ✅
- 27/27 audit bugs: ✅ fixed
- Latency: 3ms flat (10-107x faster than Node)

## Remaining gaps (7 areas, 13 tasks)

---

## TASK 1: ONNX Runtime — native MiniLM embeddings (P0)
**Replaces:** Node ONNX runtime (all-MiniLM-L6-v2, all-mpnet-base-v2)
**Rust crate:** `ort` (ONNX Runtime Rust bindings — wraps the same C library Node's onnxruntime-node uses)
**What to build:**
- Add `ort = "2"` to workspace deps
- Download all-MiniLM-L6-v2 ONNX model at init (or bundle as .onnx in the binary)
- Replace `embeddings::embed()` FNV hash vectorizer with real ONNX inference
- Output: 384-dim float32 vector (matches Node output exactly)
- Cross-runtime compatibility: same model → same vectors → same search results
**Files:** `src/crates/ruflo-cli/src/embeddings.rs`, `Cargo.toml`
**Estimated effort:** ~200 LOC model loading + inference + tokenizer
**Review focus:** Model loading path safety, memory bounds, deterministic output

## TASK 2: Wire RuVector (.rvf) into semantic search (P0)
**Replaces:** Node HNSW (hnswlib-node)
**Already integrated:** RuVector is ALREADY in ruflo-storage via rvf_adapter.rs!
- `rvf_adapter.rs` has `RvfVectorStore` (AgentDB) with `ingest_agentdb()` + `search_agentdb()`
- Git-pinned at rev `597be6a7` in Cargo.toml
- `.rvf` files on disk
**What to build:**
- Wire `rvf_adapter::RvfVectorStore::search_agentdb()` into the MCP dispatcher's `memory_search` handler
- Wire into `SqliteMemoryStore` — add a `search_semantic(query_vector, limit)` method that opens the RVF store
- On `memory_store`, embed content with ONNX (Task 1) and ingest into RVF via `ingest_agentdb()`
- On `memory_search`, embed query → RVF search → join results with SQLite metadata
- Index rebuild: iterate existing SQLite entries, embed each, ingest into fresh RVF store
**DO NOT rebuild RuVector or add hnsw_rs/instant-distance — the RVF adapter IS the vector DB.**
**Files:** `src/crates/ruflo-storage/src/memory_sqlite.rs`, `src/crates/ruflo-mcp/src/dispatcher.rs`
**Estimated effort:** ~150 LOC wiring (RVF adapter already has search/ingest)
**Review focus:** RVF store lifecycle, concurrent access, index freshness

## TASK 3: tree-sitter AST analysis (P1)
**Replaces:** Node tree-sitter (ruvector/ast-analyzer.ts)
**Rust crate:** `tree-sitter` + language grammars (`tree-sitter-typescript`, `tree-sitter-rust`, `tree-sitter-python`)
**What to build:**
- Add tree-sitter crates to ruflo-cli
- Replace regex symbol extraction in `analyze.rs` with real AST traversal
- Extract: functions, classes, imports, exports, complexity metrics from parse tree
- Supports: .ts, .tsx, .js, .rs, .py
**Files:** `src/crates/ruflo-cli/src/analyze.rs`
**Estimated effort:** ~400 LOC parser integration + symbol extraction
**Review focus:** Parser thread safety, memory bounds on large files

## TASK 4: Graph algorithms — MinCut + Louvain + Dijkstra (P1)
**Replaces:** Node graph-analyzer.ts (ruvector)
**Rust crate:** `petgraph` (pure Rust graph library)
**What to build:**
- Replace simple DFS cycle detection with `petgraph::algo` (kosaraju SCC, stoer-wagner mincut)
- Implement Louvain community detection (modularity optimization)
- Replace simple bisection with real MinCut partition suggestion
- Dijkstra shortest-path for dependency distance
**Files:** `src/crates/ruflo-cli/src/analyze.rs` (graph section)
**Estimated effort:** ~250 LOC algorithm porting
**Review focus:** Graph construction correctness, algorithm invariants

## TASK 5: SONA/EWC learning loop (P1)
**Replaces:** Node SONA instant adaptation + EWC++ consolidation
**Rust approach:** Pure Rust incremental learning (no WASM needed)
**What to build:**
- Read JSONL event stream from hooks-events.jsonl
- Pattern extraction: task → agent → outcome frequency table
- EWC-style elastic weight consolidation: track which patterns are "important" (high success rate), protect them from forgetting
- Update routing weights in `learned_routing` service based on outcome feedback
- Feed into `hooks route` — use learned weights instead of keyword-only matching
**Files:** `src/crates/ruflo-cli/src/hooks.rs`, `src/crates/ruflo-cli/src/services.rs` (learned_routing)
**Estimated effort:** ~350 LOC learning loop + weight update + routing integration
**Review focus:** Learning convergence, weight bounds, no infinite growth

## TASK 6: Bandit model router (P1)
**Replaces:** Node enhanced-model-router.ts (multi-armed bandit + A/B testing)
**Rust approach:** Thompson sampling (pure math, no external crate)
**What to build:**
- Per-task-category beta distribution (alpha=successees, beta=failures)
- Sample from each model's distribution, pick highest
- Track outcomes, update distributions
- A/B test support: allocate 10% traffic to exploration
- Persist state to `.claude-flow/model-router-state.json`
**Files:** `src/crates/ruflo-cli/src/hooks.rs` (model_route/model_outcome)
**Estimated effort:** ~200 LOC bandit + state persistence
**Review focus:** Distribution correctness, state persistence atomicity

## TASK 7: IPFS client — plugin marketplace (P2)
**Replaces:** Node ipfs-http-client
**Rust crate:** `ipfs-api` or raw `reqwest` to IPFS HTTP API (localhost:5001)
**What to build:**
- `plugins install`: resolve CID → fetch from IPFS gateway → verify hash → extract
- `plugins search`: query IPNS directory → list results
- `transfer-store publish`: add file to IPFS → return CID
- `transfer-store download`: fetch CID → verify → save
- Fall back to HTTP gateway (ipfs.io) if no local IPFS daemon
**Files:** `src/crates/ruflo-cli/src/plugins.rs`, `transfer_store.rs`
**Estimated effort:** ~400 LOC IPFS HTTP client + marketplace
**Review focus:** Hash verification, download size limits, timeout handling

## TASK 8: Auth login — OAuth PKCE flow (P2)
**Replaces:** Node @inquirer/browser PKCE flow
**Rust crate:** `oauth2` (pure Rust OAuth 2.0 client) + `open` (open browser)
**What to build:**
- PKCE flow: generate code_verifier + code_challenge
- Open browser to auth URL
- Start local HTTP server on random port for callback
- Exchange code for token
- Store token in vault (Task 0 vault with real key)
- `--token-stdin`: read token from stdin (already supported structurally)
**Files:** `src/crates/ruflo-cli/src/auth.rs`
**Estimated effort:** ~300 LOC OAuth flow + callback server
**Review focus:** Token storage security, callback server cleanup, port binding

## TASK 9: 338 more MCP tools (P2)
**Replaces:** Remaining TS mcp-tools/ files
**Approach:** Batch-port by domain, prioritizing the most-used tools
- **Batch 1 (coordination):** coordination_* (15 tools), system_* (8), progress_* (5)
- **Batch 2 (agent management):** agent_* extended (pool/health/logs/wasm-*), wasm-agent_* (12)
- **Batch 3 (infrastructure):** terminal_* (5), session_* extended (5), managed-agent_* (8)
- **Batch 4 (integrations):** github_* (5), agentdb_* (8), agenticow_* (6)
- **Batch 5 (research):** metaharness_* (15), flywheel_* (5), testgen_* (4)
- **Batch 6 (specialized):** business-pod_* (8), daa_* (6), browser_* (8), aidefence_* (4), ruvllm_* (4)
**Files:** `src/crates/ruflo-mcp/src/tools_extra.rs` (extend)
**Estimated effort:** ~20 LOC per tool × 338 = ~6800 LOC (mostly state-file I/O)
**Review focus:** State file consistency, input validation, deny policy

## TASK 10: Byte-parity fixtures — remaining 26 families (P3)
**Approach:** Capture TS overview for each, align native overview()
**Files:** 26 module overview() functions + `tests/fixtures/cli/*/overview.json`
**Estimated effort:** ~50 LOC per family (overview alignment)
**Review focus:** Exact byte match, both binaries

## TASK 11: Windows cross-compile + smoke (P3)
**Prerequisite:** `sudo apt install mingw-w64`
**What to build:**
- `cargo check --target x86_64-pc-windows-gnu` passes
- Fix any remaining Windows-incompatible code
- Run release-smoke.ps1 tests
**Files:** Any remaining unix-only code paths
**Estimated effort:** ~1-2 hours of targeted fixes

## TASK 12: DiskANN vector index (P3, optional)
**Replaces:** Node DiskANN (diskann-backend.ts)
**Rust crate:** `spydance` or custom implementation
**Note:** Only needed for >1M vector datasets. HNSW (Task 2) suffices for most use cases.
**Estimated effort:** ~500 LOC (can defer)

---

## Execution order (dependencies)

```
Phase 1 (P0 — compute foundation):
  Task 1 (ONNX) → Task 2 (HNSW) — these unblock semantic search

Phase 2 (P1 — intelligence layer):
  Task 3 (tree-sitter) + Task 4 (petgraph) — unblock real code analysis
  Task 5 (SONA/EWC) + Task 6 (bandit) — unblock adaptive routing

Phase 3 (P2 — integrations):
  Task 7 (IPFS) + Task 8 (auth) + Task 9 (MCP tools) — independent

Phase 4 (P3 — polish):
  Task 10 (byte-parity) + Task 11 (Windows) + Task 12 (DiskANN)
```

## Review cycle (per task)

1. GLM implements the task
2. `cargo build --workspace && cargo clippy -- -D warnings && cargo test --workspace`
3. Codex Sol review (gpt-5.6-sol/medium): "Review <file> for correctness bugs, fail-open paths, exit-code correctness, state-persistence safety"
4. Fix any Codex findings
5. Commit + push
6. Move to next task

## New Rust crate dependencies needed

| Crate | Purpose | Version |
|-------|---------|---------|
| `ort` | ONNX Runtime inference | 2.0 |
| `instant-distance` or `hnsw_rs` | HNSW vector index | latest |
| `tree-sitter` + grammars | AST parsing | 0.22+ |
| `petgraph` | Graph algorithms | 0.6 |
| `ipfs-api` | IPFS HTTP client | 0.5 (or reqwest) |
| `oauth2` | OAuth PKCE flow | 4.0 |
| `open` | Open browser | 5.0 |
| `tokenizers` | HuggingFace tokenizer (for MiniLM) | 0.19 |

## Total estimated effort

- Phase 1 (P0): ~500 LOC + crate integration = 1-2 sessions
- Phase 2 (P1): ~1200 LOC = 2-3 sessions
- Phase 3 (P2): ~2000 LOC (mostly MCP tool stubs) = 3-4 sessions
- Phase 4 (P3): ~500 LOC + Windows = 1-2 sessions
- **Total: ~4200 LOC, 7-11 sessions**

---

*This plan eliminates every Node dependency. After completion, `ruflo` is a single 8.5MB binary with zero JS runtime, zero npm install, zero node_modules — just pure Rust.*
