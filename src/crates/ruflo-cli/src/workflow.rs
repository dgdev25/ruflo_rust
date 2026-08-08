//! Native V3 `workflow` command — workflow execution and management.
//!
//! Source: `v3/@claude-flow/cli/src/commands/workflow.ts`. Subcommands:
//! run/validate/list/status/stop. Uses the owning MCP workflow tools; native
//! build reads local workflow definitions + degrades execution.

use std::fs;
use std::path::{Path, PathBuf};

fn workflow_dir(root: &Path) -> PathBuf {
    root.join(".claude-flow/workflows")
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowCommand {
    pub operation: String,
    pub template: Option<String>,
    pub file: Option<String>,
    pub task: Option<String>,
    pub parallel: bool,
    pub max_agents: usize,
    pub timeout: Option<u64>,
    pub dry_run: bool,
    pub workflow_id: Option<String>,
    pub json: bool,
}

pub fn run(root: &Path, command: WorkflowCommand) -> u8 {
    match command.operation.as_str() {
        "" | "list" => list(root, &command),
        "run" => run_workflow(root, &command),
        "validate" => validate(root, &command),
        "status" => status(root, &command),
        "stop" => stop(root, &command),
        _ => {
            eprintln!(
                "[ERROR] Unknown: {} (run|validate|list|status|stop)",
                command.operation
            );
            1
        }
    }
}

fn list(root: &Path, command: &WorkflowCommand) -> u8 {
    let dir = workflow_dir(root);
    println!("\nWorkflows");
    println!("{}", "\u{2500}".repeat(50));
    if !dir.is_dir() {
        println!("  No workflows defined.");
        println!("  Create one at {}/<name>.json", dir.display());
        return 0;
    }
    let mut entries: Vec<_> = fs::read_dir(&dir)
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            if name.ends_with(".json") {
                Some(name.trim_end_matches(".json").to_string())
            } else {
                None
            }
        })
        .collect();
    entries.sort();
    if entries.is_empty() {
        println!("  No workflows found.");
    } else {
        for name in &entries {
            println!("  {name}");
        }
        if command.json {
            println!();
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!(entries)).unwrap_or_default()
            );
        }
    }
    0
}

fn run_workflow(root: &Path, command: &WorkflowCommand) -> u8 {
    let file = if let Some(ref f) = command.file {
        PathBuf::from(f)
    } else if let Some(ref t) = command.template {
        workflow_dir(root).join(format!("{t}.json"))
    } else {
        eprintln!("[ERROR] --template or --file is required");
        return 1;
    };
    if !file.exists() {
        eprintln!("[ERROR] Workflow file not found: {}", file.display());
        return 1;
    }
    let raw = match fs::read_to_string(&file) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("[ERROR] Failed to read {}: {e}", file.display());
            return 1;
        }
    };
    let wf: serde_json::Value = match serde_json::from_str(&raw) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("[ERROR] Invalid workflow JSON: {e}");
            return 1;
        }
    };
    let name = wf["name"].as_str().unwrap_or("unnamed");
    let steps = wf["steps"].as_array().map(|a| a.len()).unwrap_or(0);
    println!("\nWorkflow: {name}");
    println!("  File:   {}", file.display());
    println!("  Steps:  {steps}");
    println!("  Parallel: {}", command.parallel);
    println!("  Max agents: {}", command.max_agents);
    if let Some(t) = command.timeout {
        println!("  Timeout: {t}s");
    }
    if command.dry_run {
        println!("\n[dry-run] Would execute {steps} steps.");
        return 0;
    }
    eprintln!("\n[ERROR] Workflow execution requires MCP workflow tools (not in native build).");
    eprintln!("  Use: npx ruflo workflow run --file {}", file.display());
    1
}

fn validate(root: &Path, command: &WorkflowCommand) -> u8 {
    let file = if let Some(ref f) = command.file {
        PathBuf::from(f)
    } else if let Some(ref t) = command.template {
        workflow_dir(root).join(format!("{t}.json"))
    } else {
        eprintln!("[ERROR] --template or --file is required");
        return 1;
    };
    if !file.exists() {
        eprintln!("[ERROR] Workflow file not found: {}", file.display());
        return 1;
    }
    let raw = match fs::read_to_string(&file) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("[ERROR] Failed to read {}: {e}", file.display());
            return 1;
        }
    };
    match serde_json::from_str::<serde_json::Value>(&raw) {
        Ok(wf) => {
            let name = wf["name"].as_str().unwrap_or("(no name)");
            let steps = wf["steps"].as_array().map(|a| a.len()).unwrap_or(0);
            if steps == 0 {
                eprintln!("[ERROR] Workflow '{name}' has no steps array.");
                return 1;
            }
            println!("\u{2714} Valid: {name} ({steps} steps)");
            0
        }
        Err(e) => {
            eprintln!("[ERROR] Invalid JSON: {e}");
            1
        }
    }
}

fn status(root: &Path, _command: &WorkflowCommand) -> u8 {
    let state = root.join(".claude-flow/workflow-state.json");
    if !state.exists() {
        println!("No active workflows.");
        return 0;
    }
    match fs::read_to_string(&state) {
        Ok(raw) => {
            let v: serde_json::Value = serde_json::from_str(&raw).unwrap_or(serde_json::json!({}));
            println!("{}", serde_json::to_string_pretty(&v).unwrap_or_default());
        }
        Err(_) => println!("No active workflows."),
    }
    0
}

fn stop(_root: &Path, _command: &WorkflowCommand) -> u8 {
    eprintln!("[ERROR] Workflow stop requires MCP workflow tools (not in native build).");
    eprintln!("  Use: npx ruflo workflow stop <id>");
    1
}
