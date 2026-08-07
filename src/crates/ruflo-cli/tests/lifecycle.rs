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

#[test]
fn agents_are_durable_project_scoped_records() {
    let temp = tempfile::tempdir().expect("temporary project");
    lifecycle::initialize(temp.path()).expect("initialize project");
    let agent = lifecycle::spawn_agent(temp.path(), "coder", "coder-1").expect("spawn agent");
    assert_eq!(agent.status, "idle");
    assert_eq!(
        lifecycle::list_agents(temp.path()).expect("list agents"),
        vec![agent]
    );
    assert!(lifecycle::spawn_agent(temp.path(), "coder", "coder-1").is_err());
    assert!(lifecycle::spawn_agent(temp.path(), "coder", "../../escape").is_err());
}

#[test]
fn agent_status_stop_and_metrics_are_durable() {
    let temp = tempfile::tempdir().expect("temporary project");
    lifecycle::initialize(temp.path()).expect("initialize project");
    lifecycle::spawn_agent(temp.path(), "coder", "coder-1").expect("spawn agent");
    assert_eq!(
        lifecycle::get_agent(temp.path(), "coder-1")
            .expect("status")
            .status,
        "idle"
    );
    let stopped = lifecycle::stop_agent(temp.path(), "coder-1").expect("stop agent");
    assert_eq!(stopped.status, "terminated");
    assert_eq!(
        lifecycle::agent_metrics(temp.path(), "7d")
            .expect("metrics")
            .terminated_agents,
        1
    );
    assert!(lifecycle::agent_metrics(temp.path(), "forever").is_err());
    assert!(lifecycle::get_agent(temp.path(), "../../escape").is_err());
}

#[test]
fn agent_pool_and_health_are_project_scoped() {
    let temp = tempfile::tempdir().unwrap();
    lifecycle::initialize(temp.path()).unwrap();
    lifecycle::spawn_agent(temp.path(), "coder", "coder-1").unwrap();
    let pool = lifecycle::configure_agent_pool(temp.path(), Some(2), 1, 3, true).unwrap();
    assert_eq!(pool.current_size, 2);
    assert!(temp.path().join(".swarm/agent-pool.json").is_file());
    assert_eq!(
        lifecycle::agent_health(temp.path(), Some("coder-1")).unwrap()[0].1,
        "healthy"
    );
    assert!(lifecycle::configure_agent_pool(temp.path(), Some(4), 1, 3, true).is_err());
}

#[test]
fn agent_logs_persist_lifecycle_events() {
    let temp = tempfile::tempdir().unwrap();
    lifecycle::initialize(temp.path()).unwrap();
    lifecycle::spawn_agent(temp.path(), "coder", "coder-1").unwrap();
    lifecycle::stop_agent(temp.path(), "coder-1").unwrap();
    assert_eq!(
        lifecycle::agent_logs(temp.path(), "coder-1", 10, "info", None)
            .unwrap()
            .len(),
        2
    );
}

#[test]
fn agent_logs_apply_v3_minimum_level_and_since_filters() {
    let temp = tempfile::tempdir().unwrap();
    lifecycle::initialize(temp.path()).unwrap();
    lifecycle::spawn_agent(temp.path(), "coder", "coder-1").unwrap();
    fs::write(
        temp.path().join(".swarm/logs/coder-1.jsonl"),
        concat!(
            r#"{"timestamp_ms":1,"level":"debug","message":"old debug"}"#,
            "\n",
            r#"{"timestamp_ms":1,"level":"info","message":"old info"}"#,
            "\n",
            r#"{"timestamp_ms":1,"level":"warn","message":"warn"}"#,
            "\n",
            r#"{"timestamp_ms":1,"level":"error","message":"error"}"#,
            "\n",
        ),
    )
    .unwrap();

    let warnings = lifecycle::agent_logs(temp.path(), "coder-1", 10, "warn", None).unwrap();
    assert_eq!(
        warnings
            .iter()
            .map(|entry| entry.level.as_str())
            .collect::<Vec<_>>(),
        ["error", "warn"]
    );
    assert!(
        lifecycle::agent_logs(temp.path(), "coder-1", 10, "debug", Some("30m"))
            .unwrap()
            .is_empty()
    );
    assert!(lifecycle::agent_logs(temp.path(), "coder-1", 10, "info", Some("zero")).is_err());
}

#[test]
fn tasks_are_durable_project_scoped_records() {
    let temp = tempfile::tempdir().unwrap();
    lifecycle::initialize(temp.path()).unwrap();
    let task = lifecycle::create_task(
        temp.path(),
        "implementation",
        "Build the native task lifecycle",
        "high",
    )
    .unwrap();
    assert_eq!(task.status, "pending");
    assert_eq!(task.priority, "high");
    assert_eq!(lifecycle::list_tasks(temp.path()).unwrap(), vec![task]);
}

