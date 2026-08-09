//! Services layer — ports the 48 TS `src/services/` files into native Rust.
//!
//! Each service manages state files under `.claude-flow/` and provides
//! read/write/query operations. Compute-heavy services (ONNX training, WASM,
//! LLM calls) are state-managed natively but defer their execution step —
//! matching the V3 `--dry-run` contract. This makes ruflo-rust able to MANAGE
//! every service's persisted state, query it, and coordinate with the Node
//! runtime for the compute leg.
//!
//! Groups:
//! - Worker management (bounded-pool, container-pool, queue, daemon, headless)
//! - Harness/metaharness (15 harness-* files, fable, evolve, weight-eft)
//! - Flywheel (proposer, receipt, transaction, runtime, generations)
//! - Distill/training (oracle, tuning, native, ruvector)
//! - Coordination (git-workspace, swarm-memory, pheromone, learned-routing)
//! - Infrastructure (autostart, dedup, backup, distillation, config, lease,
//!   checkpoint, supervisor, policy)
//! - Integration (agentic-bridge, registry, budget, claims)

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn root() -> PathBuf {
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

fn state_dir() -> PathBuf {
    root().join(".claude-flow/services")
}

fn state_path(name: &str) -> PathBuf {
    state_dir().join(format!("{name}.json"))
}

fn read_state(name: &str) -> Value {
    fs::read_to_string(state_path(name))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_else(|| json!({}))
}

fn write_state(name: &str, v: &Value) -> bool {
    let _ = fs::create_dir_all(state_dir());
    let path = state_path(name);
    let tmp = path.with_extension("json.tmp");
    let bytes = serde_json::to_vec_pretty(v).unwrap_or_default();
    if fs::write(&tmp, &bytes).is_err() {
        return false;
    }
    fs::rename(&tmp, &path).is_ok()
}

/// Stale-lock threshold. A lock older than this is assumed to belong to a
/// crashed process and is taken over. Matches daemon.rs.
const LOCK_STALE_MS: u64 = 10_000;

#[cfg(unix)]
fn open_create_new_private(path: &Path) -> std::io::Result<std::fs::File> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let mut f = std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .mode(0o600)
        .open(path)?;
    let _ = f.write_all(std::process::id().to_string().as_bytes());
    Ok(f)
}

#[cfg(not(unix))]
fn open_create_new_private(path: &Path) -> std::io::Result<std::fs::File> {
    use std::io::Write;
    let mut f = std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(path)?;
    let _ = f.write_all(std::process::id().to_string().as_bytes());
    Ok(f)
}

/// Unique lockfile per state file. Sits beside the JSON state so the lock is
/// collocated with the data it protects and cleaned up with it.
fn lock_path(name: &str) -> PathBuf {
    state_dir().join(format!("{name}.lock"))
}

/// O_EXCL lockfile guard. Acquired before the read in a read-modify-write
/// cycle and released (Drop) after the write. Stale locks (>LOCK_STALE_MS
/// old) are taken over, matching daemon.rs LockGuard semantics. Unix-gated;
/// on non-Unix targets the guard is a no-op stub (returns Ok immediately).
#[cfg(unix)]
struct LockGuard(PathBuf);

#[cfg(unix)]
impl LockGuard {
    fn acquire(name: &str) -> Option<Self> {
        let _ = fs::create_dir_all(state_dir());
        let path = lock_path(name);
        let deadline = now_ms() + 2000;
        loop {
            match open_create_new_private(&path) {
                Ok(_) => return Some(LockGuard(path)),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                    // Stale lock from a crashed process — take over.
                    if let Ok(meta) = fs::metadata(&path) {
                        if let Ok(mtime) = meta.modified() {
                            let age = SystemTime::now()
                                .duration_since(mtime)
                                .map(|d| d.as_millis() as u64)
                                .unwrap_or(0);
                            if age > LOCK_STALE_MS {
                                let _ = fs::remove_file(&path);
                                continue;
                            }
                        }
                    }
                    if now_ms() > deadline {
                        return None;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(50));
                }
                Err(_) => return None,
            }
        }
    }
}

#[cfg(unix)]
impl Drop for LockGuard {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

/// No-op stub for non-Unix targets. File-based advisory locks are a Unix
/// concept (O_EXCL, mode bits); on Windows/other the lock is a struct with
/// no state and acquire is always Ok. Concurrency safety on those targets
/// relies on the caller not running concurrent ruflo processes against the
/// same state dir — same caveat as daemon.rs.
#[cfg(not(unix))]
struct LockGuard;

#[cfg(not(unix))]
impl LockGuard {
    fn acquire(_name: &str) -> Option<Self> {
        Some(LockGuard)
    }
}

/// Transactional read-modify-write helper. Acquires a per-state-file lock,
/// reads the current state, applies `f`, writes the result, and releases the
/// lock on return. Returns true if the write succeeded. The lock is held for
/// the entire cycle, preventing the lost-update race two concurrent writers
/// would otherwise hit (both read same state, both modify, second write wins).
#[allow(dead_code)]
fn locked_write<F>(name: &str, f: F) -> bool
where
    F: FnOnce(&mut Value),
{
    let _guard = match LockGuard::acquire(name) {
        Some(g) => g,
        None => return false,
    };
    let mut state = read_state(name);
    f(&mut state);
    write_state(name, &state)
}

fn ensure_arr<'a>(v: &'a mut Value, key: &str) -> &'a mut Vec<Value> {
    if v[key].is_null() {
        v[key] = json!([]);
    }
    v[key].as_array_mut().expect("array")
}

