//! End-to-end swarm exec env-handling tests through the native `ruflo` binary.
//!
//! Verifies that `swarm start --dry-run` succeeds with credential env vars set
//! (dry-run never spawns, so no sanitization risk) and that an unknown `--agent`
//! value is rejected when actually spawning (non-dry-run). The budget dir is
//! redirected to a tempdir so the spawn-attempt path doesn't touch $HOME.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::{Mutex, OnceLock};

#[test]
fn dry_run_succeeds_with_credential_env_vars_set() {
    let project = tempfile::tempdir().unwrap();
    let budget = tempfile::tempdir().unwrap();
    assert_success(&run(project.path(), budget.path(), &["init"]));
    assert_success(&run(project.path(), budget.path(), &["swarm", "init"]));
    let start = run_with_env(
        project.path(),
        budget.path(),
        &["swarm", "start", "--objective", "test", "--workers", "1", "--dry-run"],
        &[("API_KEY", "sk-test-12345"), ("AUTH_TOKEN", "tok-abc")],
    );
    assert_success(&start);
    assert!(stdout(&start).contains("[dry-run]"));
}

#[test]
fn unknown_agent_is_rejected_when_spawning() {
    let project = tempfile::tempdir().unwrap();
    let budget = tempfile::tempdir().unwrap();
    assert_success(&run(project.path(), budget.path(), &["init"]));
    assert_success(&run(project.path(), budget.path(), &["swarm", "init"]));
    // Non-dry-run with a bogus agent: agent_command() rejects "bogus" and the
    // worker fails with the "Unknown agent" message. Dry-run skips validation,
    // so this must be the spawning path to exercise the rejection.
    let out = run(
        project.path(),
        budget.path(),
        &["swarm", "start", "--objective", "test", "--workers", "1", "--agent", "bogus"],
    );
    assert_ne!(out.status.code(), Some(0));
    assert!(
        stdout(&out).contains("Unknown agent 'bogus'"),
        "expected Unknown agent rejection, got: {}",
        stdout(&out)
    );
}

// ---- helpers ----------------------------------------------------------------

fn run(root: &Path, budget_dir: &Path, args: &[&str]) -> Output {
    run_with_env(root, budget_dir, args, &[])
}

fn run_with_env(root: &Path, budget_dir: &Path, args: &[&str], env: &[(&str, &str)]) -> Output {
    let _g = LOCK.lock().unwrap();
    let mut cmd = Command::new(executable());
    cmd.current_dir(root)
        .args(args)
        .env("NO_COLOR", "1")
        .env("RUFLO_AI_BUDGET_DIR", budget_dir);
    for (k, v) in env {
        cmd.env(k, v);
    }
    cmd.output().unwrap()
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
