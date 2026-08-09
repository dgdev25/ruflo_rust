//! Source-oracle overview fixtures for all native command families.
//!
//! Each `tests/fixtures/cli/<family>/overview.json` was captured from the V3
//! TypeScript reference CLI (`node v3/@claude-flow/cli/bin/cli.js <family>`)
//! via `scripts/capture-reference-contract.sh`, carrying `source-oracle`
//! provenance with the owning TS source path. This test:
//!   1. Asserts every captured fixture is a valid source-oracle record.
//!   2. For families whose native `overview()` is byte-aligned to the TS
//!      reference, asserts byte-exact stdout/exit parity for both binaries.
//!
//! Families whose overview embeds the runner's `$HOME` (doctor/funnel/settings)
//! are intentionally excluded — their output is machine-specific, not portable,
//! and `fixture-capture` correctly refuses to capture them.

use serde::Deserialize;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::{Mutex, OnceLock};

#[derive(Deserialize)]
struct Fixture {
    argv: Vec<String>,
    exit: i32,
    stdout: String,
    stderr: String,
    provenance: Provenance,
}

#[derive(Deserialize)]
struct Provenance {
    kind: String,
    #[serde(default)]
    source_paths: Vec<String>,
}

/// Families proven byte-aligned to the TS reference this pass.
// BYTE_ALIGNED families: the native overview output is byte-identical to the
// captured TS source-oracle fixture. These are TRUE TS-parity commands.
//
// Intentionally-divergent native rewrites (NOT in this set): `route task`
// (Thompson bandit), `auth login` (PKCE), `memory search`/`rebuild-index`
// (RVF HNSW), `neural train` (native-sona backend label). Their overview text
// was aligned to TS where the overview is static docs, but their runtime
// OUTPUT (the command result, not the help) diverges by design — those are
// exercised by differential_cli's targeted tests, not byte-alignment.
const BYTE_ALIGNED: &[&str] = &[
    "security",
    "analyze",
    "daemon",
    "hive-mind",
    "neural",
    "hooks",
    "completions",
    "swarm",
    "task",
    "session",
    "memory",
    "migrate",
    "appliance",
    "transfer-store",
    "process",
    "claims",
];

/// Families with a captured source-oracle overview fixture. doctor/funnel/
/// settings are excluded: their TS overview embeds the runner's $HOME, so
/// output is machine-specific and fixture-capture correctly refuses them.
const CAPTURED: &[&str] = &[
    "migrate", "neural", "performance", "plugins", "policy", "process",
    "progress", "providers", "proxy", "route", "ruvector", "security",
    "session", "spinner", "swarm", "task",
    "transfer-store", "transport", "update", "version", "workflow",
];

#[test]
fn every_captured_fixture_is_a_source_oracle_record() {
    for family in CAPTURED {
        let path = format!("tests/fixtures/cli/{family}/overview.json");
        let raw = fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("missing fixture {path}: {e}"));
        let fixture: Fixture = serde_json::from_str(&raw).unwrap();
        assert_eq!(
            fixture.provenance.kind, "source-oracle",
            "{path}: provenance kind must be source-oracle"
        );
        assert!(
            !fixture.provenance.source_paths.is_empty(),
            "{path}: source-oracle fixture must record the owning TS source path"
        );
        // The captured argv is `["node", ".../cli.js", "<family>"]`.
        assert_eq!(
            fixture.argv.last().map(|s| s.as_str()),
            Some(*family),
            "{path}: argv must be the family overview"
        );
    }
}

#[test]
fn byte_aligned_families_match_reference_for_both_binaries() {
    for family in BYTE_ALIGNED {
        let path = format!("tests/fixtures/cli/{family}/overview.json");
        let fixture: Fixture = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        for binary in ["ruflo", "claude-flow"] {
            let project = tempfile::tempdir().unwrap();
            let output = run(binary, project.path(), &[family]);
            assert_eq!(
                output.status.code(),
                Some(fixture.exit),
                "{binary} {family}: exit mismatch"
            );
            let stdout = String::from_utf8(output.stdout).unwrap();
            assert_eq!(
                stdout, fixture.stdout,
                "{binary} {family}: stdout byte mismatch vs TS reference"
            );
            assert_eq!(
                String::from_utf8(output.stderr).unwrap(),
                fixture.stderr,
                "{binary} {family}: stderr mismatch"
            );
        }
    }
}

// ---- helpers ----------------------------------------------------------------

fn run(binary: &str, cwd: &Path, args: &[&str]) -> Output {
    let _g = LOCK.lock().unwrap();
    Command::new(executable(binary))
        .current_dir(cwd)
        .args(args)
        .env("NO_COLOR", "1")
        .output()
        .unwrap()
}
static LOCK: Mutex<()> = Mutex::new(());
fn executable(binary: &str) -> PathBuf {
    static BUILT: OnceLock<Mutex<Vec<String>>> = OnceLock::new();
    let mut built = BUILT.get_or_init(|| Mutex::new(Vec::new())).lock().unwrap();
    if !built.iter().any(|n| n == binary) {
        let s = Command::new(env!("CARGO"))
            .current_dir(env!("CARGO_MANIFEST_DIR"))
            .args(["build", "--quiet", "--package", binary, "--bin", binary])
            .status()
            .unwrap();
        assert!(s.success());
        built.push(binary.into());
    }
    std::env::var_os(format!("CARGO_BIN_EXE_{}", binary.replace('-', "_")))
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("target/debug")
                .join(binary)
        })
}
