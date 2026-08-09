//! Native Rust swarm execution — spawn real agent-worker subprocesses.
//!
//! ADR-0008: `swarm start --objective --workers N` forks N agent processes
//! (claude default, `--agent codex` for codex) in parallel, each with the
//! objective + a per-worker role prompt. No Node, no MCP bridge — direct
//! `std::process::Command`. Worker stdout/stderr/exit captured + recorded into
//! the swarm state file and ruflo memory.

use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};

/// Global AI budget limits enforced by the swarm executor in addition to the
/// manual / circuit-breaker pause. Values port from services/global-ai-budget.ts.
const LIMIT_CONCURRENT: usize = 1;
const LIMIT_HOURLY: usize = 2;
const LIMIT_DAILY: usize = 12;
const HOUR_MS: u64 = 3_600_000;
const DAY_MS: u64 = 86_400_000;

/// Check if the global AI budget is paused (manual or quota-triggered circuit
/// breaker) or over one of the rate limits. Reads
/// ~/.claude-flow/ai-budget.json (same file daemon.rs manages).
/// Returns Some(reason) if paused/over-limit, None if clear.
fn check_budget_paused(_cwd: &Path) -> Option<String> {
    let budget_dir = std::env::var("RUFLO_AI_BUDGET_DIR")
        .map(PathBuf::from)
        .or_else(|_| std::env::var("RUFLO_STATE_DIR").map(PathBuf::from))
        .unwrap_or_else(|_| {
            std::env::var("HOME")
                .map(|h| PathBuf::from(h).join(".claude-flow"))
                .unwrap_or_else(|_| PathBuf::from(".claude-flow"))
        });
    let ledger_path = budget_dir.join("ai-budget.json");
    let raw = std::fs::read_to_string(&ledger_path).ok()?;
    let ledger: Value = serde_json::from_str(&raw).ok()?;
    let now = now_ms();

    // Manual / circuit-breaker pause.
    if let Some(paused_until) = ledger["pausedUntil"].as_u64().filter(|&t| t > now) {
        let _ = paused_until;
        let reason = ledger["pauseReason"]
            .as_str()
            .unwrap_or("unknown")
            .to_string();
        return Some(reason);
    }

    // Active concurrent workers.
    if let Some(active) = ledger["active"].as_array() {
        if active.len() >= LIMIT_CONCURRENT {
            return Some(format!(
                "concurrent limit reached ({active}/{limit})",
                active = active.len(),
                limit = LIMIT_CONCURRENT
            ));
        }
    }

    // Sliding-window launch counters.
    if let Some(launches) = ledger["launches"].as_array() {
        let hourly = launches
            .iter()
            .filter_map(|l| l["at"].as_u64())
            .filter(|&t| now.saturating_sub(t) <= HOUR_MS)
            .count();
        if hourly >= LIMIT_HOURLY {
            return Some(format!(
                "hourly launch limit reached ({hourly}/{limit})",
                hourly = hourly,
                limit = LIMIT_HOURLY
            ));
        }
        let daily = launches
            .iter()
            .filter_map(|l| l["at"].as_u64())
            .filter(|&t| now.saturating_sub(t) <= DAY_MS)
            .count();
        if daily >= LIMIT_DAILY {
            return Some(format!(
                "daily launch limit reached ({daily}/{limit})",
                daily = daily,
                limit = LIMIT_DAILY
            ));
        }
    }

    None
}

/// Resolve the budget ledger directory (mirrors daemon.rs::budget_dir so the
/// native swarm writes the same file the daemon reads).
fn budget_dir() -> PathBuf {
    std::env::var("RUFLO_AI_BUDGET_DIR")
        .map(PathBuf::from)
        .or_else(|_| std::env::var("RUFLO_STATE_DIR").map(PathBuf::from))
        .unwrap_or_else(|_| {
            std::env::var("HOME")
                .map(|h| PathBuf::from(h).join(".claude-flow"))
                .unwrap_or_else(|_| PathBuf::from(".claude-flow"))
        })
}

fn budget_ledger_path() -> PathBuf {
    budget_dir().join("ai-budget.json")
}

