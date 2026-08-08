#!/usr/bin/env bash
# Tasklist verifier — fails if any conversion item remains unchecked.
#
# The source of truth is docs/REMAINING_COMMAND_CONVERSIONS.md. Every
# `- [ ]` must be promoted to `- [x]` before release; this gate makes a
# forgotten checkbox a hard release block.
set -euo pipefail

cd "$(dirname "$0")/.."

doc="docs/REMAINING_COMMAND_CONVERSIONS.md"
if [[ ! -f "$doc" ]]; then
  echo "tasklist verifier: $doc not found" >&2
  exit 1
fi

# Count unchecked conversion checkboxes.
unchecked=$(grep -cE '^[[:space:]]*- \[ \]' "$doc" || true)
checked=$(grep -cE '^[[:space:]]*- \[x\]' "$doc" || true)

if (( unchecked > 0 )); then
  echo "tasklist verifier FAILED: $unchecked unchecked item(s) remain in $doc" >&2
  grep -nE '^[[:space:]]*- \[ \]' "$doc" >&2 || true
  echo >&2
  echo "Promote each to '- [x]' once the family has functional dispatch + tests." >&2
  exit 1
fi

echo "tasklist verifier passed: $checked item(s) checked, 0 unchecked."
