use ruflo_config::{
    Caller, CapabilityManifest, DispatchRequest, EffectiveConfig, Limits, PolicyConfig,
    RegisteredCapability, ToolPolicy,
};
use ruflo_types::{Capability, CapabilityStatus, RufloError};

fn registry() -> Vec<RegisteredCapability> {
    vec![
        RegisteredCapability::new("memory_search", Capability::supported("memory.search", 1)),
        RegisteredCapability::new("tools_list", Capability::supported("tools.list", 1)),
        RegisteredCapability::new(
            "workflow_run",
            Capability::migrating("workflow.run", 2, "enable Wave 2"),
        ),
        RegisteredCapability::new(
            "mcp_start",
            Capability::unsupported("mcp.start", 1, "enable the native MCP dispatcher"),
        ),
    ]
}

fn config(allow: &[&str], deny: &[&str], limits: Limits) -> EffectiveConfig {
    EffectiveConfig {
        policy: PolicyConfig {
            allow: allow.iter().map(|value| (*value).to_string()).collect(),
            deny: deny.iter().map(|value| (*value).to_string()).collect(),
        },
        limits,
    }
}

#[test]
fn deny_overrides_allowlist() {
    let policy = ToolPolicy::from_config(
        &config(
            &["memory_search"],
            &["memory_search"],
            Limits {
                max_request_bytes: 1024,
                max_concurrent_executions: 2,
                max_duration_ms: 5000,
            },
        ),
        &registry(),
    )
    .unwrap();

    assert!(matches!(
        policy.authorize(&Caller::local(), "memory_search"),
        Err(RufloError::Unauthorized { capability }) if capability == "memory.search"
    ));
    assert!(!policy.is_discoverable(&Caller::local(), "memory_search"));
    assert!(!policy.is_callable(&Caller::local(), "memory_search"));
}

#[test]
fn denied_tool_is_undiscoverable_and_uncallable() {
    let policy = ToolPolicy::from_config(
        &config(
            &[],
            &["tools_list"],
            Limits {
                max_request_bytes: 1024,
                max_concurrent_executions: 2,
                max_duration_ms: 5000,
            },
        ),
        &registry(),
    )
    .unwrap();

    assert_eq!(
        policy.discoverable_tools(&Caller::local()),
        vec!["mcp_start", "memory_search", "workflow_run"]
    );
    assert!(!policy.is_discoverable(&Caller::local(), "tools_list"));
    assert!(matches!(
        policy.authorize(&Caller::local(), "tools_list"),
        Err(RufloError::Unauthorized { capability }) if capability == "tools.list"
    ));
}

#[test]
fn rejects_unregistered_policy_tokens() {
    let error = ToolPolicy::from_config(
        &config(
            &["missing_tool"],
            &[],
            Limits {
                max_request_bytes: 1024,
                max_concurrent_executions: 2,
                max_duration_ms: 5000,
            },
        ),
        &registry(),
    )
    .unwrap_err();

    assert!(matches!(
        error,
        RufloError::InvalidInput { code, .. } if code == "policy.unknown_tool"
    ));
}

#[test]
fn enforces_request_size_concurrency_and_duration_before_dispatch() {
    let policy = ToolPolicy::from_config(
        &config(
            &[],
            &[],
            Limits {
                max_request_bytes: 64,
                max_concurrent_executions: 1,
                max_duration_ms: 10,
            },
        ),
        &registry(),
    )
    .unwrap();

    assert!(matches!(
        policy.authorize_request(
            &Caller::local(),
            "memory_search",
            DispatchRequest {
                request_bytes: 65,
                active_executions: 0,
                duration_ms: 1,
            },
        ),
        Err(RufloError::InvalidInput { code, .. }) if code == "request_too_large"
    ));
    assert!(matches!(
        policy.authorize_request(
            &Caller::local(),
            "memory_search",
            DispatchRequest {
                request_bytes: 32,
                active_executions: 1,
                duration_ms: 1,
            },
        ),
        Err(RufloError::RateLimited { retry_after_ms: 0 })
    ));
    assert!(matches!(
        policy.authorize_request(
            &Caller::local(),
            "memory_search",
            DispatchRequest {
                request_bytes: 32,
                active_executions: 0,
                duration_ms: 11,
            },
        ),
        Err(RufloError::Timeout)
    ));
}

#[test]
fn capability_manifest_reflects_registered_statuses() {
    let manifest = CapabilityManifest::from_registry(&registry());
    let capabilities = manifest.by_name();

    assert_eq!(
        capabilities["memory.search"].status,
        CapabilityStatus::Supported
    );
    assert_eq!(
        capabilities["workflow.run"].status,
        CapabilityStatus::Migrating
    );
    assert_eq!(
        capabilities["mcp.start"].status,
        CapabilityStatus::Unsupported
    );
}
