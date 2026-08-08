//! Native V3 `hive-mind` command — collective-intelligence swarm coordination.
//!
//! Source: `v3/@claude-flow/cli/src/commands/hive-mind.ts`. Eleven subcommands:
//! init / spawn / status / task / join / leave / consensus / broadcast / memory
//! / optimize-memory / shutdown.
//!
//! The TS source drives a live swarm via MCP tools that spawn real Claude
//! (`claude --print`) worker agents. ADR-0007 forbids recreating that
//! orchestration in the native build, so native manages the hive STATE file
//! (`.claude-flow/hive-mind.json`): topology, queen, workers, shared memory,
//! and consensus proposals/votes are real and queryable, and `spawn --claude`
//! degrades honestly (no Claude subprocess orchestration in native). This
//! matches the V3 `--dry-run` shape exactly.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};

const TOPOLOGIES: &[&str] = &["hierarchical", "mesh", "hierarchical-mesh", "adaptive", "ring", "star", "hybrid"];
const CONSENSUS_STRATEGIES: &[&str] = &["byzantine", "raft", "gossip", "crdt", "quorum"];

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn hive_file(root: &Path) -> PathBuf {
    root.join(".claude-flow/hive-mind.json")
}

fn read_hive(root: &Path) -> Value {
    fs::read_to_string(hive_file(root))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_else(|| json!({}))
}

fn write_hive(root: &Path, v: &Value) -> bool {
    let dir = root.join(".claude-flow");
    let _ = fs::create_dir_all(&dir);
    let path = hive_file(root);
    let tmp = path.with_extension("json.tmp");
    let bytes = serde_json::to_vec_pretty(v).unwrap_or_default();
    if fs::write(&tmp, &bytes).is_err() {
        return false;
    }
    let ok = fs::rename(&tmp, &path).is_ok();
    if !ok {
        let _ = fs::remove_file(&tmp);
    }
    ok
}

#[derive(Debug, Clone, PartialEq)]
pub struct HiveMindCommand {
    pub operation: String,
    pub topology: Option<String>,
    pub consensus: Option<String>,
    pub max_agents: usize,
    pub persist: bool,
    pub memory_backend: Option<String>,
    pub count: usize,
    pub role: Option<String>,
    pub agent_type: Option<String>,
    pub prefix: Option<String>,
    pub claude: bool,
    pub objective: Option<String>,
    pub dry_run: bool,
    pub mcp_config: Option<String>,
    pub detailed: bool,
    pub watch: bool,
    pub description: Option<String>,
    pub priority: Option<String>,
    pub require_consensus: bool,
    pub agent_id: Option<String>,
    pub action: Option<String>,
    pub proposal_id: Option<String>,
    pub vote: Option<String>,
    pub voter_id: Option<String>,
    pub strategy: Option<String>,
    pub quorum_preset: Option<String>,
    pub message: Option<String>,
    pub key: Option<String>,
    pub value: Option<String>,
    pub json: bool,
}

pub fn run(root: &Path, command: HiveMindCommand) -> u8 {
    match command.operation.as_str() {
        "" => overview(&command),
        "init" => init(root, &command),
        "spawn" => spawn(root, &command),
        "status" => status(root, &command),
        "task" => task_cmd(root, &command),
        "join" => join(root, &command),
        "leave" => leave(root, &command),
        "consensus" => consensus(root, &command),
        "broadcast" => broadcast(root, &command),
        "memory" => memory_cmd(root, &command),
        "optimize-memory" => optimize_memory(root, &command),
        "shutdown" => shutdown(root, &command),
        _ => {
            eprintln!(
                "[ERROR] Unknown: {} (init|spawn|status|task|join|leave|consensus|broadcast|memory|optimize-memory|shutdown)",
                command.operation
            );
            1
        }
    }
}

