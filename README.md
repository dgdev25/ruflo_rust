<div align="center">

# Ruflo — Rust Foundations for AI Orchestration

**A Rust foundation for custom Ruflo implementations, adapters, and reusable components—not a drop-in Node replacement.**

[![Rust](https://img.shields.io/badge/Rust-1.97+-orange.svg)](https://www.rust-lang.org)
[![License](https://img.shields.io/badge/license-MIT%2FApache--2.0-blue.svg)](#license)
[![Parity](https://img.shields.io/badge/native%20parity-partial-yellow.svg)](docs/capabilities/native-capability-manifest.json)
[![MCP Tools](https://img.shields.io/badge/MCP-4%20typed%20tools-blue.svg)](#mcp-tools)
[![N-API](https://img.shields.io/badge/N--API-addon%20included-blue.svg)](#n-api-addon-and-release-status)

</div>

---

## What is this?

Ruflo is a Rust codebase for building native Ruflo-derived applications, adapters, and experiments. It contains a reference CLI, reusable core and persistence crates, a typed MCP boundary, and an optional napi-rs addon for selected deterministic core operations.

It is **not** a drop-in replacement for the original [Node Ruflo](https://github.com/ruvnet/ruflo) application. The checked capability manifest claims **partial native parity**: unsupported CLI and MCP operations fail explicitly instead of returning success-shaped placeholder results.

Use this repository when you want to build a Rust-native variant around the contracts that are already implemented, or reuse its components inside another Rust application. Keep the Node implementation for workflows whose contract is not explicitly verified here.

## Build on it

The crates are designed as implementation building blocks, not as a finished end-user replacement product.

| Crate | Use it to build |
|-------|-----------------|
| `ruflo-core` | Deterministic embedding, similarity, and routing operations without CLI, process, or Node dependencies |
| `ruflo-storage` | SQLite metadata persistence and RVF adapter integration |
| `ruflo-memory` | A semantic-memory facade and hybrid retrieval policy over the storage layer |
| `ruflo-mcp` | A deliberately small typed MCP server boundary |
| `ruflo-napi` | A Node addon exposing selected `ruflo-core` operations in-process |
| `ruflo-cli` | A reference native CLI to extend, constrain, or use as an integration target |

These crates are currently `publish = false`. Reuse them from this workspace or a fork/local path dependency; do not assume a stable crates.io API or published npm distribution.

When creating another Rust version of Ruflo, start by depending on or adapting the smallest relevant crate, then add your own command surface and tests. Preserve the capability manifest discipline: only advertise a command, MCP tool, or migration path after its consumer contract is verified.

## Benchmark status

The former Node-versus-Rust speed and size figures are retracted. They timed unequal implementations and incorrectly described ordinary Node CLI paths as Ruflo N-API calls.

Use the [benchmark methodology](docs/benchmarks/node-vs-rust-methodology.md) and `scripts/bench-node-vs-rust.sh` for equivalent CLI measurements. The N-API suite measures only deterministic core functions through the real addon; it does not establish semantic-memory or full CLI parity.

## Quick Start

```bash
# Build
cargo build --release --locked

# Check the native CLI
./target/release/ruflo --version

# Start the MCP server (four typed, implemented tools)
./target/release/ruflo mcp start

# Inspect the capability boundary before relying on a Node-era command
cat docs/capabilities/native-capability-manifest.json
```

See the [parity remediation checklist](docs/plans/parity-remediation-checklist.md) for the current verified and unproven contracts.

## Architecture

### Rust workspace

| Crate | Purpose |
|-------|---------|
| `ruflo-cli` | Native command dispatcher and CLI services |
| `ruflo-core` | Process-independent deterministic core operations |
| `ruflo-napi` | Thin napi-rs `cdylib` adapter over `ruflo-core` |
| `ruflo-mcp` | MCP dispatcher for four typed native tools |
| `ruflo-storage` | SQLite + RuVector RVF (HNSW vector search) |
| `ruflo-runtime` | Transport layer (stdio, HTTP, NoSlim) |
| `ruflo-config` | Effective config + policy engine (ADR-324) |
| `ruflo-types` | Shared types + error taxonomy |
| `ruflo-actions` | GitHub Actions + CI helpers |
| `ruflo-codex-cli` | Codex subprocess bridge |
| `ruflo-memory` | Hybrid retrieval policy primitives |

### Native implementation boundaries

| Area | Native implementation |
|--------------|-----------------|
| Persistent metadata | `rusqlite` over the compatible `memory_entries` schema |
| Native vector index | RuVector `.rvf` via pinned adapters |
| BGE semantic path | Native tokenizer, query prefix, CLS pooling, and hybrid reranking policy |
| Node integration | napi-rs addon for deterministic `ruflo-core` functions |
| MCP | Four typed tools with JSON schemas and explicit unsupported errors |

### N-API addon and release status

- `packages/ruflo-native` contains the JavaScript loader and TypeScript declarations for `@ruflo/native`.
- The addon is built from `ruflo-napi` and has Node ABI and equality-gated benchmark coverage.
- Five native release targets are configured: Linux x86_64/aarch64, macOS x86_64/aarch64, and Windows x86_64.
- A tagged, five-target release and published per-platform npm packages have not yet supplied distribution proof. Private-repository releases upload archives, checksums, and SPDX SBOMs but cannot use GitHub artifact attestations.

Read [platform support](docs/release/platform-support.md) before treating a target as release-verified.

## MCP Tools

The native MCP server advertises only the four implemented, typed contracts:

| Tool | Backend |
|--------|-------|---------|
| `agent_spawn` | Native tracked-agent response |
| `memory_store` | Persistent SQLite memory entry |
| `memory_retrieve` | Persistent SQLite lookup |
| `memory_search` | Keyword fallback search |

Historical catalog names that do not have an equivalent native handler return a deterministic `tool.unsupported` error. They are not advertised as available.

## Testing

```bash
# Run the workspace suite
cargo test --workspace --locked

# Enforce the capability claim gate
bash scripts/verify-capability-manifest.sh

# Release build
cargo build --release --locked

# Windows cross-compile
cargo build --target x86_64-pc-windows-gnu -p ruflo --no-default-features
```

The capability gate prevents a `full-native-parity` claim while any registered capability is unproven. It is a truthfulness gate, not proof that every Node feature has been ported.

## Commands

The CLI includes native command families and deterministic unsupported-command errors. Do not infer Node feature parity from a command name alone. Key native surfaces include:

| Command | Description |
|---------|-------------|
| `memory store/retrieve/search` | Durable memory operations; semantic search uses BGE/RVF only after an explicit compatible index rebuild |
| `memory rebuild-index` | Re-embeds active entries into the native RVF index |
| `mcp start` | Stdio MCP server for the typed tools listed above |
| `embeddings ingest/search` | Native RVF-backed vector ingestion and search |
| `auth` | Session-only token handling; no project-local credential persistence |
| `providers test` | Bounded provider connectivity classifications |

See the [parity checklist](docs/plans/parity-remediation-checklist.md) and [audit](docs/audits/audit-2026-08-09-3.md) for contract-level evidence and remaining gaps.

## Windows

Windows GNU cross-compilation is checked without ONNX:

```bash
cargo build --target x86_64-pc-windows-gnu -p ruflo --no-default-features
```

The native release workflow builds a separate Windows MSVC archive and addon. See [platform support](docs/release/platform-support.md) for the five-target matrix and its evidence requirements.

## ADRs

11 Architecture Decision Records in `docs/adr/`:

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
| 0010 | Implemented | Native-only CLI rebuild boundary |
| 0011 | Implemented | Process-independent core through N-API |

## Project Structure

```
ruflo_rust/
├── src/
│   ├── crates/
│   │   ├── ruflo-cli/        # Commands, services, SONA, swarm, security
│   │   ├── ruflo-core/       # Process-independent core operations
│   │   ├── ruflo-napi/       # napi-rs cdylib adapter
│   │   ├── ruflo-mcp/        # Typed MCP dispatcher and policy
│   │   ├── ruflo-storage/    # SQLite, RuVector RVF adapter
│   │   ├── ruflo-runtime/    # Transport (stdio, HTTP)
│   │   ├── ruflo-config/     # Config + policy engine
│   │   ├── ruflo-types/      # Shared types + errors
│   │   ├── ruflo-actions/    # GitHub Actions
│   │   ├── ruflo-codex-cli/  # Codex bridge
│   │   └── ruflo-memory/     # Hybrid retrieval policy
│   └── bin/
│       ├── ruflo/            # Main binary
│       ├── claude-flow/      # claude-flow wrapper
│       └── claude-flow-codex/# codex wrapper
├── packages/ruflo-native/    # JavaScript loader and addon declarations
├── tests/                    # Contract, migration, RVF, and platform tests
├── docs/
│   ├── adr/                  # 11 ADRs
│   ├── audits/               # Audit reports
│   ├── benchmarks/           # Node vs Rust reports
│   └── plans/                # Implementation plans
├── scripts/                  # Fixture capture + verification
├── .github/workflows/        # CI and native-release matrix
└── Cargo.toml                # Workspace root
```

## License

MIT OR Apache-2.0

---

<div align="center">

**Built with ❤️ on the shoulders of giants rUv and the ecosystem he has developed**

[Original TypeScript project](https://github.com/ruvnet/ruflo) · [RuvNet](https://ruv.io)

</div>
