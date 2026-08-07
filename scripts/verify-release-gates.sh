#!/usr/bin/env bash
set -euo pipefail

test -f docs/compatibility/wave-2-entry-criteria.md
test -f docs/compatibility/wave-3-entry-criteria.md

cargo test --test capability_manifest
cargo test --test persistence_migration
cargo test --test rvf_interop
cargo test --test platform_hooks
bash scripts/release-smoke.sh --local
bash scripts/audit-supply-chain.sh
bash scripts/generate-sbom.sh --check
