//! End-to-end lifecycle dispatcher tests through the native `ruflo` binary.
//!
//! Exercises the swarm/agent/task/session CLI dispatchers that wire user intent
//! into the runtime state stores under `.claude-flow/`. Each test uses a fresh
//! tempdir so on-disk state never leaks across tests.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::{Mutex, OnceLock};

#[test]
fn init_then_swarm_init_succeeds() {
    let project = tempfile::tempdir().unwrap();
    assert_success(&run(project.path(), &["init"]));
    let swarm_init = run(project.path(), &["swarm", "init"]);
    assert_success(&swarm_init);
    assert!(stdout(&swarm_init).contains("initialized successfully"));
}

#[test]
fn swarm_status_after_init_reports_ready_swarm() {
    let project = tempfile::tempdir().unwrap();
    assert_success(&run(project.path(), &["init"]));
    assert_success(&run(project.path(), &["swarm", "init"]));
    let status = run(project.path(), &["swarm", "status"]);
    assert_success(&status);
    let s = stdout(&status);
    assert!(s.contains("Swarm swarm-"));
    assert!(s.contains("[ready]"));
    assert!(s.contains("Topology: hierarchical"));
}

#[test]
fn swarm_start_dry_run_emits_plan_without_spawning() {
    let project = tempfile::tempdir().unwrap();
    assert_success(&run(project.path(), &["init"]));
    assert_success(&run(project.path(), &["swarm", "init"]));
    let start = run(
        project.path(),
        &["swarm", "start", "--objective", "test", "--workers", "1", "--dry-run"],
    );
    assert_success(&start);
    let s = stdout(&start);
    assert!(s.contains("[dry-run]"));
    assert!(s.contains("worker 1"));
}

#[test]
fn agent_spawn_then_list_round_trip() {
    let project = tempfile::tempdir().unwrap();
    assert_success(&run(project.path(), &["init"]));
    let spawn = run(project.path(), &["agent", "spawn", "-t", "coder"]);
    assert_success(&spawn);
    assert!(stdout(&spawn).contains("spawned successfully"));
    let list = run(project.path(), &["agent", "list"]);
    assert_success(&list);
    assert!(stdout(&list).contains("coder"));
}

#[test]
fn task_create_then_list_round_trip() {
    let project = tempfile::tempdir().unwrap();
    assert_success(&run(project.path(), &["init"]));
    // `task create` requires --type (feature/bugfix/research/refactor); a bare
    // --description is rejected with a non-zero exit.
    let missing_type = run(project.path(), &["task", "create", "--description", "test"]);
    assert_eq!(missing_type.status.code(), Some(1));

    let created = run(
        project.path(),
        &["task", "create", "--type", "feature", "--description", "test"],
    );
    assert_success(&created);
    assert!(stdout(&created).contains("created successfully"));

    let list = run(project.path(), &["task", "list"]);
    assert_success(&list);
    assert!(stdout(&list).contains("test"));
}

#[test]
fn session_list_runs_clean_after_init() {
    let project = tempfile::tempdir().unwrap();
    assert_success(&run(project.path(), &["init"]));
    let list = run(project.path(), &["session", "list"]);
    assert_success(&list);
}

// ---- helpers ----------------------------------------------------------------

fn run(root: &Path, args: &[&str]) -> Output {
    let _g = LOCK.lock().unwrap();
    Command::new(executable())
        .current_dir(root)
        .args(args)
        .env("NO_COLOR", "1")
        .output()
        .unwrap()
}
static LOCK: Mutex<()> = Mutex::new(());

fn assert_success(o: &Output) {
    assert_eq!(o.status.code(), Some(0), "stderr: {}", stderr(o));
}
fn stdout(o: &Output) -> String {
    String::from_utf8(o.stdout.clone()).unwrap()
}
fn stderr(o: &Output) -> String {
    String::from_utf8(o.stderr.clone()).unwrap()
}

fn executable() -> PathBuf {
    static BUILT: OnceLock<Mutex<bool>> = OnceLock::new();
    let mut built = BUILT.get_or_init(|| Mutex::new(false)).lock().unwrap();
    if !*built {
        let s = Command::new(env!("CARGO"))
            .current_dir(env!("CARGO_MANIFEST_DIR"))
            .args(["build", "--quiet", "--package", "ruflo", "--bin", "ruflo"])
            .status()
            .unwrap();
        assert!(s.success());
        *built = true;
    }
    std::env::var_os("CARGO_BIN_EXE_ruflo")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("target/debug")
                .join("ruflo")
        })
}
