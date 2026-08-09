# Deep Audit: Node ruflo → ruflo-rust port gaps (Critical → Low)

**Date:** 2026-08-09
**Auditor:** GLM (implementation) + Codex (review)
**Method:** Systematic file-tree comparison of `v3/@claude-flow/cli/src/` (TS, 56 commands + 48 services + 48 MCP tools + 10+ security/memory/ruvector modules) vs `src/crates/` (Rust, 9 crates + 44 CLI modules + 18 MCP functions).

---

## Current state summary

| Surface | TS (Node) | Rust (native) | Gap |
|---------|:---------:|:-------------:|:---:|
| Command families (top-level) | 56 files | 44 modules (53 families) | **Covered** (overview + subcommand dispatch) |
| Subcommand depth | Full (real MCP calls, state, compute) | Partial (state management; MCP/compute degrades) | **CRITICAL** |
| MCP server tools | 48 | 18 | **30 unported** |
| Services layer | 48 files | 0 (no services crate) | **CRITICAL** |
| MCP client (callMCPTool) | Yes (delegates to server) | No | **CRITICAL** |
| Interactive prompts | @inquirer (wizard, selects) | None | **HIGH** |
| Output formatting | output.ts (printBox, printTable, ANSI, spinner) | println! per-module | **MEDIUM** |
| Memory/learning | ONNX + HNSW + EWC + BM25 + rerank | SQLite kv + hash vectorizer | **HIGH** |
| RuVector (AST/graph/DiskANN/router) | 10+ files | Regex fallbacks | **HIGH** |
| Security (AIDefence/ChannelGuard) | Full regex catalog | Ported (regex, simpler) | **LOW** |
| Transfer/IPFS | 8 subdirs | Degrades | **MEDIUM** |
| Swarm execution | Node (claude --print workers) | Native Rust (ADR-0008, this session) | **Closed** |
| Hooks event loop + SONA | Event recording + EWC++ learning | Event recording only | **MEDIUM** |
| Persistence | sql.js (WASM SQLite) | rusqlite (native SQLite) | **Better** (native > WASM) |
| Benchmark (startup) | ~120ms V8 floor | ~8ms | **16x faster** |

---

## CRITICAL (core functionality missing or non-functional)

### C1. MCP server tool surface: 30 of 48 tools unported

**Evidence:** TS `src/mcp-tools/` has 48 tool files. Rust `src/crates/ruflo-mcp/` exposes 18 functions. The missing 30 include: agent_spawn/execute/status/list/terminate/pool/health/logs, swarm_init/status/shutdown/coordinate, hive-mind tools, workflow tools, memory hybrid-search, neural train/predict, embeddings init/search, security scan, hooks route, coverage tools, model-router decide, distill tools, flywheel run/promote.

**Impact:** Agents that connect to the MCP server (Claude Code, external clients) get only 18 tools. The other 30 return "not found." This breaks agent-driven workflows that depend on MCP tool delegation.

**Fix:** Port the 30 missing MCP tool handlers into `ruflo-mcp`. Many are thin wrappers around existing CLI logic (the CLI modules already implement the behavior; the MCP tool just calls it). Estimated ~3-5 lines per tool (call the existing module function, format result as JSON).

### C2. MCP client (callMCPTool) absent

**Evidence:** TS `src/mcp-client.ts` exports `callMCPTool()` and `listMCPTools()`. Many TS commands delegate to MCP tools (e.g., `hive-mind init` calls `callMCPTool('hive-mind_init', config)`). Rust has no MCP client — these commands degrade with "needs Node runtime."

**Impact:** Commands that delegate to MCP tools can't execute their primary function natively. They print degradation warnings instead of results.

**Fix:** Implement a minimal MCP client in Rust that connects to the local MCP server (stdio or HTTP) and calls tools. OR refactor: make CLI commands call the underlying service directly (not via MCP round-trip), eliminating the client dependency.

### C3. Services layer absent (48 TS files → 0 Rust)