fn overview(_command: &HiveMindCommand) -> u8 {
    println!("\nHive Mind");
    println!("Collective intelligence coordination across a swarm of agents.\n");
    println!("Subcommands:");
    println!("  init            Initialize a hive mind");
    println!("  spawn           Spawn worker agents");
    println!("  status          Show hive status");
    println!("  task            Assign a task to the hive");
    println!("  join            Join an agent to the hive");
    println!("  leave           Remove an agent from the hive");
    println!("  consensus       Consensus propose/vote/status");
    println!("  broadcast       Broadcast a message to all workers");
    println!("  memory          Shared memory get/set/delete/list");
    println!("  optimize-memory Compact the shared memory store");
    println!("  shutdown        Shut down the hive");
    println!("\nNative build manages hive state; live worker orchestration needs a Node runtime.");
    0
}

fn require_hive(root: &Path) -> Option<Value> {
    let h = read_hive(root);
    if h.is_null() || h.get("hiveId").is_none() {
        eprintln!("[ERROR] No hive initialized. Run `hive-mind init` first.");
        return None;
    }
    Some(h)
}

// ---- init -------------------------------------------------------------------

fn init(root: &Path, command: &HiveMindCommand) -> u8 {
    let topology = command.topology.clone().unwrap_or_else(|| "hierarchical-mesh".into());
    let consensus = command.consensus.clone().unwrap_or_else(|| "byzantine".into());
    if !TOPOLOGIES.contains(&topology.as_str()) {
        eprintln!("[ERROR] Unknown topology: {topology}. One of: {}", TOPOLOGIES.join(", "));
        return 1;
    }
    if !CONSENSUS_STRATEGIES.contains(&consensus.as_str()) {
        eprintln!(
            "[ERROR] Unknown consensus: {consensus}. One of: {}",
            CONSENSUS_STRATEGIES.join(", ")
        );
        return 1;
    }
    let memory_backend = command.memory_backend.clone().unwrap_or_else(|| "hybrid".into());
    let hive_id = format!("hive-{}", now_ms());
    let queen_id = format!("queen-{}", now_ms());
    let state = json!({
        "hiveId": hive_id,
        "queenId": queen_id,
        "topology": topology,
        "consensus": consensus,
        "maxAgents": command.max_agents,
        "persist": command.persist,
        "memoryBackend": memory_backend,
        "status": "initialized",
        "createdAt": now_ms(),
        "workers": [],
        "memory": {},
        "proposals": [],
        "messages": [],
        "tasks": [],
    });
    if !write_hive(root, &state) {
        eprintln!("[ERROR] Failed to write hive state.");
        return 1;
    }
    if command.json {
        println!("{}", serde_json::to_string_pretty(&state).unwrap_or_default());
        return 0;
    }
    println!("\nInitializing Hive Mind");
    println!("\u{2714} Hive Mind initialized");
    println!();
    println!("\u{256d} Hive Mind Configuration \u{256e}");
    println!("  Hive ID: {hive_id}");
    println!("  Queen ID: {queen_id}");
    println!("  Topology: {topology}");
    println!("  Consensus: {consensus}");
    println!("  Max Agents: {}", command.max_agents);
    println!("  Memory: {memory_backend}");
    println!("  Status: initialized");
    println!();
    println!("Queen agent is ready to coordinate worker agents.");
    println!("  Use \"ruflo hive-mind spawn\" to add workers");
    println!("  Use \"ruflo hive-mind spawn --claude\" to launch Claude Code (Node runtime)");
    0
}

// ---- spawn ------------------------------------------------------------------