/// Read the budget ledger. Missing/malformed → empty ledger (fail-safe so a
/// corrupt file never blocks spawning).
fn read_budget_ledger() -> Value {
    let path = budget_ledger_path();
    match std::fs::read_to_string(&path) {
        Ok(s) if s.trim().is_empty() => json!({"version": 1, "launches": [], "active": []}),
        Ok(s) => serde_json::from_str(&s).unwrap_or_else(|_| {
            json!({"version": 1, "launches": [], "active": []})
        }),
        Err(_) => json!({"version": 1, "launches": [], "active": []}),
    }
}

/// Atomic write (tmp + rename) matching daemon.rs::write_ledger.
fn write_budget_ledger(v: &Value) -> bool {
    let path = budget_ledger_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let tmp = path.with_extension("json.tmp");
    let bytes = serde_json::to_vec_pretty(v).unwrap_or_default();
    if std::fs::write(&tmp, &bytes).is_err() {
        return false;
    }
    let ok = std::fs::rename(&tmp, &path).is_ok();
    if !ok {
        let _ = std::fs::remove_file(&tmp);
    }
    ok
}

/// Get-or-create a JSON array field on the ledger, returning a mutable
/// reference that callers can push into. Avoids the borrow-checker fight
/// around `unwrap_or(&mut Vec::new())` temporaries.
fn ensure_array_mut<'a>(v: &'a mut Value, key: &str) -> &'a mut Vec<Value> {
    if !v[key].is_array() {
        v[key] = json!([]);
    }
    v.get_mut(key).and_then(|x| x.as_array_mut()).expect("array just ensured")
}

/// Per-worker role slices for hierarchical-mesh topology. Worker 0 = queen
/// (coordinator); rest = specialists. Keeps prompts bounded.
fn worker_roles(n: usize) -> Vec<&'static str> {
    let base = &[
        "queen coordinator — break the objective into steps, assign to workers, integrate results",
        "coder — implement the core logic / functions",
        "tester — write tests + validate behavior",
        "reviewer — review for correctness, security, edge cases",
        "documenter — write README + inline docs",
        "architect — design the module structure + interfaces",
        "debugger — trace + fix any errors found",
        "optimizer — improve performance + clarity",
    ];
    (0..n)
        .map(|i| base[i % base.len()])
        .collect()
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Build the prompt for worker `i` of `n`.
pub fn worker_prompt(objective: &str, worker_idx: usize, n_workers: usize, role: &str) -> String {
    format!(
        "You are worker {idx} of {n} in a Ruflo swarm. Role: {role}.\n\
         Objective: {objective}\n\n\
         Do your part of this objective. Be concrete and produce real output \
         (code, files, analysis). Keep it focused on your role.",
        idx = worker_idx + 1,
        n = n_workers,
        role = role,
        objective = objective,
    )
}

/// Sanitize the environment: strip credential-like vars unless keep_env.
#[allow(dead_code)]
fn sanitized_env(keep_env: bool) -> Vec<(String, String)> {
    if keep_env {
        return std::env::vars().collect();
    }
    std::env::vars()
        .filter(|(k, _)| {
            let ku = k.to_uppercase();
            // Keep PATH, HOME, etc. Strip key/token/secret/credential patterns.
            let suspicious = ku.ends_with("_KEY")
                || ku.ends_with("_TOKEN")
                || ku.ends_with("_SECRET")
                || ku.ends_with("_PASSWORD")
                || ku.ends_with("_CREDENTIAL")
                || ku.contains("APIKEY")
                || ku.contains("AUTH")
                || ku.contains("JWT")
                || ku.contains("BEARER")
                || ku.contains("COOKIE")
                || ku.contains("SESSION")
                || ku == "TOKEN"
                || ku == "SECRET";
            !suspicious
        })
        .collect()
}

/// Resolve the agent binary. `claude` default; `codex` alternative. Returns
/// (binary, arg_builder).
fn agent_command(agent: &str, prompt: &str) -> Result<(String, Vec<String>), String> {
    match agent {
        "claude" | "claude-code" => Ok(("claude".into(), vec!["--print".into(), prompt.into()])),
        "codex" => Ok((
            "codex".into(),
            vec![
                "exec".into(),
                "--skip-git-repo-check".into(),
                prompt.into(),
            ],
        )),
        other => Err(format!(
            "Unknown agent '{other}'. Use 'claude' or 'codex'."
        )),
    }
}

