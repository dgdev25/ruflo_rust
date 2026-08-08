use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::{Mutex, OnceLock};

use serde::Deserialize;
use serde_json::Value;

#[derive(Deserialize)]
struct Fixture {
    source: String,
    source_sha256: String,
    environment: BTreeMap<String, String>,
    cases: Vec<FixtureCase>,
}

#[derive(Deserialize)]
struct FixtureCase {
    argv: Vec<String>,
    exit: i32,
    stdout: String,
    stderr: String,
}

#[test]
fn both_native_binaries_match_stable_source_differential_fixtures() {
    let fixture: Fixture = serde_json::from_str(
        &std::fs::read_to_string("tests/fixtures/cli/deployment/v3.json").unwrap(),
    )
    .unwrap();
    assert_eq!(
        fixture.source,
        "v3/@claude-flow/cli/src/commands/deployment.ts"
    );
    assert_eq!(
        fixture.source_sha256,
        "be3867a465b6a124f6cb0e8e39dc51e49ccdd207f5ad7318dc2e9e9aac926ea7"
    );
    for binary in ["ruflo", "claude-flow"] {
        for case in &fixture.cases {
            let project = tempfile::tempdir().unwrap();
            let output = Command::new(executable(binary))
                .current_dir(project.path())
                .args(&case.argv)
                .envs(&fixture.environment)
                .output()
                .unwrap();
            assert_eq!(
                output.status.code(),
                Some(case.exit),
                "{binary} {:?}",
                case.argv
            );
            assert_eq!(
                String::from_utf8(output.stdout).unwrap(),
                case.stdout,
                "{binary} {:?} stdout",
                case.argv
            );
            assert_eq!(
                String::from_utf8(output.stderr).unwrap(),
                case.stderr,
                "{binary} {:?} stderr",
                case.argv
            );
            assert!(!project.path().join(".claude-flow").exists());
        }
    }
}

#[test]
fn both_native_binaries_cover_the_full_durable_deployment_lifecycle() {
    for binary in ["ruflo", "claude-flow"] {
        let project = tempfile::tempdir().unwrap();
        std::fs::write(
            project.path().join("package.json"),
            r#"{"name":"fixture","version":"9.4.1"}"#,
        )
        .unwrap();

        let dry = run(
            binary,
            project.path(),
            &[
                "deploy",
                "deploy",
                "--env=preview",
                "--dry-run",
                "--description",
                "preview only",
            ],
        );
        assert_success(&dry);
        assert!(stdout(&dry).contains("Deployment Preview"));
        assert!(stdout(&dry).contains("9.4.1"));
        assert_eq!(stderr(&dry), "[INFO] Dry run - no changes will be made\n");
        assert!(
            !state_path(project.path()).exists(),
            "dry run mutated state"
        );

        assert_success(&run(
            binary,
            project.path(),
            &[
                "deployment",
                "envs",
                "-a",
                "add",
                "-n",
                "preview",
                "-t",
                "staging",
                "-u",
                "https://preview.test",
            ],
        ));
        assert_atomic_state(project.path());
        let state = read_state(project.path());
        assert_eq!(state["environments"]["preview"]["type"], "staging");
        assert_eq!(
            state["environments"]["preview"]["url"],
            "https://preview.test"
        );

        let duplicate = run(
            binary,
            project.path(),
            &["deployment", "environments", "-a", "add", "-n", "preview"],
        );
        assert_eq!(duplicate.status.code(), Some(1));
        assert_eq!(
            stderr(&duplicate),
            "[WARN] Environment 'preview' already exists\n"
        );

        let first = run(
            binary,
            project.path(),
            &[
                "deployment",
                "deploy",
                "-e",
                "prod",
                "-v",
                "1.0.0",
                "--description",
                "first",
            ],
        );
        assert_success(&first);
        assert!(stdout(&first).contains("[OK] Deployed version 1.0.0 to prod"));

        let release = run(
            binary,
            project.path(),
            &["deployment", "release", "-v", "2.0.0", "-e", "prod"],
        );
        assert_success(&release);
        assert!(stdout(&release).contains("Release 2.0.0"));

        let third = run(
            binary,
            project.path(),
            &["deployment", "deploy", "-e", "prod", "-v", "3.0.0"],
        );
        assert_success(&third);
        assert_atomic_state(project.path());

        let rollback = run(
            binary,
            project.path(),
            &["deployment", "rollback", "-e", "prod", "--steps", "7"],
        );
        assert_success(&rollback);
        assert!(stdout(&rollback).contains("[OK] Rolled back prod to version 2.0.0"));

        let state = read_state(project.path());
        let history = state["history"].as_array().unwrap();
        assert_eq!(history.len(), 4);
        assert_eq!(history[2]["status"], "rolled-back");
        assert_eq!(history[3]["version"], "2.0.0");
        assert!(valid_deployment_id(history[3]["id"].as_str().unwrap()));
        assert!(valid_timestamp(history[3]["timestamp"].as_str().unwrap()));
        assert_eq!(state["activeDeployment"], history[3]["id"]);

        let status = run(
            binary,
            project.path(),
            &["deployment", "status", "-e", "prod"],
        );
        assert_success(&status);
        assert!(stdout(&status).contains("Recent Deployments"));
        assert!(stderr(&status).contains("[INFO] Active deployment:"));

        let history_output = run(
            binary,
            project.path(),
            &["deployment", "history", "-e", "prod", "-l", "2"],
        );
        assert_success(&history_output);
        assert!(stdout(&history_output).contains("Showing 2 of 4 total records"));

        let active = state["activeDeployment"].as_str().unwrap();
        let logs = run(
            binary,
            project.path(),
            &["deployment", "logs", "-d", active, "-n", "1"],
        );
        assert_success(&logs);
        assert!(stdout(&logs).contains("1 entries shown"));
        assert!(stdout(&logs).contains(active));

        let missing = run(
            binary,
            project.path(),
            &["deployment", "logs", "-d", "dep-missing"],
        );
        assert_eq!(missing.status.code(), Some(1));
        assert!(stderr(&missing).contains("Deployment 'dep-missing' not found"));

        let targeted = run(
            binary,
            project.path(),
            &["deployment", "rollback", "-e", "prod", "-v", "1.0.0"],
        );
        assert_success(&targeted);
        assert!(stdout(&targeted).contains("to version 1.0.0"));

        assert_success(&run(
            binary,
            project.path(),
            &["deployment", "envs", "-a", "remove", "-n", "preview"],
        ));
        let state = read_state(project.path());
        assert!(state["environments"].get("preview").is_none());
        assert!(state["environments"].get("prod").is_some());
        assert_eq!(state["history"].as_array().unwrap().len(), 5);
        assert_atomic_state(project.path());
    }
}

