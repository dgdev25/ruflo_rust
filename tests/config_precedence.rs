use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use ruflo_config::{CliOverrides, EffectiveConfig};

fn temp_project_dir() -> PathBuf {
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!("ruflo-config-test-{suffix}"));
    fs::create_dir_all(&path).unwrap();
    path
}

#[test]
fn cli_overrides_environment_project_and_defaults() {
    let project = temp_project_dir();
    fs::write(
        project.join("ruflo.toml"),
        r#"
[policy]
allow = ["project_tool"]
deny = ["project_blocked"]

[limits]
max_request_bytes = 100
max_concurrent_executions = 1
max_duration_ms = 1000
"#,
    )
    .unwrap();

    let cli = CliOverrides {
        allow: Some(vec!["cli_tool".to_string()]),
        deny: Some(vec!["cli_blocked".to_string()]),
        max_request_bytes: Some(300),
        max_concurrent_executions: Some(3),
        max_duration_ms: Some(3000),
        ..CliOverrides::default()
    };

    let config = EffectiveConfig::load_with(
        &cli,
        [
            ("RUFLO_MCP_ALLOW", "env_tool"),
            ("RUFLO_MCP_DENY", "env_blocked"),
            ("RUFLO_MCP_MAX_REQUEST_BYTES", "200"),
            ("RUFLO_MCP_MAX_CONCURRENT_EXECUTIONS", "2"),
            ("RUFLO_MCP_MAX_DURATION_MS", "2000"),
        ],
        &project,
    )
    .unwrap();

    assert_eq!(config.policy.allow, vec!["cli_tool"]);
    assert_eq!(config.policy.deny, vec!["cli_blocked"]);
    assert_eq!(config.limits.max_request_bytes, 300);
    assert_eq!(config.limits.max_concurrent_executions, 3);
    assert_eq!(config.limits.max_duration_ms, 3000);
}

#[test]
fn environment_overrides_project_and_defaults() {
    let project = temp_project_dir();
    fs::write(
        project.join("ruflo.toml"),
        r#"
[policy]
allow = ["project_tool"]

[limits]
max_request_bytes = 100
"#,
    )
    .unwrap();

    let config = EffectiveConfig::load_with(
        &CliOverrides::default(),
        [
            ("RUFLO_MCP_ALLOW", "env_tool"),
            ("RUFLO_MCP_MAX_REQUEST_BYTES", "200"),
        ],
        &project,
    )
    .unwrap();

    assert_eq!(config.policy.allow, vec!["env_tool"]);
    assert!(config.policy.deny.is_empty());
    assert_eq!(config.limits.max_request_bytes, 200);
    assert_eq!(config.limits.max_concurrent_executions, 4);
    assert_eq!(config.limits.max_duration_ms, 30_000);
}
