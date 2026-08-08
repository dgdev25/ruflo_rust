//! End-to-end `issues` command tests through both native binaries (ADR-016).

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::{Mutex, OnceLock};

#[test]
fn claim_release_status_steal_list_roundtrip() {
    for binary in ["ruflo", "claude-flow"] {
        let project = tempfile::tempdir().unwrap();
        // empty list
        let empty = run(binary, project.path(), &["issues", "list"]);
        assert_eq!(empty.status.code(), Some(0));
        assert!(stdout(&empty).contains("No claims found"));
        // claim with type:id
        let claim = run(
            binary,
            project.path(),
            &["issues", "claim", "-i", "42", "-a", "coder:bot-1"],
        );
        assert_eq!(claim.status.code(), Some(0));
        assert!(stdout(&claim).contains("Claimed issue 42"));
        // double-claim rejected
        let dup = run(
            binary,
            project.path(),
            &["issues", "claim", "-i", "42", "-a", "coder:bot-2"],
        );
        assert_eq!(dup.status.code(), Some(1));
        assert!(stderr(&dup).contains("already claimed"));
        // list shows it
        let list = run(binary, project.path(), &["issues", "list"]);
        assert_eq!(list.status.code(), Some(0));
        assert!(stdout(&list).contains("42"));
        assert!(stdout(&list).contains("active"));
        // status completed sets progress=100
        let st = run(
            binary,
            project.path(),
            &["issues", "status", "-i", "42", "--set", "completed"],
        );
        assert_eq!(st.status.code(), Some(0));
        assert!(stdout(&st).contains("completed"));
        // verify progress via JSON
        let js = run(binary, project.path(), &["issues", "list", "--json"]);
        let v: serde_json::Value = serde_json::from_str(stdout(&js).trim()).unwrap();
        assert_eq!(v[0]["progress"], 100);
        // status blocked sets blockReason
        let blk = run(
            binary,
            project.path(),
            &[
                "issues",
                "status",
                "-i",
                "42",
                "--set",
                "blocked",
                "-n",
                "CI failure",
            ],
        );
        assert_eq!(blk.status.code(), Some(0));
        let js2 = run(binary, project.path(), &["issues", "list", "--json"]);
        let v2: serde_json::Value = serde_json::from_str(stdout(&js2).trim()).unwrap();
        assert_eq!(v2[0]["blockReason"], "CI failure");
        // mark stealable then steal
        let _ = run(
            binary,
            project.path(),
            &["issues", "status", "-i", "42", "--set", "stealable"],
        );
        let steal = run(
            binary,
            project.path(),
            &["issues", "steal", "-i", "42", "-a", "coder:bot-3"],
        );
        assert_eq!(steal.status.code(), Some(0));
        assert!(stdout(&steal).contains("Stolen issue 42"));
        // steal non-stealable rejected
        let nosteal = run(
            binary,
            project.path(),
            &["issues", "steal", "-i", "42", "-a", "coder:bot-4"],
        );
        assert_eq!(nosteal.status.code(), Some(1));
        assert!(stderr(&nosteal).contains("not stealable"));
        // handoff via --to
        let ho = run(
            binary,
            project.path(),
            &[
                "issues",
                "handoff",
                "-i",
                "42",
                "--to",
                "agent:reviewer:rev-1",
            ],
        );
        assert_eq!(ho.status.code(), Some(0));
        assert!(stdout(&ho).contains("Handoff requested"));
        // release
        let rel = run(
            binary,
            project.path(),
            &["issues", "release", "-i", "42", "-a", "coder:bot-3"],
        );
        assert_eq!(rel.status.code(), Some(0));
        assert!(stdout(&rel).contains("Released issue 42"));
        // release by non-owner rejected
        run(
            binary,
            project.path(),
            &["issues", "claim", "-i", "99", "-a", "coder:bot-5"],
        );
        let badrel = run(
            binary,
            project.path(),
            &["issues", "release", "-i", "99", "-a", "coder:bot-6"],
        );
        assert_eq!(badrel.status.code(), Some(1));
        assert!(stderr(&badrel).contains("not claimed by"));
        // invalid status
        let badst = run(
            binary,
            project.path(),
            &["issues", "status", "-i", "99", "--set", "bogus"],
        );
        assert_eq!(badst.status.code(), Some(1));
        assert!(stderr(&badst).contains("Invalid status"));
        // list empty again
        run(
            binary,
            project.path(),
            &["issues", "release", "-i", "99", "-a", "coder:bot-5"],
        );
        let empty2 = run(binary, project.path(), &["issues", "list"]);
        assert!(stdout(&empty2).contains("No claims found"));
        // verify savedAt present
        let raw = std::fs::read_to_string(project.path().join(".claude-flow/claims/claims.json"))
            .unwrap();
        let f: serde_json::Value = serde_json::from_str(&raw).unwrap();
        assert!(f["savedAt"].is_string());
        assert!(f["claims"].is_array());
        // no .tmp left
        assert!(!project
            .path()
            .join(".claude-flow/claims/claims.json.tmp")
            .exists());
    }
}

#[test]
fn help_and_unknown_subcommand() {
    for binary in ["ruflo", "claude-flow"] {
        let project = tempfile::tempdir().unwrap();
        let help = run(binary, project.path(), &["issues", "--help"]);
        assert_eq!(help.status.code(), Some(0));
        assert!(stdout(&help).contains("SUBCOMMANDS"));
        assert!(stdout(&help).contains("claim"));
        let bad = run(binary, project.path(), &["issues", "frobnicate"]);
        assert_ne!(bad.status.code(), Some(0));
        assert!(stderr(&bad).contains("Unknown subcommand"));
    }
}

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
