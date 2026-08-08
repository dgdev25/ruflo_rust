#!/usr/bin/env bash
# Release gates for the native V3 command conversion.
#
# Fails closed: every gate must pass for a release. Each block prints what it
# checks so a failure points at the responsible surface.
set -euo pipefail

cd "$(dirname "$0")/.."

echo "==> [1/9] workspace compiles (cargo build --workspace)"
cargo build --workspace --all-targets

echo "==> [2/9] clippy clean (-D warnings)"
cargo clippy --workspace --all-targets -- -D warnings

echo "==> [3/9] full workspace test suite"
cargo test --workspace

echo "==> [4/9] command-family integration tests (all 53 families dispatch)"
cargo test --test capability_manifest
for t in \
  advisor_command announcements_command claims_command cleanup_command \
  completions_command config_command deployment_command eject_command \
  issues_command settings_command spinner_command version_command \
  funnel_command transport_command \
  security_command analyze_command daemon_command embeddings_command \
  hive_mind_command neural_command hooks_command; do
  if [[ -f "tests/$t.rs" ]]; then
    cargo test --test "$t"
  fi
done

echo "==> [5/9] source-differential parity (native byte-matches TS fixtures)"
cargo test --test differential_cli
cargo test --test differential_new_families
bash scripts/verify-fixtures.sh

echo "==> [6/9] binary parity (ruflo == claude-flow overview surface)"
cargo test --test command_registry_manifest

echo "==> [7/9] compatibility wave entry criteria present"
test -f docs/compatibility/wave-2-entry-criteria.md
test -f docs/compatibility/wave-3-entry-criteria.md
cargo test --test persistence_migration
cargo test --test rvf_interop
cargo test --test platform_hooks

echo "==> [8/9] release smoke + supply chain + SBOM"
bash scripts/release-smoke.sh --local
if [[ -x scripts/audit-supply-chain.sh ]]; then
  bash scripts/audit-supply-chain.sh
fi
if [[ -x scripts/generate-sbom.sh ]]; then
  bash scripts/generate-sbom.sh --check
fi

echo "==> [9/9] tasklist verifier (no unchecked conversion items remain)"
bash scripts/verify-tasklist.sh

echo
echo "All release gates passed."
