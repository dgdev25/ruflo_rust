use std::collections::BTreeSet;

use serde::Deserialize;

const EXPECTED_COMMANDS: [&str; 53] = [
    "advisor",
    "agent",
    "analyze",
    "announcements",
    "appliance",
    "appliance-advanced",
    "auth",
    "autopilot",
    "benchmark",
    "claims",
    "cleanup",
    "completions",
    "config",
    "daemon",
    "deployment",
    "doctor",
    "eject",
    "embeddings",
    "funnel",
    "gaia-bench",
    "guidance",
    "hive-mind",
    "hooks",
    "init",
    "issues",
    "mcp",
    "memory",
    "metaharness",
    "migrate",
    "neural",
    "performance",
    "plugins",
    "policy",
    "process",
    "progress",
    "providers",
    "proxy",
    "route",
    "ruvector",
    "security",
    "session",
    "settings",
    "spinner",
    "start",
    "status",
    "swarm",
    "task",
    "transfer-store",
    "transport",
    "update",
    "verify",
    "version",
    "workflow",
];

#[derive(Debug, Deserialize)]
struct CommandRegistryManifest {
    schema_version: u64,
    source: String,
    registry_symbol: String,
    enumerator_symbol: String,
    enumeration_expression: String,
    command_count: usize,
    commands: Vec<String>,
}

fn manifest() -> CommandRegistryManifest {
    serde_json::from_str(include_str!("fixtures/cli/command-registry.json"))
        .expect("command registry manifest must be valid JSON")
}

#[test]
fn generated_manifest_identifies_the_owning_typescript_registry() {
    let manifest = manifest();

    assert_eq!(manifest.schema_version, 1);
    assert_eq!(manifest.source, "v3/@claude-flow/cli/src/commands/index.ts");
    assert_eq!(manifest.registry_symbol, "commandLoaders");
    assert_eq!(manifest.enumerator_symbol, "getCommandNames");
    assert_eq!(
        manifest.enumeration_expression,
        "Object.keys(commandLoaders)"
    );
}

#[test]
fn generated_manifest_has_the_exact_53_top_level_commands() {
    let manifest = manifest();
    let actual = manifest
        .commands
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    let expected = EXPECTED_COMMANDS.into_iter().collect::<BTreeSet<_>>();

    assert_eq!(manifest.command_count, 53);
    assert_eq!(manifest.commands.len(), 53, "manifest entry count drifted");
    assert_eq!(
        actual.len(),
        53,
        "manifest contains duplicate command names"
    );
    assert_eq!(
        actual, expected,
        "authoritative top-level command set drifted"
    );
}
