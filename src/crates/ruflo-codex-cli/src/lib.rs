//! Worker-free compatibility façade for the live `claude-flow-codex` CLI.
//!
//! Commands that would launch Codex or Claude workers are intentionally not
//! implemented here. They require the later native scheduler, policy,
//! worktree, cancellation, and durable-receipt contract.

use std::ffi::OsString;
use std::process::ExitCode;

const VERSION: &str = "3.0.1\n";
const TEMPLATES: &str = r#"
Available Collaboration Templates:

feature - Feature Development Swarm
  Pipeline: architect → coder → tester → reviewer
  Platforms: Claude (architect, reviewer) + Codex (coder, tester)
  Usage: npx claude-flow-codex dual run --template feature --task "Add user auth"

security - Security Audit Swarm
  Pipeline: scanner → analyzer → fixer
  Platforms: Codex (scanner, fixer) + Claude (analyzer)
  Usage: npx claude-flow-codex dual run --template security --task "src/auth/"

refactor - Refactoring Swarm
  Pipeline: analyzer → planner → refactorer → validator
  Platforms: Claude (analyzer, planner) + Codex (refactorer, validator)
  Usage: npx claude-flow-codex dual run --template refactor --task "src/legacy/"

Custom configurations can be provided via --config <path.json>
"#;
const EMPTY_DUAL_RUN: &str = r#"═══════════════════════════════════════════════════════════════
  DUAL-MODE COLLABORATIVE EXECUTION
  Claude Code + Codex workers with shared memory
═══════════════════════════════════════════════════════════════

Please specify --template <name>, a [template] argument, --worker <spec> (repeatable), or --config <path>

Templates:
  feature  - Feature development (architect -> coder -> tester -> reviewer)
  security - Security audit (scanner -> analyzer -> fixer)
  refactor - Code refactoring (analyzer -> planner -> refactorer -> validator)

Custom workers:
  --worker "claude:architect:Design the API" --worker "codex:coder:Implement it"
"#;

pub fn run(argv: impl IntoIterator<Item = OsString>) -> ExitCode {
    let args = argv
        .into_iter()
        .skip(1)
        .map(|argument| argument.to_string_lossy().into_owned())
        .collect::<Vec<_>>();

    match args
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .as_slice()
    {
        ["--version"] | ["-v"] => {
            print!("{VERSION}");
            ExitCode::SUCCESS
        }
        ["dual", "templates"] => {
            print!("{TEMPLATES}");
            ExitCode::SUCCESS
        }
        ["dual", "run"] => {
            print!("{EMPTY_DUAL_RUN}");
            ExitCode::SUCCESS
        }
        _ => {
            eprintln!(
                "error: unsupported native Codex compatibility invocation: {}",
                args.join(" ")
            );
            ExitCode::from(2)
        }
    }
}
