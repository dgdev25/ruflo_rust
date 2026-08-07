param(
    [switch]$Local,
    [string]$ArtifactDir
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

if ($Local) {
    cargo test --test platform_hooks
    if ($LASTEXITCODE -ne 0) {
        exit $LASTEXITCODE
    }
}

if ($ArtifactDir) {
    if (-not (Test-Path -LiteralPath $ArtifactDir -PathType Container)) {
        throw "artifact directory '$ArtifactDir' does not exist"
    }

    $ruflo = Join-Path $ArtifactDir "ruflo.exe"
    $claudeFlow = Join-Path $ArtifactDir "claude-flow.exe"
    $signature = Join-Path $ArtifactDir "SHA256SUMS.sig"

    if (-not (Test-Path -LiteralPath $ruflo -PathType Leaf)) {
        throw "missing expected artifact '$ruflo'"
    }
    if (-not (Test-Path -LiteralPath $claudeFlow -PathType Leaf)) {
        throw "missing expected artifact '$claudeFlow'"
    }
    if (-not (Test-Path -LiteralPath $signature -PathType Leaf)) {
        throw "missing expected signature '$signature'"
    }

    $sboms = Get-ChildItem -LiteralPath $ArtifactDir -File -Filter "*.sbom.*.json"
    if ($sboms.Count -eq 0) {
        throw "missing expected SBOM artifact in '$ArtifactDir'"
    }
}
elseif (-not $Local) {
    throw "artifact presence checks require -ArtifactDir"
}