// ============================================================================ //
// WORKER MANAGEMENT
// ============================================================================ //

/// Bounded worker pool — concurrency cap with active/in-flight tracking.
/// Ports services/bounded-worker-pool.ts (108 lines).
pub mod bounded_pool {
    use super::*;

    pub fn acquire(pool_id: &str, max_concurrent: usize) -> Result<Value, String> {
        // Hold the lock for the full read-check-modify-write cycle so two
        // concurrent acquires can't both see capacity and both succeed.
        let _guard = LockGuard::acquire("bounded-pool")
            .ok_or_else(|| "bounded-pool lock contention".to_string())?;
        let mut state = read_state("bounded-pool");
        let key = pool_id;
        let active = state[key]["active"].as_array().cloned().unwrap_or_default();
        if active.len() >= max_concurrent {
            return Err(format!("pool `{pool_id}` at capacity ({max_concurrent})"));
        }
        let slot = json!({"id": format!("slot-{}", now_ms()), "acquiredAt": now_ms()});
        let mut arr = active;
        arr.push(slot.clone());
        state[key] = json!({"active": arr, "max": max_concurrent});
        write_state("bounded-pool", &state);
        Ok(slot)
    }

    pub fn release(pool_id: &str, slot_id: &str) -> bool {
        // Lock around read-modify-write so a concurrent acquire can't observe
        // a slot we're about to remove (and vice versa).
        let _guard = match LockGuard::acquire("bounded-pool") {
            Some(g) => g,
            None => return false,
        };
        let mut state = read_state("bounded-pool");
        if let Some(arr) = state[pool_id]["active"].as_array_mut() {
            let before = arr.len();
            arr.retain(|s| s["id"].as_str() != Some(slot_id));
            let changed = arr.len() < before;
            drop(arr);
            if changed {
                write_state("bounded-pool", &state);
            }
            return changed;
        }
        false
    }

    pub fn status(pool_id: &str) -> Value {
        let state = read_state("bounded-pool");
        state[pool_id].clone()
    }
}

/// Worker queue — persistent FIFO task queue for background workers.
/// Ports services/worker-queue.ts (702 lines).
pub mod worker_queue {
    use super::*;

    pub fn enqueue(task: Value) -> Value {
        // Lock so two concurrent enqueues produce two distinct queue entries
        // rather than one lost update.
        let _guard = match LockGuard::acquire("worker-queue") {
            Some(g) => g,
            None => return json!({}),
        };
        let mut state = read_state("worker-queue");
        let queue = ensure_arr(&mut state, "queue");
        let entry = json!({
            "id": format!("wq-{}", now_ms()),
            "task": task,
            "status": "queued",
            "enqueuedAt": now_ms(),
        });
        queue.push(entry.clone());
        write_state("worker-queue", &state);
        entry
    }

    pub fn dequeue() -> Option<Value> {
        // Lock so two concurrent dequeuers can't both pop the same head.
        let _guard = LockGuard::acquire("worker-queue")?;
        let mut state = read_state("worker-queue");
        let queue = ensure_arr(&mut state, "queue");
        if queue.is_empty() {
            return None;
        }
        let task = queue.remove(0);
        write_state("worker-queue", &state);
        Some(task)
    }

    pub fn list() -> Vec<Value> {
        read_state("worker-queue")["queue"]
            .as_array()
            .cloned()
            .unwrap_or_default()
    }

    pub fn length() -> usize {
        list().len()
    }
}

/// Worker daemon state — tracks daemon worker lifecycle.
/// Ports services/worker-daemon.ts (2098 lines) — state management only.
pub mod worker_daemon {
    use super::*;

    pub fn register_worker(worker_type: &str) -> Value {
        let mut state = read_state("worker-daemon");
        let workers = ensure_arr(&mut state, "workers");
        let entry = json!({
            "type": worker_type,
            "id": format!("daemon-{worker_type}-{}", now_ms()),
            "status": "registered",
            "registeredAt": now_ms(),
        });
        workers.push(entry.clone());
        write_state("worker-daemon", &state);
        entry
    }

    pub fn list_workers() -> Vec<Value> {
        read_state("worker-daemon")["workers"]
            .as_array()
            .cloned()
            .unwrap_or_default()
    }

