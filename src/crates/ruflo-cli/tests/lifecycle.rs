use std::fs;

#[path = "../src/lifecycle.rs"]
mod lifecycle;

#[test]
fn init_creates_native_project_state_without_overwriting_existing_config() {
    let temp = tempfile::tempdir().expect("temporary project");
    lifecycle::initialize(temp.path()).expect("initialize project");

    for path in [
        ".claude-flow/config.yaml",
        ".claude-flow/data",
        ".claude-flow/logs",
        ".claude-flow/sessions",
        ".swarm/agents",
        ".swarm/tasks",
        ".agents/config.toml",
    ] {
        assert!(temp.path().join(path).exists(), "{path} should exist");
    }

    fs::write(
        temp.path().join(".claude-flow/config.yaml"),
        "custom: true\n",
    )
    .expect("custom config");
    lifecycle::initialize(temp.path()).expect("idempotent init");
    assert_eq!(
        fs::read_to_string(temp.path().join(".claude-flow/config.yaml")).expect("config"),
        "custom: true\n"
    );
}

#[test]
fn status_requires_initialization_and_counts_native_records() {
    let temp = tempfile::tempdir().expect("temporary project");
    let error = lifecycle::status(temp.path()).expect_err("uninitialized status must fail");
    assert_eq!(error.kind(), std::io::ErrorKind::NotFound);

    lifecycle::initialize(temp.path()).expect("initialize project");
    fs::write(temp.path().join(".swarm/agents/coder.json"), "{}").expect("agent record");
    fs::write(temp.path().join(".swarm/tasks/build.json"), "{}").expect("task record");
    let status = lifecycle::status(temp.path()).expect("project status");
    assert_eq!(status.agents, 1);
    assert_eq!(status.tasks, 1);
}