/// Spawn one worker subprocess and capture its output. (See
/// spawn_worker_with_idx for the actual implementation used by run_swarm.)
/// Result of one worker.
pub struct WorkerResult {
    pub worker_idx: usize,
    pub agent: String,
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
    #[allow(dead_code)]
    pub timed_out: bool,
}

/// Run a native swarm: spawn N worker subprocesses in parallel, collect.
///
/// - `objective`: the goal.
/// - `workers`: number of workers (1..=50).
/// - `agent`: "claude" or "codex".
/// - `cwd`: working directory for workers.
/// - `keep_env`: if false, strip credential-like env vars.
/// - `dry_run`: if true, return the plan without spawning.
pub fn run_swarm(
    objective: &str,
    workers: usize,
    agent: &str,
    cwd: &Path,
    keep_env: bool,
    dry_run: bool,
) -> SwarmOutcome {
    let n = workers.clamp(1, 50);
    let roles = worker_roles(n);

    // Service: global AI budget enforcement. Check the circuit breaker before
    // spawning any AI workers. If paused (manual or quota-triggered), refuse to
    // spawn. This ports the critical enforcement path from
    // services/global-ai-budget.ts into the native swarm executor.
    if !dry_run && (agent == "claude" || agent == "codex") {
        if let Some(reason) = check_budget_paused(cwd) {
            return SwarmOutcome {
                dry_run: false,
                workers: 0,
                results: vec![WorkerResult {
                    worker_idx: 0,
                    agent: agent.into(),
                    stdout: String::new(),
                    stderr: format!("AI budget paused: {reason}. Run `ruflo daemon budget resume` to re-enable."),
                    exit_code: -1,
                    timed_out: false,
                }],
                plan: vec![json!({"blocked": "budget_paused", "reason": reason})],
            };
        }
    }

    if dry_run {
        let plan: Vec<Value> = (0..n)
            .map(|i| {
                json!({
                    "worker": i + 1,
                    "agent": agent,
                    "role": roles[i],
                    "prompt": worker_prompt(objective, i, n, roles[i]),
                })
            })
            .collect();
        return SwarmOutcome {
            dry_run: true,
            workers: n,
            results: Vec::new(),
            plan,
        };
    }

    // ADR-324: enforce the policy runtime before any subprocess is spawned.
    // If `swarm.spawn` is denied, record the decision and abort with the
    // reason rather than launching workers.
    {
        let decision = crate::services::policy_runtime::evaluate("swarm.spawn", "native");
        if decision["decision"].as_str() == Some("deny") {
            let reason = format!(
                "policy denied swarm.spawn (identity={})",
                decision["identity"].as_str().unwrap_or("native")
            );
            return SwarmOutcome {
                dry_run: false,
                workers: 0,
                results: vec![WorkerResult {
                    worker_idx: 0,
                    agent: agent.into(),
                    stdout: String::new(),
                    stderr: reason.clone(),
                    exit_code: -1,
                    timed_out: false,
                }],
                plan: vec![json!({"blocked": "policy_deny", "decision": decision})],
            };
        }
    }

    // Record launch reservations in the budget ledger so daemon.rs::status and
    // check_budget_paused observe in-flight workers (concurrent-limit gate) and
    // rate-limit counters stay accurate. Permits are removed once all workers
    // finish; launches are retained (sliding-window counters).
    let spawn_pid = std::process::id();
    let reservation_at = now_ms();
    let permits: Vec<String> = (0..n)
        .map(|i| format!("swarm-{reservation_at}-{i}"))
        .collect();
    {
        let mut ledger = read_budget_ledger();
        {
            let active = ensure_array_mut(&mut ledger, "active");
            for permit in &permits {
                active.push(json!({
                    "permitId": permit,
                    "at": reservation_at,
                    "pid": spawn_pid,
                    "workerType": agent,
                }));
            }
        }
        {
            let launches = ensure_array_mut(&mut ledger, "launches");
            for _ in 0..n {
                launches.push(json!({
                    "at": reservation_at,
                    "pid": spawn_pid,
                    "workerType": agent,
                    "model": agent,
                    "workspace": cwd.to_string_lossy(),
                }));
            }
        }
        write_budget_ledger(&ledger);
    }

    // Spawn N workers in parallel (one thread each). Collect handles first,
    // then join — calling .join() inside .map() would block each spawn until
    // the previous worker finishes, defeating parallelism.
    let handles: Vec<_> = (0..n)
        .map(|i| {
            let prompt = worker_prompt(objective, i, n, roles[i]);
            let agent_for_thread = agent.to_string();
            let cwd_owned = cwd.to_path_buf();
            std::thread::spawn(move || {
                spawn_worker_with_idx(i, &agent_for_thread, &prompt, &cwd_owned, keep_env)
            })
        })
        .collect();

    let agent_owned = agent.to_string();
    let results: Vec<WorkerResult> = handles
        .into_iter()
        .enumerate()
        .map(|(i, h)| {
            h.join().unwrap_or_else(|_| WorkerResult {
                worker_idx: i,
                agent: agent_owned.clone(),
                stdout: String::new(),
                stderr: "worker thread panicked".into(),
                exit_code: -1,
                timed_out: false,
            })
        })
        .collect();

    // Pheromone-adaptive feedback: record each worker's outcome so the
    // APSC (Adaptive Pheromone Swarm Coordinator) updates per-agent EMA
    // fitness + keep/suspend eligibility. Ports services/pheromone-adaptive.ts.
    for r in &results {
        let success = r.exit_code == 0 && !r.timed_out;
        // Latency is unknown without timing each worker; use a neutral 1.0
        // (within typical budget). The pheromone layer normalizes anyway.
        let _ = crate::services::pheromone::record(
            &format!("worker-{}", r.worker_idx),
            "worker",
            if success { 1.0 } else { 0.0 },
            1.0,
            1.0,
        );
    }

    // Release the active reservations now that every worker has finished. The
    // launches array is left intact — daemon.rs prunes entries older than 24h.
    {
        let mut ledger = read_budget_ledger();
        let active = ensure_array_mut(&mut ledger, "active");
        active.retain(|a| {
            a["permitId"]
                .as_str()
                .map(|p| !permits.iter().any(|permitted| permitted == p))
                .unwrap_or(true)
        });
        write_budget_ledger(&ledger);
    }

    let plan: Vec<Value> = (0..n)
        .map(|i| {
            json!({
                "worker": i + 1,
                "agent": agent,
                "role": roles[i],
            })
        })
        .collect();

    SwarmOutcome {
        dry_run: false,
        workers: n,
        results,
        plan,
    }
}