    pub fn unregister(worker_id: &str) -> bool {
        let mut state = read_state("worker-daemon");
        if let Some(workers) = state["workers"].as_array_mut() {
            let before = workers.len();
            workers.retain(|w| w["id"].as_str() != Some(worker_id));
            if workers.len() < before {
                write_state("worker-daemon", &state);
                return true;
            }
        }
        false
    }
}

/// Headless worker executor — tracks headless (sandboxed) worker state.
/// Ports services/headless-worker-executor.ts (1637 lines) — state only.
pub mod headless {
    use super::*;

    pub fn launch(worker_type: &str, sandbox: &str) -> Value {
        let entry = json!({
            "id": format!("headless-{worker_type}-{}", now_ms()),
            "type": worker_type,
            "sandbox": sandbox,
            "status": "launched",
            "launchedAt": now_ms(),
        });
        let mut state = read_state("headless-workers");
        ensure_arr(&mut state, "workers").push(entry.clone());
        write_state("headless-workers", &state);
        entry
    }

    pub fn list() -> Vec<Value> {
        read_state("headless-workers")["workers"]
            .as_array()
            .cloned()
            .unwrap_or_default()
    }
}

/// Container worker pool — containerized worker management.
/// Ports services/container-worker-pool.ts (790 lines) — state only.
pub mod container_pool {
    use super::*;

    pub fn create(image: &str, cmd: &str) -> Value {
        let entry = json!({
            "id": format!("container-{}", now_ms()),
            "image": image,
            "command": cmd,
            "status": "created",
            "createdAt": now_ms(),
        });
        let mut state = read_state("container-pool");
        ensure_arr(&mut state, "containers").push(entry.clone());
        write_state("container-pool", &state);
        entry
    }

    pub fn list() -> Vec<Value> {
        read_state("container-pool")["containers"]
            .as_array()
            .cloned()
            .unwrap_or_default()
    }
}

// ============================================================================ //
// COORDINATION
// ============================================================================ //

/// Git workspace identity — worktree creation for swarm isolation.
/// Ports services/git-workspace-identity.ts (118 lines).
pub mod git_workspace {
    use super::*;
    use std::process::Command;

    pub fn create_worktree(root: &Path, branch: &str) -> Result<PathBuf, String> {
        let wt_path = root.join(format!(".claude-flow/worktrees/{branch}"));
        let output = Command::new("git")
            .args(["worktree", "add", "-b", branch, wt_path.to_str().unwrap_or(""), "HEAD"])
            .current_dir(root)
            .output()
            .map_err(|e| format!("git worktree: {e}"))?;
        if !output.status.success() {
            // Worktree may already exist; check if path is valid.
            if !wt_path.is_dir() {
                return Err(String::from_utf8_lossy(&output.stderr).into_owned());
            }
        }
        // Record in state.
        let mut state = read_state("git-worktrees");
        let entries = ensure_arr(&mut state, "worktrees");
        entries.push(json!({"branch": branch, "path": wt_path.display().to_string(), "createdAt": now_ms()}));
        write_state("git-worktrees", &state);
        Ok(wt_path)
    }

    pub fn remove_worktree(root: &Path, branch: &str) -> Result<(), String> {
        let _ = Command::new("git")
            .args(["worktree", "remove", "--force"])
            .current_dir(root)
            .output();
        let mut state = read_state("git-worktrees");
        if let Some(entries) = state["worktrees"].as_array_mut() {
            entries.retain(|w| w["branch"].as_str() != Some(branch));
        }
        write_state("git-worktrees", &state);
        Ok(())
    }

    pub fn list() -> Vec<Value> {
        read_state("git-worktrees")["worktrees"]
            .as_array()
            .cloned()
            .unwrap_or_default()
    }
}

/// Pheromone adaptive state — APSC (Adaptive Pheromone Swarm Coordinator).
/// Ports services/pheromone-adaptive.ts (293 lines).
pub mod pheromone {
    use super::*;

    pub fn get_state() -> Value {
        read_state("pheromone-state")
    }

    pub fn record(agent_id: &str, role: &str, success: f64, latency_norm: f64, consensus: f64) {
        let mut state = read_state("pheromone-state");
        if state["version"].is_null() {
            state = json!({"version": "ruflo.apsc-state/v1", "threshold": 0.4, "agents": {}});
        }
        if state["agents"].is_null() { state["agents"] = json!({}); }
        let agents = state["agents"].as_object_mut().unwrap();
        agents.insert(
            agent_id.into(),
            json!({
                "role": role,
                "emaSuccess": success,
                "emaLatency": latency_norm,
                "emaConsensus": consensus,
                "updatedAt": now_ms(),
            }),
        );
        write_state("pheromone-state", &state);
    }

