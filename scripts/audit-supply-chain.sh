#!/usr/bin/env bash
set -uo pipefail

workspace_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$workspace_root"

status=0

log() {
  printf '%s\n' "$*" >&2
}

fail() {
  log "FAIL: $*"
  status=1
}

pass() {
  log "PASS: $*"
}

warn() {
  log "WARN: $*"
}

run_check() {
  local label="$1"
  shift
  if "$@"; then
    pass "$label"
  else
    fail "$label"
  fi
}

require_cmd() {
  command -v "$1" >/dev/null 2>&1
}

python_cmd() {
  if require_cmd python3; then
    printf '%s\n' python3
  elif require_cmd python; then
    printf '%s\n' python
  else
    return 1
  fi
}

run_check "Cargo.lock exists" test -f Cargo.lock
run_check "deny.toml exists" test -f deny.toml
run_check "supply-chain policy doc exists" test -f docs/security/supply-chain-policy.md
run_check "python is available for policy validation" python_cmd

if pybin="$(python_cmd)"; then
  if "$pybin" - <<'PY'
from __future__ import annotations

import json
import subprocess
import sys
import tomllib
from pathlib import Path

root = Path.cwd()
deny = tomllib.loads((root / "deny.toml").read_text())
doc = (root / "docs/security/supply-chain-policy.md").read_text()

errors: list[str] = []

sources = deny.get("sources", {})
licenses = deny.get("licenses", {})
advisories = deny.get("advisories", {})

if sources.get("unknown-registry") != "deny":
    errors.append("deny.toml must set sources.unknown-registry = \"deny\"")
if sources.get("unknown-git") != "deny":
    errors.append("deny.toml must set sources.unknown-git = \"deny\"")
if sources.get("required-git-spec") != "rev":
    errors.append("deny.toml must require rev-pinned git sources")

allowed_git = set(sources.get("allow-git", []))
if "https://github.com/ruvnet/RuVector" not in allowed_git:
    errors.append("deny.toml must allow the public pinned RuVector git source")

approved_ids = set(licenses.get("allow", []))
if "Apache-2.0" not in approved_ids or "MIT" not in approved_ids:
    errors.append("deny.toml license allowlist is missing expected permissive IDs")

def documented(needle: str) -> bool:
    return needle in doc

for entry in advisories.get("ignore", []):
    if isinstance(entry, dict):
        advisory_id = entry.get("id") or entry.get("advisory") or entry.get("crate")
    else:
        advisory_id = str(entry)
    if advisory_id and not documented(advisory_id):
        errors.append(f"undocumented advisory exception: {advisory_id}")

for entry in licenses.get("exceptions", []):
    crate = entry.get("crate") if isinstance(entry, dict) else str(entry)
    if crate and not documented(crate):
        errors.append(f"undocumented license exception: {crate}")

metadata = json.loads(
    subprocess.check_output(
        ["cargo", "metadata", "--format-version", "1", "--locked"],
        text=True,
    )
)

allowed_registry = "registry+https://github.com/rust-lang/crates.io-index"
allowed_git_prefix = "git+https://github.com/ruvnet/RuVector.git?rev="
supported_targets = [
    "x86_64-unknown-linux-gnu",
    "aarch64-unknown-linux-gnu",
    "x86_64-apple-darwin",
    "aarch64-apple-darwin",
    "x86_64-pc-windows-msvc",
]

allowed_ids_for_targets: set[str] = set()
for target in supported_targets:
    filtered = json.loads(
        subprocess.check_output(
            [
                "cargo",
                "metadata",
                "--format-version",
                "1",
                "--locked",
                "--filter-platform",
                target,
            ],
            text=True,
        )
    )
    resolve = filtered.get("resolve") or {}
    if resolve.get("root"):
        allowed_ids_for_targets.add(resolve["root"])
    for node in resolve.get("nodes", []):
        allowed_ids_for_targets.add(node["id"])
        allowed_ids_for_targets.update(node.get("dependencies", []))

def normalize_expression(expr: str) -> str:
    return (
        expr.replace("MIT/Apache-2.0", "MIT OR Apache-2.0")
        .replace("Apache-2.0/MIT", "Apache-2.0 OR MIT")
        .replace("(", " ")
        .replace(")", " ")
    )

def expression_ids(expr: str) -> list[str]:
    parts = normalize_expression(expr).split()
    ids: list[str] = []
    idx = 0
    while idx < len(parts):
        token = parts[idx]
        if token in {"AND", "OR"}:
            idx += 1
            continue
        if idx + 2 < len(parts) and parts[idx + 1] == "WITH":
            ids.append(f"{parts[idx]} WITH {parts[idx + 2]}")
            idx += 3
            continue
        ids.append(token)
        idx += 1
    return ids

for package in metadata["packages"]:
    if package["id"] not in allowed_ids_for_targets and package["id"] not in metadata["workspace_members"]:
        continue
    source = package.get("source")
    if source is None:
        continue
    if source == allowed_registry:
        pass
    elif source.startswith(allowed_git_prefix):
        pass
    else:
        errors.append(
            f"unapproved source for {package['name']} {package['version']}: {source}"
        )

    license_expr = package.get("license")
    if not license_expr:
        errors.append(
            f"missing SPDX license expression for {package['name']} {package['version']}"
        )
        continue
    for identifier in expression_ids(license_expr):
        if identifier not in approved_ids:
            errors.append(
                f"unapproved SPDX identifier for {package['name']} {package['version']}: {identifier}"
            )

if errors:
    for error in errors:
        print(error, file=sys.stderr)
    sys.exit(1)
PY
  then
    pass "policy, source, and license fallback validation"
  else
    fail "policy, source, and license fallback validation"
  fi
fi

if require_cmd cargo; then
  audit_db_dir="${RUFLO_AUDIT_DB_DIR:-$workspace_root/target/supply-chain/advisory-db-${GITHUB_RUN_ID:-local}}"
  # cargo-audit initializes this exact directory as a git clone. Creating the
  # clone destination first makes first-run audits fail, particularly when a
  # build cache later restores the empty directory.
  mkdir -p "$(dirname "$audit_db_dir")"
  audit_args=(cargo audit --db "$audit_db_dir" --deny warnings)
  if [[ "${RUFLO_AUDIT_OFFLINE:-0}" == "1" ]]; then
    audit_args+=(--no-fetch --stale)
  fi
  if "${audit_args[@]}"; then
    pass "cargo audit"
  else
    fail "cargo audit (use a writable advisory DB; if offline, pre-seed $audit_db_dir and set RUFLO_AUDIT_OFFLINE=1)"
  fi
else
  fail "cargo is not available"
fi

if require_cmd cargo-deny; then
  if cargo deny check advisories bans licenses sources; then
    pass "cargo deny"
  else
    fail "cargo deny"
  fi
else
  # cargo-deny is a stricter policy layer atop cargo-audit. Its absence is an
  # environment-readiness gap (the tool isn't installed), not a supply-chain
  # finding — warn so the gate distinguishes "tool missing" from "check failed".
  warn "cargo-deny is not installed (bootstrap with: cargo install --locked --version 0.20.2 cargo-deny); skipping deny policy check (cargo-audit above is the authoritative vuln scan)"
fi

exit "$status"
