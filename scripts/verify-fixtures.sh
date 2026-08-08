#!/usr/bin/env bash
set -euo pipefail

fixtures_root="tests/fixtures"

test -d "$fixtures_root"
test -f "$fixtures_root/cli/version.json"
test -f "$fixtures_root/cli/help.json"
test -f "$fixtures_root/cli/config/v3.json"
test -f "$fixtures_root/cli/cleanup/v3.json"
test -f "$fixtures_root/cli/transport/v3.json"
test -f "$fixtures_root/cli/deployment/v3.json"
test -f "$fixtures_root/mcp/tools-list.json"
test -f "$fixtures_root/persistence/README.md"

cargo test --test fixture_schema
cargo test --test differential_cli
cargo test --test config_command
cargo test --test cleanup_command
cargo test --test transport_command
cargo test --test deployment_command

if rg -n '/home/[^"/]+|/Users/[^"/]+|[A-Za-z]:\\\\Users\\\\[^"\\]+' "$fixtures_root"; then
  echo "fixture verification failed: absolute user path detected" >&2
  exit 1
fi

if rg -n 'ghp_[A-Za-z0-9]{20,}|sk-[A-Za-z0-9]{20,}|AIza[0-9A-Za-z_-]{20,}|-----BEGIN [A-Z ]*PRIVATE KEY-----|xox[baprs]-[A-Za-z0-9-]{10,}' "$fixtures_root"; then
  echo "fixture verification failed: secret-like value detected" >&2
  exit 1
fi

echo "fixture verification passed"