    pub fn eligible() -> Vec<String> {
        let state = get_state();
        let threshold = state["threshold"].as_f64().unwrap_or(0.4);
        state["agents"]
            .as_object()
            .map(|m| {
                m.iter()
                    .filter(|(_, v)| {
                        let score = v["emaSuccess"].as_f64().unwrap_or(1.0)
                            * (1.0 - v["emaLatency"].as_f64().unwrap_or(0.0).abs())
                            * v["emaConsensus"].as_f64().unwrap_or(1.0);
                        score >= threshold
                    })
                    .map(|(k, _)| k.clone())
                    .collect()
            })
            .unwrap_or_default()
    }
}

/// Swarm memory branches — branch-aware memory operations.
/// Ports services/swarm-memory-branches.ts (278 lines).
pub mod swarm_branches {
    use super::*;

    pub fn create_branch(name: &str) -> Value {
        let mut state = read_state("swarm-branches");
        state["branches"] = json!({}); let branches = state["branches"].as_object_mut().unwrap();
        branches.insert(name.into(), json!({"createdAt": now_ms(), "entries": {}}));
        write_state("swarm-branches", &state);
        json!({"branch": name, "created": true})
    }

    pub fn list_branches() -> Vec<String> {
        read_state("swarm-branches")["branches"]
            .as_object()
            .map(|m| m.keys().cloned().collect())
            .unwrap_or_default()
    }
}

/// Learned routing — extends hooks route with persistent learning.
/// Ports services/learned-routing.ts (123 lines).
pub mod learned_routing {
    use super::*;

    pub fn record(task: &str, agent: &str, success: bool) {
        let mut state = read_state("learned-routing");
        let key = format!("routes.{}", task.to_lowercase().chars().take(30).collect::<String>());
        let entry = json!({"agent": agent, "success": success, "at": now_ms()});
        state["routes"] = json!({}); let routes = state["routes"].as_object_mut().unwrap();
        let history = routes
            .get(&task.to_lowercase())
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        let mut hist = history;
        hist.push(entry);
        routes.insert(task.to_lowercase(), json!(hist));
        write_state("learned-routing", &state);
    }

    pub fn best_agent(task: &str) -> Option<String> {
        let state = read_state("learned-routing");
        let history = state["routes"][task.to_lowercase()].as_array()?;
        let mut counts: HashMap<String, (usize, usize)> = HashMap::new();
        for h in history {
            let agent = h["agent"].as_str()?;
            let success = h["success"].as_bool().unwrap_or(false);
            let entry = counts.entry(agent.into()).or_insert((0, 0));
            entry.0 += 1;
            if success {
                entry.1 += 1;
            }
        }
        counts
            .into_iter()
            .max_by_key(|(_, (total, wins))| (*wins * 100) / (*total).max(1))
            .map(|(agent, _)| agent)
    }
}

// ============================================================================ //
// INFRASTRUCTURE
// ============================================================================ //

/// Daemon autostart — systemd/launchd/crontab config generation.
/// Ports services/daemon-autostart.ts (143 lines).
pub mod autostart {
    use super::*;

    pub fn install_cron() -> String {
        let cron = "@reboot $(which ruflo) daemon start --background\n";
        write_state("autostart", &json!({"method": "cron", "config": cron, "installedAt": now_ms()}));
        cron.into()
    }

    pub fn install_systemd() -> String {
        let unit = "[Unit]\nDescription=Ruflo Daemon\nAfter=network.target\n\n[Service]\nExecStart=$(which ruflo) daemon start --foreground\nRestart=always\n\n[Install]\nWantedBy=default.target\n";
        write_state("autostart", &json!({"method": "systemd", "config": unit, "installedAt": now_ms()}));
        unit.into()
    }

    pub fn install_launchd() -> String {
        let plist = r#"<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0"><dict>
  <key>Label</key><string>io.ruflo.daemon</string>
  <key>ProgramArguments</key><array><string>ruflo</string><string>daemon</string><string>start</string><string>--foreground</string></array>
  <key>RunAtLoad</key><true/>
</dict></plist>"#;
        write_state("autostart", &json!({"method": "launchd", "config": plist, "installedAt": now_ms()}));
        plist.into()
    }

    pub fn uninstall() -> bool {
        let path = state_path("autostart");
        if path.exists() {
            let _ = fs::remove_file(&path);
            true
        } else {
            false
        }
    }
}

/// AI job dedup — prevents duplicate AI worker launches.
/// Ports services/ai-job-dedup.ts (157 lines).
pub mod dedup {
    use super::*;

    pub fn check(job_id: &str) -> bool {
        let state = read_state("ai-job-dedup");
        state["jobs"][job_id].as_u64().is_some()
    }

    pub fn mark(job_id: &str) {
        let mut state = read_state("ai-job-dedup");
        if state["jobs"].is_null() {
            state["jobs"] = json!({});
        }
        state["jobs"][job_id] = json!(now_ms());
        write_state("ai-job-dedup", &state);
    }

