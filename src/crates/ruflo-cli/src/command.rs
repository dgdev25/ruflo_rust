use std::ffi::OsString;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParsedCommand {
    Version,
    VersionCommand {
        explain: bool,
        require_catalog_gte: Option<u64>,
    },
    Completions {
        shell: String,
    },
    CompletionsOverview,
    Doctor,
    Start {
        topology: String,
        daemon: bool,
    },
    Progress,
    Cleanup {
        force: bool,
        keep_config: bool,
    },
    CleanupHelp,
    TransportOverview,
    TransportHelp,
    TransportUseHelp,
    TransportUse {
        name: Option<String>,
        quiet: bool,
    },
    Deployment(crate::deployment::DeploymentCommand),
    Claims(crate::claims::ClaimsCommand),
    Advisor(crate::funnel::AdvisorCommand),
    Announcements(crate::announcements::AnnouncementsCommand),
    Spinner(crate::spinner::SpinnerCommand),
    Settings(crate::settings::SettingsCommand),
    MemoryStore {
        key: String,
        value: String,
        namespace: String,
        tags_json: Option<String>,
        provenance_type: String,
        upsert: bool,
        path: Option<String>,
    },
    MemoryRetrieve {
        key: String,
        namespace: String,
        value_only: bool,
        path: Option<String>,
    },
    MemorySearch {
        query: String,
        namespace: Option<String>,
        limit: usize,
        path: Option<String>,
    },
    MemoryList {
        namespace: Option<String>,
        limit: usize,
        path: Option<String>,
    },
    MemoryDelete {
        key: String,
        namespace: String,
        path: Option<String>,
    },
    MemoryStats {
        path: Option<String>,
    },
    MemoryPurge {
        namespace: String,
        dry_run: bool,
        force: bool,
        path: Option<String>,
    },
    ConfigInit {
        force: bool,
        sparc: bool,
        v3: bool,
    },
    ConfigGet {
        key: Option<String>,
        json: bool,
    },
    ConfigSet {
        key: String,
        value: String,
    },
    ConfigProviders {
        add: Option<String>,
        remove: Option<String>,
        enable: Option<String>,
        disable: Option<String>,
        json: bool,
    },
    ConfigReset {
        force: bool,
        section: Option<String>,
    },
    ConfigExport {
        output: String,
        format: String,
    },
    ConfigImport {
        file: String,
        merge: bool,
    },
    ConfigOverview,
    ConfigHelp {
        subcommand: Option<String>,
    },
    MigrateStatus,
    MigrateRun {
        target: String,
        dry_run: bool,
        backup: bool,
        force: bool,
    },
    Help,
    Init,
    Status,
    SwarmInit {
        topology: String,
        max_agents: usize,
        strategy: String,
    },
    SwarmStatus,
    SwarmStart {
        objective: String,
        strategy: String,
    },
    SwarmStop {
        swarm_id: String,
    },
    SwarmScale {
        swarm_id: String,
        target_agents: usize,
        agent_type: Option<String>,
    },
    SwarmCoordinate {
        agents: usize,
    },
    SwarmCompressMessage {
        message: Option<String>,
        message_file: Option<String>,
        budget_tokens: usize,
        mode: String,
    },
    SessionSave {
        name: String,
        description: String,
    },
    SessionList,
    SessionRestore {
        session_id: String,
    },
    SessionDelete {
        session_id: String,
    },
    SessionExport {
        session_id: Option<String>,
        output: String,
    },
    SessionImport {
        input: String,
        name: Option<String>,
    },
    SessionCurrent,
    AgentSpawn {
        agent_type: String,
        name: String,
    },
    AgentList,
    AgentStatus {
        agent_id: String,
    },
    AgentStop {
        agent_id: String,
        force: bool,
        timeout_seconds: u64,
    },
    AgentMetrics {
        agent_id: Option<String>,
        period: String,
    },
    AgentPool {
        size: Option<usize>,
        min: usize,
        max: usize,
        auto_scale: bool,
    },
    AgentHealth {
        agent_id: Option<String>,
        detailed: bool,
    },
    AgentLogs {
        agent_id: String,
        tail: usize,
        level: String,
        follow: bool,
        since: Option<String>,
    },
    TaskCreate {
        task_type: String,
        description: String,
        priority: String,
    },
    TaskList,
    TaskStatus {
        task_id: String,
    },
    TaskCancel {
        task_id: String,
        reason: String,
    },
    TaskAssign {
        task_id: String,
        agent_ids: Vec<String>,
        unassign: bool,
    },
    TaskRetry {
        task_id: String,
        reset_state: bool,
    },
    McpStart,
}