fn spawn_worker_with_idx(
    idx: usize,
    agent: &str,
    prompt: &str,
    cwd: &Path,
    keep_env: bool,
) -> WorkerResult {
    let (bin, args) = match agent_command(agent, prompt) {
        Ok(v) => v,
        Err(e) => {
            return WorkerResult {
                worker_idx: idx, agent: agent.into(), stdout: String::new(),
                stderr: e, exit_code: -1, timed_out: false,
            }
        }
    };
    let mut cmd = Command::new(&bin);
    cmd.args(&args).current_dir(cwd);
    // Close stdin so claude/codex don't hang waiting for interactive input.
    cmd.stdin(std::process::Stdio::null());
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());
    // Sanitize: strip credential-like env vars, keep the rest (PATH, HOME,
    // config dirs). Full env_clear breaks agent CLIs that need PATH etc.
    if !keep_env {
        for (k, _) in std::env::vars() {
            let ku = k.to_uppercase();
            let suspicious = ku.ends_with("_KEY")
                || ku.ends_with("_TOKEN")
                || ku.ends_with("_SECRET")
                || ku.ends_with("_PASSWORD")
                || ku.ends_with("_CREDENTIAL")
                || ku.contains("APIKEY")
                || ku.contains("AUTH")
                || ku.contains("JWT")
                || ku.contains("BEARER")
                || ku.contains("COOKIE")
                || ku.contains("SESSION")
                || ku == "TOKEN"
                || ku == "SECRET";
            if suspicious {
                cmd.env_remove(&k);
            }
        }
    }
    // Spawn the child and enforce a deadline (300s default) via a watchdog
    // thread that kills the process if it exceeds the timeout.
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            return WorkerResult {
                worker_idx: idx, agent: agent.into(), stdout: String::new(),
                stderr: format!("failed to spawn '{bin}': {e}"), exit_code: -1, timed_out: false,
            }
        }
    };
    let timeout_ms = std::env::var("RUFLO_WORKER_TIMEOUT_MS")
        .ok().and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(300_000);
    let deadline = now_ms() + timeout_ms;

    // Drain stdout/stderr in background threads so a chatty child can't fill
    // the OS pipe buffer (64 KB on Linux) and block forever while we poll.
    let stdout_handle = child.stdout.take();
    let stderr_handle = child.stderr.take();
    let stdout_buf: Arc<Mutex<Vec<u8>>> = Arc::new(Mutex::new(Vec::new()));
    let stderr_buf: Arc<Mutex<Vec<u8>>> = Arc::new(Mutex::new(Vec::new()));
    let so_buf = stdout_buf.clone();
    let stdout_drain = std::thread::spawn(move || {
        if let Some(mut h) = stdout_handle {
            let _ = h.read_to_end(&mut *so_buf.lock().unwrap());
        }
    });
    let se_buf = stderr_buf.clone();
    let stderr_drain = std::thread::spawn(move || {
        if let Some(mut h) = stderr_handle {
            let _ = h.read_to_end(&mut *se_buf.lock().unwrap());
        }
    });

    // Share the owned Child handle between the polling main thread and the
    // watchdog. Using Child::kill() on the owned handle (rather than a bare
    // libc_kill(pid, 9)) is immune to PID recycling — the OS handle keeps
    // referring to the original process even after it exits.
    let child_shared: Arc<Mutex<Option<Child>>> = Arc::new(Mutex::new(Some(child)));
    let done = Arc::new(AtomicBool::new(false));

    let wc = child_shared.clone();
    let wd = done.clone();
    let watchdog = std::thread::spawn(move || {
        // Sleep in small slices so we observe `done` quickly once the worker
        // exits — no point blocking for the full timeout after success.
        while now_ms() < deadline {
            if wd.load(Ordering::Relaxed) {
                return; // worker finished, no kill needed
            }
            std::thread::sleep(std::time::Duration::from_millis(200));
        }
        // Final check closes the race between deadline expiry and the main
        // thread setting `done` after observing the exit.
        if wd.load(Ordering::Relaxed) {
            return;
        }
        // Timeout — take the owned handle and kill it (reaps the zombie via the
        // follow-up wait). Safe: handle identity, not recycled PID.
        if let Some(mut c) = wc.lock().unwrap().take() {
            let _ = c.kill();
            let _ = c.wait();
        }
    });

    // Main thread: poll try_wait until the child exits or the watchdog reaps
    // it on timeout. Holding the mutex only across try_wait keeps contention
    // with the watchdog brief.
    let mut timed_out = false;
    let mut exit_status: Option<ExitStatus> = None;
    loop {
        let mut guard = child_shared.lock().unwrap();
        match guard.as_mut() {
            None => {
                // Watchdog already took the child for kill.
                timed_out = true;
                drop(guard);
                break;
            }
            Some(c) => match c.try_wait() {
                Ok(Some(status)) => {
                    exit_status = Some(status);
                    drop(guard);
                    break;
                }
                Ok(None) => {}
                Err(_) => {
                    drop(guard);
                    break;
                }
            },
        }
        drop(guard);
        std::thread::sleep(std::time::Duration::from_millis(100));
    }

    // Signal the watchdog to exit early (it may still be sleeping).
    done.store(true, Ordering::Relaxed);
    let _ = watchdog.join();
    let _ = stdout_drain.join();
    let _ = stderr_drain.join();

    // If we still own the child (normal exit), reap it for a definitive exit
    // code. If the watchdog took it (timeout), exit_code stays -1.
    let mut exit_code = exit_status.and_then(|s| s.code()).unwrap_or(-1);
    if let Some(mut c) = child_shared.lock().unwrap().take() {
        if let Ok(status) = c.wait() {
            exit_code = status.code().unwrap_or(exit_code);
        }
    }

    let stdout_str: String = String::from_utf8_lossy(&stdout_buf.lock().unwrap())
        .chars()
        .take(1_000_000)
        .collect();
    let stderr_str: String = String::from_utf8_lossy(&stderr_buf.lock().unwrap())
        .chars()
        .take(100_000)
        .collect();

    WorkerResult {
        worker_idx: idx,
        agent: agent.into(),
        stdout: stdout_str,
        stderr: stderr_str,
        exit_code: if timed_out { -1 } else { exit_code },
        timed_out,
    }
}

