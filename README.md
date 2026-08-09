# Ruflo — Native Rust AI Orchestration CLI

Zero Node.js dependency. Pure Rust implementation of the Ruflo/claude-flow
CLI — swarm coordination, neural learning (SONA), semantic memory (RuVector
HNSW), security scanning, and 347 MCP tools.

## Quick start

```bash
cargo build --release
./target/release/ruflo init          # set up .claude/ hooks + settings
./target/release/ruflo swarm start --objective "fix the bug" --workers 3
./target/release/ruflo mcp start     # start MCP server (347 tools)
```

## Architecture

- **9 crates:** ruflo-cli, ruflo-mcp, ruflo-storage, ruflo-runtime,
  ruflo-config, ruflo-types, ruflo-actions, ruflo-codex-cli, ruflo-memory
- **RuVector RVF:** in-RAM HNSW vector search via `rvf_adapter` (no
  hnswlib-node, no Pinecone, no server)
- **ONNX MiniLM:** real 384-dim embeddings via `ort` (no onnxruntime-node)
- **SONA:** full MLP backpropagation + EWC++ Fisher consolidation
- **Thompson-sampling bandit:** task→agent routing with Beta posteriors
- **tree-sitter:** AST analysis (rust/ts/python/go/java/c)
- **petgraph:** MinCut, Louvain communities, SCC, Dijkstra

## Key commands

| Command | Description |
|---------|-------------|
| `init` | Set up project (hooks, agents, settings, MCP config) |
| `swarm start` | Spawn N agent workers (claude/codex subprocess) |
| `route task` | Thompson-sampling task→agent routing |
| `neural train` | Train SONA MLP on router decisions |
| `neural predict` | Classify input via trained SONA |
| `memory search` | Semantic k-NN via RuVector HNSW |
| `embeddings search` | RVF-backed semantic similarity |
| `analyze boundaries` | Refactor seams (MinCut + Louvain) |
| `auth login` | OAuth PKCE flow (RFC 7636) |
| `security scan` | Regex-based secret/vuln detection |
| `transfer-store publish` | Native CIDv1 + IPFS gateway |

## Windows

Cross-compiles to `x86_64-pc-windows-gnu`. The `onnx-dynamic` feature
loads `onnxruntime.dll` at runtime — ship the DLL alongside `ruflo.exe`.

## ADRs

See `docs/adr/` (0001–0010). ADR-0008 supersedes ADR-0007 (native swarm).
ADR-0009 closes DiskANN (RVF HNSW sufficient). ADR-0010 records the
zero-Node-dependency completion.

## License

MIT OR Apache-2.0