#[test]
fn malformed_state_is_read_as_empty_and_replaced_atomically_on_mutation() {
    for binary in ["ruflo", "claude-flow"] {
        let project = tempfile::tempdir().unwrap();
        std::fs::create_dir(project.path().join(".claude-flow")).unwrap();
        std::fs::write(state_path(project.path()), "{broken").unwrap();
        let output = run(
            binary,
            project.path(),
            &["deployment", "release", "-v", "4.0.0"],
        );
        assert_success(&output);
        let state = read_state(project.path());
        assert_eq!(state["history"].as_array().unwrap().len(), 1);
        assert_eq!(state["history"][0]["version"], "4.0.0");
        assert_atomic_state(project.path());
    }
}

fn assert_atomic_state(root: &Path) {
    let state = state_path(root);
    assert!(state.is_file());
    assert!(!PathBuf::from(format!("{}.tmp", state.display())).exists());
    let bytes = std::fs::read(&state).unwrap();
    assert!(
        !bytes.ends_with(b"\n"),
        "source JSON.stringify has no final newline"
    );
    serde_json::from_slice::<Value>(&bytes).unwrap();
}

fn valid_deployment_id(value: &str) -> bool {
    let parts = value.split('-').collect::<Vec<_>>();
    parts.len() == 3
        && parts[0] == "dep"
        && !parts[1].is_empty()
        && parts[1]
            .chars()
            .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit())
        && parts[2].len() == 6
        && parts[2]
            .chars()
            .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit())
}

fn valid_timestamp(value: &str) -> bool {
    value.len() == 24
        && value.as_bytes().get(4) == Some(&b'-')
        && value.as_bytes().get(10) == Some(&b'T')
        && value.as_bytes().get(23) == Some(&b'Z')
}

fn read_state(root: &Path) -> Value {
    serde_json::from_str(&std::fs::read_to_string(state_path(root)).unwrap()).unwrap()
}

fn state_path(root: &Path) -> PathBuf {
    root.join(".claude-flow/deployments.json")
}

fn run(binary: &str, root: &Path, args: &[&str]) -> Output {
    Command::new(executable(binary))
        .current_dir(root)
        .args(args)
        .env("NO_COLOR", "1")
        .output()
        .unwrap()
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
