# Node vs Rust End-to-End Benchmark Report

**Date:** 2026-08-09
**Node CLI:** `ruflo v3.34.0` (Node.js)
**Rust CLI:** `ruflo v3.34.0` (native Rust)
**Machine:** Linux 7.0.0-28-generic x86_64

## Tier 1: Differential Command Output

### Init

| Exit | Time | First Line | CLI |
|------|------|------------|-----|
| 0 | 795ms |  | Node |
| 0 | 4ms | RuFlo V3 initialized successfully! | Rust |

**Speedup:** 159.0x (Node 795ms → Rust 4ms)

### Command Output Parity

| Command | Node Exit | Rust Exit | Node ms | Rust ms | Speedup | Output Match |
|---------|-----------|-----------|---------|---------|---------|--------------|
| `status` | 0 | 0 | 488ms | 5ms | 81.3x | ⚠️ DIFF |
| `status --json` | 0 | 0 | 476ms | 4ms | 95.2x | ⚠️ DIFF |
| `version` | 0 | 0 | 154ms | 5ms | 25.6x | ✅ EXACT |
| `route task "fix the bug" --jso` | 0 | 0 | 156ms | 5ms | 26.0x | ⚠️ DIFF |
| `route task "write tests" --jso` | 0 | 0 | 160ms | 5ms | 26.6x | ⚠️ DIFF |
| `route task "security audit" --` | 0 | 0 | 160ms | 6ms | 22.8x | ⚠️ DIFF |
| `route list-agents` | 0 | 0 | 153ms | 4ms | 30.6x | ⚠️ DIFF |
| `neural predict -i "test input"` | 0 | 0 | 470ms | 5ms | 78.3x | ⚠️ DIFF |
| `neural status --json` | 0 | 0 | 654ms | 14ms | 43.6x | ⚠️ DIFF |
| `embeddings generate -t "hello ` | 0 | 0 | 760ms | 6ms | 108.5x | ⚠️ DIFF |
| `embeddings generate -t "machin` | 0 | 0 | 494ms | 4ms | 98.8x | ⚠️ DIFF |
| `security scan --json` | 0 | 0 | 153ms | 5ms | 25.5x | ⚠️ DIFF |
| `memory stats --json` | 0 | 0 | 493ms | 7ms | 61.6x | ⚠️ DIFF |
| `claims list --json` | 0 | 0 | 169ms | 5ms | 28.1x | ⚠️ DIFF |
| `providers list --json` | 0 | 0 | 206ms | 7ms | 25.7x | ⚠️ DIFF |
| `policy status --json` | 0 | 0 | 175ms | 5ms | 29.1x | ⚠️ DIFF |
| `completions bash` | 0 | 0 | 238ms | 4ms | 47.6x | ✅ EXACT |
| `analyze --json` | 0 | 1 | 173ms | 6ms | 24.7x | ❌ MISMATCH |
| `performance benchmark -s memor` | 0 | 0 | 815ms | 4ms | 163.0x | ⚠️ DIFF |
| `performance metrics` | 0 | 0 | 212ms | 7ms | 26.5x | ⚠️ DIFF |
| `agent list` | 0 | 0 | 177ms | 6ms | 25.2x | ⚠️ DIFF |
| `task list` | 0 | 0 | 172ms | 6ms | 24.5x | ⚠️ DIFF |
| `session list` | 0 | 0 | 168ms | 5ms | 28.0x | ⚠️ DIFF |
| `hooks list` | 0 | 0 | 172ms | 5ms | 28.6x | ⚠️ DIFF |
| `swarm status` | 0 | 0 | 156ms | 5ms | 26.0x | ⚠️ DIFF |
| `auth status` | 0 | 0 | 179ms | 5ms | 29.8x | ⚠️ DIFF |
| `guidance compile -r CLAUDE.md` | 1 | 1 | 178ms | 4ms | 35.6x | ⚠️ DIFF |
| `neural benchmark -e 10` | 1 | 0 | 194ms | 5ms | 32.3x | ❌ MISMATCH |

## Tier 2: MCP Server Parity

| Metric | Node | Rust |
|--------|------|------|
| MCP tools count | 356 | 347 |

### MCP tools/list Latency (5 runs)

| Run | Node ms | Rust ms |
|-----|---------|---------|
| 1 | 208ms | 111ms |
| 2 | 214ms | 106ms |
| 3 | 106ms | 105ms |
| 4 | 205ms | 109ms |
| 5 | 215ms | 118ms |

## Tier 3: Performance Benchmarks

### Cold Start (10 runs, p50)

| Metric | Node | Rust | Speedup |
|--------|------|------|---------|
| Cold start p50 | 158ms | 4ms | 31.6x |

### Memory Store → Search Roundtrip (100 entries)

| Operation | Node | Rust | Speedup |
|-----------|------|------|---------|
| Store 100 entries | 50885ms | 240ms | 211.1x |
| Search | 478ms | 5ms | 79.6x |

### Embeddings Generate (10 texts)

| Metric | Node | Rust | Speedup |
|--------|------|------|---------|
| 10 embeddings | 4886ms | 21ms | 222.0x |

### Binary Size

| CLI | Size |
|-----|------|
| Node (npm package) |  |
| Rust (release binary) | 39M |

## Tier 4: State-File Cross-Compatibility

| Direction | Key | Node Result | Rust Result |
|-----------|-----|-------------|-------------|
| Node→Rust read | cross-test | Transformers.js loaded: Xenova/all-MiniL | Key not found: cross-test |

## Tier 5: End-to-End Workflow Timing

| Step | Node ms | Rust ms | Speedup |
|------|---------|---------|---------|
| `init` | 202ms | 6ms | 28.8x |
| `status` | 503ms | 4ms | 100.6x |
| `route task "test bug" --j` | 158ms | 5ms | 26.3x |
| `neural status --json` | 487ms | 5ms | 81.1x |
| `memory stats --json` | 589ms | 5ms | 98.1x |
| `security scan --json` | 157ms | 5ms | 26.1x |
| `completions bash` | 164ms | 5ms | 27.3x |

---
**Report generated:** 2026-08-09
