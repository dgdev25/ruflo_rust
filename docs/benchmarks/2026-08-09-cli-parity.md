# Verified Node vs Rust CLI Parity Benchmark

**Date:** 2026-08-09
**Scope:** end-to-end command latency: process startup, dispatch, and command
execution. This is not a native-compute or NAPI benchmark.

The raw 30-sample artifact is
[2026-08-09-cli-parity.json](results/2026-08-09-cli-parity.json). Each case
was checked for exit status 0 and byte-for-byte equal stdout and stderr before
measurement, following the [benchmark methodology](node-vs-rust-methodology.md).

| Exact-parity command | Node median / p95 | Rust median / p95 |
|---|---:|---:|
| `--version` | 26 ms / 34 ms | 12.5 ms / 16 ms |
| `completions bash` | 144 ms / 158 ms | 13 ms / 16 ms |

These figures support only the stated end-to-end CLI latency claim. They do
not establish native algorithm speed, memory performance, embedding quality,
or a NAPI advantage. A separate NAPI suite is required once the same Ruflo
core function is shipped as a NAPI addon and is benchmarked in a persistent
Node process against the direct Rust call.
