#!/usr/bin/env bash
set -euo pipefail
if [[ -z "${BENCH_OUTPUT:-}" ]]; then echo "BENCH_OUTPUT is required" >&2; exit 2; fi
repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
iterations=${BENCH_ITERATIONS:-30}
warmups=${BENCH_WARMUPS:-5}
"$repo_root/scripts/build-napi.sh" >/dev/null
cargo build --manifest-path "$repo_root/Cargo.toml" -p ruflo-core --bin embed_bench >/dev/null
for ((index = 0; index < warmups; index++)); do "$repo_root/target/debug/embed_bench" 1 >/dev/null; node "$repo_root/packages/ruflo-native/test/benchmark.js" 1 >/dev/null; done
rust=$($repo_root/target/debug/embed_bench "$iterations")
node_result=$(node "$repo_root/packages/ruflo-native/test/benchmark.js" "$iterations")
node - "$rust" "$node_result" "$BENCH_OUTPUT" <<'NODE'
const [rustRaw, napiRaw, output] = process.argv.slice(2);
const rust = JSON.parse(rustRaw); const napi = JSON.parse(napiRaw);
for (const field of ['provider', 'dimensions', 'fingerprint']) if (rust[field] !== napi[field]) throw new Error(`contract mismatch for ${field}`);
require('node:fs').writeFileSync(output, `${JSON.stringify({ methodology: 'persistent-process N-API versus direct ruflo-core; equality is checked before timing', fixture: 'native addon parity benchmark fixture', warmups: Number(process.env.BENCH_WARMUPS ?? 5), samples: Number(process.env.BENCH_ITERATIONS ?? 30), direct_rust: rust, napi_node: napi }, null, 2)}\n`);
NODE
