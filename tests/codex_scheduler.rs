#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;
use tempfile::TempDir;

fn run(command: &str, args: &[&str], cwd: &Path) {
    let status = Command::new(command)
        .args(args)
        .current_dir(cwd)
        .status()
        .unwrap();
    assert!(status.success(), "{command} {args:?} failed");
}

fn binary() -> PathBuf {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let status = Command::new(env!("CARGO"))
        .current_dir(root)
        .args([
            "build",
            "--quiet",
            "--package",
            "claude-flow-codex",
            "--bin",
            "claude-flow-codex",
        ])
        .status()
        .unwrap();
    assert!(status.success());
    root.join("target/debug/claude-flow-codex")
}

fn clean_git_project() -> TempDir {
    let project = tempfile::tempdir().unwrap();
    run("git", &["init", "--quiet"], project.path());
    run(
        "git",
        &["config", "user.email", "scheduler@example.test"],
        project.path(),
    );
    run(
        "git",
        &["config", "user.name", "Scheduler Test"],
        project.path(),
    );
    fs::create_dir(project.path().join(".agents")).unwrap();
    fs::write(
        project.path().join(".agents/config.toml"),
        "[swarm.automation]\nenabled = true\nmax_concurrent = 2\nmax_writers = 1\nagent_timeout_seconds = 30\nmax_output_bytes = 1024\n",
    )
    .unwrap();
    fs::write(project.path().join("README.md"), "scheduler fixture\n").unwrap();
    run("git", &["add", "."], project.path());
    run(
        "git",
        &["commit", "--quiet", "-m", "fixture"],
        project.path(),
    );
    project
}

#[test]
fn opt_in_scheduler_uses_a_worktree_and_direct_codex_exec_arguments() {
    let project = clean_git_project();
    let fixture: Value = serde_json::from_str(
        &fs::read_to_string("tests/fixtures/codex/dual-run-codex-worker-launch.json").unwrap(),
    )
    .unwrap();
    let fixture_dir = tempfile::tempdir().unwrap();
    let args_file = fixture_dir.path().join("args.txt");
    let executable = fixture_dir.path().join("fake-codex");
    fs::write(
        &executable,
        "#!/bin/sh\nprintf '%s\\n' \"$@\" > \"$CODEX_ARGS_FILE\"\n[ -z \"$RUFLO_TEST_SECRET\" ]\n",
    )
    .unwrap();
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o755)).unwrap();

    let args = fixture["argv"]
        .as_array()
        .unwrap()
        .iter()
        .map(|value| value.as_str().unwrap())
        .collect::<Vec<_>>();
    let output = Command::new(binary())
        .current_dir(project.path())
        .env("RUFLO_CODEX_EXECUTABLE", &executable)
        .env("CODEX_ARGS_FILE", &args_file)
        .env("RUFLO_TEST_SECRET", "must-not-reach-child")
        .args(args)
        .output()
        .unwrap();

    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let child_args = fs::read_to_string(&args_file).unwrap();
    let lines = child_args.lines().collect::<Vec<_>>();
    let expected_prefix = fixture["worker_launch"]["argv_prefix"]
        .as_array()
        .unwrap()
        .iter()
        .map(|value| value.as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(&lines[..expected_prefix.len()], expected_prefix);
    for clause in fixture["worker_launch"]["prompt_contains"]
        .as_array()
        .unwrap()
    {
        assert!(child_args.contains(clause.as_str().unwrap()));
    }

    let registry = project.path().join(".claude-flow/swarm/worktrees");
    let record = fs::read_dir(registry)
        .unwrap()
        .find_map(Result::ok)
        .map(|entry| fs::read_to_string(entry.path()).unwrap())
        .unwrap();
    assert!(record.contains("\"agent_id\": \"coder\""));
    assert!(record.contains("\"read_only\": false"));
    assert!(project.path().join(".swarm/memory.db").exists());
}

#[test]
fn scheduler_refuses_worker_execution_without_opt_in_automation() {
    let project = tempfile::tempdir().unwrap();
    let output = Command::new(binary())
        .current_dir(project.path())
        .args(["dual", "run", "--worker", "codex:coder:Should never run"])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("unattended swarm automation is disabled")
    );
    assert!(!project.path().join(".swarm").exists());
}
