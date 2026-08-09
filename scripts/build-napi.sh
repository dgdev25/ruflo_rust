#!/usr/bin/env bash
set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
cargo build --manifest-path "$repo_root/Cargo.toml" -p ruflo-napi
artifact="$repo_root/target/debug/libruflo_napi.so"
destination="$repo_root/packages/ruflo-native/ruflo-native.linux-x64-gnu.node"
test -f "$artifact"
cp "$artifact" "$destination"
node --test "$repo_root/packages/ruflo-native/test"/*.test.js
