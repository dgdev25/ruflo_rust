#!/usr/bin/env python3
"""Generate the Rust rebuild's top-level CLI registry manifest.

The owning TypeScript registry is intentionally supplied as an argument: it
lives in the source Ruflo checkout, not in this native rebuild repository.
"""

from __future__ import annotations

import argparse
import json
import re
import sys
from pathlib import Path


EXPECTED_COMMAND_COUNT = 53
CANONICAL_SOURCE = "v3/@claude-flow/cli/src/commands/index.ts"
DEFAULT_OUTPUT = Path("tests/fixtures/cli/command-registry.json")


def extract_registry(source: str) -> list[str]:
    loaders = re.search(
        r"const\s+commandLoaders\s*:[^=]+?=\s*\{(?P<body>.*?)^\};",
        source,
        flags=re.MULTILINE | re.DOTALL,
    )
    if loaders is None:
        raise ValueError("could not find the commandLoaders object")

    get_names = re.search(
        r"export\s+function\s+getCommandNames\s*\(\s*\)\s*:\s*string\[\]\s*\{"
        r"(?P<body>.*?)^\}",
        source,
        flags=re.MULTILINE | re.DOTALL,
    )
    if get_names is None:
        raise ValueError("could not find getCommandNames()")
    if not re.search(r"Object\.keys\(\s*commandLoaders\s*\)", get_names.group("body")):
        raise ValueError("getCommandNames() no longer enumerates commandLoaders")

    entry = re.compile(
        r"^\s*(?:'(?P<single>[^']+)'|\"(?P<double>[^\"]+)\"|"
        r"(?P<bare>[A-Za-z_$][\w$-]*))\s*:\s*\(\)\s*=>\s*import\(",
        flags=re.MULTILINE,
    )
    commands = [
        match.group("single") or match.group("double") or match.group("bare")
        for match in entry.finditer(loaders.group("body"))
    ]
    if len(commands) != len(set(commands)):
        raise ValueError("commandLoaders contains duplicate command names")
    if len(commands) != EXPECTED_COMMAND_COUNT:
        raise ValueError(
            f"expected {EXPECTED_COMMAND_COUNT} commandLoaders entries, found {len(commands)}"
        )
    return sorted(commands)


def build_manifest(source: str) -> dict[str, object]:
    commands = extract_registry(source)
    return {
        "schema_version": 1,
        "source": CANONICAL_SOURCE,
        "registry_symbol": "commandLoaders",
        "enumerator_symbol": "getCommandNames",
        "enumeration_expression": "Object.keys(commandLoaders)",
        "command_count": len(commands),
        "commands": commands,
    }


def serialized(manifest: dict[str, object]) -> str:
    return json.dumps(manifest, indent=2, ensure_ascii=False) + "\n"


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--source", required=True, type=Path)
    parser.add_argument("--output", type=Path, default=DEFAULT_OUTPUT)
    parser.add_argument(
        "--check",
        action="store_true",
        help="fail instead of writing when the committed manifest is stale",
    )
    args = parser.parse_args()

    try:
        expected = serialized(build_manifest(args.source.read_text(encoding="utf-8")))
    except (OSError, ValueError) as error:
        print(f"command registry manifest generation failed: {error}", file=sys.stderr)
        return 2

    if args.check:
        try:
            actual = args.output.read_text(encoding="utf-8")
        except OSError as error:
            print(f"cannot read manifest {args.output}: {error}", file=sys.stderr)
            return 2
        if actual != expected:
            print(
                f"{args.output} is stale; regenerate it with {Path(__file__).as_posix()}",
                file=sys.stderr,
            )
            return 1
        print(f"verified {len(json.loads(actual)['commands'])} command registry entries")
        return 0

    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(expected, encoding="utf-8")
    print(f"wrote {args.output}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
