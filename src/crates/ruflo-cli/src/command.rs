use std::ffi::OsString;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParsedCommand {
    Version,
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
    let args = argv
        .into_iter()
        .skip(1)
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    let normalized = args.iter().map(String::as_str).collect::<Vec<_>>();

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
    args.iter()
        .position(|arg| arg == long || arg == short)
        .and_then(|index| args.get(index + 1))
        .cloned()
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
}
