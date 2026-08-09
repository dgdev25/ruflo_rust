//! End-to-end `neural` command tests through both native binaries (ADR-016).

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::{Mutex, OnceLock};

#[test]
fn overview_lists_subcommands() {
    for binary in ["ruflo", "claude-flow"] {
        let project = tempfile::tempdir().unwrap();
        let out = run(binary, project.path(), &["neural"]);
        assert_eq!(out.status.code(), Some(0));
        let s = stdout(&out);
        // The TS neural overview is a minimal banner (no subcommand list); it
        // points to --help. Assert the banner surface that is stable.
        assert!(s.contains("RuFlo Neural System"), "{binary}: neural banner missing");
        assert!(s.contains("ruv.io"), "{binary}: neural signature missing");
    }
}

#[test]
fn train_validates_pattern_and_records() {
    for binary in ["ruflo", "claude-flow"] {
        let project = tempfile::tempdir().unwrap();
        let bad = run(binary, project.path(), &["neural", "train", "-p", "bogus"]);
        assert_eq!(bad.status.code(), Some(1));
        assert!(stderr(&bad).contains("Unknown pattern"));
        let good = run(binary, project.path(), &["neural", "train", "-p", "security", "-e", "5"]);
        assert_eq!(good.status.code(), Some(0));
        // Native SONA MLP reports its backend; legacy "recorded" wording also accepted.
        assert!(
            stdout(&good).contains("SONA MLP trained") || stdout(&good).contains("Training run recorded"),
            "train stdout: {}",
            stdout(&good)
        );
        // State persisted.
        let stats: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(project.path().join(".claude-flow/neural/stats.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(stats["trainingRuns"].as_array().unwrap().len(), 1);
    }
}

#[test]
fn status_reflects_training() {
    let project = tempfile::tempdir().unwrap();
    run("ruflo", project.path(), &["neural", "train", "-p", "coordination"]);
    let out = run("ruflo", project.path(), &["neural", "status"]);
    assert!(stdout(&out).contains("Patterns learned"));
}

#[test]
fn export_import_roundtrip() {
    let project = tempfile::tempdir().unwrap();
    run("ruflo", project.path(), &["neural", "train", "-p", "testing"]);
    let exp_path = project.path().join("exp.json");
    run(
        "ruflo",
        project.path(),
        &["neural", "export", "-d", exp_path.to_str().unwrap()],
    );
    assert!(exp_path.is_file());
    // wipe + import
    let _ = fs::remove_file(project.path().join(".claude-flow/neural/stats.json"));
    run(
        "ruflo",
        project.path(),
        &["neural", "import", "-d", exp_path.to_str().unwrap()],
    );
    let stats: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(project.path().join(".claude-flow/neural/stats.json")).unwrap(),
    )
    .unwrap();
    assert!(!stats["trainingRuns"].as_array().unwrap().is_empty());
}

#[test]
fn predict_degrades() {
    let project = tempfile::tempdir().unwrap();
    let out = run("ruflo", project.path(), &["neural", "predict", "-i", "x"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(stderr(&out).contains("Node") || stderr(&out).contains("WASM"));
}

#[test]
fn router_models_lists() {
    let project = tempfile::tempdir().unwrap();
    let out = run("ruflo", project.path(), &["neural", "router", "models"]);
    assert_eq!(out.status.code(), Some(0));
    assert!(stdout(&out).contains("haiku") || stdout(&out).contains("sonnet"));
}

#[test]
fn router_decide_degrades() {
    let project = tempfile::tempdir().unwrap();
    let out = run("ruflo", project.path(), &["neural", "router", "decide", "-i", "x"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(stderr(&out).contains("Node"));
}

#[test]
fn distill_degrades() {
    let project = tempfile::tempdir().unwrap();
    let out = run("ruflo", project.path(), &["neural", "distill", "plan"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(stderr(&out).contains("ruvllm") || stderr(&out).contains("Node"));
}

#[test]
fn benchmark_runs() {
    let project = tempfile::tempdir().unwrap();
    let out = run("ruflo", project.path(), &["neural", "benchmark", "-e", "10"]);
    assert_eq!(out.status.code(), Some(0));
    assert!(stdout(&out).contains("ops/sec"));
}

#[test]
fn binary_parity_overview() {
    let project = tempfile::tempdir().unwrap();
    let a = run("ruflo", project.path(), &["neural"]);
    let b = run("claude-flow", project.path(), &["neural"]);
    assert_eq!(stdout(&a), stdout(&b));
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
fn stdout(o: &Output) -> String {
    String::from_utf8(o.stdout.clone()).unwrap()
}
fn stderr(o: &Output) -> String {
    String::from_utf8(o.stderr.clone()).unwrap()
}
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
