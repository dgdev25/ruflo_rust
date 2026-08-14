use std::ffi::OsString;

use super::parse::parse;
use super::{ParsedCommand, UNSUPPORTED_COMMAND_ERROR_CODE};

    fn argv(values: &[&str]) -> Vec<OsString> {
        std::iter::once("ruflo")
            .chain(values.iter().copied())
            .map(OsString::from)
            .collect()
    }

    #[test]
    fn task_aliases_and_options_preserve_the_v3_surface() {
        assert!(matches!(
            parse(argv(&["task", "new", "-t", "implementation", "-d", "Build", "-p", "high"])),
            Ok(ParsedCommand::TaskCreate { priority, .. }) if priority == "high"
        ));
        assert!(matches!(
            parse(argv(&["task", "info", "task-1"])),
            Ok(ParsedCommand::TaskStatus { .. })
        ));
        assert!(matches!(
            parse(argv(&["task", "abort", "task-1", "-r", "operator"])),
            Ok(ParsedCommand::TaskCancel { .. })
        ));
        assert!(matches!(
            parse(argv(&["task", "assign", "task-1", "-a", "coder-1,coder-2"])),
            Ok(ParsedCommand::TaskAssign { agent_ids, .. }) if agent_ids.len() == 2
        ));
        assert!(matches!(
            parse(argv(&["task", "rerun", "task-1", "--reset-state"])),
            Ok(ParsedCommand::TaskRetry {
                reset_state: true,
                ..
            })
        ));
    }

    #[test]
    fn swarm_options_preserve_v3_defaults_and_objective_forms() {
        assert!(matches!(
            parse(argv(&["swarm", "init", "--v3-mode"])),
            Ok(ParsedCommand::SwarmInit { topology, max_agents: 15, .. }) if topology == "hierarchical-mesh"
        ));
        assert!(matches!(
            parse(argv(&["swarm", "start", "-o", "Build API", "-s", "testing"])),
            Ok(ParsedCommand::SwarmStart { objective, strategy, worktree, .. }) if objective == "Build API" && strategy == "testing" && !worktree
        ));
        assert!(matches!(
            parse(argv(&["swarm", "stop", "swarm-1"])),
            Ok(ParsedCommand::SwarmStop { swarm_id }) if swarm_id == "swarm-1"
        ));
        assert!(matches!(
            parse(argv(&["swarm", "scale", "swarm-1", "-a", "3", "-t", "coder"])),
            Ok(ParsedCommand::SwarmScale { swarm_id, target_agents: 3, agent_type: Some(agent_type) })
                if swarm_id == "swarm-1" && agent_type == "coder"
        ));
        assert!(matches!(
            parse(argv(&["swarm", "coordinate", "--agents", "5"])),
            Ok(ParsedCommand::SwarmCoordinate { agents: 5 })
        ));
        assert!(matches!(
            parse(argv(&["swarm", "compress-message", "--message-file", "note.md", "-b", "20"])),
            Ok(ParsedCommand::SwarmCompressMessage { message: None, message_file: Some(path), budget_tokens: 20, .. }) if path == "note.md"
        ));
    }

    #[test]
    fn session_aliases_cover_the_v3_persistence_surface() {
        assert!(matches!(
            parse(argv(&["session", "checkpoint", "-n", "checkpoint-1"])),
            Ok(ParsedCommand::SessionSave { name, .. }) if name == "checkpoint-1"
        ));
        assert!(matches!(
            parse(argv(&["session", "load", "session-1"])),
            Ok(ParsedCommand::SessionRestore { session_id }) if session_id == "session-1"
        ));
        assert!(matches!(
            parse(argv(&["session", "rm", "session-1"])),
            Ok(ParsedCommand::SessionDelete { session_id }) if session_id == "session-1"
        ));
        assert!(matches!(
            parse(argv(&["session", "export", "session-1", "-o", "backup.json"])),
            Ok(ParsedCommand::SessionExport { session_id: Some(session_id), output }) if session_id == "session-1" && output == "backup.json"
        ));
        assert!(matches!(
            parse(argv(&["session", "import", "backup.json", "-n", "restore"])),
            Ok(ParsedCommand::SessionImport { name: Some(name), .. }) if name == "restore"
        ));
    }

    #[test]
    fn agent_lifecycle_aliases_and_v3_options_parse() {
        assert!(matches!(
            parse(argv(&["agent", "status", "--id", "coder-1"])),
            Ok(ParsedCommand::AgentStatus { agent_id }) if agent_id == "coder-1"
        ));
        assert!(matches!(
            parse(argv(&["agent", "kill", "coder-1", "-f", "--timeout", "45"])),
            Ok(ParsedCommand::AgentStop {
                force: true,
                timeout_seconds: 45,
                ..
            })
        ));
        assert!(matches!(
            parse(argv(&["agent", "metrics", "coder-1", "-p", "7d"])),
            Ok(ParsedCommand::AgentMetrics { agent_id: Some(agent_id), period }) if agent_id == "coder-1" && period == "7d"
        ));
    }

    #[test]
    fn known_family_with_unknown_subcommand_is_rejected_not_downgraded_to_overview() {
        let error = parse(argv(&["gaia-bench", "invented-subcommand"]))
            .expect_err("unknown subcommand must fail parsing");
        assert!(error.starts_with(UNSUPPORTED_COMMAND_ERROR_CODE));
        assert!(error.contains("unsupported native CLI invocation"));
        assert!(error.contains("gaia-bench invented-subcommand"));

        assert!(matches!(
            parse(argv(&["gaia-bench"])),
            Ok(ParsedCommand::NativeOverview { name }) if name == "gaia-bench"
        ));
    }

    #[test]
    fn agent_logs_preserves_v3_filter_options() {
        assert!(matches!(
            parse(argv(&[
                "agent", "logs", "-i", "coder-1", "-n", "25", "-l", "warn", "-f", "--since", "30m"
            ])),
            Ok(ParsedCommand::AgentLogs { agent_id, tail: 25, level, follow: true, since: Some(since) })
                if agent_id == "coder-1" && level == "warn" && since == "30m"
        ));
    }

    #[test]
    fn memory_core_subcommands_preserve_v3_aliases_and_options() {
        assert!(matches!(
            parse(argv(&["memory", "store", "-k", "goal", "--value", "ship", "-n", "plans", "--tags", "v3,rust", "--provenance", "user_claim"])),
            Ok(ParsedCommand::MemoryStore { key, value, namespace, tags_json: Some(tags), provenance_type, upsert: true, .. })
                if key == "goal" && value == "ship" && namespace == "plans" && tags == r#"["v3","rust"]"# && provenance_type == "user_claim"
        ));
        assert!(matches!(
            parse(argv(&["memory", "get", "-k", "goal", "--value-only"])),
            Ok(ParsedCommand::MemoryRetrieve { key, value_only: true, .. }) if key == "goal"
        ));
        assert!(matches!(
            parse(argv(&["memory", "search", "-q", "ship", "-n", "plans", "-l", "3"])),
            Ok(ParsedCommand::MemorySearch { query, namespace: Some(namespace), limit: 3, .. }) if query == "ship" && namespace == "plans"
        ));
        assert!(matches!(
            parse(argv(&["memory", "ls", "-n", "plans", "-l", "2"])),
            Ok(ParsedCommand::MemoryList { namespace: Some(namespace), limit: 2, .. }) if namespace == "plans"
        ));
        assert!(matches!(
            parse(argv(&["memory", "rm", "goal", "-n", "plans"])),
            Ok(ParsedCommand::MemoryDelete { key, namespace, .. }) if key == "goal" && namespace == "plans"
        ));
        assert!(matches!(
            parse(argv(&["memory", "stats"])),
            Ok(ParsedCommand::MemoryStats { .. })
        ));
        assert!(matches!(
            parse(argv(&["memory", "migrate-node", "--path", ".swarm/memory.db", "--dry-run"])),
            Ok(ParsedCommand::MemoryMigrateNode { path: Some(path), dry_run: true }) if path == ".swarm/memory.db"
        ));
        assert!(matches!(
            parse(argv(&["memory", "purge", "-n", "plans", "--dry-run"])),
            Ok(ParsedCommand::MemoryPurge { namespace, dry_run: true, force: false, .. }) if namespace == "plans"
        ));
        assert!(parse(argv(&["memory", "purge", "--force"])).is_err());
        assert!(matches!(
            parse(argv(&["config", "init", "--force"])),
            Ok(ParsedCommand::ConfigInit { force: true, .. })
        ));
        assert!(matches!(
            parse(argv(&["config", "get", "-k", "policy.allow"])),
            Ok(ParsedCommand::ConfigGet { key: Some(key), .. }) if key == "policy.allow"
        ));
        assert!(matches!(
            parse(argv(&["config", "get", "limits.max_request_bytes"])),
            Ok(ParsedCommand::ConfigGet { key: Some(key), .. }) if key == "limits.max_request_bytes"
        ));
        assert!(matches!(
            parse(argv(&["migrate", "status"])),
            Ok(ParsedCommand::MigrateStatus)
        ));
        assert!(parse(argv(&["memory", "search", "-q", "ship", "-l", "0"])).is_err());
    }

    #[test]
    fn config_v3_subcommands_and_options_parse() {
        assert!(matches!(
            parse(argv(&["config", "set", "-k", "swarm.maxAgents", "-v", "20"])),
            Ok(ParsedCommand::ConfigSet { key, value }) if key == "swarm.maxAgents" && value == "20"
        ));
        assert!(parse(argv(&["config", "set", "swarm.maxAgents", "20"])).is_err());
        assert!(matches!(
            parse(argv(&["config", "providers", "--add", "local", "--format", "json"])),
            Ok(ParsedCommand::ConfigProviders { add: Some(name), json: true, .. }) if name == "local"
        ));
        assert!(matches!(
            parse(argv(&["config", "reset", "--section", "memory", "--force"])),
            Ok(ParsedCommand::ConfigReset { force: true, section: Some(section) }) if section == "memory"
        ));
        assert!(matches!(
            parse(argv(&["config", "export", "backup.json", "--format", "json"])),
            Ok(ParsedCommand::ConfigExport { output, format }) if output == "backup.json" && format == "json"
        ));
        assert_eq!(
            parse(argv(&["config", "export", "-f", "text"])),
            Err("Invalid value for --format: text. Must be one of: json, yaml".into())
        );
        assert_eq!(
            parse(argv(&["config", "export", "-f", "yaml"])),
            Err("Invalid value for --format: yaml. Must be one of: text, json, table".into())
        );
        assert!(matches!(
            parse(argv(&["config", "import", "--file", "backup.json", "--merge"])),
            Ok(ParsedCommand::ConfigImport { file, merge: true }) if file == "backup.json"
        ));
    }

    #[test]
    fn cleanup_aliases_and_force_precedence_parse() {
        assert!(matches!(
            parse(argv(&["cleanup", "--dry-run", "--force", "--keep-config"])),
            Ok(ParsedCommand::Cleanup {
                force: true,
                keep_config: true
            })
        ));
        assert!(matches!(
            parse(argv(&["clean", "-n"])),
            Ok(ParsedCommand::Cleanup {
                force: false,
                keep_config: false
            })
        ));
        assert!(matches!(
            parse(argv(&["clean", "--help"])),
            Ok(ParsedCommand::CleanupHelp)
        ));
        assert!(matches!(
            parse(argv(&["cleanup", "--unknown"])),
            Ok(ParsedCommand::Cleanup { force: false, .. })
        ));
        assert!(matches!(
            parse(argv(&["--no-color", "cleanup", "-fk"])),
            Ok(ParsedCommand::Cleanup {
                force: true,
                keep_config: true
            })
        ));
        assert!(matches!(
            parse(argv(&["cleanup", "--force", "false"])),
            Ok(ParsedCommand::Cleanup { force: false, .. })
        ));
        assert!(matches!(
            parse(argv(&["cleanup", "--force", "--no-force"])),
            Ok(ParsedCommand::Cleanup { force: false, .. })
        ));
        // --version after a command is NOT intercepted as the global version
        // flag (so `deployment deploy --version 1.0.0` works). Only standalone
        // --version (first arg) triggers Version.
        assert!(matches!(
            parse(argv(&["cleanup", "--version"])),
            Ok(ParsedCommand::Cleanup { force: false, .. })
        ));
    }

    #[test]
    fn transport_surface_accepts_positional_and_flag_names() {
        assert!(matches!(
            parse(argv(&["transport"])),
            Ok(ParsedCommand::TransportOverview)
        ));
        assert!(
            matches!(parse(argv(&["transport", "use", "SLIM"])), Ok(ParsedCommand::TransportUse { name: Some(name), .. }) if name == "slim")
        );
        assert!(
            matches!(parse(argv(&["--quiet", "transport", "use", "--transport=slim"])), Ok(ParsedCommand::TransportUse { name: Some(name), quiet: true }) if name == "slim")
        );
        assert!(matches!(
            parse(argv(&["transport", "use", "--help"])),
            Ok(ParsedCommand::TransportUseHelp)
        ));
    }

    #[test]
    fn deployment_aliases_subcommands_and_options_parse() {
        use crate::deployment::DeploymentCommand as Deployment;

        assert!(matches!(
            parse(argv(&["deployment"])),
            Ok(ParsedCommand::Deployment(Deployment::Overview))
        ));
        assert!(matches!(
            parse(argv(&["deploy", "deploy", "-e", "prod", "-v", "3.5.0", "-d", "--description", "ship"])),
            Ok(ParsedCommand::Deployment(Deployment::Deploy { env, version: Some(version), dry_run: true, description: Some(description) }))
                if env == "prod" && version == "3.5.0" && description == "ship"
        ));
        assert!(matches!(
            parse(argv(&["deployment", "envs", "-a", "add", "-n", "preview", "-t", "staging", "-u", "https://preview"])),
            Ok(ParsedCommand::Deployment(Deployment::Environments { action, name: Some(name), env_type, url: Some(url) }))
                if action == "add" && name == "preview" && env_type == "staging" && url == "https://preview"
        ));
        assert!(matches!(
            parse(argv(&["--no-color", "deployment", "history", "--limit=3"])),
            Ok(ParsedCommand::Deployment(Deployment::History {
                limit: 3,
                ..
            }))
        ));
        assert!(matches!(
            parse(argv(&["deployment", "rollback", "-e", "prod", "-s", "2"])),
            Ok(ParsedCommand::Deployment(Deployment::Rollback { env, steps: 2, .. })) if env == "prod"
        ));
        assert!(matches!(
            parse(argv(&["deployment", "logs", "--help"])),
            Ok(ParsedCommand::Deployment(Deployment::Help { subcommand: Some(name) })) if name == "logs"
        ));
        assert!(parse(argv(&["deployment", "history", "-l", "many"])).is_err());
        assert!(parse(argv(&["deployment", "unknown"])).is_err());
    }
