#!/usr/bin/env bash
set -euo pipefail

manifest=${1:-docs/capabilities/native-capability-manifest.json}
test -f "$manifest"
python3 - "$manifest" <<'PY'
import json, sys
data = json.load(open(sys.argv[1], encoding="utf-8"))
if data.get("schemaVersion") != 1:
    raise SystemExit("invalid capability-manifest schema")
statuses = {item.get("status") for item in data.get("capabilities", [])}
if not data.get("capabilities") or not statuses <= {"verified", "unproven"}:
    raise SystemExit("invalid capability entries")
if data.get("claim") == "full-native-parity" and "unproven" in statuses:
    raise SystemExit("full parity claim has unproven capabilities")
print(f"capability manifest valid: {data['claim']}")
PY
