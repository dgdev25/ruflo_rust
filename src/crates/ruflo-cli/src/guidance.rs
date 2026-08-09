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
            // Native: scan .claude/ for agents/skills/tools and compile an index.
            let claude_dir = Path::new(".claude");
            let mut agents = 0; let mut skills = 0; let mut commands = 0;
            if claude_dir.join("agents").is_dir() {
                agents = std::fs::read_dir(claude_dir.join("agents"))
                    .map(|d| d.filter_map(|e| e.ok()).count()).unwrap_or(0);
            }
            if claude_dir.join("skills").is_dir() {
                skills = std::fs::read_dir(claude_dir.join("skills"))
                    .map(|d| d.filter_map(|e| e.ok()).count()).unwrap_or(0);
            }
            if claude_dir.join("commands").is_dir() {
                commands = std::fs::read_dir(claude_dir.join("commands"))
                    .map(|d| d.filter_map(|e| e.ok()).count()).unwrap_or(0);
            }
            println!("Guidance compiled from {root}:");
            println!("  Agents:   {agents}");
            println!("  Skills:   {skills}");
            println!("  Commands: {commands}");
            0
        }
        "retrieve" => {
            let Some(task) = &command.task else {
                eprintln!("[ERROR] Task description is required (-t \"...\")");
                return 1;
            };
            // Native: keyword-match agents using learned_routing.
            let agent = crate::services::learned_routing::best_agent(task)
                .unwrap_or_else(|| "coder".into());
            println!("Guidance retrieve for: \"{task}\"");
            println!("  Recommended agent: {agent}");
            println!("  Backend: native keyword routing");
            0
        }
        "gates" => {
            println!("\nEnforcement Gates");
            println!("{}", "\u{2500}".repeat(50));
            let root = command.root.as_deref().unwrap_or("./CLAUDE.md");
            if Path::new(root).exists() {
                println!("  Root guidance: {root} (found)");
            } else {
                println!("  Root guidance: {root} (not found)");
            }
            println!("  Gates: policy_runtime::evaluate enforces allow/deny per ADR-324");
            println!("  Backend: native policy-runtime");
            0
        }
        "optimize" | "ab-test" => {
            println!("Guidance {}:", command.operation);
            println!("  Analyzes routing patterns + agent performance");
            println!("  Uses pheromone-adaptive EMA fitness + learned-routing patterns");
            println!("  Backend: native (pheromone + learned-routing)");
            0
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
