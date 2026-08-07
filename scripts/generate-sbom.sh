#!/usr/bin/env bash
set -euo pipefail

workspace_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$workspace_root"

artifact_dir=""
check_mode=0
output_name="ruflo.spdx.json"
target_triple=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --artifact-dir)
      artifact_dir="${2:?missing artifact dir}"
      shift 2
      ;;
    --output-name)
      output_name="${2:?missing output name}"
      shift 2
      ;;
    --check)
      check_mode=1
      shift
      ;;
    --target)
      target_triple="${2:?missing target triple}"
      shift 2
      ;;
    *)
      echo "usage: scripts/generate-sbom.sh [--check] [--target <triple>] [--artifact-dir <dir>] [--output-name <name>]" >&2
      exit 2
      ;;
  esac
done

tmp_dir="$(mktemp -d)"
trap 'rm -rf "$tmp_dir"' EXIT

python_cmd() {
  if command -v python3 >/dev/null 2>&1; then
    printf '%s\n' python3
  elif command -v python >/dev/null 2>&1; then
    printf '%s\n' python
  else
    return 1
  fi
}

pybin="$(python_cmd)"

metadata_json="$tmp_dir/metadata.json"
if [[ -n "$target_triple" ]]; then
  cargo metadata --format-version 1 --locked --filter-platform "$target_triple" > "$metadata_json"
else
  cargo metadata --format-version 1 --locked > "$metadata_json"
fi

default_output_dir="$workspace_root/target/supply-chain"
mkdir -p "$default_output_dir"
sbom_path="$default_output_dir/$output_name"

timestamp_epoch="${SOURCE_DATE_EPOCH:-}"
if [[ -z "$timestamp_epoch" ]]; then
  if git rev-parse HEAD >/dev/null 2>&1; then
    timestamp_epoch="$(git log -1 --format=%ct HEAD)"
  else
    timestamp_epoch="$(stat -c %Y Cargo.lock)"
  fi
fi

"$pybin" - "$metadata_json" "$sbom_path" "$timestamp_epoch" <<'PY'
from __future__ import annotations

import json
import re
import sys
import tomllib
from datetime import datetime, timezone
from pathlib import Path
from urllib.parse import quote

metadata_path = Path(sys.argv[1])
output_path = Path(sys.argv[2])
timestamp_epoch = int(sys.argv[3])

root = Path.cwd()
metadata = json.loads(metadata_path.read_text())
lock = tomllib.loads((root / "Cargo.lock").read_text())

workspace_members = set(metadata["workspace_members"])
packages = sorted(
    metadata["packages"],
    key=lambda pkg: (pkg["name"], pkg["version"], pkg["id"]),
)
package_index = {pkg["id"]: pkg for pkg in packages}

checksums: dict[tuple[str, str, str], str] = {}
for entry in lock.get("package", []):
    source = entry.get("source") or "path"
    checksum = entry.get("checksum")
    if checksum:
        checksums[(entry["name"], entry["version"], source)] = checksum

revision = "workspace"
git_head = root / ".git"
if git_head.exists():
    import subprocess

    try:
        revision = (
            subprocess.check_output(["git", "rev-parse", "HEAD"], text=True).strip()
        )
    except Exception:
        revision = "workspace"

created = datetime.fromtimestamp(timestamp_epoch, tz=timezone.utc).strftime(
    "%Y-%m-%dT%H:%M:%SZ"
)

def spdx_id(raw: str, idx: int) -> str:
    sanitized = re.sub(r"[^A-Za-z0-9.-]+", "-", raw).strip("-")
    return f"SPDXRef-Package-{idx:04d}-{sanitized}"

def source_url(source: str | None) -> str:
    if source is None:
        return "NOASSERTION"
    if source.startswith("registry+"):
        return source.removeprefix("registry+")
    if source.startswith("git+"):
        return source.removeprefix("git+").split("#", 1)[0]
    return source

def package_purl(name: str, version: str) -> str:
    return f"pkg:cargo/{quote(name, safe='')}" + f"@{quote(version, safe='')}"

