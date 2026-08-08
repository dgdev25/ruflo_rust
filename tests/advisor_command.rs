//! End-to-end `advisor` command tests through both native binaries.
//!
//! Source of truth: `v3/@claude-flow/cli/src/commands/advisor.ts` + funnel
//! consent/state/advisor-tip modules (ADR-302/305/316). Uses RUFLO_STATE_DIR to
//! isolate the user-level state so tests never touch real ~/.ruflo.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::{Mutex, OnceLock};

#[test]
fn status_reports_not_granted_by_default() {
    for binary in ["ruflo", "claude-flow"] {
        let (project, _state) = isolated();
        let out = run(
            binary,
            project.path(),
            &["advisor", "status"],
            _state.path(),
        );
        assert_success(&out);
        assert_eq!(
            stdout(&out),
            "Advisor tip consent: not granted\n",
            "{binary}"
        );
    }
}

#[test]
fn enable_requires_yes_then_records_consent() {
    for binary in ["ruflo", "claude-flow"] {
        let (project, state) = isolated();

        // Without --yes: prints disclosure + confirmation hint, no consent recorded.
        let prompt = run(binary, project.path(), &["advisor", "enable"], state.path());
        assert_success(&prompt);
        assert!(stdout(&prompt).contains("Enabling the co-pilot advisor tip."));
        assert!(stdout(&prompt).contains("Re-run with --yes to confirm"));
        assert!(!consent_granted(state.path(), "advisor-tips"));

        // With --yes: records consent, success message.
        let confirm = run(
            binary,
            project.path(),
            &["advisor", "enable", "--yes"],
            state.path(),
        );
        assert_success(&confirm);
        assert!(stdout(&confirm).contains("Advisor tip enabled."));
        assert!(consent_granted(state.path(), "advisor-tips"));

        // Idempotent: already enabled short-circuits.
        let again = run(
            binary,
            project.path(),
            &["advisor", "enable", "--yes"],
            state.path(),
        );
        assert_success(&again);
        assert_eq!(stdout(&again), "Advisor tip is already enabled.\n");
    }
}

#[test]
fn disable_revokes_consent_and_status_reflects_it() {
    for binary in ["ruflo", "claude-flow"] {
        let (project, state) = isolated();
        run(
            binary,
            project.path(),
            &["advisor", "enable", "--yes"],
            state.path(),
        );
        assert!(consent_granted(state.path(), "advisor-tips"));

        let disable = run(
            binary,
            project.path(),
            &["advisor", "disable"],
            state.path(),
        );
        assert_success(&disable);
        assert!(stdout(&disable).contains("Advisor tip disabled."));
        assert!(!consent_granted(state.path(), "advisor-tips"));

        let status = run(binary, project.path(), &["advisor", "status"], state.path());
        assert_eq!(stdout(&status), "Advisor tip consent: not granted\n");
    }
}

#[test]
fn default_action_is_status_and_help_works() {
    for binary in ["ruflo", "claude-flow"] {
        let (project, state) = isolated();
        let default = run(binary, project.path(), &["advisor"], state.path());
        assert_success(&default);
        assert_eq!(stdout(&default), "Advisor tip consent: not granted\n");

        let help = run(binary, project.path(), &["advisor", "--help"], state.path());
        assert_success(&help);
        assert!(stdout(&help).contains("SUBCOMMANDS"));
        assert!(stdout(&help).contains("enable"));

        let enable_help = run(
            binary,
            project.path(),
            &["advisor", "enable", "-h"],
            state.path(),
        );
        assert_success(&enable_help);
        assert!(stdout(&enable_help).contains("--yes"));
    }
}

#[test]
fn status_shows_cached_tip_when_present_and_fresh() {
    for binary in ["ruflo", "claude-flow"] {
        let (project, state) = isolated();
        // Consent first, then seed a fresh cached tip.
        run(
            binary,
            project.path(),
            &["advisor", "enable", "--yes"],
            state.path(),
        );
        std::fs::write(
            state.path().join("advisor-tip.json"),
            serde_json::json!({
                "_ts": now_millis(),
                "headline": "Reduce context",
                "detail": "Compact long transcripts"
            })
            .to_string(),
        )
        .unwrap();

        let status = run(binary, project.path(), &["advisor", "status"], state.path());
        assert_success(&status);
        let out = stdout(&status);
        assert!(out.contains("Advisor tip consent: granted"));
        assert!(out.contains("Current tip: Reduce context"));
        assert!(out.contains("Compact long transcripts"));
    }
}

#[test]
fn unknown_subcommand_errors() {
    for binary in ["ruflo", "claude-flow"] {
        let (project, state) = isolated();
        let bad = run(
            binary,
            project.path(),
            &["advisor", "frobnicate"],
            state.path(),
        );
        assert_ne!(bad.status.code(), Some(0));
        assert!(stderr(&bad).contains("Unknown subcommand 'frobnicate'"));
    }
}

fn consent_granted(state_dir: &Path, domain: &str) -> bool {
    let path = state_dir.join("consent.json");
    let Ok(raw) = std::fs::read_to_string(&path) else {
        return false;
    };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&raw) else {
        return false;
    };
    let r = v.get(domain).unwrap_or(&serde_json::Value::Null);
    r.get("granted").and_then(|g| g.as_bool()) == Some(true)
        && r.get("policyVersion").and_then(|p| p.as_u64()) == Some(1)
        && r.get("at").and_then(|a| a.as_str()).is_some()
}

fn now_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}

/// Returns (project_tmpdir, state_dir). state_dir is a unique tempdir whose path
/// is fed to the binary via RUFLO_STATE_DIR; project is the CWD.
fn isolated() -> (tempfile::TempDir, tempfile::TempDir) {
    let project = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    (project, state)
}

fn assert_success(output: &Output) {
    assert_eq!(output.status.code(), Some(0), "stderr: {}", stderr(output));
}

fn stdout(output: &Output) -> String {
    String::from_utf8(output.stdout.clone()).unwrap()
}

fn stderr(output: &Output) -> String {
    String::from_utf8(output.stderr.clone()).unwrap()
}

#[allow(clippy::too_many_arguments)]
fn run(binary: &str, root: &Path, args: &[&str], state_dir: &Path) -> Output {
    Command::new(executable(binary))
        .current_dir(root)
        .args(args)
        .env("NO_COLOR", "1")
        .env("RUFLO_STATE_DIR", state_dir)
        .output()
        .unwrap()
}

fn executable(binary: &str) -> PathBuf {
    static BUILT: OnceLock<Mutex<Vec<String>>> = OnceLock::new();
    let mut built = BUILT.get_or_init(|| Mutex::new(Vec::new())).lock().unwrap();
    if !built.iter().any(|name| name == binary) {
        let status = Command::new(env!("CARGO"))
            .current_dir(env!("CARGO_MANIFEST_DIR"))
            .args(["build", "--quiet", "--package", binary, "--bin", binary])
            .status()
            .unwrap();
        assert!(status.success(), "failed to build {binary}");
        built.push(binary.to_string());
    }
    std::env::var_os(format!("CARGO_BIN_EXE_{}", binary.replace('-', "_")))
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("target/debug")
                .join(binary)
        })
}
