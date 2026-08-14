//! End-to-end `daemon` command tests through both native binaries (ADR-016).

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::{Mutex, OnceLock};

fn run_with_budget(binary: &str, cwd: &Path, budget_dir: &Path, args: &[&str]) -> Output {
    let _g = LOCK.lock().unwrap();
    Command::new(executable(binary))
        .current_dir(cwd)
        .args(args)
        .env("NO_COLOR", "1")
        .env("RUFLO_AI_BUDGET_DIR", budget_dir)
        .output()
        .unwrap()
}

#[test]
fn overview_lists_subcommands() {
    for binary in ["ruflo", "claude-flow"] {
        let project = tempfile::tempdir().unwrap();
        let budget = tempfile::tempdir().unwrap();
        let out = run_with_budget(binary, project.path(), budget.path(), &["daemon"]);
        assert_eq!(out.status.code(), Some(0));
        let s = stdout(&out);
        for sub in ["start", "stop", "status", "trigger", "enable"] {
            assert!(s.contains(sub), "{binary}: overview missing '{sub}'");
        }
    }
}

#[test]
fn start_status_stop_reports_live_running_pid_twice() {
    let project = tempfile::tempdir().unwrap();
    let budget = tempfile::tempdir().unwrap();
    for cycle in 1..=2 {
        let start = run_with_budget(
            "ruflo",
            project.path(),
            budget.path(),
            &["daemon", "start", "-w", "map"],
        );
        assert_eq!(start.status.code(), Some(0), "cycle {cycle} start: {}", stdout(&start));
        let mut status_out = String::new();
        let mut pid: Option<u32> = None;
        for _ in 0..40 {
            let status = run_with_budget("ruflo", project.path(), budget.path(), &["daemon", "status"]);
            status_out = stdout(&status);
            if status_out.contains("RUNNING") {
                if let Some(p) = status_out
                    .lines()
                    .find_map(|l| l.split("PID:").nth(1).and_then(|s| s.trim().parse().ok()))
                {
                    pid = Some(p);
                    break;
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        let pid = pid.expect(&format!("cycle {cycle} never RUNNING: {status_out}"));
        assert!(
            Path::new(&format!("/proc/{pid}")).exists() || cfg!(not(target_os = "linux")),
            "cycle {cycle} pid {pid} not alive"
        );
        let stop = run_with_budget("ruflo", project.path(), budget.path(), &["daemon", "stop"]);
        assert_eq!(stop.status.code(), Some(0), "cycle {cycle} stop");
        let after = run_with_budget("ruflo", project.path(), budget.path(), &["daemon", "status"]);
        let after_s = stdout(&after);
        assert!(
            !after_s.contains("RUNNING") || after_s.contains("STOPPED"),
            "cycle {cycle} still running: {after_s}"
        );
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}

#[test]
fn start_records_state_and_status_reflects_it() {
    for binary in ["ruflo", "claude-flow"] {
        let project = tempfile::tempdir().unwrap();
        let budget = tempfile::tempdir().unwrap();
        let start = run_with_budget(
            binary,
            project.path(),
            budget.path(),
            &["daemon", "start", "-w", "map,audit"],
        );
        assert_eq!(start.status.code(), Some(0));
        assert!(stdout(&start).contains("map, audit"));
        // State file written.
        assert!(project.path().join(".claude-flow/daemon-state.json").is_file());
        let status = run_with_budget(binary, project.path(), budget.path(), &["daemon", "status"]);
        assert_eq!(status.status.code(), Some(0));
        let s = stdout(&status);
        assert!(s.contains("Workers Enabled: 2"));
        assert!(s.contains("TTL: 12h"));
        let _ = run_with_budget(binary, project.path(), budget.path(), &["daemon", "stop"]);
    }
}

#[test]
fn enable_then_disable_worker() {
    let project = tempfile::tempdir().unwrap();
    let budget = tempfile::tempdir().unwrap();
    run_with_budget("ruflo", project.path(), budget.path(), &["daemon", "start", "-w", "map"]);
    let en = run_with_budget("ruflo", project.path(), budget.path(), &["daemon", "enable", "-w", "predict"]);
    assert!(stdout(&en).contains("enabled"));
    let dis = run_with_budget("ruflo", project.path(), budget.path(), &["daemon", "enable", "-w", "map", "-d"]);
    assert!(stdout(&dis).contains("disabled"));
    let state: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(project.path().join(".claude-flow/daemon-state.json")).unwrap(),
    )
    .unwrap();
    let workers = state["config"]["workers"].as_array().unwrap();
    let map = workers.iter().find(|w| w["type"] == "map").unwrap();
    assert_eq!(map["enabled"], false);
    let predict = workers.iter().find(|w| w["type"] == "predict").unwrap();
    assert_eq!(predict["enabled"], true);
    let _ = run_with_budget("ruflo", project.path(), budget.path(), &["daemon", "stop"]);
}

#[test]
fn budget_pause_show_resume_roundtrip() {
    for binary in ["ruflo", "claude-flow"] {
        let project = tempfile::tempdir().unwrap();
        let budget = tempfile::tempdir().unwrap();
        let pause = run_with_budget(
            binary,
            project.path(),
            budget.path(),
            &["daemon", "budget", "pause", "--reason", "testing"],
        );
        assert_eq!(pause.status.code(), Some(0));
        let show = run_with_budget(binary, project.path(), budget.path(), &["daemon", "budget", "show"]);
        assert!(stdout(&show).contains("PAUSED"));
        assert!(stdout(&show).contains("testing"));
        // Ledger persisted with sentinel.
        let ledger: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(budget.path().join("ai-budget.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(ledger["pausedUntil"].as_u64(), Some(9_000_000_000_000u64));
        let resume = run_with_budget(binary, project.path(), budget.path(), &["daemon", "budget", "resume"]);
        assert_eq!(resume.status.code(), Some(0));
        let show2 = run_with_budget(binary, project.path(), budget.path(), &["daemon", "budget", "show"]);
        assert!(!stdout(&show2).contains("PAUSED"));
    }
}

#[test]
fn budget_show_clean_when_empty() {
    let project = tempfile::tempdir().unwrap();
    let budget = tempfile::tempdir().unwrap();
    let show = run_with_budget("ruflo", project.path(), budget.path(), &["daemon", "budget", "show"]);
    assert_eq!(show.status.code(), Some(0));
    let s = stdout(&show);
    assert!(s.contains("Launches (last hour): 0/2"));
    assert!(s.contains("Circuit breaker: closed"));
}

#[test]
fn trigger_records_marker() {
    let project = tempfile::tempdir().unwrap();
    let budget = tempfile::tempdir().unwrap();
    let out = run_with_budget("ruflo", project.path(), budget.path(), &["daemon", "trigger", "-w", "audit"]);
    assert_eq!(out.status.code(), Some(0));
    assert!(project.path().join(".claude-flow/daemon-triggers.jsonl").is_file());
}

#[test]
fn enable_requires_worker_flag() {
    let project = tempfile::tempdir().unwrap();
    let budget = tempfile::tempdir().unwrap();
    let out = run_with_budget("ruflo", project.path(), budget.path(), &["daemon", "enable"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(stderr(&out).contains("--worker"));
}

#[test]
fn budget_unknown_sub_errors() {
    let project = tempfile::tempdir().unwrap();
    let budget = tempfile::tempdir().unwrap();
    let out = run_with_budget("ruflo", project.path(), budget.path(), &["daemon", "budget", "bogus"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(stderr(&out).contains("Unknown budget op"));
}

#[test]
fn binary_parity_overview() {
    let project = tempfile::tempdir().unwrap();
    let budget = tempfile::tempdir().unwrap();
    let a = run_with_budget("ruflo", project.path(), budget.path(), &["daemon"]);
    let b = run_with_budget("claude-flow", project.path(), budget.path(), &["daemon"]);
    assert_eq!(stdout(&a), stdout(&b));
}

// ---- helpers ----------------------------------------------------------------

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
