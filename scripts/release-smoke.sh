#!/usr/bin/env bash
set -euo pipefail

local_only=0
artifact_dir=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --local)
      local_only=1
      shift
      ;;
    --artifact-dir)
      artifact_dir="${2:?missing artifact dir}"
      shift 2
      ;;
    *)
      echo "usage: scripts/release-smoke.sh [--local] [--artifact-dir <dir>]" >&2
      exit 2
      ;;
  esac
done

if [[ $local_only -eq 1 ]]; then
  cargo test --test platform_hooks
fi

if [[ -n "$artifact_dir" ]]; then
  test -d "$artifact_dir"
  test -f "$artifact_dir/ruflo${EXE_SUFFIX:-}"
  test -f "$artifact_dir/claude-flow${EXE_SUFFIX:-}"
  test -f "$artifact_dir/SHA256SUMS.sig"
  find "$artifact_dir" -maxdepth 1 -type f \( -name '*.sbom.spdx.json' -o -name '*.sbom.cdx.json' \) | grep -q .
elif [[ $local_only -eq 0 ]]; then
  echo "artifact presence checks require --artifact-dir" >&2
  exit 2
fi