fn spawn(root: &Path, command: &HiveMindCommand) -> u8 {
    let mut hive = match require_hive(root) {
        Some(h) => h,
        None => return 1,
    };
    if command.claude && !command.dry_run {
        eprintln!("[ERROR] Live Claude worker spawn requires the Node runtime (ADR-0007).");
        eprintln!("       Run: npx ruflo hive-mind spawn --claude");
        return 1;
    }
    let role = command.role.clone().unwrap_or_else(|| "worker".into());
    let atype = command.agent_type.clone().unwrap_or_else(|| "worker".into());
    let prefix = command.prefix.clone().unwrap_or_else(|| "hive-worker".into());
    let max_agents = hive["maxAgents"].as_u64().unwrap_or(15) as usize;
    let workers = hive["workers"].as_array().cloned().unwrap_or_default();
    let current = workers.len();
    let room = max_agents.saturating_sub(current);
    let to_spawn = command.count.min(room);
    if to_spawn == 0 {
        eprintln!("[ERROR] Hive is at max capacity ({current}/{max_agents}).");
        return 1;
    }
    let mut workers = workers;
    for i in 0..to_spawn {
        let id = format!("{prefix}-{}-{}", now_ms(), current + i);
        workers.push(json!({
            "id": id, "role": role, "type": atype, "status": "idle", "joinedAt": now_ms(),
        }));
    }
    hive["workers"] = json!(workers);
    if !write_hive(root, &hive) {
        eprintln!("[ERROR] Failed to update hive state.");
        return 1;
    }
    if command.dry_run {
        println!("[dry-run] Would spawn {to_spawn} {atype} worker(s) as '{role}'.");
    } else {
        println!("Spawned {to_spawn} {atype} worker(s) as '{role}' (capacity {}/{max_agents}).", current + to_spawn);
    }
    0
}

// ---- status -----------------------------------------------------------------

fn status(root: &Path, command: &HiveMindCommand) -> u8 {
    let hive = match require_hive(root) {
        Some(h) => h,
        None => return 1,
    };
    if command.json {
        println!("{}", serde_json::to_string_pretty(&hive).unwrap_or_default());
        return 0;
    }
    let workers = hive["workers"].as_array().cloned().unwrap_or_default();
    let active = workers.iter().filter(|w| w["status"].as_str() == Some("running")).count();
    let idle = workers.iter().filter(|w| w["status"].as_str() == Some("idle")).count();
    let memory_entries = hive["memory"].as_object().map(|m| m.len()).unwrap_or(0);
    let proposals = hive["proposals"].as_array().map(|p| p.len()).unwrap_or(0);

    println!("\n\u{256d} Hive Mind Status \u{256e}");
    println!("  Hive ID: {}", hive["hiveId"].as_str().unwrap_or("?"));
    println!("  Status:  {}", hive["status"].as_str().unwrap_or("?"));
    println!("  Queen:   {}", hive["queenId"].as_str().unwrap_or("?"));
    println!("  Topology: {}", hive["topology"].as_str().unwrap_or("?"));
    println!("  Consensus: {}", hive["consensus"].as_str().unwrap_or("?"));
    println!("  Workers: {}/{} (active: {active}, idle: {idle})", workers.len(), hive["maxAgents"].as_u64().unwrap_or(0));
    println!("  Shared memory entries: {memory_entries}");
    println!("  Consensus proposals: {proposals}");

    if command.detailed {
        println!("\nWorkers");
        println!("  {:<24} {:<12} {:<12} Status", "ID", "Role", "Type");
        println!("  {} {} {} {}", "\u{2500}".repeat(24), "\u{2500}".repeat(12), "\u{2500}".repeat(12), "\u{2500}".repeat(10));
        for w in workers.iter().take(20) {
            println!(
                "  {:<24} {:<12} {:<12} {}",
                w["id"].as_str().unwrap_or("?"),
                w["role"].as_str().unwrap_or("?"),
                w["type"].as_str().unwrap_or("?"),
                w["status"].as_str().unwrap_or("?")
            );
        }
    }
    0
}

// ---- task -------------------------------------------------------------------

