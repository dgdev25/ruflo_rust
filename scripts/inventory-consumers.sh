#!/usr/bin/env bash

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
MATRIX_PATH="$ROOT_DIR/docs/compatibility/contract-matrix.md"

usage() {
  cat <<'EOF'
Usage:
  scripts/inventory-consumers.sh --check

Validates the checked-in P0 contract matrix for Task 3.
EOF
}

trim() {
  sed 's/^[[:space:]]*//;s/[[:space:]]*$//'
}

strip_markdown_ticks() {
  local value="$1"
  if [[ "$value" == \`*\` ]]; then
    value="${value#\`}"
    value="${value%\`}"
  fi
  printf '%s' "$value"
}

validate_path() {
  local value="$1"
  if [[ "$value" = /* ]]; then
    [[ -e "$value" ]]
  else
    [[ -e "$ROOT_DIR/$value" ]]
  fi
}

check_matrix() {
  [[ -f "$MATRIX_PATH" ]] || {
    echo "missing matrix: $MATRIX_PATH" >&2
    return 1
  }

  local found_header=0
  local in_table=0
  local row_count=0
  local error_count=0

  while IFS= read -r line; do
    if [[ $found_header -eq 0 ]]; then
      if [[ "$line" =~ ^[[:space:]]*\|\ priority\ \|\ consumer\ \|\ invocation\ \|\ contract\ \|\ fixture\ \|\ blocker\ \|\ wave\ \|\ status\ \|\ owner\ \|\ evidence\ \|[[:space:]]*$ ]]; then
        found_header=1
        in_table=1
      fi
      continue
    fi

    if [[ $in_table -eq 1 && "$line" =~ ^[[:space:]]*\|[[:space:]]*- ]]; then
      continue
    fi

    if [[ $in_table -eq 1 && ! "$line" =~ ^[[:space:]]*\| ]]; then
      break
    fi

    [[ $in_table -eq 1 ]] || continue

    local raw="${line#|}"
    raw="${raw%|}"
    IFS='|' read -r c1 c2 c3 c4 c5 c6 c7 c8 c9 c10 <<<"$raw"

    local priority consumer invocation contract fixture blocker wave status owner evidence
    priority="$(printf '%s' "$c1" | trim)"
    consumer="$(printf '%s' "$c2" | trim)"
    invocation="$(printf '%s' "$c3" | trim)"
    contract="$(printf '%s' "$c4" | trim)"
    fixture="$(printf '%s' "$c5" | trim)"
    fixture="$(strip_markdown_ticks "$fixture")"
    blocker="$(printf '%s' "$c6" | trim)"
    wave="$(printf '%s' "$c7" | trim)"
    status="$(printf '%s' "$c8" | trim)"
    owner="$(printf '%s' "$c9" | trim)"
    evidence="$(printf '%s' "$c10" | trim)"

    row_count=$((row_count + 1))

    for pair in \
      "priority:$priority" \
      "consumer:$consumer" \
      "invocation:$invocation" \
      "contract:$contract" \
      "wave:$wave" \
      "status:$status" \
      "owner:$owner" \
      "evidence:$evidence"; do
      local name="${pair%%:*}"
      local value="${pair#*:}"
      if [[ -z "$value" || "$value" == "-" ]]; then
        echo "matrix row $row_count is missing required field: $name" >&2
        error_count=$((error_count + 1))
      fi
    done

    if [[ "$priority" == "P0" && "$fixture" == "-" && "$blocker" == "-" ]]; then
      echo "matrix row $row_count has neither fixture nor blocker" >&2
      error_count=$((error_count + 1))
    fi

    if [[ "$fixture" != "-" && -n "$fixture" ]] && ! validate_path "$fixture"; then
      echo "matrix row $row_count fixture path does not exist: $fixture" >&2
      error_count=$((error_count + 1))
    fi

    IFS=';' read -r -a evidence_paths <<<"$evidence"
    for path in "${evidence_paths[@]}"; do
      path="$(printf '%s' "$path" | trim)"
      [[ -n "$path" ]] || continue
      if ! validate_path "$path"; then
        echo "matrix row $row_count evidence path does not exist: $path" >&2
        error_count=$((error_count + 1))
      fi
    done
  done <"$MATRIX_PATH"

  if [[ $found_header -eq 0 ]]; then
    echo "did not find the expected matrix header in $MATRIX_PATH" >&2
    return 1
  fi

  if [[ $row_count -eq 0 ]]; then
    echo "matrix contains no data rows" >&2
    return 1
  fi

  if [[ $error_count -ne 0 ]]; then
    echo "contract matrix validation failed with $error_count error(s)" >&2
    return 1
  fi

  echo "contract matrix OK: $row_count row(s) validated"
}

case "${1:-}" in
  --check)
    check_matrix
    ;;
  -h|--help|"")
    usage
    [[ "${1:-}" == "" ]] && exit 1
    ;;
  *)
    usage >&2
    exit 1
    ;;
esac