    pub fn expire(max_age_ms: u64) {
        let now = now_ms();
        let mut state = read_state("ai-job-dedup");
        if let Some(jobs) = state["jobs"].as_object_mut() {
            jobs.retain(|_, v| {
                let at = v.as_u64().unwrap_or(0);
                now.saturating_sub(at) < max_age_ms
            });
        }
        write_state("ai-job-dedup", &state);
    }
}

/// Memory backup — backup/restore of memory store.
/// Ports services/memory-backup.ts (248 lines).
pub mod backup {
    use super::*;

    pub fn create(src: &Path) -> Result<PathBuf, String> {
        let backup_dir = root().join(".claude-flow/backups");
        fs::create_dir_all(&backup_dir).map_err(|e| e.to_string())?;
        let backup_path = backup_dir.join(format!("memory-{}.db", now_ms()));
        fs::copy(src, &backup_path).map_err(|e| e.to_string())?;
        let mut state = read_state("memory-backups");
        ensure_arr(&mut state, "backups").push(json!({
            "path": backup_path.display().to_string(),
            "source": src.display().to_string(),
            "createdAt": now_ms(),
        }));
        write_state("memory-backups", &state);
        Ok(backup_path)
    }

    pub fn list() -> Vec<Value> {
        read_state("memory-backups")["backups"]
            .as_array()
            .cloned()
            .unwrap_or_default()
    }
}

/// Memory distillation — distill memory entries into structured intelligence.
/// Ports services/memory-distillation.ts (430 lines) — state management only.
pub mod distillation {
    use super::*;

    pub fn record_episode(source: &str, summary: &str, patterns: Vec<String>) -> Value {
        let mut state = read_state("memory-distillation");
        let episodes = ensure_arr(&mut state, "episodes");
        let episode = json!({
            "id": format!("ep-{}", now_ms()),
            "source": source,
            "summary": summary,
            "patterns": patterns,
            "createdAt": now_ms(),
        });
        episodes.push(episode.clone());
        write_state("memory-distillation", &state);
        episode
    }

    pub fn list_episodes() -> Vec<Value> {
        read_state("memory-distillation")["episodes"]
            .as_array()
            .cloned()
            .unwrap_or_default()
    }
}

/// Workspace lease — time-limited workspace exclusivity.
/// Ports services/workspace-lease.ts (207 lines).
pub mod lease {
    use super::*;

    pub fn acquire(workspace: &str, holder: &str, ttl_ms: u64) -> Result<Value, String> {
        // Lock around read-check-write so two callers can't both observe an
        // unleased workspace and both "win" it (lost-update → split-brain lease).
        let _guard = LockGuard::acquire("workspace-leases")
            .ok_or_else(|| "workspace-leases lock contention".to_string())?;
        let mut state = read_state("workspace-leases");
        let now = now_ms();
        let existing = state[workspace].clone();
        if !existing.is_null() {
            let expires = existing["expiresAt"].as_u64().unwrap_or(0);
            if expires > now && existing["holder"].as_str() != Some(holder) {
                return Err(format!("workspace `{workspace}` leased by {}", existing["holder"].as_str().unwrap_or("?")));
            }
        }
        let lease = json!({"holder": holder, "acquiredAt": now, "expiresAt": now + ttl_ms});
        state[workspace] = lease.clone();
        write_state("workspace-leases", &state);
        Ok(lease)
    }

    pub fn release(workspace: &str, holder: &str) -> bool {
        let mut state = read_state("workspace-leases");
        if state[workspace]["holder"].as_str() == Some(holder) {
            state[workspace] = Value::Null;
            write_state("workspace-leases", &state);
            return true;
        }
        false
    }
}

/// Checkpoint gate — validates preconditions before proceeding.
/// Ports services/checkpoint-gate.ts (288 lines).
pub mod checkpoint {
    use super::*;

    pub fn validate(name: &str, checks: Vec<(&str, bool)>) -> Result<Value, String> {
        let failures: Vec<&str> = checks.iter().filter(|(_, ok)| !ok).map(|(n, _)| *n).collect();
        let result = json!({
            "checkpoint": name,
            "passed": failures.is_empty(),
            "failures": failures,
            "at": now_ms(),
        });
        let mut state = read_state("checkpoints");
        ensure_arr(&mut state, "history").push(result.clone());
        write_state("checkpoints", &state);
        if failures.is_empty() {
            Ok(result)
        } else {
            Err(format!("checkpoint `{name}` failed: {}", failures.join(", ")))
        }
    }

    pub fn history() -> Vec<Value> {
        read_state("checkpoints")["history"]
            .as_array()
            .cloned()
            .unwrap_or_default()
    }
}

/// Repo supervisor — monitors repo health.
/// Ports services/repo-supervisor.ts (237 lines) — state only.
pub mod supervisor {
    use super::*;

    pub fn record_check(status: &str, issues: Vec<String>) -> Value {
        let mut state = read_state("repo-supervisor");
        let entry = json!({
            "status": status,
            "issues": issues,
            "checkedAt": now_ms(),
        });
        ensure_arr(&mut state, "checks").push(entry.clone());
        write_state("repo-supervisor", &state);
        entry
    }

