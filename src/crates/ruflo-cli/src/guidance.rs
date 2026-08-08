//! Native V3 `guidance` command — guidance control plane.
//!
//! Source: `v3/@claude-flow/cli/src/commands/guidance.ts`. Subcommands:
//! compile/retrieve/gates/status/optimize/ab-test. Requires @claude-flow/guidance
//! (TypeScript module). Degrades with file-existence checks.

use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GuidanceCommand {
    pub operation: String,
    pub root: Option<String>,
    pub local: Option<String>,
    pub output: Option<String>,
    pub json: bool,
    pub task: Option<String>,
    pub max_shards: usize,
    pub gate: Option<String>,
}

pub fn run(_root: &Path, command: GuidanceCommand) -> u8 {
    match command.operation.as_str() {
        "" | "status" => {
            println!("\nGuidance Compiler");
            println!("{}", "\u{2500}".repeat(50));
            let root = command.root.as_deref().unwrap_or("./CLAUDE.md");
            if Path::new(root).exists() {
                println!("  Root guidance: {root} (found)");
            } else {
                println!("  Root guidance: {root} (not found)");
            }
            println!();
            println!("Subcommands:");
            println!("  compile   Compile guidance into a bundle");
            println!("  retrieve  Retrieve guidance shards for a task");
            println!("  gates     List enforcement gates");
            println!("  status    Show guidance status");
            println!("  optimize  Optimize shard weights");
            println!("  ab-test   Run A/B test on guidance");
            0
        }
        "compile" => {
            let root = command.root.as_deref().unwrap_or("./CLAUDE.md");
            if !Path::new(root).exists() {
                eprintln!("[ERROR] Root guidance file not found: {root}");
                return 1;
            }
            eprintln!("[ERROR] Guidance compilation not available in native build.");
            eprintln!("  Requires @claude-flow/guidance compiler module.");
            eprintln!("  Use: npx ruflo guidance compile -r {root}");
            1
        }
        "retrieve" => {
            let Some(task) = &command.task else {
                eprintln!("[ERROR] Task description is required (-t \"...\")");
                return 1;
            };
            let root = command.root.as_deref().unwrap_or("./CLAUDE.md");
            if !Path::new(root).exists() {
                eprintln!("[ERROR] Root guidance file not found: {root}");
                return 1;
            }
            eprintln!("[ERROR] Guidance retrieval not available in native build.");
            eprintln!("  Task: \"{task}\"");
            eprintln!("  Use: npx ruflo guidance retrieve -t \"{task}\"");
            1
        }
        "gates" => {
            println!("\nEnforcement Gates");
            println!("{}", "\u{2500}".repeat(50));
            let root = command.root.as_deref().unwrap_or("./CLAUDE.md");
            if Path::new(root).exists() {
                println!("  Root guidance: {root} (found, gates deferred to TS module)");
            } else {
                println!("  Root guidance: {root} (not found)");
            }
            eprintln!("\n[ERROR] Gate evaluation requires @claude-flow/guidance.");
            1
        }
        "optimize" | "ab-test" => {
            eprintln!(
                "[ERROR] Guidance {op} not available in native build.",
                op = command.operation
            );
            eprintln!("  Requires @claude-flow/guidance.");
            eprintln!("  Use: npx ruflo guidance {op}", op = command.operation);
            1
        }
        _ => {
            eprintln!(
                "[ERROR] Unknown: {} (compile|retrieve|gates|status|optimize|ab-test)",
                command.operation
            );
            1
        }
    }
}
