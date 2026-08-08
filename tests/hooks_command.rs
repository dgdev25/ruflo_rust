//! End-to-end `hooks` command tests through both native binaries (ADR-016).

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::{Mutex, OnceLock};

#[test]
fn overview_lists_subcommands() {
    for binary in ["ruflo", "claude-flow"] {
        let project = tempfile::tempdir().unwrap();
        let out = run(binary, project.path(), &["hooks"]);
        assert_eq!(out.status.code(), Some(0));
        let s = stdout(&out);
        for sub in ["route", "list", "metrics", "model-route", "statusline", "pre-edit"] {
            assert!(s.contains(sub), "{binary}: overview missing '{sub}'");
        }
    }
}

#[test]
fn event_hooks_record_to_jsonl() {
    for binary in ["ruflo", "claude-flow"] {
        let project = tempfile::tempdir().unwrap();
        let out = run(binary, project.path(), &["hooks", "pre-edit", "--file", "src/x.rs"]);
        assert_eq!(out.status.code(), Some(0));
        let raw = fs::read_to_string(project.path().join(".claude-flow/hooks-events.jsonl")).unwrap();
        let v: serde_json::Value = serde_json::from_str(raw.trim()).unwrap();
        assert_eq!(v["event"], "pre-edit");
        assert_eq!(v["filePath"], "src/x.rs");
    }
}

#[test]
fn route_picks_tester_for_test_task() {
    let project = tempfile::tempdir().unwrap();
    let out = run("ruflo", project.path(), &["hooks", "route", "-t", "write a unit test"]);
    assert_eq!(out.status.code(), Some(0));
    assert!(stdout(&out).contains("Agent: tester"));
}

#[test]
fn route_requires_task() {
    let project = tempfile::tempdir().unwrap();
    let out = run("ruflo", project.path(), &["hooks", "route"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(stderr(&out).contains("Task description required"));
}

#[test]
fn explain_shows_last_decision() {
    let project = tempfile::tempdir().unwrap();
    run("ruflo", project.path(), &["hooks", "route", "-t", "refactor the module"]);
    let out = run("ruflo", project.path(), &["hooks", "explain"]);
    assert_eq!(out.status.code(), Some(0));
    assert!(stdout(&out).contains("refactor the module"));
}

#[test]
fn model_route_and_outcome_and_stats() {
    let project = tempfile::tempdir().unwrap();
    let mr = run("ruflo", project.path(), &["hooks", "model-route", "-t", "security audit"]);
    assert!(stdout(&mr).contains("opus"));
    let bad = run("ruflo", project.path(), &["hooks", "model-outcome", "-m", "sonnet", "--outcome", "bogus"]);
    assert_eq!(bad.status.code(), Some(1));
    let ok = run("ruflo", project.path(), &["hooks", "model-outcome", "-m", "sonnet", "--outcome", "success"]);
    assert_eq!(ok.status.code(), Some(0));
    let stats = run("ruflo", project.path(), &["hooks", "model-stats"]);
    assert!(stdout(&stats).contains("Decisions recorded"));
}

#[test]
fn metrics_and_list_reflect_events() {
    let project = tempfile::tempdir().unwrap();
    run("ruflo", project.path(), &["hooks", "post-edit"]);
    run("ruflo", project.path(), &["hooks", "post-edit"]);
    let metrics = run("ruflo", project.path(), &["hooks", "metrics"]);
    assert!(stdout(&metrics).contains("Total events recorded: 2"));
    let list = run("ruflo", project.path(), &["hooks", "list"]);
    // post-edit should show 2 executions
    assert!(stdout(&list).contains("post-edit"));
}

#[test]
fn worker_dispatch_validates_trigger() {
    let project = tempfile::tempdir().unwrap();
    let bad = run("ruflo", project.path(), &["hooks", "worker-dispatch", "-t", "bogus"]);
    assert_eq!(bad.status.code(), Some(1));
    assert!(stderr(&bad).contains("Unknown worker trigger"));
}

#[test]
fn statusline_renders() {
    let project = tempfile::tempdir().unwrap();
    let out = run("ruflo", project.path(), &["hooks", "statusline"]);
    assert_eq!(out.status.code(), Some(0));
    assert!(stdout(&out).contains("ruflo"));
}

#[test]
fn binary_parity_overview() {
    let project = tempfile::tempdir().unwrap();
    let a = run("ruflo", project.path(), &["hooks"]);
    let b = run("claude-flow", project.path(), &["hooks"]);
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