pub fn parse(argv: impl IntoIterator<Item = OsString>) -> Result<ParsedCommand, String> {
    let mut args = argv
        .into_iter()
        .skip(1)
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    if args
        .iter()
        .any(|value| matches!(value.as_str(), "--version" | "-V"))
    {
        return Ok(ParsedCommand::Version);
    }
    if let Some(index) = args.iter().position(|value| {
        matches!(
            value.as_str(),
            "config" | "transport" | "cleanup" | "clean" | "deployment" | "deploy" | "claims"
        )
    }) {
        if index > 0 {
            let prefix = args[..index].to_vec();
            let valid_prefix = prefix.iter().enumerate().all(|(position, value)| {
                matches!(
                    value.as_str(),
                    "--no-color"
                        | "-v"
                        | "--verbose"
                        | "-Q"
                        | "--quiet"
                        | "-n"
                        | "--dry-run"
                        | "-f"
                        | "--force"
                        | "--no-force"
                        | "-k"
                        | "--keep-config"
                        | "--no-keep-config"
                ) || value == "--format"
                    || position > 0 && prefix[position - 1] == "--format"
                    || value.starts_with("--format=")
            });
            if valid_prefix {
                let mut reordered = args[index..].to_vec();
                reordered.extend_from_slice(&prefix);
                args = reordered;
            }
        }
    }
    let normalized = args.iter().map(String::as_str).collect::<Vec<_>>();

    if normalized.first() == Some(&"version") {
        let require_catalog_gte =
            option_value(&args, "--require-catalog-gte", "--require-catalog-gte")
                .map(|value| {
                    value
                        .parse()
                        .map_err(|_| "catalog generation must be a non-negative integer")
                })
                .transpose()?;
        return Ok(ParsedCommand::VersionCommand {
            explain: args.iter().any(|value| value == "--explain"),
            require_catalog_gte,
        });
    }
    if normalized.first() == Some(&"completions") {
        let Some(shell) = normalized.get(1).copied() else {
            return Ok(ParsedCommand::CompletionsOverview);
        };
        // completions.ts powershellCommand aliases `pwsh` -> powershell.
        let shell = if shell == "pwsh" { "powershell" } else { shell };
        if !matches!(shell, "bash" | "zsh" | "fish" | "powershell") {
            return Err("unsupported shell; use bash, zsh, fish, or powershell".into());
        }
        return Ok(ParsedCommand::Completions {
            shell: shell.to_string(),
        });
    }
    if normalized.first() == Some(&"doctor") {
        return Ok(ParsedCommand::Doctor);
    }
    if normalized.first() == Some(&"start") {
        return Ok(ParsedCommand::Start {
            topology: option_value(&args, "--topology", "--topology")
                .unwrap_or_else(|| "hierarchical-mesh".into()),
            daemon: args.iter().any(|value| value == "--daemon"),
        });
    }
    if normalized.first() == Some(&"progress") {
        return Ok(ParsedCommand::Progress);
    }
    if matches!(normalized.first(), Some(&"cleanup" | &"clean")) {
        if normalized
            .iter()
            .skip(1)
            .any(|value| *value == "--help" || *value == "-h")
        {
            return Ok(ParsedCommand::CleanupHelp);
        }
        return Ok(ParsedCommand::Cleanup {
            force: boolean_option(&args, "--force", "-f", false),
            keep_config: boolean_option(&args, "--keep-config", "-k", false),
        });
    }
    if normalized.first() == Some(&"transport") {
        let quiet = args
            .iter()
            .any(|value| matches!(value.as_str(), "--quiet" | "-Q"));
        if normalized.get(1) == Some(&"use") {
            if normalized
                .iter()
                .any(|value| matches!(*value, "--help" | "-h"))
            {
                return Ok(ParsedCommand::TransportUseHelp);
            }
            let name = option_value(&args, "--transport", "--transport")
                .or_else(|| {
                    config_positionals(&args, 2, &["--transport", "--format"])
                        .first()
                        .cloned()
                })
                .map(|value| value.trim().to_lowercase());
            return Ok(ParsedCommand::TransportUse { name, quiet });
        }
        if normalized
            .iter()
            .any(|value| matches!(*value, "--help" | "-h"))
        {
            return Ok(ParsedCommand::TransportHelp);
        }
        return Ok(ParsedCommand::TransportOverview);
    }
    if matches!(normalized.first(), Some(&"deployment" | &"deploy")) {
        use crate::deployment::DeploymentCommand as Deployment;

        let subcommand = normalized.get(1).copied();
        if normalized
            .iter()
            .any(|value| matches!(*value, "--help" | "-h"))
        {
            return Ok(ParsedCommand::Deployment(Deployment::Help {
                subcommand: subcommand
                    .filter(|value| !value.starts_with('-'))
                    .map(str::to_owned),
            }));
        }
        match subcommand {
            None => return Ok(ParsedCommand::Deployment(Deployment::Overview)),
            Some("deploy") => {
                return Ok(ParsedCommand::Deployment(Deployment::Deploy {
                    env: option_value(&args, "--env", "-e").unwrap_or_else(|| "staging".into()),
                    version: option_value(&args, "--version", "-v"),
                    dry_run: boolean_option(&args, "--dry-run", "-d", false),
                    description: option_value(&args, "--description", "--description"),
                }));
            }
            Some("status") => {
                return Ok(ParsedCommand::Deployment(Deployment::Status {
                    env: option_value(&args, "--env", "-e"),
                }));
            }
            Some("rollback") => {
                return Ok(ParsedCommand::Deployment(Deployment::Rollback {
                    env: option_value(&args, "--env", "-e").unwrap_or_default(),
                    version: option_value(&args, "--version", "-v"),
                    steps: deployment_number(&args, "--steps", "-s", 1, "rollback steps")?,
                }));
            }
            Some("history") => {
                return Ok(ParsedCommand::Deployment(Deployment::History {
                    env: option_value(&args, "--env", "-e"),
                    limit: deployment_number(&args, "--limit", "-l", 10, "history limit")?,
                }));
            }
            Some("environments" | "envs") => {
                return Ok(ParsedCommand::Deployment(Deployment::Environments {
                    action: option_value(&args, "--action", "-a").unwrap_or_else(|| "list".into()),
                    name: option_value(&args, "--name", "-n"),
                    env_type: option_value(&args, "--type", "-t").unwrap_or_else(|| "local".into()),
                    url: option_value(&args, "--url", "-u"),
                }));
            }
            Some("logs") => {
                return Ok(ParsedCommand::Deployment(Deployment::Logs {
                    deployment: option_value(&args, "--deployment", "-d"),
                    env: option_value(&args, "--env", "-e"),
                    lines: deployment_number(&args, "--lines", "-n", 50, "log lines")?,
                }));
            }
            Some("release") => {
                return Ok(ParsedCommand::Deployment(Deployment::Release {
                    version: option_value(&args, "--version", "-v"),
                    env: option_value(&args, "--env", "-e").unwrap_or_else(|| "production".into()),
                    description: option_value(&args, "--description", "-d"),
                }));
            }
            Some(other) => return Err(format!("Unknown subcommand '{other}' for deployment")),
        }
    }
    if normalized.first() == Some(&"claims") {
        use crate::claims::ClaimsCommand as Claims;

        let subcommand = normalized.get(1).copied();
        if normalized
            .iter()
            .any(|value| matches!(*value, "--help" | "-h"))
        {
            return Ok(ParsedCommand::Claims(Claims::Help {
                subcommand: subcommand
                    .filter(|value| !value.starts_with('-'))
                    .map(str::to_owned),
            }));
        }
        let claim = || option_value(&args, "--claim", "-c");
        let user = || option_value(&args, "--user", "-u");
        let role = || option_value(&args, "--role", "-r");
        match subcommand {
            None => return Ok(ParsedCommand::Claims(Claims::Overview)),
            Some("list") => {
                return Ok(ParsedCommand::Claims(Claims::List {
                    user: user(),
                    role: role(),
                    resource: option_value(&args, "--resource", "--resource"),
                }));
            }
            Some("check") => {
                return Ok(ParsedCommand::Claims(Claims::Check {
                    claim: claim(),
                    user: user(),
                    resource: option_value(&args, "--resource", "-r"),
                }));
            }
            Some("grant") => {
                return Ok(ParsedCommand::Claims(Claims::Grant {
                    claim: claim(),
                    user: user(),
                    role: role(),
                    scope: option_value(&args, "--scope", "-s").unwrap_or_else(|| "global".into()),
                    expires: option_value(&args, "--expires", "-e"),
                }));
            }
            Some("revoke") => {
                return Ok(ParsedCommand::Claims(Claims::Revoke {
                    claim: claim(),
                    user: user(),
                    role: role(),
                }));
            }
            Some("roles") => {
                return Ok(ParsedCommand::Claims(Claims::Roles {
                    action: option_value(&args, "--action", "-a").unwrap_or_else(|| "list".into()),
                    name: option_value(&args, "--name", "-n"),
                }));
            }
            Some("policies") => {
                return Ok(ParsedCommand::Claims(Claims::Policies {
                    action: option_value(&args, "--action", "-a").unwrap_or_else(|| "list".into()),
                    name: option_value(&args, "--name", "-n"),
                }));
            }
            Some(other) => return Err(format!("Unknown subcommand '{other}' for claims")),
        }
    }
    if normalized.first() == Some(&"advisor") {
        use crate::funnel::AdvisorCommand as Advisor;
        let subcommand = normalized.get(1).copied();
        if args.iter().any(|v| matches!(v.as_str(), "--help" | "-h")) {
            return Ok(ParsedCommand::Advisor(Advisor::Help {
                subcommand: subcommand
                    .filter(|v| !v.starts_with('-'))
                    .map(str::to_owned),
            }));
        }
        let yes = args.iter().any(|v| v == "--yes");
        let cmd = match subcommand {
            None => Advisor::Status,
            Some("status") => Advisor::Status,
            Some("enable") => Advisor::Enable { yes },
            Some("disable") => Advisor::Disable,
            Some(other) => {
                return Err(format!("Unknown subcommand '{other}' for advisor"));
            }
        };
        return Ok(ParsedCommand::Advisor(cmd));
    }
    if normalized.first() == Some(&"announcements") {
        use crate::announcements::AnnouncementsCommand as Ann;
        let subcommand = normalized.get(1).copied();
        if args.iter().any(|v| matches!(v.as_str(), "--help" | "-h")) {
            return Ok(ParsedCommand::Announcements(Ann::Help {
                subcommand: subcommand
                    .filter(|v| !v.starts_with('-'))
                    .map(str::to_owned),
            }));
        }
        let json = args.iter().any(|v| v == "--json");
        let yes = args.iter().any(|v| v == "--yes");
        let cmd = match subcommand {
            None => Ann::List { json },
            Some("list") => Ann::List { json },
            Some("enable") => Ann::Enable { yes },
            Some("disable") => Ann::Disable,
            Some("reset") => Ann::Reset { yes },
            Some(other) => {
                return Err(format!("Unknown subcommand '{other}' for announcements"));
            }
        };
        return Ok(ParsedCommand::Announcements(cmd));
    }
    if normalized.first() == Some(&"spinner") {
        use crate::spinner::SpinnerCommand as Spin;
        let subcommand = normalized.get(1).copied();
        if args.iter().any(|v| matches!(v.as_str(), "--help" | "-h")) {
            return Ok(ParsedCommand::Spinner(Spin::Help {
                subcommand: subcommand
                    .filter(|v| !v.starts_with('-'))
                    .map(str::to_owned),
            }));
        }
        let json = args.iter().any(|v| v == "--json");
        let yes = args.iter().any(|v| v == "--yes");
        let cmd = match subcommand {
            None => Spin::List { json },
            Some("list") => Spin::List { json },
            Some("enable") => Spin::Enable { yes },
            Some("disable") => Spin::Disable,
            Some("reset") => Spin::Reset { yes },
            Some(other) => return Err(format!("Unknown subcommand '{other}' for spinner")),
        };
        return Ok(ParsedCommand::Spinner(cmd));
    }
    if normalized.first() == Some(&"settings") {
        use crate::settings::SettingsCommand as Set;
        if args.iter().any(|v| matches!(v.as_str(), "--help" | "-h")) {
            return Ok(ParsedCommand::Settings(Set::Help {
                subcommand: normalized
                    .get(1)
                    .filter(|v| !v.starts_with('-'))
                    .copied()
                    .map(str::to_owned),
            }));
        }
        // `settings notices ...`
        if normalized.get(1).copied() == Some("notices") {
            let clear = args.iter().any(|v| v == "--clear");
            let cmd = match normalized.get(2).copied() {
                None => Set::NoticesStatus,
                Some("status") => Set::NoticesStatus,
                Some("off") => Set::NoticesOff,
                Some("on") => Set::NoticesOn,
                Some("id") => Set::NoticesId,
                Some("rate-limited") => Set::NoticesRateLimited { clear },
                Some("quota-low") => Set::NoticesQuotaLow { clear },
                Some(other) => return Err(format!("Unknown notices subcommand '{other}'")),
            };
            return Ok(ParsedCommand::Settings(cmd));
        }
        return Ok(ParsedCommand::Settings(Set::Overview));
    }
    if normalized.len() >= 2 && normalized[0] == "memory" && normalized[1] == "store" {
        let key = option_value(&args, "--key", "-k").ok_or("memory key is required")?;
        let value = option_value(&args, "--value", "--value")
            .or_else(|| memory_positionals(&args).into_iter().next())
            .ok_or("memory value is required")?;
        return Ok(ParsedCommand::MemoryStore {
            key,
            value,
            namespace: option_value(&args, "--namespace", "-n").unwrap_or_else(|| "default".into()),
            tags_json: option_value(&args, "--tags", "--tags").map(|tags| {
                serde_json::to_string(
                    &tags
                        .split(',')
                        .map(str::trim)
                        .filter(|tag| !tag.is_empty())
                        .collect::<Vec<_>>(),
                )
                .expect("tag serialization is infallible")
            }),
            provenance_type: option_value(&args, "--provenance", "--provenance")
                .unwrap_or_else(|| "unknown".into()),
            upsert: !args.iter().any(|argument| argument == "--no-upsert"),
            path: option_value(&args, "--path", "--path"),
        });
    }
    if normalized.len() >= 2
        && normalized[0] == "memory"
        && matches!(normalized[1], "retrieve" | "get")
    {
        let key = option_value(&args, "--key", "-k")
            .or_else(|| memory_positionals(&args).into_iter().next())
            .ok_or("memory key is required")?;
        return Ok(ParsedCommand::MemoryRetrieve {
            key,
            namespace: option_value(&args, "--namespace", "-n").unwrap_or_else(|| "default".into()),
            value_only: args.iter().any(|argument| argument == "--value-only"),
            path: option_value(&args, "--path", "--path"),
        });
    }
    if normalized.len() >= 2 && normalized[0] == "memory" && normalized[1] == "search" {
        let query = option_value(&args, "--query", "-q")
            .or_else(|| memory_positionals(&args).into_iter().next())
            .ok_or("memory search query is required")?;
        return Ok(ParsedCommand::MemorySearch {
            query,
            namespace: option_value(&args, "--namespace", "-n"),
            limit: parse_positive_usize(
                option_value(&args, "--limit", "-l"),
                10,
                "memory search limit",
            )?,
            path: option_value(&args, "--path", "--path"),
        });
    }
    if normalized.len() >= 2 && normalized[0] == "memory" && matches!(normalized[1], "list" | "ls")
    {
        return Ok(ParsedCommand::MemoryList {
            namespace: option_value(&args, "--namespace", "-n"),
            limit: parse_positive_usize(
                option_value(&args, "--limit", "-l"),
                50,
                "memory list limit",
            )?,
            path: option_value(&args, "--path", "--path"),
        });
    }
    if normalized.len() >= 2
        && normalized[0] == "memory"
        && matches!(normalized[1], "delete" | "rm")
    {
        let key = option_value(&args, "--key", "-k")
            .or_else(|| memory_positionals(&args).into_iter().next())
            .ok_or("memory key is required")?;
        return Ok(ParsedCommand::MemoryDelete {
            key,
            namespace: option_value(&args, "--namespace", "-n").unwrap_or_else(|| "default".into()),
            path: option_value(&args, "--path", "--path"),
        });
    }
    if normalized.len() >= 2 && normalized[0] == "memory" && normalized[1] == "stats" {
        return Ok(ParsedCommand::MemoryStats {
            path: option_value(&args, "--path", "--path"),
        });
    }
    if normalized.len() >= 2 && normalized[0] == "memory" && normalized[1] == "purge" {
        return Ok(ParsedCommand::MemoryPurge {
            namespace: option_value(&args, "--namespace", "-n")
                .ok_or("memory purge requires --namespace")?,
            dry_run: args
                .iter()
                .any(|argument| argument == "--dry-run" || argument == "-d"),
            force: args
                .iter()
                .any(|argument| argument == "--force" || argument == "-f"),
            path: option_value(&args, "--path", "--path"),
        });
    }
    if normalized.first() == Some(&"config") {
        if normalized
            .iter()
            .any(|value| matches!(*value, "--version" | "-V"))
        {
            return Ok(ParsedCommand::Version);
        }
        if normalized
            .iter()
            .any(|value| matches!(*value, "--help" | "-h"))
        {
            return Ok(ParsedCommand::ConfigHelp {
                subcommand: normalized
                    .get(1)
                    .filter(|value| !value.starts_with('-'))
                    .map(|value| (*value).to_string()),
            });
        }
        if normalized.len() == 1 {
            return Ok(ParsedCommand::ConfigOverview);
        }
        if let Some(format) = option_value(&args, "--format", "--format") {
            if !matches!(format.as_str(), "text" | "json" | "table") {
                return Err(format!(
                    "Invalid value for --format: {format}. Must be one of: text, json, table"
                ));
            }
        }
    }
    if normalized.len() >= 2 && normalized[0] == "config" && normalized[1] == "init" {
        return Ok(ParsedCommand::ConfigInit {
            force: boolean_option(&args, "--force", "-f", false),
            sparc: boolean_option(&args, "--sparc", "--sparc", false),
            v3: boolean_option(&args, "--v3", "--v3", true),
        });
    }
    if normalized.len() >= 2 && normalized[0] == "config" && normalized[1] == "get" {
        return Ok(ParsedCommand::ConfigGet {
            key: option_value(&args, "--key", "-k").or_else(|| {
                config_positionals(&args, 2, &["--key", "-k", "--format"])
                    .first()
                    .cloned()
            }),
            json: option_value(&args, "--format", "--format").as_deref() == Some("json"),
        });
    }
    if normalized.len() >= 2 && normalized[0] == "config" && normalized[1] == "set" {
        let key = option_value(&args, "--key", "-k");
        let value = option_value(&args, "--value", "-v");
        let (key, value) = match (key, value) {
            (Some(key), Some(value)) => (key, value),
            (None, None) => {
                return Err(
                    "Required option missing: --key\n[ERROR] Required option missing: --value"
                        .into(),
                )
            }
            (None, Some(_)) => return Err("Required option missing: --key".into()),
            (Some(_), None) => return Err("Required option missing: --value".into()),
        };
        return Ok(ParsedCommand::ConfigSet { key, value });
    }
    if normalized.len() >= 2 && normalized[0] == "config" && normalized[1] == "providers" {
        return Ok(ParsedCommand::ConfigProviders {
            add: option_value(&args, "--add", "-a"),
            remove: option_value(&args, "--remove", "-r"),
            enable: option_value(&args, "--enable", "--enable"),
            disable: option_value(&args, "--disable", "--disable"),
            json: option_value(&args, "--format", "--format").as_deref() == Some("json"),
        });
    }
    if normalized.len() >= 2 && normalized[0] == "config" && normalized[1] == "reset" {
        let section = option_value(&args, "--section", "--section");
        if let Some(value) = section.as_deref() {
            if !matches!(
                value,
                "agents" | "swarm" | "memory" | "mcp" | "providers" | "all"
            ) {
                return Err(
                    "config reset section must be agents, swarm, memory, mcp, providers, or all"
                        .into(),
                );
            }
        }
        return Ok(ParsedCommand::ConfigReset {
            force: boolean_option(&args, "--force", "-f", false),
            section,
        });
    }
    if normalized.len() >= 2 && normalized[0] == "config" && normalized[1] == "export" {
        let positionals = config_positionals(&args, 2, &["--output", "-o", "--format", "-f"]);
        let format = option_value(&args, "--format", "-f").unwrap_or_else(|| "json".into());
        if matches!(format.as_str(), "text" | "table") {
            return Err(format!(
                "Invalid value for --format: {format}. Must be one of: json, yaml"
            ));
        }
        if format != "json" {
            return Err(format!(
                "Invalid value for --format: {format}. Must be one of: text, json, table"
            ));
        }
        return Ok(ParsedCommand::ConfigExport {
            output: option_value(&args, "--output", "-o")
                .or_else(|| positionals.first().cloned())
                .unwrap_or_else(|| "claude-flow.config.export.json".into()),
            format,
        });
    }
    if normalized.len() >= 2 && normalized[0] == "config" && normalized[1] == "import" {
        let file = option_value(&args, "--file", "-f").ok_or("Required option missing: --file")?;
        return Ok(ParsedCommand::ConfigImport {
            file,
            merge: boolean_option(&args, "--merge", "--merge", false),
        });
    }
    if normalized.first() == Some(&"config") {
        return Ok(ParsedCommand::ConfigOverview);
    }
    if normalized.as_slice() == ["migrate", "status"] {
        return Ok(ParsedCommand::MigrateStatus);
    }
    if normalized.len() >= 2 && normalized[0] == "migrate" && normalized[1] == "run" {
        let target = option_value(&args, "--target", "-t").unwrap_or_else(|| "all".into());
        if !matches!(target.as_str(), "config" | "all") {
            return Err("native migrate run currently supports target config or all".into());
        }
        return Ok(ParsedCommand::MigrateRun {
            target,
            dry_run: args.iter().any(|value| value == "--dry-run"),
            backup: !args.iter().any(|value| value == "--no-backup"),
            force: args.iter().any(|value| value == "--force" || value == "-f"),
        });
    }
    if normalized.starts_with(&["agent", "spawn"]) {
        let agent_type = option_value(&args, "--type", "-t").ok_or("agent type is required")?;
        let name =
            option_value(&args, "--name", "-n").unwrap_or_else(|| format!("{agent_type}-native"));
        return Ok(ParsedCommand::AgentSpawn { agent_type, name });
    }
    if normalized.len() >= 2 && normalized[0] == "agent" && normalized[1] == "status" {
        let agent_id = normalized
            .get(2)
            .filter(|value| !value.starts_with('-'))
            .map(|value| value.to_string())
            .or_else(|| option_value(&args, "--id", "--id"))
            .ok_or("agent ID is required")?;
        return Ok(ParsedCommand::AgentStatus { agent_id });
    }
    if normalized.len() >= 3 && normalized[0] == "agent" && matches!(normalized[1], "stop" | "kill")
    {
        let timeout_seconds = option_value(&args, "--timeout", "--timeout")
            .unwrap_or_else(|| "30".into())
            .parse()
            .map_err(|_| "agent stop timeout must be a positive integer")?;
        if timeout_seconds == 0 {
            return Err("agent stop timeout must be a positive integer".into());
        }
        return Ok(ParsedCommand::AgentStop {
            agent_id: normalized[2].into(),
            force: args
                .iter()
                .any(|argument| argument == "--force" || argument == "-f"),
            timeout_seconds,
        });
    }
    if normalized.len() >= 2 && normalized[0] == "agent" && normalized[1] == "metrics" {
        return Ok(ParsedCommand::AgentMetrics {
            agent_id: normalized
                .get(2)
                .filter(|value| !value.starts_with('-'))
                .map(|value| value.to_string()),
            period: option_value(&args, "--period", "-p").unwrap_or_else(|| "24h".into()),
        });
    }
    if normalized.len() >= 2 && normalized[0] == "agent" && normalized[1] == "pool" {
        let min = option_value(&args, "--min", "--min")
            .unwrap_or_else(|| "1".into())
            .parse()
            .map_err(|_| "agent pool min must be a positive integer")?;
        let max = option_value(&args, "--max", "--max")
            .unwrap_or_else(|| "10".into())
            .parse()
            .map_err(|_| "agent pool max must be a positive integer")?;
        let size = option_value(&args, "--size", "-s")
            .map(|value| {
                value
                    .parse()
                    .map_err(|_| "agent pool size must be a positive integer")
            })
            .transpose()?;
        return Ok(ParsedCommand::AgentPool {
            size,
            min,
            max,
            auto_scale: !args.iter().any(|argument| argument == "--no-auto-scale"),
        });
    }
    if normalized.len() >= 2 && normalized[0] == "agent" && normalized[1] == "health" {
        return Ok(ParsedCommand::AgentHealth {
            agent_id: normalized
                .get(2)
                .filter(|value| !value.starts_with('-'))
                .map(|value| value.to_string())
                .or_else(|| option_value(&args, "--id", "-i")),
            detailed: args
                .iter()
                .any(|argument| argument == "--detailed" || argument == "-d"),
        });
    }
    if normalized.len() >= 2 && normalized[0] == "agent" && normalized[1] == "logs" {
        let agent_id = normalized
            .get(2)
            .filter(|value| !value.starts_with('-'))
            .map(|value| value.to_string())
            .or_else(|| option_value(&args, "--id", "-i"))
            .ok_or("agent ID is required. Use --id or -i")?;
        let tail = option_value(&args, "--tail", "-n")
            .unwrap_or_else(|| "50".into())
            .parse()
            .map_err(|_| "agent log tail must be a positive integer")?;
        let level = option_value(&args, "--level", "-l").unwrap_or_else(|| "info".into());
        if tail == 0 || !matches!(level.as_str(), "debug" | "info" | "warn" | "error") {
            return Err("agent logs requires a positive tail and a valid level".into());
        }
        return Ok(ParsedCommand::AgentLogs {
            agent_id,
            tail,
            level,
            // V3 exposes this flag but the command currently obtains one MCP
            // result rather than opening a separate streaming transport.
            follow: args
                .iter()
                .any(|argument| argument == "--follow" || argument == "-f"),
            since: option_value(&args, "--since", "--since"),
        });
    }
    if normalized.len() >= 2 && normalized[0] == "swarm" && normalized[1] == "init" {
        let topology = if args.iter().any(|argument| argument == "--v3-mode") {
            "hierarchical-mesh".into()
        } else {
            option_value(&args, "--topology", "-t").unwrap_or_else(|| "hierarchical".into())
        };
        let max_agents = option_value(&args, "--max-agents", "-m")
            .unwrap_or_else(|| "15".into())
            .parse()
            .map_err(|_| "max agents must be a positive integer")?;
        let strategy =
            option_value(&args, "--strategy", "-s").unwrap_or_else(|| "development".into());
        return Ok(ParsedCommand::SwarmInit {
            topology,
            max_agents,
            strategy,
        });
    }
    if normalized.len() >= 2 && normalized[0] == "swarm" && normalized[1] == "start" {
        let objective = option_value(&args, "--objective", "-o")
            .or_else(|| {
                normalized
                    .get(2)
                    .filter(|value| !value.starts_with('-'))
                    .map(|value| value.to_string())
            })
            .ok_or("swarm objective is required")?;
        let strategy =
            option_value(&args, "--strategy", "-s").unwrap_or_else(|| "development".into());
        return Ok(ParsedCommand::SwarmStart {
            objective,
            strategy,
        });
    }
    if normalized.len() >= 3 && normalized[0] == "swarm" && normalized[1] == "stop" {
        return Ok(ParsedCommand::SwarmStop {
            swarm_id: normalized[2].into(),
        });
    }
    if normalized.len() >= 3 && normalized[0] == "swarm" && normalized[1] == "scale" {
        let target_agents = option_value(&args, "--agents", "-a")
            .ok_or("target agent count required. Use --agents or -a")?
            .parse()
            .map_err(|_| "target agent count must be a positive integer")?;
        if target_agents == 0 {
            return Err("target agent count must be a positive integer".into());
        }
        return Ok(ParsedCommand::SwarmScale {
            swarm_id: normalized[2].into(),
            target_agents,
            agent_type: option_value(&args, "--type", "-t"),
        });
    }
    if normalized.len() >= 2 && normalized[0] == "swarm" && normalized[1] == "coordinate" {
        let agents = option_value(&args, "--agents", "--agents")
            .unwrap_or_else(|| "15".into())
            .parse()
            .map_err(|_| "coordination agent count must be a positive integer")?;
        if !(1..=15).contains(&agents) {
            return Err("coordination agent count must be between 1 and 15".into());
        }
        return Ok(ParsedCommand::SwarmCoordinate { agents });
    }
    if normalized.len() >= 2 && normalized[0] == "swarm" && normalized[1] == "compress-message" {
        let budget_tokens = option_value(&args, "--budget-tokens", "-b")
            .unwrap_or_else(|| "200".into())
            .parse()
            .map_err(|_| "budget tokens must be a positive integer")?;
        let mode = option_value(&args, "--mode", "--mode").unwrap_or_else(|| "hybrid".into());
        if budget_tokens == 0 || !matches!(mode.as_str(), "keyword" | "sentence" | "hybrid") {
            return Err(
                "budget must be positive and mode must be keyword, sentence, or hybrid".into(),
            );
        }
        return Ok(ParsedCommand::SwarmCompressMessage {
            message: option_value(&args, "--message", "-m"),
            message_file: option_value(&args, "--message-file", "--message-file"),
            budget_tokens,
            mode,
        });
    }
    if normalized.len() >= 2
        && normalized[0] == "session"
        && matches!(normalized[1], "save" | "create" | "checkpoint")
    {
        return Ok(ParsedCommand::SessionSave {
            name: option_value(&args, "--name", "-n").unwrap_or_else(|| "native-session".into()),
            description: option_value(&args, "--description", "-d").unwrap_or_default(),
        });
    }
    if normalized.len() >= 3
        && normalized[0] == "session"
        && matches!(normalized[1], "restore" | "load")
    {
        return Ok(ParsedCommand::SessionRestore {
            session_id: normalized[2].into(),
        });
    }
    if normalized.len() >= 3
        && normalized[0] == "session"
        && matches!(normalized[1], "delete" | "rm" | "remove")
    {
        return Ok(ParsedCommand::SessionDelete {
            session_id: normalized[2].into(),
        });
    }
    if normalized.len() >= 2 && normalized[0] == "session" && normalized[1] == "export" {
        let session_id = normalized
            .get(2)
            .filter(|value| !value.starts_with('-'))
            .map(|value| value.to_string());
        let output =
            option_value(&args, "--output", "-o").ok_or("session export output is required")?;
        return Ok(ParsedCommand::SessionExport { session_id, output });
    }
    if normalized.len() >= 3 && normalized[0] == "session" && normalized[1] == "import" {
        return Ok(ParsedCommand::SessionImport {
            input: normalized[2].into(),
            name: option_value(&args, "--name", "-n"),
        });
    }
    if normalized.len() >= 2
        && matches!(normalized[0], "task")
        && matches!(normalized[1], "create" | "new" | "add")
    {
        let task_type = option_value(&args, "--type", "-t").ok_or("task type is required")?;
        let description =
            option_value(&args, "--description", "-d").ok_or("task description is required")?;
        let priority = option_value(&args, "--priority", "-p").unwrap_or_else(|| "normal".into());
        return Ok(ParsedCommand::TaskCreate {
            task_type,
            description,
            priority,
        });
    }
    if normalized.len() >= 3
        && matches!(normalized[0], "task")
        && matches!(normalized[1], "status" | "info" | "get")
    {
        return Ok(ParsedCommand::TaskStatus {
            task_id: normalized[2].into(),
        });
    }
    if normalized.len() >= 3
        && matches!(normalized[0], "task")
        && matches!(normalized[1], "cancel" | "abort" | "stop")
    {
        return Ok(ParsedCommand::TaskCancel {
            task_id: normalized[2].into(),
            reason: option_value(&args, "--reason", "-r")
                .unwrap_or_else(|| "Cancelled by user via CLI".into()),
        });
    }
    if normalized.len() >= 3 && normalized[0] == "task" && normalized[1] == "assign" {
        let unassign = args.iter().any(|arg| arg == "--unassign");
        let agent_ids = option_value(&args, "--agent", "-a")
            .unwrap_or_default()
            .split(',')
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
            .collect::<Vec<_>>();
        if agent_ids.is_empty() && !unassign {
            return Err("agent ID is required. Use --agent or --unassign".into());
        }
        return Ok(ParsedCommand::TaskAssign {
            task_id: normalized[2].into(),
            agent_ids,
            unassign,
        });
    }
    if normalized.len() >= 3
        && matches!(normalized[0], "task")
        && matches!(normalized[1], "retry" | "rerun")
    {
        return Ok(ParsedCommand::TaskRetry {
            task_id: normalized[2].into(),
            reset_state: args.iter().any(|arg| arg == "--reset-state"),
        });
    }
    match normalized.as_slice() {
        ["--version"] | ["-V"] => Ok(ParsedCommand::Version),
        ["--help"] | ["-h"] | ["--quiet", "--help"] | ["-Q", "--help"] => Ok(ParsedCommand::Help),
        ["init"] => Ok(ParsedCommand::Init),
        ["status"] | ["status", "--json"] => Ok(ParsedCommand::Status),
        ["swarm", "status"] => Ok(ParsedCommand::SwarmStatus),
        ["session", "list"] | ["session", "ls"] => Ok(ParsedCommand::SessionList),
        ["session", "current"] => Ok(ParsedCommand::SessionCurrent),
        ["agent", "list"] | ["agent", "ls"] => Ok(ParsedCommand::AgentList),
        ["task", "list"] | ["task", "ls"] => Ok(ParsedCommand::TaskList),
        ["mcp", "start"] => Ok(ParsedCommand::McpStart),
        [] => Err("no command provided".to_string()),
        _ => Err(format!(
            "unsupported native CLI invocation: {}",
            args.join(" ")
        )),
    }
}

