# Supply-Chain Policy

Updated: 2026-08-07

This workspace enforces lockfile-backed Rust supply-chain checks before release candidates are accepted.

## Allowed Sources

- crates.io registry index: `https://github.com/rust-lang/crates.io-index`
- public pinned git source: `https://github.com/ruvnet/RuVector` with a Cargo `rev=` specifier only

Any other registry or git source is rejected.

## Approved SPDX License IDs

- `Apache-2.0`
- `Apache-2.0 WITH LLVM-exception`
- `BSD-1-Clause`
- `BSD-2-Clause`
- `BSD-3-Clause`
- `BSL-1.0`
- `MIT`
- `Unicode-3.0`
- `Unlicense`

Composite expressions are allowed only when every SPDX identifier in the expression is drawn from the approved list above.

## Advisory Exceptions

No active advisory exceptions as of 2026-08-07.

If an advisory must be ignored temporarily, it must be added to both `deny.toml` and this document with:

`RUSTSEC ID | affected crate@version | reason | expiry date | owner`

Time-limited means a concrete expiry date, not "until upstream fixes it".

## License Exceptions

No active license exceptions as of 2026-08-07.

If a crate-specific license exception is ever approved, it must be listed in both `deny.toml` and this document with:

`crate@version | SPDX expression | reason | expiry date | owner`

## Verification

- `bash scripts/audit-supply-chain.sh`
- `bash scripts/generate-sbom.sh --check`

`scripts/audit-supply-chain.sh` performs local lockfile, source, license, advisory-policy, and tool bootstrap checks. `scripts/generate-sbom.sh` emits a deterministic SPDX 2.3 SBOM from `cargo metadata --locked` and records a SHA-256 digest next to the artifact.