**Evidence:** TS `src/services/` has 48 files implementing core logic: `global-ai-budget.ts` (budget enforcement), `daemon-autostart.ts` (crontab/launchd), `distill-oracle.ts` (distillation), `flywheel-*.ts` (receipt loop), `evolve-proof.ts` (Darwin evolution), `fable-harness.ts`, `checkpoint-gate.ts`, `bounded-worker-pool.ts`, `claim-service.ts`, `config-file-manager.ts`, `git-workspace-identity.ts`, etc.

Rust has NO services crate. The CLI modules manage state directly (daemon.rs manages budget state; hooks.rs manages events) but the LOGIC (budget enforcement circuit-breaker, learning loops, evolution, worktree management) is absent.

**Impact:** The daemon budget is managed (pause/resume/show) but the ENFORCEMENT path (reserve/release/active-slot tracking) doesn't run natively. Distill/flywheel/evolve degrade. This means ruflo-rust manages state but doesn't execute the intelligence layer.

**Fix:** Port the critical services incrementally. Priority order: `claim-service` (RBAC enforcement), `global-ai-budget` (reserve/release/active tracking), `bounded-worker-pool` (concurrency limits), `git-workspace-identity` (worktree isolation for swarms), `daemon-autostart` (systemd/launchd adapters). Lower: distill/flywheel/evolve (research-quality tools).

---

## HIGH (important for feature parity)

### H1. Interactive prompts absent

**Evidence:** TS `src/prompt.ts` uses `@inquirer/prompts` for init wizard, interactive topology/consensus selection, MCP config, etc. Rust has no TUI library.

**Impact:** `init` wizard, interactive `swarm init`, interactive `config` are non-interactive only (flag-based). Users lose the guided setup experience.

**Fix:** Add `dialoguer` crate (Rust TUI prompts: select, input, confirm). Wire into init wizard + interactive subcommands.

### H2. Memory/learning layer shallow

**Evidence:** TS `src/memory/` has 10+ files: `bge-embedder.ts` (ONNX BGE embeddings), `cross-encoder-rerank.ts` (cross-encoder reranking), `ewc-consolidation.ts` (EWC++ neural consolidation), `hybrid-retrieval.ts` (BM25 + HNSW + MMR), `lucene-bm25.ts` (BM25 implementation), `graph-edge-writer.ts` (causal graph), `embedding-quantization.ts` (RaBitQ), `intelligence.ts` (SONA learning stats).

Rust has: `ruflo-storage` (SQLite key-value + semantic search via deterministic hash vectorizer). No ONNX, no HNSW, no EWC, no BM25, no rerank.

**Impact:** Semantic search works (deterministic vectorizer) but produces lower-quality results than ONNX embeddings. The learning loop (SONA/EWC pattern consolidation) doesn't run natively.

