//! End-to-end smoke tests for six previously-untested native command modules.
//!
//! Each test asserts the dispatcher's overview/status surface runs cleanly
//! (exit 0) under the native `ruflo` binary. `verify local` is the exception:
//! without a local witness manifest it exits 1 with a graceful error rather
//! than crashing.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::{Mutex, OnceLock};

#[test]
fn auth_status_runs_clean() {
    let project = tempfile::tempdir().unwrap();
    let out = run(project.path(), &["auth", "status"]);
    assert_eq!(out.status.code(), Some(0));
    assert!(stdout(&out).contains("Auth Status"));
}

#[test]
fn benchmark_overview_runs_clean() {
    let project = tempfile::tempdir().unwrap();
    // `benchmark overview` is not a valid subcommand; the bare `benchmark`
    // invocation prints the suite overview and exits 0.
    let out = run(project.path(), &["benchmark"]);
    assert_eq!(out.status.code(), Some(0));
    let s = stdout(&out);
    assert!(s.contains("Benchmark"));
    assert!(s.contains("pretrain"));
}

#[test]
fn verify_local_fails_gracefully_without_manifest() {
    let project = tempfile::tempdir().unwrap();
    let out = run(project.path(), &["verify", "local"]);
    // No witness manifest available in the native build → exits 1 with a
    // clear error message rather than panicking or hanging.
    assert_eq!(out.status.code(), Some(1));
    assert!(stderr(&out).contains("witness manifest") || stdout(&out).contains("witness manifest"));
}

#[test]
fn route_list_agents_runs_clean() {
    let project = tempfile::tempdir().unwrap();
    let out = run(project.path(), &["route", "list-agents"]);
    assert_eq!(out.status.code(), Some(0));
}

#[test]
fn providers_list_runs_clean() {
    let project = tempfile::tempdir().unwrap();
    let out = run(project.path(), &["providers", "list"]);
    assert_eq!(out.status.code(), Some(0));
    let s = stdout(&out);
    assert!(s.contains("Anthropic"));
    assert!(s.contains("OpenAI"));
}

#[test]
fn guidance_status_runs_clean() {
    let project = tempfile::tempdir().unwrap();
    let out = run(project.path(), &["guidance", "status"]);
    assert_eq!(out.status.code(), Some(0));
    assert!(stdout(&out).contains("Guidance"));
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
