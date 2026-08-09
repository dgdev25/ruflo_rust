# Platform Support

Status as of 2026-08-09: `.github/workflows/native-release.yml` automates
native-runner release archives for Linux x86_64/aarch64, macOS x86_64/aarch64,
and Windows x86_64. Every archive contains `ruflo`, `claude-flow`, and the
matching Ruflo N-API `.node` addon. The tag workflow verifies those filenames
before publishing. This is automation, not retroactive evidence: a target is
only release-verified after a successful tagged workflow run for that target.

## Supported release matrix

| OS | Architecture | Rust target | Required host evidence |
| --- | --- | --- | --- |
| Linux | x86_64 | `x86_64-unknown-linux-gnu` | native runner archive, `platform_hooks`, N-API loader contract |
| Linux | aarch64 | `aarch64-unknown-linux-gnu` | native ARM runner archive, `platform_hooks`, N-API loader contract |
| macOS | x86_64 | `x86_64-apple-darwin` | native runner archive, `platform_hooks`, N-API loader contract |
| macOS | aarch64 | `aarch64-apple-darwin` | native runner archive, `platform_hooks`, N-API loader contract |
| Windows | x86_64 | `x86_64-pc-windows-msvc` | native runner archive, `platform_hooks`, N-API loader contract |

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
