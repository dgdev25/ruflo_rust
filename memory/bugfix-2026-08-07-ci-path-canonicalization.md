# CI path canonicalization fix

## Reported failure

GitHub Actions run `31204829392` failed on macOS x86_64, macOS aarch64, and
Windows x86_64 while Linux and supply-chain jobs passed.

## Root cause

`NativeActionExecutor` canonicalizes both the project root and working
directory before enforcing their containment relationship. The native-actions
test compared that canonical result with an uncanonicalized `tempfile` path.
macOS resolves `/var` through `/private/var`; Windows can resolve an 8.3 user
path into a long-path representation. The mismatch was in the test oracle,
not in the secure executor behavior.

## Fix

Canonicalize the expected nested directory before comparing it with the action
output. While verifying the repair, resolve existing Clippy warnings without
changing behavior: use an irrefutable `let`, provide the builder's `Default`,
name migration callback types, derive the configuration default, and box RVF
backend variants.

The fresh verification run then exposed an independent supply-chain issue:
`scripts/audit-supply-chain.sh` created Cargo Audit's exact advisory-database
clone destination before Cargo Audit could initialize it. The CI target cache
could restore that empty directory. The script now creates only its parent,
and selects a unique advisory database per GitHub run to avoid a restored
empty clone destination.

The subsequent macOS aarch64 run found the same test-oracle error in the
migration suite: the backup path is derived from a canonical persisted path,
but its parent project root was compared without canonicalization. Its
containment assertion now compares canonical roots.

Finally, after macOS tests passed, release artifact staging failed because
Apple runners provide `shasum` rather than GNU `sha256sum`. The POSIX staging
step now selects `sha256sum` when present and otherwise uses `shasum -a 256`.

The remaining Windows release smoke check found one SBOM correctly, but
PowerShell represents a single pipeline result as a scalar without `.Count`.
The script now wraps the discovery result in `@(...)`, making zero, one, and
many SBOM artifacts behave consistently.

## Verification

- `cargo fmt --all -- --check`
- `cargo test --workspace --all-targets`
- `cargo test --features stateless-http --test mcp_stateless_http --test mcp_stdio`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `git diff --check`
- `RUFLO_AUDIT_DB_DIR=<fresh temporary child path> bash scripts/audit-supply-chain.sh`

## Lesson

Path equality assertions around security canonicalization must compare canonical
paths. A display-path string is not a portable filesystem identity.
