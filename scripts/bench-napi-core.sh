#!/usr/bin/env bash
set -euo pipefail
if [[ -z "${BENCH_OUTPUT:-}" ]]; then echo "BENCH_OUTPUT is required" >&2; exit 2; fi
repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
iterations=${BENCH_ITERATIONS:-30}
warmups=${BENCH_WARMUPS:-5}
profile=${BENCH_PROFILE:-release}
if [[ "$profile" != "debug" && "$profile" != "release" ]]; then echo "BENCH_PROFILE must be debug or release" >&2; exit 2; fi
NAPI_PROFILE="$profile" "$repo_root/scripts/build-napi.sh" >/dev/null
build_args=(build --manifest-path "$repo_root/Cargo.toml" -p ruflo-core --bin embed_bench)
if [[ "$profile" == "release" ]]; then build_args+=(--release); fi
cargo "${build_args[@]}" >/dev/null
bench_bin="$repo_root/target/$profile/embed_bench"
for ((index = 0; index < warmups; index++)); do "$bench_bin" 1 >/dev/null; node "$repo_root/packages/ruflo-native/test/benchmark.js" 1 >/dev/null; done
rust=$($bench_bin "$iterations")
node_result=$(node "$repo_root/packages/ruflo-native/test/benchmark.js" "$iterations")
node - "$rust" "$node_result" "$BENCH_OUTPUT" <<'NODE'
const [rustRaw, napiRaw, output] = process.argv.slice(2);
const rust = JSON.parse(rustRaw); const napi = JSON.parse(napiRaw);
for (const field of ['provider', 'dimensions', 'fingerprint']) if (rust[field] !== napi[field]) throw new Error(`contract mismatch for ${field}`);
if (!Array.isArray(napi.javascript_samples_ns) || napi.javascript_samples_ns.length !== napi.samples_ns.length) throw new Error('missing JavaScript reference samples');
const javascript_reference = { provider: napi.provider, dimensions: napi.dimensions, fingerprint: napi.fingerprint, samples_ns: napi.javascript_samples_ns };
require('node:fs').writeFileSync(output, `${JSON.stringify({ methodology: 'persistent-process direct ruflo-core, napi-rs, and a literal JavaScript port; vector fingerprint equality is checked before timing', fixture: 'native addon parity benchmark fixture', warmups: Number(process.env.BENCH_WARMUPS ?? 5), samples: Number(process.env.BENCH_ITERATIONS ?? 30), direct_rust: rust, napi_node: napi, javascript_reference }, null, 2)}\n`);
NODE
