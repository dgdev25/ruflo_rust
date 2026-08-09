#!/usr/bin/env bash
# Reproducible end-to-end CLI benchmark for commands with exact parity.
#
# This deliberately measures process startup, dispatch, and command execution.
# It is not a native-compute or N-API benchmark: those require the same Rust
# implementation to be invoked through a real napi-rs addon and directly from
# Rust, which this repository does not yet provide.
set -euo pipefail

ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
NODE_BIN=${NODE_BIN:-node}
NODE_CLI=${NODE_CLI:-/mnt/datadisk/dev/ruflo/v3/@claude-flow/cli/bin/cli.js}
RUST_BIN=${RUST_BIN:-"$ROOT/target/release/ruflo"}
ITERS=${BENCH_ITERS:-30}
WARMUP=${BENCH_WARMUP:-5}
OUTPUT=${BENCH_OUTPUT:?Set BENCH_OUTPUT to a JSON path for the raw measurements.}

[[ "$ITERS" =~ ^[1-9][0-9]*$ ]] || { echo "BENCH_ITERS must be a positive integer" >&2; exit 2; }
[[ "$WARMUP" =~ ^[0-9]+$ ]] || { echo "BENCH_WARMUP must be a non-negative integer" >&2; exit 2; }
command -v "$NODE_BIN" >/dev/null || { echo "Node executable not found: $NODE_BIN" >&2; exit 2; }
[[ -f "$NODE_CLI" ]] || { echo "Node CLI not found: $NODE_CLI" >&2; exit 2; }
[[ -x "$RUST_BIN" ]] || { echo "Rust binary not executable: $RUST_BIN" >&2; exit 2; }

mkdir -p "$(dirname "$OUTPUT")"
WORKDIR=$(mktemp -d)
trap 'rm -rf "$WORKDIR"' EXIT
NODE_CMD=("$NODE_BIN" "$NODE_CLI")
RUST_CMD=("$RUST_BIN")
CASE_FILE="$WORKDIR/cases.jsonl"

median() {
  local file=$1 count
  count=$(wc -l < "$file")
  awk -v n="$count" '
    { values[NR] = $1 }
    END {
      if (n % 2) print values[(n + 1) / 2];
      else print (values[n / 2] + values[n / 2 + 1]) / 2;
    }
  ' "$file"
}

p95() {
  local file=$1 count rank
  count=$(wc -l < "$file")
  rank=$(( (95 * count + 99) / 100 ))
  sed -n "${rank}p" "$file"
}

mean() {
  awk '{ total += $1 } END { if (NR) printf "%.3f", total / NR; else print 0 }' "$1"
}

measure() {
  local samples=$1
  shift
  : > "$samples"
  for _ in $(seq 1 "$WARMUP"); do "$@" >/dev/null 2>&1; done
  for _ in $(seq 1 "$ITERS"); do
    local started finished
    started=$(date +%s%N)
    "$@" >/dev/null 2>&1
    finished=$(date +%s%N)
    printf '%s\n' "$(( (finished - started) / 1000000 ))" >> "$samples"
  done
  sort -n "$samples" -o "$samples"
}

verify_contract() {
  local name=$1
  shift
  local node_stdout="$WORKDIR/${name}.node.stdout"
  local rust_stdout="$WORKDIR/${name}.rust.stdout"
  local node_stderr="$WORKDIR/${name}.node.stderr"
  local rust_stderr="$WORKDIR/${name}.rust.stderr"
  "${NODE_CMD[@]}" "$@" >"$node_stdout" 2>"$node_stderr"
  "${RUST_CMD[@]}" "$@" >"$rust_stdout" 2>"$rust_stderr"
  if ! cmp -s "$node_stdout" "$rust_stdout" || ! cmp -s "$node_stderr" "$rust_stderr"; then
    echo "Contract mismatch for case '$name'; refusing to benchmark non-equivalent commands." >&2
    diff -u "$node_stdout" "$rust_stdout" >&2 || true
    exit 1
  fi
}

run_case() {
  local name=$1
  shift
  local node_samples="$WORKDIR/${name}.node.ms"
  local rust_samples="$WORKDIR/${name}.rust.ms"
  verify_contract "$name" "$@"
  measure "$node_samples" "${NODE_CMD[@]}" "$@"
  measure "$rust_samples" "${RUST_CMD[@]}" "$@"
  printf '{"name":"%s","contract":"exact stdout and stderr","node":{"samples_ms":[' "$name" >> "$CASE_FILE"
  awk 'BEGIN { separator = "" } { printf "%s%s", separator, $1; separator = "," }' "$node_samples" >> "$CASE_FILE"
  printf '],"median_ms":%s,"p95_ms":%s,"mean_ms":%s},"rust":{"samples_ms":[' \
    "$(median "$node_samples")" "$(p95 "$node_samples")" "$(mean "$node_samples")" >> "$CASE_FILE"
  awk 'BEGIN { separator = "" } { printf "%s%s", separator, $1; separator = "," }' "$rust_samples" >> "$CASE_FILE"
  printf '],"median_ms":%s,"p95_ms":%s,"mean_ms":%s}}\n' \
    "$(median "$rust_samples")" "$(p95 "$rust_samples")" "$(mean "$rust_samples")" >> "$CASE_FILE"
}

# These commands have byte-for-byte output parity in the current checkouts.
# Add a case only after its semantic contract is verified, rather than timing a
# command that happens to share an exit code.
run_case version --version
run_case completions-bash completions bash

{
  printf '{\n'
  printf '  "schema_version": 1,\n'
  printf '  "kind": "end_to_end_cli",\n'
  printf '  "measured_at_utc": "%s",\n' "$(date -u +%FT%TZ)"
  printf '  "iterations": %s,\n' "$ITERS"
  printf '  "warmup_iterations": %s,\n' "$WARMUP"
  printf '  "methodology": {"contract":"exit 0 and exact stdout/stderr before timing","scope":"cold process startup plus dispatch and command execution","napi_native_compute":"not measured: this checkout has no Ruflo napi-rs addon"},\n'
  printf '  "cases": ['
  paste -sd, "$CASE_FILE"
  printf ']\n}\n'
} > "$OUTPUT"

echo "Wrote verified end-to-end CLI measurements to $OUTPUT"
