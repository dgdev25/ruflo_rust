#!/usr/bin/env bash
# Smoke test the benchmark's contract gate without either full checkout.
set -euo pipefail

ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
WORKDIR=$(mktemp -d)
trap 'rm -rf "$WORKDIR"' EXIT

cat > "$WORKDIR/node" <<'EOF'
#!/usr/bin/env bash
shift
case "${1:-} ${2:-}" in
  '--version ')
    if [[ ${BENCH_FAKE_MISMATCH:-0} == 1 ]]; then echo 'ruflo v0.0.0'; else echo 'ruflo v3.34.0'; fi
    ;;
  'completions bash') echo 'complete -W "ruflo" ruflo' ;;
  *) exit 2 ;;
esac
EOF
cat > "$WORKDIR/rust" <<'EOF'
#!/usr/bin/env bash
case "${1:-} ${2:-}" in
  '--version ') echo 'ruflo v3.34.0' ;;
  'completions bash') echo 'complete -W "ruflo" ruflo' ;;
  *) exit 2 ;;
esac
EOF
chmod +x "$WORKDIR/node" "$WORKDIR/rust"
touch "$WORKDIR/node-cli.js"

BENCH_ITERS=2 BENCH_WARMUP=1 NODE_BIN="$WORKDIR/node" NODE_CLI="$WORKDIR/node-cli.js" \
  RUST_BIN="$WORKDIR/rust" BENCH_OUTPUT="$WORKDIR/result.json" \
  bash "$ROOT/scripts/bench-node-vs-rust.sh"

grep -F '"kind": "end_to_end_cli"' "$WORKDIR/result.json" >/dev/null
grep -F '"name":"version"' "$WORKDIR/result.json" >/dev/null
grep -F '"name":"completions-bash"' "$WORKDIR/result.json" >/dev/null

if BENCH_FAKE_MISMATCH=1 BENCH_ITERS=1 BENCH_WARMUP=0 NODE_BIN="$WORKDIR/node" \
  NODE_CLI="$WORKDIR/node-cli.js" RUST_BIN="$WORKDIR/rust" \
  BENCH_OUTPUT="$WORKDIR/mismatch.json" bash "$ROOT/scripts/bench-node-vs-rust.sh" >/dev/null 2>&1; then
  echo "benchmark accepted a mismatched command contract" >&2
  exit 1
fi

echo "benchmark harness smoke test passed"