#[test]
fn task_lifecycle_enforces_assignment_cancellation_and_retry_contracts() {
    let temp = tempfile::tempdir().unwrap();
    lifecycle::initialize(temp.path()).unwrap();
    lifecycle::spawn_agent(temp.path(), "coder", "coder-1").unwrap();
    let task = lifecycle::create_task(temp.path(), "implementation", "Build it", "normal").unwrap();

    let assigned =
        lifecycle::assign_task(temp.path(), &task.id, &["coder-1".into()], false).unwrap();
    assert_eq!(assigned.status, "assigned");
    assert_eq!(assigned.assigned_agent_ids, ["coder-1"]);
    assert_eq!(
        lifecycle::get_task(temp.path(), &task.id).unwrap(),
        assigned
    );
    assert!(lifecycle::cancel_task(temp.path(), &task.id, "operator stop").is_ok());
    assert!(lifecycle::retry_task(temp.path(), &task.id, false).is_err());
    let mut failed = lifecycle::get_task(temp.path(), &task.id).unwrap();
    failed.status = "failed".into();
    fs::write(
        temp.path()
            .join(".swarm/tasks")
            .join(format!("{}.json", failed.id)),
        serde_json::to_vec_pretty(&failed).unwrap(),
    )
    .unwrap();
    let retried = lifecycle::retry_task(temp.path(), &task.id, false).unwrap();
    assert_eq!(retried.status, "queued");
    assert_eq!(retried.retry_count, 1);
    assert!(lifecycle::assign_task(temp.path(), &task.id, &["missing".into()], false).is_err());
}

#[test]
fn swarm_state_is_durable_and_status_is_derived_from_project_records() {
    let temp = tempfile::tempdir().unwrap();
    lifecycle::initialize(temp.path()).unwrap();
    lifecycle::spawn_agent(temp.path(), "coder", "coder-1").unwrap();
    lifecycle::create_task(temp.path(), "implementation", "Build it", "normal").unwrap();

    let swarm =
        lifecycle::initialize_swarm(temp.path(), "hierarchical-mesh", 15, "development").unwrap();
    assert!(temp.path().join(".swarm/state.json").is_file());
    assert_eq!(swarm.status, "ready");

    let running = lifecycle::start_swarm(temp.path(), "Build the Rust port", "testing").unwrap();
    assert_eq!(running.status, "running");
    assert_eq!(running.objective.as_deref(), Some("Build the Rust port"));
    let status = lifecycle::swarm_status(temp.path()).unwrap();
    assert_eq!(status.swarm, Some(running));
    assert_eq!(status.agents_total, 1);
    assert_eq!(status.tasks_total, 1);

    let completed = lifecycle::finish_swarm(temp.path(), true).unwrap();
    assert_eq!(completed.status, "completed");
    let stopped = lifecycle::stop_swarm(temp.path(), &completed.id).unwrap();
    assert_eq!(stopped.status, "stopped");
    assert!(lifecycle::initialize_swarm(temp.path(), "not-a-topology", 1, "development").is_err());
    assert!(lifecycle::initialize_swarm(temp.path(), "mesh", 101, "development").is_err());
}

#[test]
fn sessions_snapshot_restore_export_import_and_delete_project_state() {
    let temp = tempfile::tempdir().unwrap();
    lifecycle::initialize(temp.path()).unwrap();
    lifecycle::spawn_agent(temp.path(), "coder", "coder-1").unwrap();
    lifecycle::create_task(temp.path(), "implementation", "Initial task", "normal").unwrap();
    lifecycle::initialize_swarm(temp.path(), "hierarchical", 2, "development").unwrap();

    let saved = lifecycle::save_session(temp.path(), "checkpoint", "native snapshot").unwrap();
    let second = lifecycle::save_session(temp.path(), "checkpoint-2", "second snapshot").unwrap();
    assert_ne!(saved.session_id, second.session_id);
    assert_eq!(lifecycle::current_session(temp.path()).unwrap(), second);
    assert_eq!(lifecycle::list_sessions(temp.path()).unwrap().len(), 2);

    lifecycle::spawn_agent(temp.path(), "tester", "tester-1").unwrap();
    lifecycle::restore_session(temp.path(), &saved.session_id).unwrap();
    assert_eq!(lifecycle::list_agents(temp.path()).unwrap().len(), 1);
    assert_eq!(lifecycle::list_tasks(temp.path()).unwrap().len(), 1);

    let export = temp.path().join("session-export.json");
    lifecycle::export_session(temp.path(), &saved.session_id, &export).unwrap();
    let imported = lifecycle::import_session(temp.path(), &export, Some("imported")).unwrap();
    assert_eq!(imported.name, "imported");
    lifecycle::delete_session(temp.path(), &second.session_id).unwrap();
    assert!(lifecycle::delete_session(temp.path(), "../../escape").is_err());
}
