use std::ffi::OsString;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParsedCommand {
    Version,
    Help,
    Init,
    Status,
    AgentSpawn { agent_type: String, name: String },
    AgentList,
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
    match normalized.as_slice() {
        ["--version"] | ["-V"] => Ok(ParsedCommand::Version),
        ["--help"] | ["-h"] | ["--quiet", "--help"] | ["-Q", "--help"] => Ok(ParsedCommand::Help),
        ["init"] => Ok(ParsedCommand::Init),
        ["status"] | ["status", "--json"] => Ok(ParsedCommand::Status),
        ["agent", "list"] | ["agent", "ls"] => Ok(ParsedCommand::AgentList),
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