**Fix:** This is the deepest gap. Options: (a) link RuVector/RVF Rust library directly (if available as a crate); (b) port BM25 + HNSW natively (medium effort); (c) accept the deterministic vectorizer for basic search, defer ONNX-quality embeddings to a linked native library. ADR-0001 (compose, don't recreate) applies.

### H3. RuVector integration (AST/graph/DiskANN/router)

**Evidence:** TS `src/ruvector/` has 10+ files: `ast-analyzer.ts` (tree-sitter AST), `graph-analyzer.ts` (MinCut/Louvain/Dijkstra), `diskann-backend.ts` (DiskANN vector index), `enhanced-model-router.ts` (bandit/AB-test model router), `coverage-tools.ts` (coverage routing).

Rust has regex-based fallbacks (ported this session): regex symbol extraction, simple DFS cycle detection, connected-components, edge-cut bisection. No tree-sitter AST, no DiskANN, no bandit router.

**Impact:** Code analysis is functional but lower-fidelity than tree-sitter AST. Graph analysis lacks real MinCut/Louvain. Model routing is keyword-based, not bandit-optimized.

**Fix:** Link tree-sitter (available as Rust crate `tree-sitter`). Port MinCut/Louvain from algorithm references. For DiskANN: link the RuVector Rust library if available (ADR-0001).

---

## MEDIUM (degrades gracefully, documented)

### M1. Output formatting (output.ts)

**Evidence:** TS `output.ts` provides printBox, printTable, ANSI colors, createSpinner, printList, printNumberedList. Rust uses per-module `println!` with manual formatting.

**Impact:** Formatting differs between TS and Rust (byte-parity issues). Tables/boxes don't match exactly. No spinner for long operations.

**Fix:** Create a shared `output.rs` module with box/table/spinner helpers. Align formatting to TS output.ts byte-for-byte. Reduces per-module duplication.

### M2. Transfer/IPFS

**Evidence:** TS `src/transfer/` has 8 subdirs: anonymization, IPFS client, serialization, model store, exports. Rust degrades with "IPFS registry not available in native build."

**Impact:** Plugin install/search/rate from IPFS registry don't work natively. Users must use `npx ruflo plugins install`.

**Fix:** Implement IPFS client in Rust (`ipfs-api` crate) or link a Rust IPFS library. Medium effort.

### M3. Hooks learning loop (SONA/EWC)

**Evidence:** TS hooks system records events AND learns: SONA instant adaptation, EWC++ consolidation, neural pattern training. Rust records events to JSONL but doesn't run the learning loop.

**Impact:** Hooks audit trail works; the learning/adaptation that consumes it runs in the Node daemon. Native hooks are fire-and-forget, not adaptive.

**Fix:** Port the SONA/EWC learning loop to process the JSONL event stream. Depends on H2 (memory/learning layer). Large effort.

### M4. Prompt suggestion engine

**Evidence:** TS `src/suggest.ts` provides "did you mean?" command suggestions. Rust returns "unsupported native CLI invocation" with no suggestion.

**Impact:** UX: mistyped commands give no helpful guidance.

**Fix:** Add Levenshtein-distance suggestion (10-line function using `strsim` crate).

---

## LOW (cosmetic / future enhancements)

### L1. Spinner for long operations

TS uses ora-like spinner. Rust has none. Add `indicatif` crate.

### L2. Log filters (log-filters.ts)

TS filters known noisy log lines. Rust doesn't. Add a log-filter module.

### L3. ANSI color gating

TS gates ANSI by NO_COLOR / non-TTY detection. Rust prints plain text (good default but misses color when desired).

### L4. Per-subcommand byte-parity fixtures

20 of 46 overview fixtures byte-aligned. 17 remaining + per-subcommand/flag/error fixtures are partial. Infra wired; coverage expansion is ongoing.

---

## What's BETTER in Rust (advantages gained)

- **Startup**: 13-16x faster (no V8 overhead)
- **Footprint**: 230x smaller (8.3 MB vs 1.9 GB)
- **Persistence**: native rusqlite > WASM sql.js (no FFI boundary)
- **Swarm execution**: native subprocess spawning (ADR-0008, no Node)
- **Memory safety**: Rust ownership prevents the class of bugs TS can have
- **Distribution**: single static binary, no npm install, no node_modules

---

## Recommended port priority (if continuing)

1. **C1 (MCP tools)**: port 30 missing MCP tool handlers — biggest agent-interop gap
2. **H1 (prompts)**: add `dialoguer` for init wizard — biggest UX gap
3. **M1 (output)**: shared output module — fixes byte-parity + reduces duplication
4. **C2 (MCP client)**: eliminate MCP round-trip by calling services directly
5. **C3 (services)**: port claim-service + global-ai-budget enforcement — closes the RBAC/budget loop
6. **H2/H3 (memory/ruvector)**: link native Rust libraries per ADR-0001

---

*This audit is grounded in actual file-tree comparison of `v3/@claude-flow/cli/src/` (Node ruflo, 56 commands + 48 services + 48 MCP tools + security/memory/ruvector modules) vs `src/crates/` (ruflo-rust, 9 crates + 44 modules + 18 MCP functions). No memory-based assertions.*