pub struct SwarmOutcome {
    pub dry_run: bool,
    pub workers: usize,
    pub results: Vec<WorkerResult>,
    pub plan: Vec<Value>,
}

impl SwarmOutcome {
    /// Render a human-readable summary.
    pub fn summary(&self) -> String {
        if self.dry_run {
            let mut s = format!("[dry-run] Swarm plan: {} workers\n", self.workers);
            for p in &self.plan {
                s.push_str(&format!(
                    "  worker {}: {} ({})\n",
                    p["worker"], p["agent"], p["role"]
                ));
            }
            return s;
        }
        let succeeded = self.results.iter().filter(|r| r.exit_code == 0).count();
        let mut s = format!(
            "\nSwarm complete: {}/{} workers succeeded\n",
            succeeded,
            self.results.len()
        );
        for r in &self.results {
            let status = if r.exit_code == 0 { "OK" } else { "FAIL" };
            let preview: String = r.stdout.chars().take(120).collect();
            s.push_str(&format!(
                "  worker {} [{}] exit={} :: {}\n",
                r.worker_idx + 1,
                status,
                r.exit_code,
                preview.replace('\n', " ")
            ));
            if !r.stderr.is_empty() && r.exit_code != 0 {
                let err_preview: String = r.stderr.chars().take(100).collect();
                s.push_str(&format!("    stderr: {}\n", err_preview.replace('\n', " ")));
            }
        }
        s
    }