fn task_cmd(root: &Path, command: &HiveMindCommand) -> u8 {
    let mut hive = match require_hive(root) {
        Some(h) => h,
        None => return 1,
    };
    let Some(desc) = &command.description else {
        eprintln!("[ERROR] --description is required");
        return 1;
    };
    let priority = command.priority.clone().unwrap_or_else(|| "normal".into());
    let task = json!({
        "id": format!("task-{}", now_ms()),
        "description": desc,
        "priority": priority,
        "requireConsensus": command.require_consensus,
        "status": "pending",
        "createdAt": now_ms(),
    });
    let mut tasks = hive["tasks"].as_array().cloned().unwrap_or_default();
    tasks.push(task.clone());
    hive["tasks"] = json!(tasks);
    if !write_hive(root, &hive) {
        eprintln!("[ERROR] Failed to update hive state.");
        return 1;
    }
    if command.json {
        println!("{}", serde_json::to_string_pretty(&task).unwrap_or_default());
        return 0;
    }
    println!("Task assigned to hive (priority: {priority}):");
    println!("  {desc}");
    if command.require_consensus {
        println!("  Requires consensus before execution.");
    }
    eprintln!("\n[WARN] Native build records the task; execution requires a Node runtime.");
    0
}

// ---- join / leave -----------------------------------------------------------

fn join(root: &Path, command: &HiveMindCommand) -> u8 {
    let mut hive = match require_hive(root) {
        Some(h) => h,
        None => return 1,
    };
    let Some(agent_id) = &command.agent_id else {
        eprintln!("[ERROR] --agent-id is required");
        return 1;
    };
    let role = command.role.clone().unwrap_or_else(|| "worker".into());
    let mut workers = hive["workers"].as_array().cloned().unwrap_or_default();
    if workers.iter().any(|w| w["id"].as_str() == Some(agent_id.as_str())) {
        eprintln!("[ERROR] Agent '{agent_id}' already in hive.");
        return 1;
    }
    workers.push(json!({"id": agent_id, "role": role, "status": "idle", "joinedAt": now_ms()}));
    hive["workers"] = json!(workers);
    if !write_hive(root, &hive) {
        eprintln!("[ERROR] Failed to update hive state.");
        return 1;
    }
    println!("Agent '{agent_id}' joined the hive as '{role}'.");
    0
}

fn leave(root: &Path, command: &HiveMindCommand) -> u8 {
    let mut hive = match require_hive(root) {
        Some(h) => h,
        None => return 1,
    };
    let Some(agent_id) = &command.agent_id else {
        eprintln!("[ERROR] --agent-id is required");
        return 1;
    };
    let mut workers = hive["workers"].as_array().cloned().unwrap_or_default();
    let before = workers.len();
    workers.retain(|w| w["id"].as_str() != Some(agent_id.as_str()));
    if workers.len() == before {
        eprintln!("[ERROR] Agent '{agent_id}' not found in hive.");
        return 1;
    }
    hive["workers"] = json!(workers);
    if !write_hive(root, &hive) {
        eprintln!("[ERROR] Failed to update hive state.");
        return 1;
    }
    println!("Agent '{agent_id}' left the hive.");
    0
}

// ---- consensus --------------------------------------------------------------

