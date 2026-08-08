//! End-to-end `eject` command tests through both native binaries (ADR-150 Phase 2).
//!
//! Source: v3/@claude-flow/cli/src/commands/eject.ts. Covers the deterministic
//! local gates (name required, target-inside-repo, target-exists, dry-run plan,
//! --format json). The --confirm metaharness subprocess is network-bound and not
//! exercised here; its degradation contract (metaharness absent → exit 0) is the
//! reference behavior.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::{Mutex, OnceLock};

use serde_json::Value;

#[test]
fn name_required_exits_2() {
    for binary in ["ruflo", "claude-flow"] {
        let project = tempfile::tempdir().unwrap();
        let out = run(binary, project.path(), &["eject"]);
        assert_eq!(out.status.code(), Some(2));
        assert!(stderr(&out).contains("eject: --name is required"));

        // empty --name= is JS-falsy -> also rejected (eject.ts:136).
        let empty = run(binary, project.path(), &["eject", "--name", ""]);
        assert_eq!(empty.status.code(), Some(2));
        assert!(stderr(&empty).contains("eject: --name is required"));
    }
}

#[test]
fn target_inside_repo_refused_exit_2() {
    for binary in ["ruflo", "claude-flow"] {
        let project = tempfile::tempdir().unwrap();
        let inside = project.path().join("scaffold");
        let out = run(
            binary,
            project.path(),
            &[
                "eject",
                "--name",
                "h",
                "--target",
                &inside.display().to_string(),
            ],
        );
        assert_eq!(out.status.code(), Some(2));
        assert!(stderr(&out).contains("refusing to write"));
        assert!(stderr(&out).contains("inside the calling repo"));
    }
}

#[test]
fn target_exists_refused_exit_2() {
    for binary in ["ruflo", "claude-flow"] {
        let project = tempfile::tempdir().unwrap();
        let existing = tempfile::tempdir().unwrap();
        let out = run(
            binary,
            project.path(),
            &[
                "eject",
                "--name",
                "h",
                "--target",
                &existing.path().display().to_string(),
            ],
        );
        assert_eq!(out.status.code(), Some(2));
        assert!(stderr(&out).contains("already exists"));
    }
}

#[test]
fn dry_run_table_plan_exit_0() {
    for binary in ["ruflo", "claude-flow"] {
        let project = tempfile::tempdir().unwrap();
        let out = run(binary, project.path(), &["eject", "--name", "my-harness"]);
        assert_eq!(out.status.code(), Some(0));
        let s = stdout(&out);
        assert!(s.contains("# ruflo eject (dry-run)"));
        assert!(s.contains("name:       my-harness"));
        assert!(s.contains("Would execute:"));
        assert!(s.contains("metaharness@latest --from-existing"));
        assert!(s.contains("Re-run with --confirm to actually eject."));
    }
}

#[test]
fn dry_run_json_plan_shape() {
    for binary in ["ruflo", "claude-flow"] {
        let project = tempfile::tempdir().unwrap();
        let out = run(
            binary,
            project.path(),
            &["eject", "--name", "h", "--format", "json"],
        );
        assert_eq!(out.status.code(), Some(0));
        let v: Value = serde_json::from_str(stdout(&out).trim()).unwrap();
        assert_eq!(v["name"], "h");
        assert_eq!(v["dryRun"], true);
        assert_eq!(v["confirm"], false);
        assert!(v["command"]
            .as_str()
            .unwrap()
            .contains("metaharness@latest"));
        assert!(v["target"].as_str().unwrap().contains("ruflo-eject-"));
    }
}

#[test]
fn outside_target_is_accepted_in_dry_run() {
    for binary in ["ruflo", "claude-flow"] {
        let project = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        // outside.path() exists -> refused; use a sibling non-existent path
        let fresh = outside.path().join("fresh-harness");
        let out = run(
            binary,
            project.path(),
            &[
                "eject",
                "--name",
                "h",
                "--target",
                &fresh.display().to_string(),
            ],
        );
        assert_eq!(out.status.code(), Some(0));
        assert!(stdout(&out).contains("# ruflo eject (dry-run)"));
    }
}

#[test]
fn help_exits_zero() {
    for binary in ["ruflo", "claude-flow"] {
        let project = tempfile::tempdir().unwrap();
        let out = run(binary, project.path(), &["eject", "--help"]);
        assert_eq!(out.status.code(), Some(0));
        assert!(stdout(&out).contains("--name"));
        assert!(stdout(&out).contains("--confirm"));
    }
}

fn run(binary: &str, cwd: &Path, args: &[&str]) -> Output {
    let _g = LOCK.lock().unwrap();
    Command::new(executable(binary))
        .current_dir(cwd)
        .args(args)
        .env("NO_COLOR", "1")
        .env_remove("RUFLO_FUNNEL")
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