fn option_value(args: &[String], long: &str, short: &str) -> Option<String> {
    args.iter().enumerate().rev().find_map(|(index, argument)| {
        if argument == long || argument == short {
            args.get(index + 1).cloned()
        } else {
            argument
                .strip_prefix(&format!("{long}="))
                .or_else(|| argument.strip_prefix(&format!("{short}=")))
                .map(str::to_owned)
        }
    })
}

fn boolean_option(args: &[String], long: &str, short: &str, default: bool) -> bool {
    let no_flag = format!("--no-{}", long.trim_start_matches("--"));
    for (index, argument) in args.iter().enumerate().rev() {
        if argument == &no_flag {
            return false;
        }
        if argument == long || argument == short {
            return args
                .get(index + 1)
                .and_then(|value| match value.as_str() {
                    "true" => Some(true),
                    "false" => Some(false),
                    _ => None,
                })
                .unwrap_or(true);
        }
        if let Some(value) = argument.strip_prefix(&format!("{long}=")) {
            return value != "false";
        }
        if short.len() == 2
            && argument.starts_with('-')
            && !argument.starts_with("--")
            && argument[1..].contains(&short[1..])
        {
            return true;
        }
    }
    default
}

fn deployment_number(
    args: &[String],
    long: &str,
    short: &str,
    default: i64,
    label: &str,
) -> Result<i64, String> {
    option_value(args, long, short)
        .map(|value| {
            value
                .parse::<i64>()
                .map_err(|_| format!("{label} must be a number"))
        })
        .transpose()
        .map(|value| value.unwrap_or(default))
}

