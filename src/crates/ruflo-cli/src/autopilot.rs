//! Native V3 `autopilot` command — persistent swarm completion.
//!
//! Source: `v3/@claude-flow/cli/src/commands/autopilot.ts`. Subcommands:
//! status/enable/disable/config/reset/log/learn/history/predict/check.
//! State in .claude-flow/autopilot.json. Learning/prediction deferred (AgentDB).

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{json, Value};

fn state_file(root: &Path) -> PathBuf {
    root.join(".claude-flow/autopilot.json")
}
fn log_file(root: &Path) -> PathBuf {
    root.join(".claude-flow/autopilot-log.jsonl")
}

fn load_state(root: &Path) -> Value {
    fs::read_to_string(state_file(root)).ok()
        .and_then(|r| serde_json::from_str(&r).ok())
        .unwrap_or_else(|| json!({"enabled": false, "maxIterations": 100, "timeoutMinutes": 1440, "iterations": 0, "taskSources": ["team-tasks","swarm-tasks","file-checklist"]}))
}

fn save_state(root: &Path, state: &Value) -> bool {
    let dir = root.join(".claude-flow");
    let _ = fs::create_dir_all(&dir);
    let path = state_file(root);
    let tmp = path.with_extension("json.tmp");
    let Ok(bytes) = serde_json::to_vec_pretty(state) else {
        return false;
    };
    fs::write(&tmp, &bytes).is_ok() && fs::rename(&tmp, &path).is_ok()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutopilotCommand {
    pub operation: String,
    pub json: bool,
    pub max_iterations: Option<u64>,
    pub timeout: Option<u64>,
    pub task_sources: Option<String>,
    pub last: Option<u64>,
    pub clear: bool,
    pub query: Option<String>,
    pub limit: Option<u64>,
}

pub fn run(root: &Path, command: AutopilotCommand) -> u8 {
    match command.operation.as_str() {
        "status" | "" => status(root, &command),
        "enable" => enable(root),
        "disable" => disable(root),
        "config" => config(root, &command),
        "reset" => reset(root),
        "log" => log(root, &command),
        "learn" => {
            if command.json {
                println!(
                    "{}",
                    json!({"available": false, "note": "AgentDB not initialized in native build."})
                );
            } else {
                println!("Learning not available (AgentDB not initialized). Autopilot still works for task completion tracking.");
            }
            0
        }
        "history" => {
            println!("Usage: autopilot history --query \"search terms\" [--limit N]");
            0
        }
        "predict" => {
            println!("Action: unknown");
            println!("Confidence: 0");
            println!("Note: prediction requires AgentDB (not available in native build)");
            0
        }
        "check" => {
            let state = load_state(root);
            let enabled = state["enabled"].as_bool() == Some(true);
            if enabled {
                println!("CONTINUE: autopilot is enabled; keep agents working.");
            } else {
                println!("ALLOW STOP: autopilot is disabled.");
            }
            0
        }
        _ => {
            eprintln!("[ERROR] Unknown: {} (status|enable|disable|config|reset|log|learn|history|predict|check)", command.operation);
            1
        }
    }
}

fn status(root: &Path, command: &AutopilotCommand) -> u8 {
    let state = load_state(root);
    if command.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&state).unwrap_or_default()
        );
    } else {
        let enabled = state["enabled"].as_bool() == Some(true);
        let max_it = state["maxIterations"].as_u64().unwrap_or(100);
        let timeout = state["timeoutMinutes"].as_u64().unwrap_or(1440);
        let iters = state["iterations"].as_u64().unwrap_or(0);
        println!(
            "Autopilot: {}",
            if enabled { "enabled" } else { "disabled" }
        );
        println!("  Iterations: {iters}/{max_it}");
        println!("  Timeout: {timeout} min");
    }
    0
}

fn enable(root: &Path) -> u8 {
    let mut state = load_state(root);
    if let Some(o) = state.as_object_mut() {
        o.insert("enabled".into(), json!(true));
        o.insert("iterations".into(), json!(0));
    }
    save_state(root, &state);
    let max_it = state["maxIterations"].as_u64().unwrap_or(100);
    let timeout = state["timeoutMinutes"].as_u64().unwrap_or(1440);
    println!("\u{2714} Autopilot enabled (max {max_it} iterations, {timeout} min timeout)");
    0
}

fn disable(root: &Path) -> u8 {
    let mut state = load_state(root);
    if let Some(o) = state.as_object_mut() {
        o.insert("enabled".into(), json!(false));
    }
    save_state(root, &state);
    println!("Autopilot disabled");
    0
}

fn config(root: &Path, command: &AutopilotCommand) -> u8 {
    let mut state = load_state(root);
    if let Some(o) = state.as_object_mut() {
        if let Some(max_it) = command.max_iterations {
            if !(1..=1000).contains(&max_it) {
                eprintln!("[ERROR] max-iterations must be 1-1000");
                return 1;
            }
            o.insert("maxIterations".into(), json!(max_it));
        }
        if let Some(timeout) = command.timeout {
            if !(1..=1440).contains(&timeout) {
                eprintln!("[ERROR] timeout must be 1-1440 minutes");
                return 1;
            }
            o.insert("timeoutMinutes".into(), json!(timeout));
        }
        if let Some(ref sources) = command.task_sources {
            let arr: Vec<Value> = sources.split(',').map(|s| json!(s.trim())).collect();
            o.insert("taskSources".into(), json!(arr));
        }
    }
    save_state(root, &state);
    println!("Autopilot config updated");
    0
}

fn reset(root: &Path) -> u8 {
    let mut state = load_state(root);
    if let Some(o) = state.as_object_mut() {
        o.insert("iterations".into(), json!(0));
    }
    save_state(root, &state);
    println!("Autopilot state reset (iterations=0, timer restarted)");
    0
}

fn log(root: &Path, command: &AutopilotCommand) -> u8 {
    if command.clear {
        let _ = fs::write(log_file(root), "");
        println!("Autopilot log cleared");
        return 0;
    }
    let raw = fs::read_to_string(log_file(root)).unwrap_or_default();
    let lines: Vec<&str> = raw.lines().filter(|l| !l.is_empty()).collect();
    let limit = command.last.unwrap_or(20) as usize;
    let limited: Vec<&&str> = lines.iter().rev().take(limit).collect();
    if command.json {
        let entries: Vec<Value> = limited
            .iter()
            .filter_map(|l| serde_json::from_str(l).ok())
            .collect();
        println!(
            "{}",
            serde_json::to_string_pretty(&json!(entries)).unwrap_or_default()
        );
    } else if limited.is_empty() {
        println!("No autopilot log entries.");
    } else {
        for line in limited.iter().rev() {
            if let Ok(v) = serde_json::from_str::<Value>(line) {
                let action = v["action"].as_str().unwrap_or("?");
                let ts = v["timestamp"].as_str().unwrap_or("?");
                println!("  [{ts}] {action}");
            }
        }
    }
    0
}
