# CI path canonicalization impact analysis

## Target

`tests/native_actions.rs::canonicalized_working_directory_stays_beneath_project_root`
compares the executor's canonical working directory with an uncanonicalized
temporary-directory path. This fails on systems that expose a different
presentation path after canonicalization.

## Dependency map

- Caller under test: `NativeActionExecutor::execute` in
  `src/crates/ruflo-actions/src/executor.rs`.
- Dependencies: `Path::canonicalize`, filesystem metadata, and `starts_with`
  containment validation.
- Shared contract: action working directories are canonicalized before being
  returned or used, and must remain beneath the canonical project root.
- CI callers: the `compatibility` workflow executes `cargo test --workspace
  --all-targets` on Linux, macOS, and Windows.

## Blast radius

- Safe: declarative-action allowlisting, timeout, concurrency, and plugin-wave
  behavior do not depend on this assertion's expected-string formatting.
- Needs testing: the same native-actions integration suite on the local host,
  plus the CI platform matrix.
- High risk: weakening canonicalization would make containment checks depend on
  platform aliases and could allow incorrect path-boundary decisions.
- Unknown: no other tests compare this action output to an uncanonicalized
  temporary directory; repository search found this as the sole direct case.

## Safety constraints

1. Preserve canonicalization in the executor and the canonical containment
   check.
2. The test must assert the canonical expected directory, not a platform
   spelling.
3. Native-actions, complete workspace tests, release smoke, and CI matrix must
   pass.
4. Rollback: revert only the assertion change if it exposes a genuine executor
   contract change.
