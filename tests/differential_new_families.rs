//! Source-differential parity for the 7 largest command families.
//!
//! Each `tests/fixtures/cli/<family>/overview.json` was captured from the V3
//! TypeScript reference CLI (`node v3/@claude-flow/cli/bin/cli.js <family>`)
//! via `scripts/capture-reference-contract.sh`, carrying `source-oracle`
//! provenance with the owning TS source path. This test replays the native
//! `ruflo` and `claude-flow` binaries with the same argv and asserts byte-exact
//! stdout/exit parity — the V3 fallback-overview contract.

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
}

const FAMILIES: &[&str] = &[
    "security",
    "analyze",
    "daemon",
    "embeddings",
    "hive-mind",
    "neural",
    "hooks",
];

#[test]
fn overview_byte_matches_reference_for_both_binaries() {
    for family in FAMILIES {
        let path = format!("tests/fixtures/cli/{family}/overview.json");
        let fixture: Fixture = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        // The captured argv is `["node", ".../cli.js", "<family>"]`; the native
        // replay uses just the family name.
        assert_eq!(
            fixture.argv.last().map(|s| s.as_str()),
            Some(*family),
            "fixture argv for {family} is not the family overview"
        );
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
            // stderr is empty for the overview path in both TS and native.
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
