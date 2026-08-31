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
    if !v[key].is_array() {
        v[key] = json!([]);  // reset non-array/non-null to empty array (#8: was .expect)
    }
    v[key].as_array_mut().expect("array just ensured")
}

// Pub wrappers for cross-module access (flywheel_ledger.rs)
pub fn read_state_pub(name: &str) -> Value { read_state(name) }
pub fn write_state_pub(name: &str, v: &Value) -> bool { write_state(name, v) }
pub fn ensure_arr_pub<'a>(v: &'a mut Value, key: &str) -> &'a mut Vec<Value> { ensure_arr(v, key) }

// ============================================================================ //
// WORKER MANAGEMENT
// ============================================================================ //

/// Bounded worker pool — concurrency cap with active/in-flight tracking.
/// Ports services/bounded-worker-pool.ts (108 lines).

pub mod bounded_pool;
pub mod worker_queue;
pub mod worker_daemon;
pub mod headless;
pub mod container_pool;
pub mod git_workspace;
pub mod pheromone;
pub mod swarm_branches;
pub mod learned_routing;
pub mod autostart;
pub mod dedup;
pub mod backup;
pub mod distillation;
pub mod lease;
pub mod checkpoint;
pub mod supervisor;
pub mod policy_runtime;
pub mod global_budget;
pub mod harness;
pub mod fable;
pub mod evolve;
pub mod weight_eft;
pub mod flywheel_receipt;
pub mod flywheel_tx;
pub mod flywheel_proposer;
pub mod distill_oracle;
pub mod distill_tuning;
pub mod native_training;
pub mod ruvector_training;
pub mod agentic_bridge;
pub mod registry;
pub mod claim_service;
pub mod evolve_proof_v2;
pub mod flywheel_tx_v2;
pub mod weight_eft_v2;
pub mod policy_runtime_v2;
pub mod learned_routing_v2;
pub mod fable_v2;
pub mod flywheel_proposer_v2;
pub mod ruvector_training_v2;
pub mod pheromone_v2;
pub mod worker_daemon_v2;
pub mod swarm_branches_v2;
pub mod checkpoint_v2;
pub mod git_identity_v2;

#[cfg(test)]
mod tests;
#[cfg(test)]
mod headless_tests;
#[cfg(test)]
mod budget_tests;
#[cfg(test)]
mod git_workspace_tests;