    pub fn latest() -> Option<Value> {
        let state = read_state("repo-supervisor");
        state["checks"].as_array()?.last().cloned()
    }
}

/// Policy runtime — ADR-324 policy evaluation state.
/// Ports services/policy-runtime.ts (407 lines) — state management.
pub mod policy_runtime {
    use super::*;

    pub fn evaluate(action: &str, identity: &str) -> Value {
        let state = read_state("policy-runtime");
        let rules = state["rules"].as_array().cloned().unwrap_or_default();
        let mut decision = "allow".to_string();
        for rule in &rules {
            if rule["action"].as_str() == Some(action) || rule["action"].as_str() == Some("*") {
                if rule["effect"].as_str() == Some("deny") {
                    decision = "deny".to_string();
                    break;
                }
            }
        }
        let result = json!({
            "action": action,
            "identity": identity,
            "decision": decision,
            "evaluatedAt": now_ms(),
        });
        let mut st = read_state("policy-runtime");
        ensure_arr(&mut st, "ledger").push(result.clone());
        write_state("policy-runtime", &st);
        result
    }

    pub fn add_rule(action: &str, effect: &str) -> bool {
        let mut state = read_state("policy-runtime");
        let rules = ensure_arr(&mut state, "rules");
        rules.push(json!({"action": action, "effect": effect, "addedAt": now_ms()}));
        write_state("policy-runtime", &state);
        true
    }

    pub fn ledger() -> Vec<Value> {
        read_state("policy-runtime")["ledger"]
            .as_array()
            .cloned()
            .unwrap_or_default()
    }
}

// ============================================================================ //
// HARNESS/METAHARNESS (state management — execution deferred to LLM runtime)
// ============================================================================ //

/// Generic harness state — covers all 15 harness-* services with a shared
/// state-file pattern. Each harness service (loop, benchmark, canary, replay,
/// verify, etc.) stores its runs/cycles/results in a named state file.
pub mod harness {
    use super::*;

    pub fn record_run(harness_type: &str, data: Value) -> Value {
        let file = format!("harness-{harness_type}");
        let mut state = read_state(&file);
        let run = json!({
            "id": format!("run-{}", now_ms()),
            "data": data,
            "recordedAt": now_ms(),
        });
        ensure_arr(&mut state, "runs").push(run.clone());
        write_state(&file, &state);
        run
    }

    pub fn list_runs(harness_type: &str) -> Vec<Value> {
        let file = format!("harness-{harness_type}");
        read_state(&file)["runs"].as_array().cloned().unwrap_or_default()
    }

    pub fn get_state(harness_type: &str) -> Value {
        read_state(&format!("harness-{harness_type}"))
    }

    /// All harness service types (15 from TS).
    pub const HARNESS_TYPES: &[&str] = &[
        "loop", "benchmark", "canary", "replay", "verify", "worker",
        "hosts", "corpus-harvester", "frozen-eval", "improvement-ledger",
        "project-anchor", "qualification", "flywheel", "flywheel-runtime",
        "flywheel-generations",
    ];
}

/// Fable harness — story-based test harness state.
/// Ports services/fable-harness.ts (480 lines) — state only.
pub mod fable {
    use super::*;
    pub fn record_story(name: &str, steps: Vec<String>) -> Value {
        let mut state = read_state("fable-harness");
        let story = json!({"name": name, "steps": steps, "recordedAt": now_ms()});
        ensure_arr(&mut state, "stories").push(story.clone());
        write_state("fable-harness", &state);
        story
    }
    pub fn list() -> Vec<Value> {
        read_state("fable-harness")["stories"].as_array().cloned().unwrap_or_default()
    }
}

/// Evolve proof — Darwin evolution receipt state.
/// Ports services/evolve-proof.ts (493 lines) — state only.
pub mod evolve {
    use super::*;
    pub fn record_champion(fitness: f64, generation: usize, surface: &str) -> Value {
        let mut state = read_state("evolve-proof");
        let champ = json!({"fitness": fitness, "generation": generation, "surface": surface, "at": now_ms()});
        ensure_arr(&mut state, "champions").push(champ.clone());
        write_state("evolve-proof", &state);
        champ
    }
    pub fn champions() -> Vec<Value> {
        read_state("evolve-proof")["champions"].as_array().cloned().unwrap_or_default()
    }
}

/// Weight EFT — elastic weight consolidation state.
/// Ports services/weight-eft.ts (506 lines) — state only.
pub mod weight_eft {
    use super::*;
    pub fn record(task: &str, weights: Vec<f64>) -> Value {
        let mut state = read_state("weight-eft");
        let entry = json!({"task": task, "weights": weights, "at": now_ms()});
        ensure_arr(&mut state, "records").push(entry.clone());
        write_state("weight-eft", &state);
        entry
    }
    pub fn records() -> Vec<Value> {
        read_state("weight-eft")["records"].as_array().cloned().unwrap_or_default()
    }
}