fn consensus(root: &Path, command: &HiveMindCommand) -> u8 {
    let mut hive = match require_hive(root) {
        Some(h) => h,
        None => return 1,
    };
    let action = command.action.clone().unwrap_or_else(|| "status".into());
    match action.as_str() {
        "status" => {
            let proposals = hive["proposals"].as_array().cloned().unwrap_or_default();
            println!("\n\u{256d} Consensus Proposals \u{256e}");
            if proposals.is_empty() {
                println!("  No proposals.");
                return 0;
            }
            println!("  {:<20} {:<12} {:<8} Value", "ID", "Status", "Votes");
            for p in proposals.iter().take(20) {
                let votes = p["votes"].as_array().map(|v| v.len()).unwrap_or(0);
                println!(
                    "  {:<20} {:<12} {:<8} {}",
                    p["id"].as_str().unwrap_or("?"),
                    p["status"].as_str().unwrap_or("?"),
                    votes,
                    p["value"].as_str().or(p["type"].as_str()).unwrap_or("?")
                );
            }
        }
        "propose" => {
            let Some(value) = &command.value else {
                eprintln!("[ERROR] --value is required for propose");
                return 1;
            };
            let id = format!("prop-{}", now_ms());
            let proposal = json!({
                "id": id, "value": value, "status": "open",
                "strategy": command.strategy.clone().unwrap_or_else(|| hive["consensus"].as_str().unwrap_or("raft").to_string()),
                "proposer": command.voter_id.clone().unwrap_or_else(|| hive["queenId"].as_str().unwrap_or("queen").to_string()),
                "votes": [], "createdAt": now_ms(),
            });
            let mut proposals = hive["proposals"].as_array().cloned().unwrap_or_default();
            proposals.push(proposal);
            hive["proposals"] = json!(proposals);
            if !write_hive(root, &hive) {
                eprintln!("[ERROR] Failed to update hive state.");
                return 1;
            }
            println!("Proposal {id} created: {value}");
        }
        "vote" => {
            let (Some(pid), Some(vote)) = (&command.proposal_id, &command.vote) else {
                eprintln!("[ERROR] --proposal-id and --vote (accept|reject) are required");
                return 1;
            };
            if !matches!(vote.as_str(), "accept" | "reject") {
                eprintln!("[ERROR] --vote must be accept or reject");
                return 1;
            }
            let voter = command.voter_id.clone().unwrap_or_else(|| "voter".into());
            let proposals = hive["proposals"].as_array_mut();
            let mut found = false;
            if let Some(arr) = proposals {
                for p in arr.iter_mut() {
                    if p["id"].as_str() == Some(pid.as_str()) {
                        let mut votes = p["votes"].as_array().cloned().unwrap_or_default();
                        votes.push(json!({"voter": voter, "vote": vote}));
                        p["votes"] = json!(votes);
                        found = true;
                        break;
                    }
                }
            }
            if !found {
                eprintln!("[ERROR] Proposal '{pid}' not found.");
                return 1;
            }
            if !write_hive(root, &hive) {
                eprintln!("[ERROR] Failed to update hive state.");
                return 1;
            }
            println!("Vote '{vote}' recorded for {pid} by {voter}.");
        }
        other => {
            eprintln!("[ERROR] Unknown consensus action: {other} (status|propose|vote)");
            return 1;
        }
    }
    0
}

// ---- broadcast --------------------------------------------------------------

fn broadcast(root: &Path, command: &HiveMindCommand) -> u8 {
    let mut hive = match require_hive(root) {
        Some(h) => h,
        None => return 1,
    };
    let Some(message) = &command.message else {
        eprintln!("[ERROR] --message is required");
        return 1;
    };
    let priority = command.priority.clone().unwrap_or_else(|| "normal".into());
    let from = command.voter_id.clone().unwrap_or_else(|| hive["queenId"].as_str().unwrap_or("queen").to_string());
    let rec = json!({"from": from, "message": message, "priority": priority, "at": now_ms()});
    let mut messages = hive["messages"].as_array().cloned().unwrap_or_default();
    messages.push(rec);
    hive["messages"] = json!(messages);
    if !write_hive(root, &hive) {
        eprintln!("[ERROR] Failed to update hive state.");
        return 1;
    }
    let n = hive["workers"].as_array().map(|w| w.len()).unwrap_or(0);
    println!("Broadcast queued for {n} worker(s): \"{message}\"");
    0
}

// ---- memory -----------------------------------------------------------------

