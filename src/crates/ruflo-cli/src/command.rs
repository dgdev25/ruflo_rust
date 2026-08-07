use std::ffi::OsString;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParsedCommand {
    Version,
    Help,
    McpStartPlaceholder {
        capability: &'static str,
        wave: u8,
        migration: &'static str,
    },
}

pub fn parse(argv: impl IntoIterator<Item = OsString>) -> Result<ParsedCommand, String> {
    let args = argv
        .into_iter()
        .skip(1)
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    let normalized = args.iter().map(String::as_str).collect::<Vec<_>>();

    match normalized.as_slice() {
        ["--version"] | ["-V"] => Ok(ParsedCommand::Version),
        ["--help"] | ["-h"] | ["--quiet", "--help"] | ["-Q", "--help"] => Ok(ParsedCommand::Help),
        ["mcp", "start"] => Ok(ParsedCommand::McpStartPlaceholder {
            capability: "mcp.start",
            wave: 1,
            migration: "enable the native MCP dispatcher",
        }),
        [] => Err("no command provided".to_string()),
        _ => Err(format!(
            "unsupported native CLI invocation: {}",
            args.join(" ")
        )),
    }
}