    /// Record results into ruflo memory (one entry per worker).
    pub fn record_to_memory(&self, root: &Path) {
        let db = root.join(".swarm/memory.db");
        let _ = std::fs::create_dir_all(db.parent().unwrap_or(root));
        for r in &self.results {
            let key = format!("swarm-worker-{}-{}", r.worker_idx + 1, now_ms());
            let val = json!({
                "worker": r.worker_idx + 1,
                "agent": r.agent,
                "exit": r.exit_code,
                "stdout_head": r.stdout.chars().take(500).collect::<String>(),
            });
            let _ = Command::new(std::env::current_exe().unwrap_or_else(|_| PathBuf::from("ruflo")))
                .args([
                    "memory", "store",
                    "--key", &key,
                    "--value", &val.to_string(),
                    "--path", db.to_str().unwrap_or(".swarm/memory.db"),
                    "--namespace", "swarm",
                ])
                .current_dir(root)
                .output();
        }
    }
}

// Suppress unused warnings for helpers retained for future use.
#[allow(dead_code)]
fn _unused(_a: &Arc<Mutex<HashMap<String, String>>>, _b: ExitStatus) {}

/// Work-stealing: when a worker is idle, find a stealable claim (an issue
/// another agent marked available) and take it. Returns the stolen issue id
/// or None. Ports the work-stealing half of services/claim-service.ts into
/// the swarm path.
pub fn steal_work(stealer: &str) -> Option<String> {
    let stealable = crate::services::claim_service::stealable(None).ok()?;
    let issue = stealable.first()?.clone();
    let issue_id = issue["issueId"].as_str().or(issue["id"].as_str())?.to_string();
    match crate::services::claim_service::steal(&issue_id, stealer, "worker") {
        Ok(_) => Some(issue_id),
        Err(_) => None,
    }
}

/// Pheromone snapshot: the current per-agent EMA fitness + eligibility the
/// APSC uses to keep/suspend workers. Exposed for swarm status display.
pub fn pheromone_snapshot() -> Value {
    let snap = crate::services::pheromone::get_state();
    let eligible = crate::services::pheromone::eligible();
    json!({
        "agents": snap.get("agents").cloned().unwrap_or(json!({})),
        "eligible": eligible,
        "threshold": snap.get("threshold").cloned().unwrap_or(json!(null)),
    })
}

#[cfg(test)]
mod steal_tests {
    use super::*;

    #[test]
    fn steal_work_returns_none_when_nothing_stealable() {
        // No claims state → stealable() returns empty → None.
        assert!(steal_work("test-stealer").is_none());
    }

    #[test]
    fn pheromone_snapshot_returns_object() {
        let snap = pheromone_snapshot();
        assert!(snap.is_object());
        assert!(snap["eligible"].is_array());
    }
}
