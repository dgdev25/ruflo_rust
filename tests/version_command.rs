//! End-to-end `version` command tests through both native binaries.
//!
//! Source of truth: `v3/@claude-flow/cli/src/commands/version.ts`. The bare
//! `version` subcommand prints semver; `--explain` renders the ANV breakdown when
//! a catalog-manifest.json is discoverable; `--require-catalog-gte` gates scripts.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::{Mutex, OnceLock};

const VERSION: &str = "3.34.0";

#[test]
fn both_binaries_print_plain_semver_for_version_subcommand() {
    for binary in ["ruflo", "claude-flow"] {
        let project = tempfile::tempdir().unwrap();
        let output = run(binary, project.path(), &["version"]);
        assert_success(&output);
        assert_eq!(stdout(&output), format!("{VERSION}\n"));
    }
}

#[test]
fn both_binaries_global_version_flag_is_ruflo_prefixed() {
    for binary in ["ruflo", "claude-flow"] {
        let project = tempfile::tempdir().unwrap();
        let output = run(binary, project.path(), &["--version"]);
        assert_success(&output);
        assert_eq!(stdout(&output), format!("ruflo v{VERSION}\n"));
    }
}

#[test]
fn explain_without_catalog_manifest_degrades_to_plain_semver_note() {
    for binary in ["ruflo", "claude-flow"] {
        let project = tempfile::tempdir().unwrap();
        let output = run(binary, project.path(), &["version", "--explain"]);
        assert_success(&output);
        assert!(stdout(&output).contains(&format!("Installed: ruflo@{VERSION}")));
        assert!(stdout(&output).contains("no catalog-manifest.json"));
    }
}

#[test]
fn require_catalog_gte_zero_passes_and_nonzero_fails_without_manifest() {
    for binary in ["ruflo", "claude-flow"] {
        let project = tempfile::tempdir().unwrap();
        let pass = run(
            binary,
            project.path(),
            &["version", "--require-catalog-gte", "0"],
        );
        assert_eq!(pass.status.code(), Some(0));
        assert_eq!(stdout(&pass), "OK (installed catalog is 0)\n");

        let fail = run(
            binary,
            project.path(),
            &["version", "--require-catalog-gte", "40"],
        );
        assert_eq!(fail.status.code(), Some(1));
        assert!(stderr(&fail).contains("below required 40"));
    }
}

#[test]
fn explain_with_catalog_manifest_renders_anv_breakdown() {
    for binary in ["ruflo", "claude-flow"] {
        let project = tempfile::tempdir().unwrap();
        std::fs::write(
            project.path().join("catalog-manifest.json"),
            serde_json::json!({
                "schemaVersion": 1,
                "generation": 42,
                "generatedAt": "2026-08-01T00:00:00Z",
                "gitSha": "abc1234",
                "catalog": {"agents": 12, "tools": 34, "skills": 56},
                "benchmark": {"tier": 3, "verifiedAt": "2026-08-02T00:00:00Z", "signature": "s"}
            })
            .to_string(),
        )
        .unwrap();

        let explain = run(binary, project.path(), &["version", "--explain"]);
        assert_success(&explain);
        let out = stdout(&explain);
        assert!(out.contains(&format!("ruflo@{VERSION}+ad.1.gabc1234.cat42.hal3")));
        assert!(out.contains("generation 42"));
        assert!(out.contains("agents: 12 types"));
        assert!(out.contains("GAIA tier 3"));

        // require-catalog-gte now reads generation 42 from the manifest.
        let gate_pass = run(
            binary,
            project.path(),
            &["version", "--require-catalog-gte", "40"],
        );
        assert_eq!(gate_pass.status.code(), Some(0));
        assert_eq!(stdout(&gate_pass), "OK (installed catalog is 42)\n");

        let gate_fail = run(
            binary,
            project.path(),
            &["version", "--require-catalog-gte", "50"],
        );
        assert_eq!(gate_fail.status.code(), Some(1));
    }
}

#[test]
fn corrupt_manifest_is_treated_as_absent() {
    for binary in ["ruflo", "claude-flow"] {
        let project = tempfile::tempdir().unwrap();
        std::fs::write(project.path().join("catalog-manifest.json"), "{broken").unwrap();
        let explain = run(binary, project.path(), &["version", "--explain"]);
        assert_success(&explain);
        assert!(stdout(&explain).contains("no catalog-manifest.json"));
        // Corrupt manifest → generation 0.
        let gate = run(
            binary,
            project.path(),
            &["version", "--require-catalog-gte", "1"],
        );
        assert_eq!(gate.status.code(), Some(1));
    }
}

fn assert_success(output: &Output) {
    assert_eq!(output.status.code(), Some(0), "stderr: {}", stderr(output));
}

fn stdout(output: &Output) -> String {
    String::from_utf8(output.stdout.clone()).unwrap()
}

fn stderr(output: &Output) -> String {
    String::from_utf8(output.stderr.clone()).unwrap()
}

fn run(binary: &str, root: &Path, args: &[&str]) -> Output {
    Command::new(executable(binary))
        .current_dir(root)
        .args(args)
        .env("NO_COLOR", "1")
        .output()
        .unwrap()
}

fn executable(binary: &str) -> PathBuf {
    static BUILT: OnceLock<Mutex<Vec<String>>> = OnceLock::new();
    let mut built = BUILT.get_or_init(|| Mutex::new(Vec::new())).lock().unwrap();
    if !built.iter().any(|name| name == binary) {
        let status = Command::new(env!("CARGO"))
            .current_dir(env!("CARGO_MANIFEST_DIR"))
            .args(["build", "--quiet", "--package", binary, "--bin", binary])
            .status()
            .unwrap();
        assert!(status.success(), "failed to build {binary}");
        built.push(binary.to_string());
    }
    std::env::var_os(format!("CARGO_BIN_EXE_{}", binary.replace('-', "_")))
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("target/debug")
                .join(binary)
        })
}
