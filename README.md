<div align="center">

# Ruflo — Native Rust AI Orchestration CLI

**Zero Node.js dependency. Pure Rust. 30-195x faster.**

[![Rust](https://img.shields.io/badge/Rust-1.97+-orange.svg)](https://www.rust-lang.org)
[![License](https://img.shields.io/badge/license-MIT%2FApache--2.0-blue.svg)](#license)
[![Tests](https://img.shields.io/badge/tests-516%20passing-green.svg)](#testing)
[![Clippy](https://img.shields.io/badge/clippy-0%20warnings-green.svg)](#testing)
[![MCP Tools](https://img.shields.io/badge/MCP-347%20tools-blue.svg)](#mcp-tools)

</div>

---

## What is this?

Ruflo is a complete native Rust rewrite of the [ruflo](https://github.com/ruvnet/ruflo) AI agent orchestration CLI — originally a TypeScript/Node.js application. Every command, service, and MCP tool has been ported to pure Rust with **zero Node.js runtime dependencies**.

The result: a **39 MB single binary** (vs 2 GB npm package) that starts in **4ms** (vs 148ms), with no Node runtime, no `npm install`, no WASM bridge, no Transformers.js — just Rust.

## Performance vs Node

Full benchmark report: [`docs/benchmarks/node-vs-rust-full.md`](docs/benchmarks/node-vs-rust-full.md)

| Benchmark | Node.js | Rust | Speedup |
|-----------|---------|------|---------|
| Cold start | 148ms | 4ms | **30x** |
| Memory store (100 entries) | 46.5s | 291ms | **159x** |
| Embeddings (10 texts) | 4.7s | 23ms | **195x** |
| 7-step E2E workflow | 2,375ms | 44ms | **53x** |
| Binary size | 2.0 GB | 39 MB | **51x smaller** |
| MCP tools/list latency | ~200ms | ~110ms | **2x** |

## Quick Start

```bash
# Build
cargo build --release

# Initialize a project
./target/release/ruflo init

# Start an agent swarm (spawns real claude/codex subprocesses)
./target/release/ruflo swarm start --objective "fix the failing tests" --workers 3

# Start the MCP server (347 tools)
./target/release/ruflo mcp start

# Semantic search via RuVector HNSW
./target/release/ruflo memory search -q "authentication patterns"

# Route a task via Thompson-sampling bandit
./target/release/ruflo route task "implement user authentication"
```

## Architecture

### 9 Rust Crates

| Crate | Purpose |
|-------|---------|
| `ruflo-cli` | Command dispatcher + all 48 services + 58 command modules |
| `ruflo-mcp` | MCP stdio/http dispatcher + 347 tool handlers |
| `ruflo-storage` | SQLite + RuVector RVF (HNSW vector search) |
| `ruflo-runtime` | Transport layer (stdio, HTTP, NoSlim) |
| `ruflo-config` | Effective config + policy engine (ADR-324) |
| `ruflo-types` | Shared types + error taxonomy |
| `ruflo-actions` | GitHub Actions + CI helpers |
| `ruflo-codex-cli` | Codex subprocess bridge |
| `ruflo-memory` | Memory entry types |

### Native Replacements (zero Node deps)

| TS Component | Rust Equivalent |
|--------------|-----------------|
| Transformers.js (ONNX) | `ort` crate + all-MiniLM-L6-v2 (384-dim) |
| hnswlib-node | RuVector `.rvf` (SIMD HNSW, git-pinned) |
| tree-sitter (Node) | `tree-sitter` crate (Rust+TS+Python+Go+Java+C) |
| Node graph algorithms | `petgraph` (MinCut, Louvain, SCC, Dijkstra) |
| Q-learning router | Thompson-sampling bandit (Beta posterials) |
| WASM SONA training | Native MLP backpropagation + EWC++ Fisher |
| Node swarm (MCP bridge) | Direct subprocess spawning (`std::process::Command`) |
| sql.js (SQLite WASM) | `rusqlite` (native SQLite via bundled C) |
| Ed25519 signing | HMAC-SHA256 (RFC 2104, no extra dep) |
| IPFS HTTP client | `curl` subprocess + native CIDv1 |
| OAuth PKCE (browser) | RFC 7636 S256 (verified against test vector) |

### Key Subsystems

- **SONA Neural Net** — Full MLP (input→hidden→output) with SGD+momentum, L2 regularization, cross-entropy loss, EWC++ (Elastic Weight Consolidation) with Fisher Information Matrix. Trains on router decision history. `src/crates/ruflo-cli/src/sona.rs`
- **Thompson Bandit Router** — Per-agent Beta(α,β) posteriors sampled via Marsaglia-Tsang Gamma. Keyword matching as capability prior. `src/crates/ruflo-cli/src/route.rs`
- **RuVector HNSW** — k-NN vector search via `.rvf` files. Backend-tagged to prevent hash↔ONNX vector mismatch. `src/crates/ruflo-storage/src/rvf_adapter.rs`
- **Flywheel Ledger** — Hash-chained append-only JSONL with HMAC-SHA256 signatures + compare-and-swap promotion. ADR-322. `src/crates/ruflo-cli/src/flywheel_ledger.rs`
- **Pheromone Swarm** — APSC (Adaptive Pheromone Swarm Coordinator) with EMA fitness, eligibility thresholds, suspension/reactivation. `src/crates/ruflo-cli/src/swarm_exec.rs`

## MCP Tools

347 tools across 20+ domains:

| Domain | Tools | Backend |
|--------|-------|---------|
| memory | store/retrieve/search/stats | SQLite + RVF HNSW |
| agent | spawn/list/status/terminate/execute | State + subprocess |
| swarm | init/status/shutdown/coordinate | State + pheromone |
| embeddings | generate/search/compare/ingest | ort ONNX + hash fallback |
| security | scan/defend/PII/threat | Regex + AIDefence |
| neural | train/predict/distill/optimize | SONA MLP + EWC++ |
| route | task/feedback/stats | Thompson bandit |
| graph | scc/boundaries/communities | petgraph |
| crypto | sha256/hmac/base64/uuid | Native |
| wasm | create/prompt/gallery/status | Subprocess isolation |
| browser | open/screenshot/snapshot/eval | Chromium headless |
| + 20 more domains | | |

## Testing

```bash
# Run all tests (516 tests)
cargo test --workspace

# Clippy (0 warnings)
cargo clippy --workspace

# Release build
cargo build --release

# Windows cross-compile
cargo build --target x86_64-pc-windows-gnu -p ruflo --no-default-features
```

Test coverage:
- 265 unit tests in `src/`
- 251 integration tests in `tests/`
- 16 byte-parity fixtures (overview output matches TS reference)
- 34 differential command tests (Node vs Rust)
- 13 end-to-end smoke tests
- Full benchmark report: [`docs/benchmarks/node-vs-rust-full.md`](docs/benchmarks/node-vs-rust-full.md)

## Commands

58 command families. Key commands:

| Command | Description |
|---------|-------------|
| `init` | Set up project (hooks, agents, settings, MCP config) |
| `swarm start` | Spawn N agent workers (claude/codex subprocess) |
| `route task` | Thompson-sampling task→agent routing |
| `neural train` | Train SONA MLP on router decisions |
| `neural predict` | Classify input via trained SONA |
| `neural distill` | Full distillation pipeline (label→tune→fit) |
| `memory search` | Semantic k-NN via RuVector HNSW |
| `memory rebuild-index` | Re-embed all entries with active backend |
| `embeddings search` | RVF-backed semantic similarity |
| `embeddings ingest` | Embed + store in RVF HNSW |
| `analyze boundaries` | Refactor seams (MinCut + Louvain) |
| `auth login` | OAuth PKCE flow (RFC 7636) |
| `security scan` | Regex-based secret/vuln detection |
| `transfer-store publish` | Native CIDv1 + IPFS gateway download |
| `appliance build` | RVFA manifest with SHA-256 checksums |
| `policy evaluate` | HMAC-signed policy receipts |
| `workflow run` | Native step-by-step workflow execution |

## Windows

Cross-compiles to `x86_64-pc-windows-gnu`. Two build paths:
- **Debug** (no ONNX): `cargo build --target x86_64-pc-windows-gnu --no-default-features`
- **Release** (with ONNX): `cargo build --features onnx-dynamic` + ship `onnxruntime.dll`

CI workflows in `.github/workflows/` handle both paths automatically.

## ADRs

10 Architecture Decision Records in `docs/adr/`:

| ADR | Status | Title |
|-----|--------|-------|
| 0001 | Accepted | Compose existing native rUv components |
| 0002 | Accepted | Contract-first compatibility waves |
| 0003 | Accepted | One dispatcher for stdio + stateless MCP |
| 0004 | Implemented | Migrate persistence through fixture-led RVF ports |
| 0005 | Implemented | Native-only plugin/hook execution |
| 0006 | Accepted | Secure remote MCP + persistence boundaries |
| 0007 | Superseded | Codex-only dual-run (superseded by 0008) |
| 0008 | Implemented | Native Rust swarm execution (claude + codex) |
| 0009 | Accepted | RVF HNSW sufficient — DiskANN not adopted |
| 0010 | Implemented | Zero Node dependency — native rebuild complete |

## Project Structure

```
ruflo_rust/
├── src/
│   ├── crates/
│   │   ├── ruflo-cli/        # Commands, services, SONA, swarm, security
│   │   ├── ruflo-mcp/        # MCP dispatcher, 347 tools, policy
│   │   ├── ruflo-storage/    # SQLite, RuVector RVF adapter
│   │   ├── ruflo-runtime/    # Transport (stdio, HTTP)
│   │   ├── ruflo-config/     # Config + policy engine
│   │   ├── ruflo-types/      # Shared types + errors
│   │   ├── ruflo-actions/    # GitHub Actions
│   │   ├── ruflo-codex-cli/  # Codex bridge
│   │   └── ruflo-memory/     # Memory types
│   └── bin/
│       ├── ruflo/            # Main binary
│       ├── claude-flow/      # claude-flow wrapper
│       └── claude-flow-codex/# codex wrapper
├── tests/                    # 251 integration tests
├── docs/
│   ├── adr/                  # 10 ADRs
│   ├── audits/               # Audit reports
│   ├── benchmarks/           # Node vs Rust reports
│   └── plans/                # Implementation plans
├── scripts/                  # Fixture capture + verification
├── .github/workflows/        # CI (Windows cross-compile)
└── Cargo.toml                # Workspace root
```

## License

MIT OR Apache-2.0

---

<div align="center">

**Built with ❤️ by the rUv.io ecosystem**

[Original TypeScript project](https://github.com/ruvnet/ruflo) · [RuvNet](https://ruv.io)

</div>
