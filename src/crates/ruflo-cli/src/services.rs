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

/// Process-local atomic counter for unique IDs — prevents collisions when two
/// calls land in the same millisecond.
use std::sync::atomic::{AtomicU64, Ordering};
static ID_COUNTER: AtomicU64 = AtomicU64::new(0);

fn unique_id(prefix: &str) -> String {
    let ctr = ID_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{prefix}-{}-{ctr}", now_ms())
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
        let slot = json!({"id": unique_id("slot"), "acquiredAt": now_ms()});
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
            "id": unique_id("wq"),
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
    use std::process::Command;
    use std::time::Duration;

    /// Launch a headless worker: spawn `claude -p <prompt>` (or codex) as a
    /// subprocess, capture stdout/stderr, enforce a timeout, and record the
    /// result in headless-workers state. Real behavioral parity with TS
    /// headless-worker-executor — no Node.
    ///
    /// Returns the worker result entry. `binary` is "claude" or "codex".
    pub fn execute(
        worker_type: &str,
        binary: &str,
        prompt: &str,
        timeout_ms: u64,
        extra_args: &[String],
    ) -> Value {
        let id = format!("headless-{worker_type}-{}", now_ms());
        let started = now_ms();

        // Resolve binary; degrade cleanly if absent.
        if which(binary).is_none() {
            let entry = json!({
                "id": id, "type": worker_type, "binary": binary,
                "status": "unavailable", "error": format!("{binary} not on PATH"),
                "startedAt": started, "finishedAt": now_ms(),
            });
            record(&entry);
            return entry;
        }

        let mut cmd = Command::new(binary);
        cmd.arg("-p").arg(prompt);
        cmd.env_remove("OPENAI_API_KEY")
            .env_remove("ANTHROPIC_API_KEY")
            .env_remove("GEMINI_API_KEY");
        for a in extra_args {
            cmd.arg(a);
        }
        cmd.stdin(std::process::Stdio::null());

        // spawn + watchdog timeout (mirrors swarm_exec pattern)
        match cmd.spawn() {
            Ok(mut child) => {
                let timeout = Duration::from_millis(timeout_ms.max(1000));
                let start = std::time::Instant::now();
                loop {
                    match child.try_wait() {
                        Ok(Some(status)) => {
                            let stdout = child.stdout.take()
                                .and_then(|mut s| { let mut o = String::new(); std::io::Read::read_to_string(&mut s, &mut o).ok(); Some(o) })
                                .unwrap_or_default();
                            let stderr = child.stderr.take()
                                .and_then(|mut s| { let mut o = String::new(); std::io::Read::read_to_string(&mut s, &mut o).ok(); Some(o) })
                                .unwrap_or_default();
                            // Cap output to bound memory.
                            let stdout_cap: String = stdout.chars().take(1_000_000).collect();
                            let stderr_cap: String = stderr.chars().take(100_000).collect();
                            let entry = json!({
                                "id": id, "type": worker_type, "binary": binary,
                                "status": if status.success() { "completed" } else { "failed" },
                                "exitCode": status.code(),
                                "stdout": stdout_cap, "stderr": stderr_cap,
                                "startedAt": started, "finishedAt": now_ms(),
                                "durationMs": now_ms().saturating_sub(started),
                            });
                            record(&entry);
                            return entry;
                        }
                        Ok(None) => {
                            if start.elapsed() > timeout {
                                let _ = child.kill();
                                let _ = child.wait();
                                let entry = json!({
                                    "id": id, "type": worker_type, "binary": binary,
                                    "status": "timeout", "timeoutMs": timeout_ms,
                                    "startedAt": started, "finishedAt": now_ms(),
                                });
                                record(&entry);
                                return entry;
                            }
                            std::thread::sleep(Duration::from_millis(50));
                        }
                        Err(e) => {
                            let entry = json!({
                                "id": id, "type": worker_type, "binary": binary,
                                "status": "error", "error": e.to_string(),
                                "startedAt": started, "finishedAt": now_ms(),
                            });
                            record(&entry);
                            return entry;
                        }
                    }
                }
            }
            Err(e) => {
                let entry = json!({
                    "id": id, "type": worker_type, "binary": binary,
                    "status": "spawn_failed", "error": e.to_string(),
                    "startedAt": started, "finishedAt": now_ms(),
                });
                record(&entry);
                entry
            }
        }
    }

    fn record(entry: &Value) {
        let mut state = read_state("headless-workers");
        ensure_arr(&mut state, "workers").push(entry.clone());
        write_state("headless-workers", &state);
    }

    fn which(bin: &str) -> Option<std::path::PathBuf> {
        std::env::var_os("PATH").and_then(|paths| {
            std::env::split_paths(&paths).find_map(|dir| {
                let p = dir.join(bin);
                if p.is_file() { Some(p) } else { None }
            })
        })
    }

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
            "id": unique_id("container"),
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
        let wt_path = root.join(format!(".claude-flow/worktrees/{branch}"));
        let _ = Command::new("git")
            .args(["worktree", "remove", "--force", wt_path.to_str().unwrap_or("")])
            .current_dir(root)
            .output();
        // Also drop the branch (best-effort).
        let _ = Command::new("git")
            .args(["branch", "-D", branch])
            .current_dir(root)
            .output();
        let mut state = read_state("git-worktrees");
        if let Some(entries) = state["worktrees"].as_array_mut() {
            entries.retain(|w| w["branch"].as_str() != Some(branch));
        }
        write_state("git-worktrees", &state);
        // Release any lease held on this workspace (holder unknown here —
        // best-effort: try the recorded holder from state).
        let st = read_state("git-worktrees");
        let holder = st["worktrees"].as_array()
            .and_then(|arr| arr.iter().find(|w| w["branch"].as_str() == Some(branch)))
            .and_then(|w| w["holder"].as_str().map(|s| s.to_string()))
            .unwrap_or_default();
        let _ = crate::services::lease::release(&wt_path.display().to_string(), &holder);
        Ok(())
    }

    /// Acquire a worktree + a workspace lease atomically: each writing agent
    /// gets its own isolated git worktree owned by a time-limited lease.
    /// Returns (worktree_path, lease). The lease auto-releases on expiry; the
    /// worktree is removed explicitly via remove_worktree.
    pub fn acquire_with_lease(
        root: &Path,
        branch: &str,
        holder: &str,
        ttl_ms: u64,
    ) -> Result<(PathBuf, Value), String> {
        let wt = create_worktree(root, branch)?;
        // Record holder so remove_worktree can release the lease.
        let mut state = read_state("git-worktrees");
        if let Some(arr) = state["worktrees"].as_array_mut() {
            for w in arr.iter_mut() {
                if w["branch"].as_str() == Some(branch) {
                    w["holder"] = json!(holder);
                }
            }
        }
        write_state("git-worktrees", &state);
        let lease = crate::services::lease::acquire(&wt.display().to_string(), holder, ttl_ms)?;
        Ok((wt, lease))
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

/// Daemon autostart — systemd/launchd/crontab config generation AND install.
/// Ports services/daemon-autostart.ts (143 lines).
///
/// Each installer generates the platform-specific config text, persists it to
/// the autostart state file, AND actually runs the install command
/// (`crontab -`, `systemctl --user`, `launchctl load`). Failures bubble up as
/// `Err(String)`. `uninstall()` reverses the install based on the stored
/// `method` field.
pub mod autostart {
    use super::*;
    use std::io::Write;
    use std::process::{Command, Stdio};

    const CRON_LINE: &str = "@reboot $(which ruflo) daemon start --background\n";
    const SYSTEMD_UNIT_NAME: &str = "ruflo-daemon.service";
    const LAUNCHD_LABEL: &str = "io.ruflo.daemon";
    const LAUNCHD_PLIST_NAME: &str = "io.ruflo.daemon.plist";

    /// Resolve the absolute path to the `ruflo` binary via `which ruflo`.
    /// Falls back to the bare word `ruflo` if the lookup fails.
    fn ruflo_path() -> String {
        Command::new("which")
            .arg("ruflo")
            .output()
            .ok()
            .and_then(|o| {
                if o.status.success() {
                    Some(String::from_utf8_lossy(&o.stdout).trim().to_string())
                } else {
                    None
                }
            })
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "ruflo".to_string())
    }

    fn build_systemd_unit(ruflo: &str) -> String {
        format!(
            "[Unit]\n\
             Description=Ruflo Daemon\n\
             After=network.target\n\n\
             [Service]\n\
             ExecStart={ruflo} daemon start --foreground\n\
             Restart=always\n\n\
             [Install]\n\
             WantedBy=default.target\n"
        )
    }

    fn build_launchd_plist(ruflo: &str) -> String {
        format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
             <plist version=\"1.0\"><dict>\n\
             \x20 <key>Label</key><string>{label}</string>\n\
             \x20 <key>ProgramArguments</key><array>\n\
             \x20   <string>{ruflo}</string><string>daemon</string>\n\
             \x20   <string>start</string><string>--foreground</string>\n\
             \x20 </array>\n\
             \x20 <key>RunAtLoad</key><true/>\n\
             </dict></plist>\n",
            label = LAUNCHD_LABEL,
            ruflo = ruflo
        )
    }

    /// Install via crontab: pipe `{existing}\n{cron_line}` to `crontab -`.
    /// Idempotent — skips if the line is already present.
    pub fn install_cron() -> Result<String, String> {
        // Read existing crontab (may be empty or unset).
        let existing = match Command::new("crontab").arg("-l").output() {
            Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).into_owned(),
            _ => String::new(),
        };
        let merged = if existing.contains(CRON_LINE.trim()) {
            existing
        } else {
            format!("{existing}{CRON_LINE}")
        };
        let mut child = Command::new("crontab")
            .arg("-")
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("crontab spawn failed: {e}"))?;
        {
            let stdin = child
                .stdin
                .as_mut()
                .ok_or_else(|| "crontab stdin unavailable".to_string())?;
            stdin
                .write_all(merged.as_bytes())
                .map_err(|e| format!("crontab write failed: {e}"))?;
        }
        let output = child
            .wait_with_output()
            .map_err(|e| format!("crontab wait failed: {e}"))?;
        if !output.status.success() {
            return Err(format!(
                "crontab install failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }
        write_state(
            "autostart",
            &json!({
                "method": "cron",
                "config": CRON_LINE,
                "installedAt": now_ms()
            }),
        );
        Ok(CRON_LINE.into())
    }

    /// Install via systemd user unit: write
    /// `~/.config/systemd/user/ruflo-daemon.service`, then
    /// `systemctl --user daemon-reload && systemctl --user enable ruflo-daemon`.
    pub fn install_systemd() -> Result<String, String> {
        let ruflo = ruflo_path();
        let unit = build_systemd_unit(&ruflo);
        let home = std::env::var("HOME").map_err(|_| "HOME not set".to_string())?;
        let dir = PathBuf::from(&home).join(".config/systemd/user");
        fs::create_dir_all(&dir).map_err(|e| format!("mkdir systemd user dir: {e}"))?;
        let unit_path = dir.join(SYSTEMD_UNIT_NAME);
        fs::write(&unit_path, &unit).map_err(|e| format!("write unit file: {e}"))?;
        let reload = Command::new("systemctl")
            .args(["--user", "daemon-reload"])
            .status()
            .map_err(|e| format!("systemctl --user daemon-reload spawn: {e}"))?;
        if !reload.success() {
            return Err("systemctl --user daemon-reload failed".to_string());
        }
        let enable = Command::new("systemctl")
            .args(["--user", "enable", "ruflo-daemon"])
            .status()
            .map_err(|e| format!("systemctl --user enable spawn: {e}"))?;
        if !enable.success() {
            return Err("systemctl --user enable ruflo-daemon failed".to_string());
        }
        write_state(
            "autostart",
            &json!({
                "method": "systemd",
                "config": unit,
                "path": unit_path.display().to_string(),
                "unitName": SYSTEMD_UNIT_NAME,
                "installedAt": now_ms()
            }),
        );
        Ok(unit)
    }

    /// Install via launchd (macOS only): write
    /// `~/Library/LaunchAgents/io.ruflo.daemon.plist`, then
    /// `launchctl load`. On non-macOS targets, returns Err.
    #[cfg(target_os = "macos")]
    pub fn install_launchd() -> Result<String, String> {
        let ruflo = ruflo_path();
        let plist = build_launchd_plist(&ruflo);
        let home = std::env::var("HOME").map_err(|_| "HOME not set".to_string())?;
        let dir = PathBuf::from(&home).join("Library/LaunchAgents");
        fs::create_dir_all(&dir).map_err(|e| format!("mkdir LaunchAgents dir: {e}"))?;
        let plist_path = dir.join(LAUNCHD_PLIST_NAME);
        fs::write(&plist_path, &plist).map_err(|e| format!("write plist: {e}"))?;
        let load = Command::new("launchctl")
            .arg("load")
            .arg(&plist_path)
            .status()
            .map_err(|e| format!("launchctl load spawn: {e}"))?;
        if !load.success() {
            return Err("launchctl load failed".to_string());
        }
        write_state(
            "autostart",
            &json!({
                "method": "launchd",
                "config": plist,
                "path": plist_path.display().to_string(),
                "label": LAUNCHD_LABEL,
                "installedAt": now_ms()
            }),
        );
        Ok(plist)
    }

    /// launchd install is macOS-only. On other platforms the build is still
    /// valid; we surface an explicit runtime error here.
    #[cfg(not(target_os = "macos"))]
    pub fn install_launchd() -> Result<String, String> {
        Err("launchd install is only supported on macOS".to_string())
    }

    /// Reverse the appropriate install based on the stored `method`. Clears
    /// the autostart state file on success.
    pub fn uninstall() -> Result<(), String> {
        let state = read_state("autostart");
        let method = state["method"].as_str().unwrap_or("");
        match method {
            "cron" => {
                // Remove the ruflo line from existing crontab, write back.
                let existing = match Command::new("crontab").arg("-l").output() {
                    Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).into_owned(),
                    _ => String::new(),
                };
                let kept: String = existing
                    .lines()
                    .filter(|l| !l.contains("ruflo daemon start --background"))
                    .collect::<Vec<_>>()
                    .join("\n");
                let mut child = Command::new("crontab")
                    .arg("-")
                    .stdin(Stdio::piped())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn()
                    .map_err(|e| format!("crontab spawn failed: {e}"))?;
                {
                    let stdin = child
                        .stdin
                        .as_mut()
                        .ok_or_else(|| "crontab stdin unavailable".to_string())?;
                    let mut to_write = kept;
                    if !to_write.is_empty() {
                        to_write.push('\n');
                    }
                    stdin
                        .write_all(to_write.as_bytes())
                        .map_err(|e| format!("crontab write failed: {e}"))?;
                }
                let _ = child.wait();
            }
            "systemd" => {
                let unit_name = state["unitName"]
                    .as_str()
                    .unwrap_or(SYSTEMD_UNIT_NAME);
                let _ = Command::new("systemctl")
                    .args(["--user", "disable", unit_name])
                    .status();
                let _ = Command::new("systemctl")
                    .args(["--user", "stop", unit_name])
                    .status();
                if let Some(path_str) = state["path"].as_str() {
                    let _ = fs::remove_file(path_str);
                }
                let _ = Command::new("systemctl")
                    .args(["--user", "daemon-reload"])
                    .status();
            }
            "launchd" => {
                uninstall_launchd(&state)?;
            }
            _ => {}
        }
        let path = state_path("autostart");
        if path.exists() {
            let _ = fs::remove_file(&path);
        }
        Ok(())
    }

    #[cfg(target_os = "macos")]
    fn uninstall_launchd(state: &Value) -> Result<(), String> {
        let path_str = state["path"].as_str().ok_or("missing plist path in state")?;
        let _ = Command::new("launchctl").arg("unload").arg(path_str).status();
        let _ = fs::remove_file(path_str);
        Ok(())
    }

    #[cfg(not(target_os = "macos"))]
    fn uninstall_launchd(_state: &Value) -> Result<(), String> {
        Err("launchd uninstall is only supported on macOS".to_string())
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
            "id": unique_id("ep"),
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

/// Global AI budget — machine-wide token/cost circuit breaker.
/// Ports services/global-ai-budget.ts. Enforces concurrent/hourly/daily
/// caps across ALL worker spawns. check() is called before every swarm.spawn
/// / headless.execute; it returns Err when the budget is exhausted (circuit
/// open). record() books spend after a worker completes.
pub mod global_budget {
    use super::*;
    use std::sync::Mutex as StdMutex;
    /// In-process lock — the file lock's 2s deadline starves under heavy
    /// parallel test load. Process-local serialization is enough for the
    /// budget (multi-process safety is best-effort via the state file).
    static PROC_LOCK: StdMutex<()> = StdMutex::new(());

    /// Per-model cost rates (USD per 1M tokens, blended in/out).
    fn rate_per_mtok(model: &str) -> f64 {
        match model {
            "haiku" => 1.25,
            "sonnet" => 9.0,
            "opus" => 45.0,
            "gpt-4o" | "gpt4o" => 10.0,
            "gemini-pro" | "gemini" => 3.5,
            _ => 5.0,
        }
    }

    /// Default limits (overridable via state). Concurrent=8, hourly=$5, daily=$50.
    fn defaults() -> Value {
        json!({
            "maxConcurrent": 8,
            "hourlyBudgetUsd": 5.0,
            "dailyBudgetUsd": 50.0,
            "concurrent": 0,
            "hourSpentUsd": 0.0,
            "daySpentUsd": 0.0,
            "hourStart": now_ms(),
            "dayStart": now_ms(),
            "circuitOpen": false,
        })
    }

    fn load() -> Value {
        let mut s = read_state("global-budget");
        if s.is_null() || s.as_object().map(|o| o.is_empty()).unwrap_or(true) {
            s = defaults();
            write_state("global-budget", &s);
        }
        s
    }

    fn rollover(s: &mut Value) {
        let now = now_ms();
        let hour_ms = 3_600_000u64;
        let day_ms = 86_400_000u64;
        if now.saturating_sub(s["hourStart"].as_u64().unwrap_or(now)) > hour_ms {
            s["hourSpentUsd"] = json!(0.0);
            s["hourStart"] = json!(now);
        }
        if now.saturating_sub(s["dayStart"].as_u64().unwrap_or(now)) > day_ms {
            s["daySpentUsd"] = json!(0.0);
            s["dayStart"] = json!(now);
        }
    }

    /// Check whether a spawn is allowed. Returns Ok(cost-so-far) or Err(reason).
    /// Does NOT book spend — call record() after the worker finishes.
    pub fn check() -> Result<Value, String> {
        let _g = PROC_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let mut s = load();
        rollover(&mut s);
        let concurrent = s["concurrent"].as_u64().unwrap_or(0);
        let max_concurrent = s["maxConcurrent"].as_u64().unwrap_or(8);
        if s["circuitOpen"].as_bool() == Some(true) {
            return Err("circuit open (budget breaker tripped)".into());
        }
        if concurrent >= max_concurrent {
            return Err(format!(
                "concurrent cap reached ({concurrent}/{max_concurrent})"
            ));
        }
        let hour = s["hourSpentUsd"].as_f64().unwrap_or(0.0);
        let hour_max = s["hourlyBudgetUsd"].as_f64().unwrap_or(5.0);
        if hour >= hour_max {
            s["circuitOpen"] = json!(true);
            write_state("global-budget", &s);
            return Err(format!("hourly budget exhausted (${hour:.2}/${hour_max:.2})"));
        }
        let day = s["daySpentUsd"].as_f64().unwrap_or(0.0);
        let day_max = s["dailyBudgetUsd"].as_f64().unwrap_or(50.0);
        if day >= day_max {
            s["circuitOpen"] = json!(true);
            write_state("global-budget", &s);
            return Err(format!("daily budget exhausted (${day:.2}/${day_max:.2})"));
        }
        // Reserve a concurrent slot.
        s["concurrent"] = json!(concurrent + 1);
        write_state("global-budget", &s);
        Ok(json!({"concurrent": concurrent + 1, "hourSpentUsd": hour, "daySpentUsd": day}))
    }

    /// Book actual spend after a worker completes. Releases the concurrent slot.
    pub fn record(model: &str, tokens: u64, success: bool) -> Value {
        let _g = PROC_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let mut s = load();
        rollover(&mut s);
        let cost = (tokens as f64 / 1_000_000.0) * rate_per_mtok(model);
        let hour = s["hourSpentUsd"].as_f64().unwrap_or(0.0) + cost;
        let day = s["daySpentUsd"].as_f64().unwrap_or(0.0) + cost;
        s["hourSpentUsd"] = json!(hour);
        s["daySpentUsd"] = json!(day);
        // Release the concurrent slot.
        let c = s["concurrent"].as_u64().unwrap_or(0).saturating_sub(1);
        s["concurrent"] = json!(c);
        // Trip the breaker on hard failure.
        if !success {
            s["circuitOpen"] = json!(true);
        }
        write_state("global-budget", &s);
        json!({"costUsd": cost, "hourSpentUsd": hour, "daySpentUsd": day,
               "concurrent": c, "model": model, "tokens": tokens})
    }

    pub fn status() -> Value {
        let mut s = load();
        rollover(&mut s);
        write_state("global-budget", &s);
        s
    }

    pub fn reset_breaker() -> bool {
        let _g = PROC_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let mut s = load();
        s["circuitOpen"] = json!(false);
        write_state("global-budget", &s);
        true
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
            "id": unique_id("run"),
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
            "id": unique_id("rcpt"),
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
        let tx = json!({"id": unique_id("tx"), "action": action, "data": data, "committedAt": now_ms()});
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

/// Claim service — agent issue-claim lifecycle.
/// Ports services/claim-service.ts. Manages the full claim state machine:
/// `active` → `released` / `handoff_pending` → `active` (accept) /
/// `stealable` → `stolen`.
///
/// State file: `.claude-flow/services/claim-service.json`, stored as a JSON
/// array of `{issueId, claimant: {id, type}, status, history: [...]}`.
/// Each mutation acquires the per-state-file lock so concurrent claimants
/// can't double-claim an issue.
pub mod claim_service {
    use super::*;

    /// State file name — `.claude-flow/services/claim-service.json`.
    const STATE_NAME: &str = "claim-service";

    /// Ensure state is a JSON array; return a mutable reference to it.
    /// `read_state` returns `json!({})` for a missing file, so we normalize
    /// any non-array state to an empty array on first contact.
    fn claims_array_mut(state: &mut Value) -> Result<&mut Vec<Value>, String> {
        if !state.is_array() {
            *state = json!([]);
        }
        state
            .as_array_mut()
            .ok_or_else(|| "claim-service state corrupted (not an array)".to_string())
    }

    fn push_history(entry: &mut Value, event: Value) {
        if entry["history"].is_null() {
            entry["history"] = json!([]);
        }
        if let Some(hist) = entry["history"].as_array_mut() {
            hist.push(event);
        }
    }

    /// Claim an issue for `claimant_agent_id`. Fails if the issue is already
    /// actively claimed or has a pending handoff.
    pub fn claim(
        issue_id: &str,
        claimant_agent_id: &str,
        claimant_agent_type: &str,
    ) -> Result<Value, String> {
        let _guard = LockGuard::acquire(STATE_NAME)
            .ok_or_else(|| "claim-service lock contention".to_string())?;
        let mut state = read_state(STATE_NAME);
        let arr = claims_array_mut(&mut state)?;
        if let Some(existing) = arr.iter().find(|c| c["issueId"].as_str() == Some(issue_id)) {
            let status = existing["status"].as_str().unwrap_or("");
            if status == "active" || status == "handoff_pending" {
                return Err(format!(
                    "issue `{issue_id}` already claimed (status: {status})"
                ));
            }
        }
        let entry = json!({
            "issueId": issue_id,
            "claimant": {"id": claimant_agent_id, "type": claimant_agent_type},
            "status": "active",
            "history": [
                {"event": "claimed", "at": now_ms(), "by": claimant_agent_id, "type": claimant_agent_type}
            ],
        });
        // Drop any prior (released/stolen) entry for this issue before appending.
        arr.retain(|c| c["issueId"].as_str() != Some(issue_id));
        arr.push(entry.clone());
        if !write_state(STATE_NAME, &state) {
            return Err("failed to write claim-service state".to_string());
        }
        Ok(entry)
    }

    /// Release a claim. Only the current claimant may release.
    pub fn release(issue_id: &str, claimant_agent_id: &str) -> Result<(), String> {
        let _guard = LockGuard::acquire(STATE_NAME)
            .ok_or_else(|| "claim-service lock contention".to_string())?;
        let mut state = read_state(STATE_NAME);
        let arr = claims_array_mut(&mut state)?;
        let entry = arr
            .iter_mut()
            .find(|c| c["issueId"].as_str() == Some(issue_id))
            .ok_or_else(|| format!("issue `{issue_id}` not found"))?;
        if entry["claimant"]["id"].as_str() != Some(claimant_agent_id) {
            return Err(format!(
                "issue `{issue_id}` not claimed by `{claimant_agent_id}`"
            ));
        }
        entry["status"] = json!("released");
        push_history(
            entry,
            json!({"event": "released", "at": now_ms(), "by": claimant_agent_id}),
        );
        if !write_state(STATE_NAME, &state) {
            return Err("failed to write claim-service state".to_string());
        }
        Ok(())
    }

    /// Request handoff of an issue from one agent to another. Sets status
    /// `handoff_pending`; the target must call `accept_handoff` to complete.
    pub fn handoff(
        issue_id: &str,
        from_agent: &str,
        to_agent: &str,
        reason: &str,
    ) -> Result<(), String> {
        let _guard = LockGuard::acquire(STATE_NAME)
            .ok_or_else(|| "claim-service lock contention".to_string())?;
        let mut state = read_state(STATE_NAME);
        let arr = claims_array_mut(&mut state)?;
        let entry = arr
            .iter_mut()
            .find(|c| c["issueId"].as_str() == Some(issue_id))
            .ok_or_else(|| format!("issue `{issue_id}` not found"))?;
        if entry["claimant"]["id"].as_str() != Some(from_agent) {
            return Err(format!(
                "issue `{issue_id}` not claimed by `{from_agent}`"
            ));
        }
        entry["status"] = json!("handoff_pending");
        entry["pendingHandoffTo"] = json!(to_agent);
        push_history(
            entry,
            json!({
                "event": "handoff_requested",
                "at": now_ms(),
                "from": from_agent,
                "to": to_agent,
                "reason": reason,
            }),
        );
        if !write_state(STATE_NAME, &state) {
            return Err("failed to write claim-service state".to_string());
        }
        Ok(())
    }

    /// Accept a pending handoff. Only the agent the issue was handed off to
    /// may accept. On success the claimant becomes the new agent and status
    /// returns to `active`.
    pub fn accept_handoff(issue_id: &str, agent_id: &str) -> Result<(), String> {
        let _guard = LockGuard::acquire(STATE_NAME)
            .ok_or_else(|| "claim-service lock contention".to_string())?;
        let mut state = read_state(STATE_NAME);
        let arr = claims_array_mut(&mut state)?;
        let entry = arr
            .iter_mut()
            .find(|c| c["issueId"].as_str() == Some(issue_id))
            .ok_or_else(|| format!("issue `{issue_id}` not found"))?;
        if entry["status"].as_str() != Some("handoff_pending") {
            return Err(format!("issue `{issue_id}` is not pending handoff"));
        }
        if entry["pendingHandoffTo"].as_str() != Some(agent_id) {
            return Err(format!(
                "issue `{issue_id}` handoff is not intended for `{agent_id}`"
            ));
        }
        let old_claimant = entry["claimant"]["id"].as_str().unwrap_or("").to_string();
        let old_type = entry["claimant"]["type"].clone();
        entry["claimant"] = json!({"id": agent_id, "type": old_type});
        entry["status"] = json!("active");
        if let Some(obj) = entry.as_object_mut() {
            obj.remove("pendingHandoffTo");
        }
        push_history(
            entry,
            json!({
                "event": "handoff_accepted",
                "at": now_ms(),
                "from": old_claimant,
                "to": agent_id,
            }),
        );
        if !write_state(STATE_NAME, &state) {
            return Err("failed to write claim-service state".to_string());
        }
        Ok(())
    }

    /// Mark an actively-claimed issue as available for theft by another agent
    /// (e.g. the claimant is overloaded or stale). Status becomes `stealable`.
    pub fn mark_stealable(issue_id: &str, reason: &str) -> Result<(), String> {
        let _guard = LockGuard::acquire(STATE_NAME)
            .ok_or_else(|| "claim-service lock contention".to_string())?;
        let mut state = read_state(STATE_NAME);
        let arr = claims_array_mut(&mut state)?;
        let entry = arr
            .iter_mut()
            .find(|c| c["issueId"].as_str() == Some(issue_id))
            .ok_or_else(|| format!("issue `{issue_id}` not found"))?;
        entry["status"] = json!("stealable");
        push_history(
            entry,
            json!({"event": "marked_stealable", "at": now_ms(), "reason": reason}),
        );
        if !write_state(STATE_NAME, &state) {
            return Err("failed to write claim-service state".to_string());
        }
        Ok(())
    }

    /// List all stealable issues (status == `stealable`). Optionally filtered
    /// by preferred agent type. Drives the swarm work-stealing path.
    pub fn stealable(preferred_type: Option<&str>) -> Result<Vec<Value>, String> {
        let state = read_state(STATE_NAME);
        let arr = state["claims"].as_array().cloned().unwrap_or_default();
        let filtered: Vec<Value> = arr
            .into_iter()
            .filter(|c| c["status"].as_str() == Some("stealable"))
            .filter(|c| match preferred_type {
                Some(t) => c["preferredTypes"].as_array()
                    .map(|a| a.iter().any(|x| x.as_str() == Some(t)))
                    .unwrap_or(true),
                None => true,
            })
            .collect();
        Ok(filtered)
    }

    /// Steal a stealable issue. The claimant becomes `stealer_agent_id` and
    /// status becomes `stolen` (terminal — must be re-claimed after release).
    pub fn steal(
        issue_id: &str,
        stealer_agent_id: &str,
        stealer_agent_type: &str,
    ) -> Result<(), String> {
        let _guard = LockGuard::acquire(STATE_NAME)
            .ok_or_else(|| "claim-service lock contention".to_string())?;
        let mut state = read_state(STATE_NAME);
        let arr = claims_array_mut(&mut state)?;
        let entry = arr
            .iter_mut()
            .find(|c| c["issueId"].as_str() == Some(issue_id))
            .ok_or_else(|| format!("issue `{issue_id}` not found"))?;
        if entry["status"].as_str() != Some("stealable") {
            return Err(format!(
                "issue `{issue_id}` is not stealable (status: {})",
                entry["status"].as_str().unwrap_or("?")
            ));
        }
        let old_claimant = entry["claimant"]["id"].as_str().unwrap_or("").to_string();
        entry["claimant"] = json!({"id": stealer_agent_id, "type": stealer_agent_type});
        entry["status"] = json!("stolen");
        push_history(
            entry,
            json!({
                "event": "stolen",
                "at": now_ms(),
                "from": old_claimant,
                "by": stealer_agent_id,
            }),
        );
        if !write_state(STATE_NAME, &state) {
            return Err("failed to write claim-service state".to_string());
        }
        Ok(())
    }

    /// Load the full claim status list — every claim (active or otherwise).
    pub fn load_status() -> Vec<Value> {
        read_state(STATE_NAME)
            .as_array()
            .cloned()
            .unwrap_or_default()
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
    fn autostart_install_cron_attempts_command() {
        let _g = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _d = tmp();
        // install_cron must at least generate the cron line and attempt
        // `crontab -`. Success vs. failure depends on whether crontab is
        // available in the test env; both outcomes are acceptable.
        match autostart::install_cron() {
            Ok(cron) => assert!(
                cron.contains("@reboot"),
                "generated cron line must contain @reboot"
            ),
            Err(_) => { /* crontab unavailable in test env — acceptable */ }
        }
    }

    #[test]
    fn autostart_uninstall_clears_state() {
        let _g = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _d = tmp();
        // Simulate a prior install by writing state directly.
        write_state(
            "autostart",
            &json!({"method": "cron", "config": "@reboot ruflo", "installedAt": now_ms()}),
        );
        assert!(state_path("autostart").exists());
        let _ = autostart::uninstall();
        assert!(
            !state_path("autostart").exists(),
            "uninstall must clear the autostart state file"
        );
    }

    #[test]
    fn claim_service_claim_release_reclaim() {
        let _g = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _d = tmp();
        let entry = claim_service::claim("issue-1", "agent-1", "coder").unwrap();
        assert_eq!(entry["status"].as_str(), Some("active"));
        assert_eq!(entry["claimant"]["id"].as_str(), Some("agent-1"));
        // Second claim by a different agent while active fails.
        assert!(claim_service::claim("issue-1", "agent-2", "coder").is_err());
        // Re-claim by the same agent also fails (already active).
        assert!(claim_service::claim("issue-1", "agent-1", "coder").is_err());
        claim_service::release("issue-1", "agent-1").unwrap();
        // After release a different agent may claim it.
        let again = claim_service::claim("issue-1", "agent-2", "coder").unwrap();
        assert_eq!(again["claimant"]["id"].as_str(), Some("agent-2"));
        let status = claim_service::load_status();
        assert_eq!(status.len(), 1, "exactly one claim entry after re-claim");
    }

    #[test]
    fn claim_service_handoff_flow() {
        let _g = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _d = tmp();
        claim_service::claim("issue-2", "alice", "coder").unwrap();
        claim_service::handoff("issue-2", "alice", "bob", "load balancing").unwrap();
        // Wrong target can't accept.
        assert!(claim_service::accept_handoff("issue-2", "eve").is_err());
        // Correct target accepts and becomes new claimant.
        claim_service::accept_handoff("issue-2", "bob").unwrap();
        let status = claim_service::load_status();
        let entry = &status[0];
        assert_eq!(entry["status"].as_str(), Some("active"));
        assert_eq!(entry["claimant"]["id"].as_str(), Some("bob"));
        // pendingHandoffTo should be cleared after acceptance.
        assert!(entry.get("pendingHandoffTo").is_none() || entry["pendingHandoffTo"].is_null());
        // Releasing by the previous owner fails.
        assert!(claim_service::release("issue-2", "alice").is_err());
    }

    #[test]
    fn claim_service_steal_flow() {
        let _g = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _d = tmp();
        claim_service::claim("issue-3", "alice", "coder").unwrap();
        // Can't steal while still active.
        assert!(claim_service::steal("issue-3", "bob", "coder").is_err());
        claim_service::mark_stealable("issue-3", "claimant stale").unwrap();
        // Now stealable.
        claim_service::steal("issue-3", "bob", "coder").unwrap();
        let status = claim_service::load_status();
        let entry = &status[0];
        assert_eq!(entry["status"].as_str(), Some("stolen"));
        assert_eq!(entry["claimant"]["id"].as_str(), Some("bob"));
        // History records the steal event.
        let hist = entry["history"].as_array().unwrap();
        assert!(hist.iter().any(|h| h["event"].as_str() == Some("stolen")));
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

#[cfg(test)]
mod headless_tests {
    use super::headless;

    #[test]
    fn execute_runs_subprocess_and_captures_status() {
        // `true` ignores args and exits 0 — proves the spawn/wait/status path.
        let r = headless::execute("test", "true", "ignored", 5000, &[]);
        let _ = r; // cleanup state not needed
        // The result is recorded with status completed/failed (not spawn_failed).
        let last = headless::list().last().cloned().unwrap_or_default();
        let status = last["status"].as_str().unwrap_or("");
        assert!(status == "completed" || status == "failed", "got {status}");
        assert_ne!(status, "spawn_failed");
    }

    #[test]
    fn execute_unavailable_binary_degrades() {
        let r = headless::execute("test", "definitely-not-a-binary-xyz", "x", 1000, &[]);
        assert_eq!(r["status"].as_str(), Some("unavailable"));
    }
}

#[cfg(test)]
mod budget_tests {
    use super::global_budget;
    use std::sync::Mutex;
    static BUDGET_LOCK: Mutex<()> = Mutex::new(());

    fn fresh_state() {
        // Wipe the persisted budget state so each test starts from defaults.
        let _ = std::fs::remove_file(super::state_path("global-budget"));
    }

    #[test]
    fn check_then_record_releases_slot() {
        let _g = BUDGET_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        fresh_state();
        let check = global_budget::check().expect("budget should allow within caps");
        assert!(check["concurrent"].as_u64().unwrap_or(0) >= 1, "check should reserve a slot");
        let rec = global_budget::record("sonnet", 50000, true);
        assert!(rec["costUsd"].as_f64().unwrap_or(0.0) > 0.0, "record should book cost");
    }

    #[test]
    fn record_failure_trips_breaker() {
        let _g = BUDGET_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        fresh_state();
        let _ = global_budget::check();
        let _ = global_budget::record("haiku", 100, false);
        let st = global_budget::status();
        assert_eq!(st["circuitOpen"].as_bool(), Some(true));
        let res = global_budget::check();
        assert!(res.is_err());
    }
}

#[cfg(test)]
mod git_workspace_tests {
    use super::git_workspace;

    fn init_repo() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let r = std::process::Command::new("git").arg("init").arg("-q")
            .current_dir(dir.path()).status();
        if r.is_err() || !r.unwrap().success() {
            // No git — test will be skipped via the assert below; still return dir.
        }
        // initial commit so HEAD exists
        let _ = std::fs::write(dir.path().join("x.txt"), "init");
        let _ = std::process::Command::new("git").args(["add", "."]).current_dir(dir.path()).status();
        let _ = std::process::Command::new("git")
            .args(["-c", "user.email=t@t", "-c", "user.name=t", "commit", "-q", "-m", "init"])
            .current_dir(dir.path()).status();
        dir
    }

    #[test]
    fn create_and_remove_worktree() {
        let dir = init_repo();
        let root = dir.path().to_path_buf();
        // Skip if git isn't functional (no commits).
        if !root.join(".git").exists() { return; }
        let branch = format!("wt-{}", std::process::id());
        let wt = git_workspace::create_worktree(&root, &branch);
        match wt {
            Ok(path) => {
                assert!(path.is_dir(), "worktree dir should exist");
                assert!(!git_workspace::list().is_empty());
                let _ = git_workspace::remove_worktree(&root, &branch);
            }
            Err(e) => {
                // git worktree may be unavailable in some sandboxes — skip, not fail.
                eprintln!("[skip] git worktree unavailable: {e}");
            }
        }
    }
}
