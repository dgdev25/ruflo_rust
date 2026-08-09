//! End-to-end `daemon stop` tests through the native `ruflo` binary.
//!
//! `daemon stop` (per-workspace) operates only on the workspace's daemon-state
//! file and exits cleanly whether or not a daemon is running. `daemon stop --all`
//! enumerates running ruflo/claude-flow daemon processes via /proc, SIGTERMs
//! them, and exits 0 regardless of how many it found. The budget dir is
//! redirected to a tempdir to avoid writing to the developer's home.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::{Mutex, OnceLock};

#[test]
fn stop_without_running_daemon_marks_stopped() {
    let project = tempfile::tempdir().unwrap();
    let budget = tempfile::tempdir().unwrap();
    let out = run(project.path(), budget.path(), &["daemon", "stop"]);
    assert_eq!(out.status.code(), Some(0));
    // Graceful: reports no daemon found rather than crashing.
    assert!(stdout(&out).contains("No running daemon"));
}

#[test]
fn stop_all_does_not_crash() {
    let project = tempfile::tempdir().unwrap();
    let budget = tempfile::tempdir().unwrap();
    let out = run(project.path(), budget.path(), &["daemon", "stop", "--all"]);
    // The command always exits 0; it may find 0 or more daemon processes
    // depending on what else is running on the machine.
    assert_eq!(out.status.code(), Some(0));
    assert!(stdout(&out).contains("daemon process(es)"));
}

// ---- helpers ----------------------------------------------------------------

fn run(root: &Path, budget_dir: &Path, args: &[&str]) -> Output {
    let _g = LOCK.lock().unwrap();
    Command::new(executable())
        .current_dir(root)
        .args(args)
        .env("NO_COLOR", "1")
        .env("RUFLO_AI_BUDGET_DIR", budget_dir)
        .output()
        .unwrap()
}
static LOCK: Mutex<()> = Mutex::new(());

fn stdout(o: &Output) -> String {
    String::from_utf8(o.stdout.clone()).unwrap()
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