// ============================================================================ //
// FLYWHEEL (state management)
// ============================================================================ //

/// Flywheel receipt — immutable receipt ledger.
/// Ports services/flywheel-receipt.ts (441 lines) — state only.
pub mod flywheel_receipt {
    use super::*;
    pub fn create(event: &str, payload: Value) -> Value {
        let mut state = read_state("flywheel-receipts");
        let receipt = json!({
            "id": format!("rcpt-{}", now_ms()),
            "event": event,
            "payload": payload,
            "createdAt": now_ms(),
        });
        ensure_arr(&mut state, "receipts").push(receipt.clone());
        write_state("flywheel-receipts", &state);
        receipt
    }
    pub fn list() -> Vec<Value> {
        read_state("flywheel-receipts")["receipts"].as_array().cloned().unwrap_or_default()
    }
}

/// Flywheel transaction — transactional flywheel operations.
/// Ports services/flywheel-transaction.ts (451 lines) — state only.
pub mod flywheel_tx {
    use super::*;
    pub fn commit(action: &str, data: Value) -> Value {
        let mut state = read_state("flywheel-transactions");
        let tx = json!({"id": format!("tx-{}", now_ms()), "action": action, "data": data, "committedAt": now_ms()});
        ensure_arr(&mut state, "transactions").push(tx.clone());
        write_state("flywheel-transactions", &state);
        tx
    }
    pub fn history() -> Vec<Value> {
        read_state("flywheel-transactions")["transactions"].as_array().cloned().unwrap_or_default()
    }
}

/// Flywheel proposer — candidate proposal state.
/// Ports services/flywheel-proposer.ts (251 lines) — state only.
pub mod flywheel_proposer {
    use super::*;
    pub fn propose(candidate: &str, source: &str) -> Value {
        let mut state = read_state("flywheel-proposals");
        let prop = json!({"candidate": candidate, "source": source, "proposedAt": now_ms()});
        ensure_arr(&mut state, "proposals").push(prop.clone());
        write_state("flywheel-proposals", &state);
        prop
    }
    pub fn proposals() -> Vec<Value> {
        read_state("flywheel-proposals")["proposals"].as_array().cloned().unwrap_or_default()
    }
}

// ============================================================================ //
// DISTILL/TRAINING (state management — execution deferred to WASM/ONNX)
// ============================================================================ //

/// Distill oracle — distillation source oracle state.
/// Ports services/distill-oracle.ts (521 lines) — state only.
pub mod distill_oracle {
    use super::*;
    pub fn record(model: &str, accuracy: f64) -> Value {
        let mut state = read_state("distill-oracle");
        let entry = json!({"model": model, "accuracy": accuracy, "at": now_ms()});
        ensure_arr(&mut state, "evaluations").push(entry.clone());
        write_state("distill-oracle", &state);
        entry
    }
    pub fn evaluations() -> Vec<Value> {
        read_state("distill-oracle")["evaluations"].as_array().cloned().unwrap_or_default()
    }
}

/// Distill tuning — hyperparameter tuning state.
/// Ports services/distill-tuning.ts (695 lines) — state only.
pub mod distill_tuning {
    use super::*;
    pub fn record_trial(params: Value, score: f64) -> Value {
        let mut state = read_state("distill-tuning");
        let trial = json!({"params": params, "score": score, "trialNum": state["trials"].as_array().map(|a| a.len()).unwrap_or(0), "at": now_ms()});
        ensure_arr(&mut state, "trials").push(trial.clone());
        write_state("distill-tuning", &state);
        trial
    }
    pub fn trials() -> Vec<Value> {
        read_state("distill-tuning")["trials"].as_array().cloned().unwrap_or_default()
    }
}

/// Native training — TrainingPipeline state.
/// Ports services/native-training.ts (190 lines) — state only.
pub mod native_training {
    use super::*;
    pub fn record_checkpoint(model: &str, epoch: usize, loss: f64) -> Value {
        let mut state = read_state("native-training");
        let ckpt = json!({"model": model, "epoch": epoch, "loss": loss, "savedAt": now_ms()});
        ensure_arr(&mut state, "checkpoints").push(ckpt.clone());
        write_state("native-training", &state);
        ckpt
    }
    pub fn checkpoints() -> Vec<Value> {
        read_state("native-training")["checkpoints"].as_array().cloned().unwrap_or_default()
    }
}

/// RuVector training — WASM SIMD training state.
/// Ports services/ruvector-training.ts (956 lines) — state only.
pub mod ruvector_training {
    use super::*;
    pub fn get_stats() -> Value {
        read_state("ruvector-training")
    }
    pub fn record_session(model: &str, duration_ms: u64, patterns: usize) -> Value {
        let mut state = read_state("ruvector-training");
        let session = json!({"model": model, "durationMs": duration_ms, "patterns": patterns, "at": now_ms()});
        ensure_arr(&mut state, "sessions").push(session.clone());
        write_state("ruvector-training", &state);
        session
    }
}

