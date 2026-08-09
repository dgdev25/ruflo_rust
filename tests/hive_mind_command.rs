//! End-to-end `hive-mind` command tests through both native binaries (ADR-016).

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::{Mutex, OnceLock};

#[test]
fn overview_lists_subcommands() {
    for binary in ["ruflo", "claude-flow"] {
        let project = tempfile::tempdir().unwrap();
        let out = run(binary, project.path(), &["hive-mind"]);
        assert_eq!(out.status.code(), Some(0));
        let s = stdout(&out);
        for sub in ["init", "spawn", "status", "consensus", "memory", "shutdown"] {
            assert!(s.contains(sub), "{binary}: overview missing '{sub}'");
        }
    }
}

#[test]
fn init_creates_state() {
    for binary in ["ruflo", "claude-flow"] {
        let project = tempfile::tempdir().unwrap();
        let out = run(binary, project.path(), &["hive-mind", "init", "-t", "hierarchical-mesh", "-c", "byzantine"]);
        assert_eq!(out.status.code(), Some(0));
        assert!(stdout(&out).contains("Hive Mind initialized"));
        let state: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(project.path().join(".claude-flow/hive-mind.json")).unwrap()).unwrap();
        assert_eq!(state["topology"], "hierarchical-mesh");
        assert_eq!(state["consensus"], "byzantine");
    }
}

#[test]
fn init_rejects_unknown_topology() {
    let project = tempfile::tempdir().unwrap();
    let out = run("ruflo", project.path(), &["hive-mind", "init", "-t", "bogus"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(stderr(&out).contains("Unknown topology"));
}

#[test]
fn requires_init_first() {
    let project = tempfile::tempdir().unwrap();
    for op in ["spawn", "status", "consensus", "memory", "broadcast"] {
        let out = run("ruflo", project.path(), &["hive-mind", op]);
        assert_eq!(out.status.code(), Some(1), "{op} must require init");
        assert!(stderr(&out).contains("No hive initialized"));
    }
}

#[test]
fn spawn_join_leave_roundtrip() {
    let project = tempfile::tempdir().unwrap();
    run("ruflo", project.path(), &["hive-mind", "init"]);
    let spawn = run("ruflo", project.path(), &["hive-mind", "spawn", "--count", "3", "--role", "worker"]);
    assert!(stdout(&spawn).contains("capacity 3/15"));
    let join = run("ruflo", project.path(), &["hive-mind", "join", "--agent-id", "x-1"]);
    assert!(stdout(&join).contains("joined"));
    // duplicate rejected
    let dup = run("ruflo", project.path(), &["hive-mind", "join", "--agent-id", "x-1"]);
    assert_eq!(dup.status.code(), Some(1));
    let leave = run("ruflo", project.path(), &["hive-mind", "leave", "--agent-id", "x-1"]);
    assert!(stdout(&leave).contains("left"));
}

#[test]
fn spawn_claude_native() {
    let project = tempfile::tempdir().unwrap();
    run("ruflo", project.path(), &["hive-mind", "init"]);
    let out = run("ruflo", project.path(), &["hive-mind", "spawn", "--claude", "--count", "1"]);
    // spawn --claude now runs natively (headless::execute spawns claude -p).
    // If claude binary is absent, it reports "unavailable" — either way exit 0.
    assert_eq!(out.status.code(), Some(0));
}

#[test]
fn consensus_propose_vote_status() {
    let project = tempfile::tempdir().unwrap();
    run("ruflo", project.path(), &["hive-mind", "init"]);
    let prop = run("ruflo", project.path(), &["hive-mind", "consensus", "-a", "propose", "--value", "deploy"]);
    let s = stdout(&prop);
    let pid = s
        .lines()
        .find_map(|l| l.trim().strip_prefix("Proposal "))
        .and_then(|l| l.split_whitespace().next())
        .unwrap();
    let vote = run(
        "ruflo",
        project.path(),
        &["hive-mind", "consensus", "-a", "vote", "--proposal-id", pid, "--vote", "accept", "--voter-id", "w1"],
    );
    assert_eq!(vote.status.code(), Some(0));
    let status = run("ruflo", project.path(), &["hive-mind", "consensus", "-a", "status"]);
    assert!(stdout(&status).contains(pid));
}

#[test]
fn memory_set_get_delete() {
    let project = tempfile::tempdir().unwrap();
    run("ruflo", project.path(), &["hive-mind", "init"]);
    run("ruflo", project.path(), &["hive-mind", "memory", "-a", "set", "--key", "goal", "--value", "ship"]);
    let get = run("ruflo", project.path(), &["hive-mind", "memory", "-a", "get", "--key", "goal"]);
    assert!(stdout(&get).contains("ship"));
    let del = run("ruflo", project.path(), &["hive-mind", "memory", "-a", "delete", "--key", "goal"]);
    assert!(stdout(&del).contains("Deleted"));
}

#[test]
fn broadcast_and_shutdown() {
    let project = tempfile::tempdir().unwrap();
    run("ruflo", project.path(), &["hive-mind", "init"]);
    run("ruflo", project.path(), &["hive-mind", "spawn", "--count", "2"]);
    let bcast = run("ruflo", project.path(), &["hive-mind", "broadcast", "--message", "hello"]);
    assert!(stdout(&bcast).contains("2 worker"));
    let stop = run("ruflo", project.path(), &["hive-mind", "shutdown"]);
    assert!(stdout(&stop).contains("shut down"));
}

#[test]
fn binary_parity_overview() {
    let project = tempfile::tempdir().unwrap();
    let a = run("ruflo", project.path(), &["hive-mind"]);
    let b = run("claude-flow", project.path(), &["hive-mind"]);
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