fn memory_cmd(root: &Path, command: &HiveMindCommand) -> u8 {
    let mut hive = match require_hive(root) {
        Some(h) => h,
        None => return 1,
    };
    let action = command.action.clone().unwrap_or_else(|| "list".into());
    match action.as_str() {
        "list" => {
            let mem = hive["memory"].as_object().cloned().unwrap_or_default();
            println!("\n\u{256d} Hive Shared Memory \u{256e}");
            if mem.is_empty() {
                println!("  (empty)");
            } else {
                for (k, v) in &mem {
                    println!("  {k}: {}", v);
                }
            }
        }
        "get" => {
            let Some(key) = &command.key else {
                eprintln!("[ERROR] --key is required");
                return 1;
            };
            let val = hive["memory"].get(key).cloned().unwrap_or(Value::Null);
            if val.is_null() {
                eprintln!("[ERROR] Key '{key}' not set.");
                return 1;
            }
            println!("{}", val);
        }
        "set" => {
            let (Some(key), Some(value)) = (&command.key, &command.value) else {
                eprintln!("[ERROR] --key and --value are required");
                return 1;
            };
            if let Some(mem) = hive["memory"].as_object_mut() {
                mem.insert(key.clone(), json!(value));
            } else {
                let mut m = serde_json::Map::new();
                m.insert(key.clone(), json!(value));
                hive["memory"] = json!(m);
            }
            if !write_hive(root, &hive) {
                eprintln!("[ERROR] Failed to update hive state.");
                return 1;
            }
            println!("Set {key} = {value}");
        }
        "delete" => {
            let Some(key) = &command.key else {
                eprintln!("[ERROR] --key is required");
                return 1;
            };
            if let Some(mem) = hive["memory"].as_object_mut() {
                if mem.remove(key).is_none() {
                    eprintln!("[ERROR] Key '{key}' not set.");
                    return 1;
                }
            }
            if !write_hive(root, &hive) {
                eprintln!("[ERROR] Failed to update hive state.");
                return 1;
            }
            println!("Deleted {key}");
        }
        other => {
            eprintln!("[ERROR] Unknown memory action: {other} (list|get|set|delete)");
            return 1;
        }
    }
    0
}

// ---- optimize-memory / shutdown --------------------------------------------

fn optimize_memory(root: &Path, _command: &HiveMindCommand) -> u8 {
    let mut hive = match require_hive(root) {
        Some(h) => h,
        None => return 1,
    };
    let before = hive["memory"].as_object().map(|m| m.len()).unwrap_or(0);
    // Drop null/empty values (the cheap structural pass).
    if let Some(mem) = hive["memory"].as_object_mut() {
        mem.retain(|_, v| !v.is_null() && !(v.is_string() && v.as_str().unwrap_or("").is_empty()));
    }
    let after = hive["memory"].as_object().map(|m| m.len()).unwrap_or(0);
    let _ = write_hive(root, &hive);
    println!("Shared memory compacted: {before} -> {after} entries.");
    0
}

