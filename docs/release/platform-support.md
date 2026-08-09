# Platform Support

Status as of 2026-08-09: Windows build and release workflows exist. Linux and
macOS release artifacts are not yet automated, so the full five-platform matrix
is not a completed release claim; run the platform smoke scripts on target
hosts before promotion.

## Supported release matrix

| OS | Architecture | Rust target | Required host evidence |
| --- | --- | --- | --- |
| Linux | x86_64 | `x86_64-unknown-linux-gnu` | `cargo test --test platform_hooks`, workspace tests, POSIX smoke |
| Linux | aarch64 | `aarch64-unknown-linux-gnu` | `cargo test --test platform_hooks`, workspace tests, POSIX smoke |
| macOS | x86_64 | `x86_64-apple-darwin` | `cargo test --test platform_hooks`, workspace tests, POSIX smoke |
| macOS | aarch64 | `aarch64-apple-darwin` | `cargo test --test platform_hooks`, workspace tests, POSIX smoke |
| Windows | x86_64 | `x86_64-pc-windows-msvc` | `cargo test --test platform_hooks`, workspace tests, PowerShell smoke |

## What the smoke gate checks

- Real `ruflo` and `claude-flow` aliases respond with the checked-in version contract.
- `ruflo mcp start` keeps stdout newline-delimited JSON-RPC only.
- Migration locks and runtime cancellation remain available on the host platform.
- Hook rendering stays tokenized as `ruflo[.exe] mcp start` with no shell pipeline or `cmd.exe /c` wrapper.
- When a prepared release artifact directory is supplied, the smoke script fails closed if either binary, the detached signature artifact, or the SBOM artifact is missing.

## Local usage

- POSIX: `bash scripts/release-smoke.sh --local`
- PowerShell: `pwsh -File scripts/release-smoke.ps1 -Local`
- Artifact-bundle validation: add `--artifact-dir <dir>` or `-ArtifactDir <dir>` to the command above.

Local runs verify only the current host platform. Record five target-host runs
with a release candidate before promoting it.
