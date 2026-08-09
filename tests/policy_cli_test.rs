//! End-to-end `policy` CLI tests through the native `ruflo` binary (ADR-324).
//!
//! Covers the read-only status surface, rule list (empty + seeded), audit
//! ledger, and the interactive-terminal guard on rule mutation. Mutation ops
//! (`rule add`, `budget set`, `approve`, `revoke`) intentionally require a TTY
//! so non-interactive callers fail closed.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::{Mutex, OnceLock};

#[test]
fn status_reports_legacy_mode_with_empty_ledger() {
    let project = tempfile::tempdir().unwrap();
    let out = run(project.path(), &["policy", "status"]);
    assert_eq!(out.status.code(), Some(0));
    let v: serde_json::Value = serde_json::from_str(&stdout(&out)).unwrap();
    assert_eq!(v["mode"].as_str(), Some("legacy"));
    assert_eq!(v["rules"].as_u64(), Some(0));
    assert_eq!(v["ledger"]["valid"], true);
}

#[test]
fn rule_list_returns_empty_array_by_default() {
    let project = tempfile::tempdir().unwrap();
    let out = run(project.path(), &["policy", "rule", "list"]);
    assert_eq!(out.status.code(), Some(0));
    let v: serde_json::Value = serde_json::from_str(&stdout(&out)).unwrap();
    assert!(v["rules"].as_array().unwrap().is_empty());
}

#[test]
fn audit_returns_empty_receipts() {
    let project = tempfile::tempdir().unwrap();
    let out = run(project.path(), &["policy", "audit"]);
    assert_eq!(out.status.code(), Some(0));
    let v: serde_json::Value = serde_json::from_str(&stdout(&out)).unwrap();
    assert!(v["receipts"].as_array().unwrap().is_empty());
}

#[test]
fn rule_add_fails_closed_without_interactive_tty() {
    let project = tempfile::tempdir().unwrap();
    // The test harness pipes stdin/stdout, so the interactive guard rejects
    // the mutation. `policy rule add <json>` is the correct positional shape;
    // --name/--effect flags are not supported by this CLI.
    let out = run(
        project.path(),
        &["policy", "rule", "add", "{\"id\":\"test-rule\",\"effect\":\"deny\"}"],
    );
    assert_eq!(out.status.code(), Some(1));
    assert!(
        stderr(&out).contains("interactive local terminal"),
        "expected interactive-terminal guard, got: {}",
        stderr(&out)
    );
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
