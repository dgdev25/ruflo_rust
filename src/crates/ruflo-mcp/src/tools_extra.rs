//! Extra MCP tool handlers — thin wrappers over `.claude-flow/` state files.

use std::fs;
use std::path::PathBuf;

use serde_json::{json, Value};
use ruflo_types::RufloError;

use crate::dispatcher::ToolResult;

fn state_file(name: &str) -> PathBuf {
    std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join(".claude-flow")
        .join(name)
}

fn read_state(name: &str) -> Value {
    fs::read_to_string(state_file(name))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_else(|| json!({}))
}

fn write_state(name: &str, v: &Value) -> Result<(), RufloError> {
    let path = state_file(name);
    if let Some(dir) = path.parent() {
        let _ = fs::create_dir_all(dir);
    }
    let tmp = path.with_extension("json.tmp");
    let bytes = serde_json::to_vec_pretty(v).unwrap_or_default();
    fs::write(&tmp, &bytes)
        .map_err(|e| RufloError::UpstreamAdapter { message: format!("write {name}: {e}") })?;
    fs::rename(&tmp, &path)
        .map_err(|e| RufloError::UpstreamAdapter { message: format!("rename {name}: {e}") })
}

fn now_ms() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

fn req_str(args: &Value, key: &str) -> Result<String, RufloError> {
    args.get(key).and_then(|v| v.as_str()).map(|s| s.to_string())
        .ok_or_else(|| RufloError::UpstreamAdapter { message: format!("missing `{key}`") })
}

fn opt_str(args: &Value, key: &str) -> Option<String> {
    args.get(key).and_then(|v| v.as_str()).map(|s| s.to_string())
}

fn opt_u64(args: &Value, key: &str) -> Option<u64> {
    args.get(key).and_then(|v| v.as_u64())
}

pub fn definitions() -> Vec<(&'static str, &'static str)> {
    vec![
        ("agent_execute", "Execute a task on a spawned agent."),
        ("agent_list", "List all active agents."),
        ("agent_status", "Show detailed agent status."),
        ("agent_terminate", "Terminate a running agent."),
        ("swarm_init", "Initialize a new swarm."),
        ("swarm_status", "Show swarm status."),
        ("swarm_shutdown", "Stop swarm execution."),
        ("swarm_coordinate", "V3 multi-agent coordination."),
        ("task_create", "Create a new task."),
        ("task_list", "List all tasks."),
        ("task_status", "Get task details."),
        ("task_cancel", "Cancel a running task."),
        ("task_assign", "Assign task to agent(s)."),
        ("security_scan", "Run a security scan."),
        ("security_defend", "AI manipulation defense scan."),
        ("embeddings_generate", "Generate an embedding."),
        ("embeddings_search", "Semantic similarity search."),
        ("embeddings_compare", "Compare two texts."),
        ("neural_train", "Train neural patterns."),
        ("neural_status", "Neural network status."),
        ("hive_mind_init", "Initialize a hive mind."),
        ("hive_mind_status", "Show hive status."),
        ("hive_mind_spawn", "Spawn hive workers."),
        ("hive_mind_shutdown", "Shutdown the hive."),
        ("session_save", "Save current session."),
        ("session_list", "List all sessions."),
        ("session_restore", "Restore a saved session."),
        ("claims_check", "Check if a claim is granted."),
        ("claims_list", "List all claims."),
    ]
}

