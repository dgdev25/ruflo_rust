# Node vs Rust Benchmark Report — Retracted

**Status:** Retracted on 2026-08-09. Do not use the previous speedup or size
figures for product, technical, or marketing claims.

The former report mixed CLI cold-start timing with unequal implementations:

- 24 of 34 commands had different output and six had different exit codes.
- The runner discarded command failures, so a failed invocation could still be
  timed and reported.
- Embeddings used different backends and workloads (Node model loading versus a
  Rust hash vectorizer).
- The report described the exercised Node paths as NAPI-backed, but the Node
  checkout only resolves `@napi-rs/keyring`; no Ruflo NAPI addon is available
  for the benchmarked operations.
- Its 8.5 MB Rust binary-size claim is stale; the verified binary used in the
  audit is 39 MB.

Use [the benchmark methodology](node-vs-rust-methodology.md) and
`scripts/bench-node-vs-rust.sh` for future measurements. The harness records
raw samples, refuses non-equivalent commands, and reports its scope explicitly.
