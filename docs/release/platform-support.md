# Platform Support

Status as of 2026-08-07: Wave 1 compatibility is evidenced through native GitHub Actions runners, not through local cross-platform emulation.

## Supported release matrix

| OS | Architecture | Rust target | GitHub runner label | Evidence |
| --- | --- | --- | --- | --- |
| Linux | x86_64 | `x86_64-unknown-linux-gnu` | `ubuntu-24.04` | `cargo test --test platform_hooks`, workspace tests, POSIX smoke |
| Linux | aarch64 | `aarch64-unknown-linux-gnu` | `ubuntu-24.04-arm` | `cargo test --test platform_hooks`, workspace tests, POSIX smoke |
| macOS | x86_64 | `x86_64-apple-darwin` | `macos-15-intel` | `cargo test --test platform_hooks`, workspace tests, POSIX smoke |
| macOS | aarch64 | `aarch64-apple-darwin` | `macos-14` | `cargo test --test platform_hooks`, workspace tests, POSIX smoke |
| Windows | x86_64 | `x86_64-pc-windows-msvc` | `windows-2025` | `cargo test --test platform_hooks`, workspace tests, PowerShell smoke |

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

CI is the evidence route for the full five-platform matrix. Local runs verify only the current host platform.