// ============================================================================ //
// INTEGRATION
// ============================================================================ //

/// Agentic-flow bridge — tracks bridge state.
/// Ports services/agentic-flow-bridge.ts (109 lines).
pub mod agentic_bridge {
    use super::*;
    pub fn status() -> Value {
        read_state("agentic-bridge")
    }
    pub fn set_connected(version: &str) -> bool {
        write_state("agentic-bridge", &json!({"connected": true, "version": version, "connectedAt": now_ms()}));
        true
    }
}

/// Registry API — package registry state.
/// Ports services/registry-api.ts (203 lines).
pub mod registry {
    use super::*;
    pub fn list_packages() -> Vec<Value> {
        read_state("registry")["packages"].as_array().cloned().unwrap_or_default()
    }
    pub fn register(name: &str, version: &str) -> Value {
        let mut state = read_state("registry");
        let entry = json!({"name": name, "version": version, "registeredAt": now_ms()});
        ensure_arr(&mut state, "packages").push(entry.clone());
        write_state("registry", &state);
        entry
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    static TEST_LOCK: Mutex<()> = Mutex::new(());

    fn tmp() -> PathBuf {
        let dir = tempfile::tempdir().unwrap().keep();
        std::env::set_current_dir(&dir).unwrap();
        dir
    }

    #[test]
    fn bounded_pool_acquire_release() {
        let _g = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _d = tmp();
        let slot = bounded_pool::acquire("test", 2).unwrap();
        assert!(bounded_pool::acquire("test", 1).is_err()); // full
        assert!(bounded_pool::release("test", slot["id"].as_str().unwrap()));
        let s2 = bounded_pool::acquire("test", 1).unwrap(); // now free
        assert_eq!(s2["id"].as_str().unwrap().starts_with("slot-"), true);
    }

    #[test]
    fn worker_queue_fifo() {
        let _g = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _d = tmp();
        worker_queue::enqueue(json!({"task": "a"}));
        worker_queue::enqueue(json!({"task": "b"}));
        assert_eq!(worker_queue::length(), 2);
        let first = worker_queue::dequeue().unwrap();
        assert_eq!(first["task"]["task"].as_str(), Some("a"));
    }

    #[test]
    fn dedup_check_mark() {
        let _g = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _d = tmp();
        assert!(!dedup::check("job1"));
        dedup::mark("job1");
        assert!(dedup::check("job1"));
    }

    #[test]
    fn lease_acquire_release() {
        let _g = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _d = tmp();
        let lease = lease::acquire("ws1", "agent1", 60000).unwrap();
        assert!(lease::acquire("ws1", "agent2", 60000).is_err()); // held
        assert!(lease::release("ws1", "agent1"));
        lease::acquire("ws1", "agent2", 60000).unwrap(); // now free
    }

    #[test]
    fn checkpoint_validates() {
        let _g = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _d = tmp();
        assert!(checkpoint::validate("gate1", vec![("a", true), ("b", true)]).is_ok());
        assert!(checkpoint::validate("gate2", vec![("a", false)]).is_err());
    }

    #[test]
    fn pheromone_record_eligible() {
        let _g = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _d = tmp();
        pheromone::record("agent1", "coder", 0.9, 0.1, 1.0);
        pheromone::record("agent2", "coder", 0.1, 0.9, 0.1);
        let eligible = pheromone::eligible();
        assert!(eligible.contains(&"agent1".to_string()));
        assert!(!eligible.contains(&"agent2".to_string()));
    }

    #[test]
    fn harness_record_and_list() {
        let _g = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _d = tmp();
        harness::record_run("benchmark", json!({"score": 42}));
        assert_eq!(harness::list_runs("benchmark").len(), 1);
    }

    #[test]
    fn flywheel_receipt_create_list() {
        let _g = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _d = tmp();
        flywheel_receipt::create("eval", json!({"result": "pass"}));
        assert_eq!(flywheel_receipt::list().len(), 1);
    }

    #[test]
    fn autostart_generates_configs() {
        let _g = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _d = tmp();
        let cron = autostart::install_cron();
        assert!(cron.contains("@reboot"));
        let systemd = autostart::install_systemd();
        assert!(systemd.contains("[Unit]"));
        let launchd = autostart::install_launchd();
        assert!(launchd.contains("<plist"));
    }

    #[test]
    fn policy_evaluate() {
        let _g = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _d = tmp();
        policy_runtime::add_rule("swarm.spawn", "deny");
        let result = policy_runtime::evaluate("swarm.spawn", "user1");
        assert_eq!(result["decision"].as_str(), Some("deny"));
        let allow = policy_runtime::evaluate("swarm.status", "user1");
        assert_eq!(allow["decision"].as_str(), Some("allow"));
    }
}