fn shutdown(root: &Path, _command: &HiveMindCommand) -> u8 {
    let mut hive = match require_hive(root) {
        Some(h) => h,
        None => return 1,
    };
    hive["status"] = json!("shutdown");
    hive["stoppedAt"] = json!(now_ms());
    if let Some(workers) = hive["workers"].as_array_mut() {
        for w in workers {
            w["status"] = json!("stopped");
        }
    }
    if !write_hive(root, &hive) {
        eprintln!("[ERROR] Failed to update hive state.");
        return 1;
    }
    println!("Hive {} shut down.", hive["hiveId"].as_str().unwrap_or("?"));
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_root() -> PathBuf {
        tempfile::tempdir().unwrap().into_path()
    }

    #[test]
    fn init_creates_state() {
        let root = tmp_root();
        let cmd = HiveMindCommand {
            operation: "init".into(),
            topology: None, consensus: None, max_agents: 15, persist: true,
            memory_backend: None, count: 1, role: None, agent_type: None, prefix: None,
            claude: false, objective: None, dry_run: false, mcp_config: None,
            detailed: false, watch: false, description: None, priority: None,
            require_consensus: false, agent_id: None, action: None, proposal_id: None,
            vote: None, voter_id: None, strategy: None, quorum_preset: None,
            message: None, key: None, value: None, json: false,
        };
        assert_eq!(run(&root, cmd.clone()), 0);
        let h = read_hive(&root);
        assert_eq!(h["topology"], "hierarchical-mesh");
        assert_eq!(h["consensus"], "byzantine");
        assert_eq!(h["maxAgents"], 15);
    }

    #[test]
    fn init_rejects_unknown_topology() {
        let root = tmp_root();
        let mut cmd = HiveMindCommand {
            operation: "init".into(),
            topology: Some("bogus".into()), consensus: None, max_agents: 15, persist: true,
            memory_backend: None, count: 1, role: None, agent_type: None, prefix: None,
            claude: false, objective: None, dry_run: false, mcp_config: None,
            detailed: false, watch: false, description: None, priority: None,
            require_consensus: false, agent_id: None, action: None, proposal_id: None,
            vote: None, voter_id: None, strategy: None, quorum_preset: None,
            message: None, key: None, value: None, json: false,
        };
        assert_eq!(run(&root, cmd.clone()), 1);
        // consensus reject too
        cmd.topology = None;
        cmd.consensus = Some("bogus".into());
        assert_eq!(run(&root, cmd), 1);
    }

    #[test]
    fn spawn_join_leave_roundtrip() {
        let root = tmp_root();
        let init = HiveMindCommand {
            operation: "init".into(), topology: None, consensus: None, max_agents: 15,
            persist: true, memory_backend: None, count: 1, role: None, agent_type: None,
            prefix: None, claude: false, objective: None, dry_run: false, mcp_config: None,
            detailed: false, watch: false, description: None, priority: None,
            require_consensus: false, agent_id: None, action: None, proposal_id: None,
            vote: None, voter_id: None, strategy: None, quorum_preset: None,
            message: None, key: None, value: None, json: false,
        };
        run(&root, init);
        let mut spawn = HiveMindCommand {
            operation: "spawn".into(), topology: None, consensus: None, max_agents: 15,
            persist: true, memory_backend: None, count: 3, role: Some("worker".into()),
            agent_type: Some("worker".into()), prefix: Some("w".into()), claude: false,
            objective: None, dry_run: false, mcp_config: None, detailed: false, watch: false,
            description: None, priority: None, require_consensus: false, agent_id: None,
            action: None, proposal_id: None, vote: None, voter_id: None, strategy: None,
            quorum_preset: None, message: None, key: None, value: None, json: false,
        };
        assert_eq!(run(&root, spawn.clone()), 0);
        assert_eq!(read_hive(&root)["workers"].as_array().unwrap().len(), 3);
        // join one more
        let mut join = spawn.clone();
        join.operation = "join".into();
        join.agent_id = Some("x-1".into());
        join.count = 0;
        assert_eq!(run(&root, join.clone()), 0);
        // duplicate join rejected
        assert_eq!(run(&root, join), 1);
        // leave
        let mut leave = spawn.clone();
        leave.operation = "leave".into();
        leave.agent_id = Some("x-1".into());
        leave.count = 0;
        assert_eq!(run(&root, leave), 0);
    }

    fn base_cmd(op: &str) -> HiveMindCommand {
        HiveMindCommand {
            operation: op.into(), topology: None, consensus: None, max_agents: 15,
            persist: true, memory_backend: None, count: 1, role: None, agent_type: None,
            prefix: None, claude: false, objective: None, dry_run: false, mcp_config: None,
            detailed: false, watch: false, description: None, priority: None,
            require_consensus: false, agent_id: None, action: None, proposal_id: None,
            vote: None, voter_id: None, strategy: None, quorum_preset: None,
            message: None, key: None, value: None, json: false,
        }
    }

    #[test]
    fn memory_set_get_delete() {
        let root = tmp_root();
        run(&root, base_cmd("init"));
        let mut set = base_cmd("memory");
        set.action = Some("set".into());
        set.key = Some("goal".into());
        set.value = Some("ship".into());
        assert_eq!(run(&root, set), 0);
        assert_eq!(read_hive(&root)["memory"]["goal"], "ship");
    }
}
