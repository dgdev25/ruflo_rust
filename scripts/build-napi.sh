#!/usr/bin/env bash
set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
profile=${NAPI_PROFILE:-debug}
build_args=(build --manifest-path "$repo_root/Cargo.toml" -p ruflo-napi)
if [[ "$profile" == "release" ]]; then build_args+=(--release); fi
if [[ "$profile" != "debug" && "$profile" != "release" ]]; then
  echo "NAPI_PROFILE must be debug or release" >&2
  exit 2
fi
cargo "${build_args[@]}"

case "$(uname -s)-$(uname -m)" in
  Linux-x86_64) suffix=linux-x64-gnu; artifact="$repo_root/target/$profile/libruflo_napi.so" ;;
  Darwin-x86_64) suffix=darwin-x64; artifact="$repo_root/target/$profile/libruflo_napi.dylib" ;;
  Darwin-arm64) suffix=darwin-arm64; artifact="$repo_root/target/$profile/libruflo_napi.dylib" ;;
  *) echo "unsupported local N-API build platform: $(uname -s)-$(uname -m)" >&2; exit 1 ;;
esac

destination="$repo_root/packages/ruflo-native/ruflo-native.$suffix.node"
test -f "$artifact"
cp "$artifact" "$destination"
node --test "$repo_root/packages/ruflo-native/test"/*.test.js