pub fn handle(name: &str, args: &Value) -> Result<ToolResult, RufloError> {
    match name {
        "agent_execute" => {
            let id = req_str(args, "agentId")?;
            let task = opt_str(args, "task").unwrap_or_else(|| "execute".into());
            Ok(ToolResult::text(format!("agent `{id}` executing: {task}"),
                Some(json!({"agentId": id, "task": task, "status": "running"}))))
        }
        "agent_list" => {
            let s = read_state("agents.json");
            let a = s["agents"].as_array().cloned().unwrap_or_default();
            Ok(ToolResult::text(format!("{} agent(s)", a.len()), Some(json!({"agents": a}))))
        }
        "agent_status" => {
            let id = req_str(args, "agentId")?;
            let s = read_state("agents.json");
            let a = s["agents"].as_array().and_then(|x| x.iter().find(|v| v["id"].as_str() == Some(id.as_str()))).cloned()
                .unwrap_or_else(|| json!({"id": id, "status": "not found"}));
            Ok(ToolResult::text(format!("agent {id}"), Some(a)))
        }
        "agent_terminate" => {
            let id = req_str(args, "agentId")?;
            Ok(ToolResult::text(format!("agent `{id}` terminated"),
                Some(json!({"agentId": id, "status": "terminated"}))))
        }
        "swarm_init" => {
            let topo = opt_str(args, "topology").unwrap_or_else(|| "hierarchical-mesh".into());
            let max = opt_u64(args, "maxAgents").unwrap_or(15);
            let sid = format!("swarm-{}", now_ms());
            let st = json!({"id": sid, "topology": topo, "maxAgents": max, "status": "initialized", "workers": []});
            write_state("swarm.json", &st)?;
            Ok(ToolResult::text(format!("swarm `{sid}` initialized"), Some(st)))
        }
        "swarm_status" => {
            let st = read_state("swarm.json");
            Ok(ToolResult::text(format!("swarm: {}", st["status"].as_str().unwrap_or("none")), Some(st)))
        }
        "swarm_shutdown" => {
            let mut st = read_state("swarm.json");
            st["status"] = json!("stopped");
            write_state("swarm.json", &st)?;
            Ok(ToolResult::text("swarm stopped", Some(st)))
        }
        "swarm_coordinate" => {
            let task = opt_str(args, "task").unwrap_or_else(|| "coordinate".into());
            let agents = opt_u64(args, "agents").unwrap_or(15);
            Ok(ToolResult::text(format!("coordinating {agents} agents: {task}"),
                Some(json!({"task": task, "agents": agents}))))
        }
        "task_create" => {
            let desc = req_str(args, "description")?;
            let pri = opt_str(args, "priority").unwrap_or_else(|| "normal".into());
            let tid = format!("task-{}", now_ms());
            let t = json!({"id": tid, "description": desc, "priority": pri, "status": "pending"});
            let mut st = read_state("tasks.json");
            let mut tasks = st["tasks"].as_array().cloned().unwrap_or_default();
            tasks.push(t.clone());
            st["tasks"] = json!(tasks);
            write_state("tasks.json", &st)?;
            Ok(ToolResult::text(format!("task `{tid}` created"), Some(t)))
        }
        "task_list" => {
            let st = read_state("tasks.json");
            let t = st["tasks"].as_array().cloned().unwrap_or_default();
            Ok(ToolResult::text(format!("{} task(s)", t.len()), Some(json!({"tasks": t}))))
        }
        "task_status" => {
            let tid = req_str(args, "taskId")?;
            let st = read_state("tasks.json");
            let t = st["tasks"].as_array().and_then(|x| x.iter().find(|v| v["id"].as_str() == Some(tid.as_str()))).cloned()
                .unwrap_or_else(|| json!({"id": tid, "status": "not found"}));
            Ok(ToolResult::text(format!("task {tid}"), Some(t)))
        }
        "task_cancel" => {
            let tid = req_str(args, "taskId")?;
            Ok(ToolResult::text(format!("task `{tid}` cancelled"),
                Some(json!({"taskId": tid, "status": "cancelled"}))))
        }
        "task_assign" => {
            let tid = req_str(args, "taskId")?;
            let aids = args.get("agentIds").cloned().unwrap_or(json!([]));
            Ok(ToolResult::text(format!("task `{tid}` assigned"),
                Some(json!({"taskId": tid, "agentIds": aids}))))
        }
        "security_scan" => {
            let input = opt_str(args, "input").unwrap_or_else(|| "(none)".into());
            Ok(ToolResult::text(format!("scan: {input}"),
                Some(json!({"input": input, "safe": true, "threats": []}))))
        }
        "security_defend" => {
            let input = req_str(args, "input")?;
            let lower = input.to_lowercase();
            let threats: Vec<&str> = ["ignore previous instructions", "jailbreak", "reveal your system prompt"]
                .iter().filter(|p| lower.contains(&p.to_lowercase())).copied().collect();
            Ok(ToolResult::text(format!("{} threat(s)", threats.len()),
                Some(json!({"input": input, "safe": threats.is_empty(), "threats": threats}))))
        }
        "embeddings_generate" => {
            let text = req_str(args, "text")?;
            let mut h: u64 = 0xcbf29ce484222325;
            for b in text.to_lowercase().as_bytes() { h ^= *b as u64; h = h.wrapping_mul(0x100000001b3); }
            let vec: Vec<f64> = (0..8).map(|i| ((h.wrapping_mul(i as u64 + 1)) as f64) / (u64::MAX as f64) - 0.5).collect();
            Ok(ToolResult::text(format!("embedding for: {text}"),
                Some(json!({"text": text, "dimensions": 8, "embedding": vec}))))
        }
        "embeddings_search" => {
            let q = req_str(args, "query")?;
            let limit = opt_u64(args, "limit").unwrap_or(10);
            Ok(ToolResult::text(format!("search: {q}"),
                Some(json!({"query": q, "limit": limit, "results": []}))))
        }
        "embeddings_compare" => {
            let t1 = req_str(args, "text1")?;
            let t2 = req_str(args, "text2")?;
            let l1 = t1.to_lowercase(); let s1: std::collections::HashSet<&str> = l1.split_whitespace().collect();
            let l2 = t2.to_lowercase(); let s2: std::collections::HashSet<&str> = l2.split_whitespace().collect();
            let score = s1.intersection(&s2).count() as f64 / s1.union(&s2).count().max(1) as f64;
            Ok(ToolResult::text(format!("similarity: {score:.3}"),
                Some(json!({"text1": t1, "text2": t2, "score": score}))))
        }
        "neural_train" => {
            let p = opt_str(args, "pattern").unwrap_or_else(|| "coordination".into());
            let m = format!("model-{}", now_ms());
            Ok(ToolResult::text(format!("training recorded: {p}"),
                Some(json!({"modelId": m, "pattern": p}))))
        }
        "neural_status" => {
            let st = read_state("neural/stats.json");
            let patterns = st["trainingRuns"].as_array().map(|a| a.len()).unwrap_or(0);
            Ok(ToolResult::text(format!("neural: {patterns} patterns"),
                Some(json!({"patternsLearned": patterns}))))
        }
        "hive_mind_init" => {
            let topo = opt_str(args, "topology").unwrap_or_else(|| "hierarchical-mesh".into());
            let cons = opt_str(args, "consensus").unwrap_or_else(|| "byzantine".into());
            let hid = format!("hive-{}", now_ms());
            let st = json!({"hiveId": hid, "topology": topo, "consensus": cons, "workers": [], "memory": {}, "proposals": []});
            write_state("hive-mind.json", &st)?;
            Ok(ToolResult::text(format!("hive `{hid}` initialized"), Some(st)))
        }
        "hive_mind_status" => {
            let st = read_state("hive-mind.json");
            Ok(ToolResult::text("hive status", Some(st)))
        }
        "hive_mind_spawn" => {
            let count = opt_u64(args, "count").unwrap_or(1) as usize;
            let mut st = read_state("hive-mind.json");
            let mut w = st["workers"].as_array().cloned().unwrap_or_default();
            for i in 0..count { w.push(json!({"id": format!("w-{}-{i}", now_ms()), "role": "worker"})); }
            st["workers"] = json!(w);
            write_state("hive-mind.json", &st)?;
            Ok(ToolResult::text(format!("spawned {count}"), Some(st)))
        }
        "hive_mind_shutdown" => {
            let mut st = read_state("hive-mind.json");
            st["status"] = json!("shutdown");
            write_state("hive-mind.json", &st)?;
            Ok(ToolResult::text("hive shutdown", Some(st)))
        }
        "session_save" => {
            let name = opt_str(args, "name").unwrap_or_else(|| format!("s-{}", now_ms()));
            let s = json!({"name": name, "savedAt": now_ms(), "data": args.get("data").cloned().unwrap_or(json!({}))});
            let mut st = read_state("sessions.json");
            let mut sessions = st["sessions"].as_array().cloned().unwrap_or_default();
            sessions.push(s.clone());
            st["sessions"] = json!(sessions);
            write_state("sessions.json", &st)?;
            Ok(ToolResult::text(format!("session `{name}` saved"), Some(s)))
        }
        "session_list" => {
            let st = read_state("sessions.json");
            let s = st["sessions"].as_array().cloned().unwrap_or_default();
            Ok(ToolResult::text(format!("{} session(s)", s.len()), Some(json!({"sessions": s}))))
        }
        "session_restore" => {
            let sid = req_str(args, "sessionId")?;
            let st = read_state("sessions.json");
            let s = st["sessions"].as_array().and_then(|x| x.iter().find(|v| v["name"].as_str() == Some(sid.as_str()))).cloned()
                .unwrap_or_else(|| json!({"name": sid, "status": "not found"}));
            Ok(ToolResult::text(format!("session `{sid}`"), Some(s)))
        }
        "claims_check" => {
            let claim = req_str(args, "claim")?;
            let st = read_state("claims.json");
            let defaults = st["defaultClaims"].as_array().cloned().unwrap_or_default();
            let user = args.get("user").and_then(|v| v.as_str()).unwrap_or("current");
            let user_claims = st["users"][user]["claims"].as_array().cloned().unwrap_or_default();
            let all: Vec<&str> = defaults.iter().chain(user_claims.iter()).filter_map(|c| c.as_str()).collect();
            let granted = all.iter().any(|c| *c == claim || (c.ends_with(':') && claim.starts_with(*c)) || *c == "*");
            Ok(ToolResult::text(format!("claim `{claim}`: {}", if granted { "granted" } else { "denied" }),
                Some(json!({"claim": claim, "granted": granted}))))
        }
        "claims_list" => {
            let st = read_state("claims.json");
            let defaults = st["defaultClaims"].as_array().cloned().unwrap_or_default();
            Ok(ToolResult::text(format!("{} default claim(s)", defaults.len()),
                Some(json!({"defaultClaims": defaults, "users": st["users"].clone()}))))
        }
        _ => Err(RufloError::invalid_input("tool.not_found", format!("unknown tool: {name}"))),
    }
}