spdx_ids: dict[str, str] = {}
document_packages = []
describes = []

for idx, package in enumerate(packages, start=1):
    source = package.get("source")
    pid = spdx_id(f"{package['name']}-{package['version']}", idx)
    spdx_ids[package["id"]] = pid
    pkg = {
        "SPDXID": pid,
        "name": package["name"],
        "versionInfo": package["version"],
        "downloadLocation": source_url(source),
        "filesAnalyzed": False,
        "licenseConcluded": "NOASSERTION",
        "licenseDeclared": package.get("license") or "NOASSERTION",
        "copyrightText": "NOASSERTION",
        "externalRefs": [
            {
                "referenceCategory": "PACKAGE-MANAGER",
                "referenceType": "purl",
                "referenceLocator": package_purl(package["name"], package["version"]),
            }
        ],
    }
    checksum = checksums.get((package["name"], package["version"], source or "path"))
    if checksum:
        pkg["checksums"] = [{"algorithm": "SHA256", "checksumValue": checksum}]
    document_packages.append(pkg)
    if package["id"] in workspace_members:
        describes.append(
            {
                "spdxElementId": "SPDXRef-DOCUMENT",
                "relationshipType": "DESCRIBES",
                "relatedSpdxElement": pid,
            }
        )

relationships = list(describes)
resolve = metadata.get("resolve") or {}
for node in resolve.get("nodes", []):
    source_id = spdx_ids.get(node["id"])
    if source_id is None:
        continue
    for dep_id in node.get("dependencies", []):
        target_id = spdx_ids.get(dep_id)
        if target_id is None:
            continue
        relationships.append(
            {
                "spdxElementId": source_id,
                "relationshipType": "DEPENDS_ON",
                "relatedSpdxElement": target_id,
            }
        )

document = {
    "spdxVersion": "SPDX-2.3",
    "dataLicense": "CC0-1.0",
    "SPDXID": "SPDXRef-DOCUMENT",
    "name": "ruflo-workspace",
    "documentNamespace": f"https://example.invalid/ruflo/spdx/{revision}",
    "creationInfo": {
        "created": created,
        "creators": ["Tool: scripts/generate-sbom.sh"],
    },
    "documentDescribes": [rel["relatedSpdxElement"] for rel in describes],
    "packages": document_packages,
    "relationships": relationships,
}

output_path.write_text(json.dumps(document, indent=2, sort_keys=False) + "\n")
PY

sha256_cmd=""
if command -v sha256sum >/dev/null 2>&1; then
  sha256_cmd="sha256sum"
elif command -v shasum >/dev/null 2>&1; then
  sha256_cmd="shasum -a 256"
else
  echo "missing sha256 tool: need sha256sum or shasum" >&2
  exit 1
fi

digest_path="${sbom_path}.sha256"
digest_line="$($sha256_cmd "$sbom_path")"
printf '%s\n' "$digest_line" > "$digest_path"

if [[ -n "$artifact_dir" ]]; then
  mkdir -p "$artifact_dir"
  artifact_sbom="$artifact_dir/$output_name"
  artifact_digest="$artifact_sbom.sha256"
  cp "$sbom_path" "$artifact_sbom"
  cp "$digest_path" "$artifact_digest"
fi

if [[ $check_mode -eq 1 ]]; then
  "$pybin" - "$sbom_path" "$digest_path" "$metadata_json" <<'PY'
from __future__ import annotations

import json
import sys
from pathlib import Path

sbom_path = Path(sys.argv[1])
digest_path = Path(sys.argv[2])
metadata_path = Path(sys.argv[3])

sbom = json.loads(sbom_path.read_text())
metadata = json.loads(metadata_path.read_text())

assert sbom["spdxVersion"] == "SPDX-2.3"
assert sbom["SPDXID"] == "SPDXRef-DOCUMENT"
assert sbom["packages"], "SBOM package list is empty"
assert len(sbom["packages"]) == len(metadata["packages"]), "SBOM package count drifted from cargo metadata"
assert digest_path.exists(), "digest file missing"
assert digest_path.read_text().strip(), "digest file is empty"
PY
fi
