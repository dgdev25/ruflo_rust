# Node vs Rust Benchmark Methodology

## Status

This is the authoritative process for future Node versus Rust performance
claims. The earlier reports are retracted because they timed unequal commands
and labelled ordinary Node CLI paths as NAPI-backed.

## Two benchmarks, two claims

| Suite | What it measures | Valid claim |
|---|---|---|
| End-to-end CLI | A new process, runtime startup, command dispatch, and an exactly equivalent command | User-observed CLI latency |
| Native core through NAPI | The same Rust function invoked directly by Rust and through a published `napi-rs` addon in an already-running Node process | NAPI boundary and JavaScript-call overhead |

Neither suite substitutes for the other. NAPI reduces JavaScript-to-native work
for a supported function, but it does not remove the Node process startup cost.

## Current end-to-end suite

Run from this repository after building the release binary:

```bash
BENCH_OUTPUT=docs/benchmarks/results/node-vs-rust-$(date -u +%Y%m%dT%H%M%SZ).json \
  bash scripts/bench-node-vs-rust.sh
```

The runner performs five warmups and 30 measured iterations by default. Before
timing, it requires matching exit status, stdout, and stderr. It records every
sample plus median, p95, and mean in the JSON artifact. The current exact-parity
cases are `--version` and `completions bash`; adding a case requires a semantic
contract test first.

## NAPI acceptance criteria

No Ruflo NAPI addon currently exists in this checkout. A dependency on
`@napi-rs/keyring` is not evidence that Ruflo's CLI, embeddings, memory, or
security paths run through NAPI.

Before publishing a native-compute comparison, add a distributable `napi-rs`
package that calls a shared public Rust core API, then benchmark the exact same
input and result validation in both modes. Record addon version, platform,
Node version, Rust compiler, CPU governor, warmup count, and raw samples. The
NAPI suite must run in one persistent Node process; otherwise it is simply the
end-to-end CLI suite again.

## Prohibited comparisons

Do not calculate a speedup when implementations use different algorithms,
models, state paths, output schemas, or error outcomes. Examples from the
retracted report include ONNX model loading versus a hash vectorizer, and
commands that produced different output or failed on one side.