fn memory_positionals(args: &[String]) -> Vec<String> {
    const VALUE_OPTIONS: &[&str] = &[
        "--key",
        "-k",
        "--value",
        "--namespace",
        "-n",
        "--tags",
        "--provenance",
        "--path",
        "--query",
        "-q",
        "--limit",
        "-l",
        "--threshold",
        "--type",
        "-t",
    ];
    let mut positionals = Vec::new();
    let mut index = 2;
    while index < args.len() {
        if VALUE_OPTIONS.contains(&args[index].as_str()) {
            index += 2;
        } else if args[index].starts_with('-') {
            index += 1;
        } else {
            positionals.push(args[index].clone());
            index += 1;
        }
    }
    positionals
}

fn parse_positive_usize(
    value: Option<String>,
    default: usize,
    name: &str,
) -> Result<usize, String> {
    let value = match value {
        Some(value) => value,
        None => return Ok(default),
    };
    let parsed = value
        .parse::<usize>()
        .map_err(|_| format!("{name} must be a positive integer"))?;
    if parsed == 0 {
        return Err(format!("{name} must be a positive integer"));
    }
    Ok(parsed)
}

fn config_positionals(
    args: &[String],
    first_argument_index: usize,
    value_options: &[&str],
) -> Vec<String> {
    let mut values = Vec::new();
    let mut index = first_argument_index;
    while index < args.len() {
        if value_options.contains(&args[index].as_str()) || args[index] == "--format" {
            index += 2;
        } else if args[index].starts_with('-') {
            index += 1;
        } else {
            values.push(args[index].clone());
            index += 1;
        }
    }
    values
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;

    use super::{parse, ParsedCommand};

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
            Ok(ParsedCommand::SwarmStart { objective, strategy }) if objective == "Build API" && strategy == "testing"
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
        assert!(matches!(
            parse(argv(&["cleanup", "--version"])),
            Ok(ParsedCommand::Version)
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
}
