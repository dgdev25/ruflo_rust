# Benchmark: Node ruflo (V8 + napi-rs) vs ruflo-rust (native binary)

**Date:** 2026-08-09
**Machine:** Linux 7.0.0-28-generic, x86_64
**Node:** v24.18.0
**Rust:** release profile, optimized

## Methodology

- **What was tested:** wall-clock time per CLI invocation for 6 representative
  commands spanning startup-dominated and compute-bound workloads.
- **How:** bash script (`scripts/bench-node-vs-rust.sh`) runs each command 10
  times via `date +%s%N` nanosecond timing, reports min/avg/max milliseconds.
- **Node CLI:** `node v3/@claude-flow/cli/bin/cli.js` (compiled dist/, pnpm
  install, full node_modules).
- **Rust CLI:** `target/release/ruflo` (release profile, LTO, 8.3 MB binary).
- **Fair comparison note:** Node's compute hot-paths (ONNX, HNSW) are
  accelerated via napi-rs (native Rust addons). The gap is primarily V8
  startup + JS module loading overhead (~120ms floor), not algorithmic speed.

## Results

### Startup-dominated (V8 init + module loading)

| Command | Node avg (ms) | Rust avg (ms) | Speedup |
|---------|:---:|:---:|:---:|
| `--version` | 27 | 9 | **3x** |
| `security` (overview) | 127 | 10 | **13x** |
| `analyze` (overview) | 127 | 9 | **14x** |

### Compute (napi-rs vs native)

| Command | Node avg (ms) | Rust avg (ms) | Speedup |
|---------|:---:|:---:|:---:|
| `embeddings generate -t "..."` | 271 | 8 | **34x** |
| `security scan -t <dir> --depth quick` | 131 | 8 | **16x** |

### State operations

| Command | Node avg (ms) | Rust avg (ms) | Speedup |
|---------|:---:|:---:|:---:|
| `memory store --key k --value v` | 142 | 9 | **16x** |

### Footprint

| Metric | Node | Rust | Ratio |
|--------|:---:|:---:|:---:|
| node_modules / binary | 1.9 GB | 8.3 MB | **230x smaller** |

## Interpretation

- **Startup/dispatch** (overview, memory store): workload identical in both
  CLIs (print text / write JSON). The 13-16x delta is purely V8 overhead —
  JS engine init + TypeScript module resolution + import loading (~120ms
  floor). Rust eliminates this entirely (~8ms).
- **Compute** (embeddings): not fully apples-to-apples. TS loads an ONNX model
  via napi-rs (~250ms model-load time). Rust uses a deterministic hash
  vectorizer (no model — ADR-0005 forbids ONNX runtime natively). The 34x
  includes model-load time that napi-rs cannot eliminate.
- **napi-rs caveat:** napi-rs accelerates the compute loop but does NOT
  eliminate V8 startup or JS module loading. For CLI tools invoked many times
  (hooks, CI, scripts), the ~120ms per-invocation floor compounds. A Rust
  native binary eliminates it.
- **Footprint:** 1.9 GB node_modules vs 8.3 MB binary = 230x smaller. Matters
  for Docker images, CI runners, distribution.
