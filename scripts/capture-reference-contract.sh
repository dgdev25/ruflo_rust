#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
Usage:
  scripts/capture-reference-contract.sh [--replace] <fixture-path> -- <command> [args...]

Approved fixture paths:
  tests/fixtures/cli/**/*.json
EOF
}

if [[ $# -lt 3 ]]; then
  usage >&2
  exit 64
fi

replace=0
if [[ "${1:-}" == "--replace" ]]; then
  replace=1
  shift
fi

fixture_path="${1:-}"
shift

if [[ "${1:-}" != "--" ]]; then
  usage >&2
  exit 64
fi
shift

if [[ "$fixture_path" == /* || "$fixture_path" == ../* || "$fixture_path" == *"/../"* ]]; then
  echo "refusing unsafe fixture path: $fixture_path" >&2
  exit 65
fi

case "$fixture_path" in
  tests/fixtures/cli/*.json|tests/fixtures/cli/*/*.json|tests/fixtures/cli/*/*/*.json)
    ;;
  tests/fixtures/mcp/tools-list.json)
    echo "refusing to capture reduced-schema fixture with CLI recorder: $fixture_path" >&2
    exit 65
    ;;
  *)
    echo "refusing to capture unapproved fixture path: $fixture_path" >&2
    exit 65
    ;;
esac

if [[ -e "$fixture_path" && $replace -ne 1 ]]; then
  echo "refusing to overwrite existing fixture without --replace: $fixture_path" >&2
  exit 66
fi

mkdir -p "$(dirname "$fixture_path")"
tmpdir="$(mktemp -d)"
trap 'rm -rf "$tmpdir"' EXIT
stdout_file="$tmpdir/stdout"
stderr_file="$tmpdir/stderr"
recorded_at="$(date -u +"%Y-%m-%dT%H:%M:%SZ")"

set +e
"$@" >"$stdout_file" 2>"$stderr_file"
exit_code=$?
set -e

cargo run --quiet --bin fixture-capture -- \
  "$fixture_path" \
  "$stdout_file" \
  "$stderr_file" \
  "$exit_code" \
  "$recorded_at" \
  "$@"
