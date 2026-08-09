#!/usr/bin/env bash
# Benchmark: Node ruflo (TS + napi-rs) vs ruflo-rust (native binary).
# Fair: same commands, same workload, 10 iterations, nanosecond timing.
set -uo pipefail

NODE="node /mnt/datadisk/dev/ruflo/v3/@claude-flow/cli/bin/cli.js"
RUST="/mnt/datadisk/dev/ruflo_rust/target/release/ruflo"
ITERS=10
T=$(mktemp -d)
mkdir -p "$T/src"; echo 'const x="sk_live_1234567890abcdefghijkl";eval(y);' > "$T/src/app.js"

bench() {
  local label="$1"; shift
  local sum=0 min=999999 max=0
  for i in $(seq 1 $ITERS); do
    local t0=$(date +%s%N)
    "$@" >/dev/null 2>&1
    local t1=$(date +%s%N)
    local ms=$(( (t1 - t0) / 1000000 ))
    [ "$ms" -lt "$min" ] && min=$ms
    [ "$ms" -gt "$max" ] && max=$ms
    sum=$((sum + ms))
  done
  local avg=$((sum / ITERS))
  printf "  %-34s min=%-6s avg=%-6s max=%-6s ms\n" "$label" "$min" "$avg" "$max"
}

echo "================================================================"
echo "  Ruflo Benchmark: Node (V8+napi-rs) vs Rust (native binary)"
echo "  $ITERS iterations · wall-clock ms · $(date -u +%FT%TZ)"
echo "================================================================"
echo
echo "--- Startup-dominated (V8 init + module loading) ---"
bench "TS  --version"          $NODE --version
bench "Rust --version"         $RUST --version
bench "TS  security overview"  $NODE security
bench "Rust security overview" $RUST security
bench "TS  analyze overview"   $NODE analyze
bench "Rust analyze overview"  $RUST analyze
echo
echo "--- Compute (napi-rs vs native) ---"
bench "TS  embeddings gen"     $NODE embeddings generate -t "hello world benchmark test"
bench "Rust embeddings gen"    $RUST embeddings generate -t "hello world benchmark test"
bench "TS  security scan"      $NODE security scan -t "$T" --depth quick
bench "Rust security scan"     $RUST security scan -t "$T" --depth quick
echo
echo "--- State ops ---"
bench "TS  memory store"       $NODE memory store --key bench --value test --path "$T/m.db"
bench "Rust memory store"      $RUST memory store --key bench --value test --path "$T/m.db"
echo
echo "--- Footprint ---"
NODE_SIZE=$(du -sh /mnt/datadisk/dev/ruflo/v3/node_modules 2>/dev/null | cut -f1)
RUST_SIZE=$(ls -lh /mnt/datadisk/dev/ruflo_rust/target/release/ruflo | awk '{print $5}')
printf "  Node node_modules: %-8s\n" "$NODE_SIZE"
printf "  Rust binary:       %-8s\n" "$RUST_SIZE"
echo
echo "  napi-rs note: Node's compute hot-paths (ONNX, HNSW) are already"
echo "  native Rust via napi-rs. The gap above is V8 startup + JS module"
echo "  loading overhead — dominant for short-lived CLI invocations."
echo "================================================================"
rm -rf "$T"
