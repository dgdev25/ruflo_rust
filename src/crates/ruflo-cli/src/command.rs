use std::ffi::OsString;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParsedCommand {
    Version,
    Help,
    Init,
    Status,
    AgentSpawn {
        agent_type: String,
        name: String,
    },
    AgentList,
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
}
