//! Native V3 `daemon` command — background worker daemon lifecycle + budget.
//!
//! Source: `v3/@claude-flow/cli/src/commands/daemon.ts` and the budget service
//! `v3/@claude-flow/cli/src/services/global-ai-budget.ts`. Subcommands:
//! start / stop / status / trigger / enable / budget(show|pause|resume) /
//! install-supervisor / uninstall-supervisor.
//!
//! The actual worker event-loop is Node-based (ADR-0005: no JS runtime in the
//! native build), so `start` cannot fork that loop. What native CAN do — and
//! does here — is manage the SAME state files the daemon uses, so:
//!   - `status` / `stop` reflect and control a Node-started daemon,
//!   - `budget show|pause|resume` is fully functional (atomic ledger mutation
//!     with the same file layout, limits, and circuit-breaker semantics),
//!   - `start` / `trigger` / `enable` record real state honestly and degrade
//!     on the worker-execution step.
//!
//! Budget state lives at `~/.claude-flow/ai-budget.json` (override
//! `RUFLO_AI_BUDGET_DIR`), matching the TS service exactly so a native pause is
//! honored by any daemon instance.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

// Portable private-file opens: 0600 on Unix (via OpenOptionsExt), default ACLs
// elsewhere. Keeps the documented Windows target compiling.
#[cfg(unix)]
fn open_create_new_private(path: &Path) -> std::io::Result<std::fs::File> {
    use std::os::unix::fs::OpenOptionsExt;
    std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .mode(0o600)
        .open(path)
}
#[cfg(not(unix))]
fn open_create_new_private(path: &Path) -> std::io::Result<std::fs::File> {
    std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(path)
}
#[cfg(unix)]
fn open_append_private(path: &Path) -> std::io::Result<std::fs::File> {
    use std::os::unix::fs::OpenOptionsExt;
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .mode(0o600)
        .open(path)
}
#[cfg(not(unix))]
fn open_append_private(path: &Path) -> std::io::Result<std::fs::File> {
    std::fs::OpenOptions::new().create(true).append(true).open(path)
}

use serde_json::{json, Value};

const DEFAULT_WORKERS: &[&str] = &["map", "audit", "optimize", "consolidate", "testgaps"];
const LIMIT_CONCURRENT: u64 = 1;
const LIMIT_PER_HOUR: u64 = 2;
const LIMIT_PER_DAY: u64 = 12;
/// Cooldown for quota-error circuit-breaker pauses (TS service default). Native
/// build does not run workers, so quota errors are never recorded here; kept for
/// parity documentation.
#[allow(dead_code)]
const QUOTA_PAUSE_MINUTES: u64 = 60;
const HOUR_MS: u64 = 60 * 60 * 1000;
const DAY_MS: u64 = 24 * HOUR_MS;
const ACTIVE_STALE_MS: u64 = 30 * 60 * 1000;
const LOCK_STALE_MS: u64 = 10_000;
const MANUAL_PAUSE_SENTINEL: u64 = 9_000_000_000_000;

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn budget_dir() -> PathBuf {
    if let Ok(d) = std::env::var("RUFLO_AI_BUDGET_DIR") {
        return PathBuf::from(d);
    }
    if let Ok(d) = std::env::var("RUFLO_STATE_DIR") {
        return PathBuf::from(d);
    }
    home_dir().join(".claude-flow")
}

fn home_dir() -> PathBuf {
    std::env::var("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."))
}

fn ledger_file() -> PathBuf {
    budget_dir().join("ai-budget.json")
}

fn lock_file() -> PathBuf {
    budget_dir().join("ai-budget.lock")
}

fn receipts_file() -> PathBuf {
    budget_dir().join("ai-budget-receipts.jsonl")
}

fn ensure_budget_dir() {
    let _ = fs::create_dir_all(budget_dir());
}

/// Acquire the O_EXCL mutation lock the TS service uses. Returns a guard that
/// releases on drop. Stale locks (>LOCK_STALE_MS old) are taken over, matching
/// the TS stale-lock recovery.
struct LockGuard;
impl LockGuard {
    fn acquire() -> Option<Self> {
        ensure_budget_dir();
        let deadline = now_ms() + 2000;
        loop {
            let res = open_create_new_private(&lock_file());
            match res {
                Ok(mut f) => {
                    let _ = f.write_all(std::process::id().to_string().as_bytes());
                    return Some(LockGuard);
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                    // Stale lock from a crashed process — take over.
                    if let Ok(meta) = fs::metadata(lock_file()) {
                        if let Ok(mtime) = meta.modified() {
                            let age = SystemTime::now()
                                .duration_since(mtime)
                                .map(|d| d.as_millis() as u64)
                                .unwrap_or(0);
                            if age > LOCK_STALE_MS {
                                let _ = fs::remove_file(lock_file());
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
impl Drop for LockGuard {
    fn drop(&mut self) {
        let _ = fs::remove_file(lock_file());
    }
}

fn empty_ledger() -> Value {
    json!({"version": 1, "launches": [], "active": []})
}

/// Read the budget ledger. A missing file is a fresh (empty) ledger; a
/// MALFORMED existing file is a hard error — falling back to empty would
/// silently reopen the circuit breaker and reset usage counters (fail-open).
fn read_ledger() -> Result<Value, String> {
    let path = ledger_file();
    match fs::read_to_string(&path) {
        Ok(s) if s.trim().is_empty() => Ok(empty_ledger()),
        Ok(s) => serde_json::from_str::<Value>(&s)
            .map_err(|e| format!("budget ledger at {} is malformed: {e}", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(empty_ledger()),
        Err(e) => Err(format!("cannot read budget ledger at {}: {e}", path.display())),
    }
}

fn write_ledger(v: &Value) -> bool {
    ensure_budget_dir();
    let tmp = ledger_file().with_extension("json.tmp");
    let bytes = serde_json::to_vec_pretty(v).unwrap_or_default();
    if fs::write(&tmp, &bytes).is_err() {
        return false;
    }
    let ok = fs::rename(&tmp, ledger_file()).is_ok();
    if !ok {
        let _ = fs::remove_file(&tmp);
    }
    ok
}

fn append_receipt(rec: Value) {
    ensure_budget_dir();
    if let Ok(mut f) = open_append_private(&receipts_file()) {
        let _ = writeln!(f, "{}", rec);
    }
}

fn prune_ledger(mut ledger: Value, now: u64) -> Value {
    // Drop launches older than 24h; drop active reservations older than the
    // stale window (crashed daemon). Mutates in place.
    if let Some(launches) = ledger["launches"].as_array_mut() {
        launches.retain(|l| {
            let at = l["at"].as_u64().unwrap_or(0);
            now.saturating_sub(at) < DAY_MS
        });
    }
    if let Some(active) = ledger["active"].as_array_mut() {
        active.retain(|a| {
            let at = a["at"].as_u64().unwrap_or(0);
            now.saturating_sub(at) < ACTIVE_STALE_MS
        });
    }
    ledger
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DaemonCommand {
    pub operation: String,
    pub sub: Option<String>,
    pub workers: Option<String>,
    pub background: bool,
    pub foreground: bool,
    pub headless: bool,
    pub ttl: Option<u64>,
    pub all: bool,
    pub verbose: bool,
    pub show_modes: bool,
    pub worker: Option<String>,
    pub reason: Option<String>,
    pub disable: bool,
    pub quiet: bool,
}

pub fn run(root: &Path, command: DaemonCommand) -> u8 {
    match command.operation.as_str() {
        "" | "help" => overview(&command),
        "start" => start(root, &command),
        "stop" => stop(root, &command),
        "status" => status(root, &command),
        "trigger" => trigger(root, &command),
        "enable" => enable(root, &command),
        "budget" => budget(&command),
        "install-supervisor" => install_supervisor(root, &command),
        "uninstall-supervisor" => uninstall_supervisor(root, &command),
        _ => {
            eprintln!(
                "[ERROR] Unknown: {} (start|stop|status|trigger|enable|budget|install-supervisor|uninstall-supervisor)",
                command.operation
            );
            1
        }
    }
}

fn overview(_command: &DaemonCommand) -> u8 {
    print!(r####"
RuFlo Daemon - Background Task Management

Node.js-based background worker system that auto-runs like shell daemons.
Manages 12 specialized workers for continuous optimization and monitoring.

Headless Mode
Workers can run in headless mode using E2B sandboxes for isolated execution.
Use --headless flag with start/trigger commands. Sandbox modes: strict, permissive, disabled.

Available Workers
  - map         - Codebase mapping (5 min interval)
  - audit       - Security analysis (10 min interval)
  - optimize    - Performance optimization (15 min interval)
  - consolidate - Memory distillation: memory_entries -> episodes/reasoning_patterns/causal_edges (30 min interval, ADR-174; --no-distill to disable)
  - testgaps    - Test coverage analysis (20 min interval)
  - predict     - Predictive preloading (2 min, disabled by default)
  - document    - Auto-documentation (60 min, disabled by default)
  - ultralearn  - Deep knowledge acquisition (manual trigger)
  - refactor    - Code refactoring suggestions (manual trigger)
  - benchmark   - Performance benchmarking (manual trigger)
  - deepdive    - Deep code analysis (manual trigger)
  - preload     - Resource preloading (manual trigger)

Subcommands
  - start   - Start the daemon
  - stop    - Stop the daemon
  - status  - Show daemon status
  - trigger - Manually run a worker
  - enable  - Enable/disable a worker

Run "claude-flow daemon <subcommand> --help" for details
"####);
    0
}

// ---- per-project daemon state ----------------------------------------------

fn daemon_state_file(root: &Path) -> PathBuf {
    root.join(".claude-flow/daemon-state.json")
}

fn read_daemon_state(root: &Path) -> Value {
    fs::read_to_string(daemon_state_file(root))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_else(|| json!({}))
}

fn write_daemon_state(root: &Path, v: &Value) -> bool {
    let dir = root.join(".claude-flow");
    let _ = fs::create_dir_all(&dir);
    let path = daemon_state_file(root);
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

#[cfg(unix)]
fn is_process_running(pid: u32) -> bool {
    // kill(pid, 0) returns Ok if the process exists (or we lack permission to
    // signal it — still "exists"). Either way it's running.
    libc_kill(pid, 0) == 0
}

#[cfg(not(unix))]
fn is_process_running(_pid: u32) -> bool {
    // No portable pid-liveness probe on Windows without a crate; conservatively
    // report not-running so status reflects that the native-managed daemon
    // isn't a live background process here.
    false
}

// Minimal libc kill(2) binding for pid liveness — avoids pulling a crate for
// one syscall. Unix-only: the documented Windows target uses a different
// process model and does not signal via kill(2).
#[cfg(unix)]
unsafe extern "C" {
    fn kill(pid: u32, sig: i32) -> i32;
}
#[cfg(unix)]
pub fn libc_kill(pid: u32, sig: i32) -> i32 {
    // Safety: kill(2) is a sound libc syscall; pid/sig are plain integers.
    unsafe { kill(pid, sig) }
}
#[cfg(not(unix))]
fn libc_kill(_pid: u32, _sig: i32) -> i32 {
    -1
}

// ---- start ------------------------------------------------------------------

fn start(root: &Path, command: &DaemonCommand) -> u8 {
    let workers: Vec<String> = command
        .workers
        .as_deref()
        .map(|w| w.split(',').map(str::trim).filter(|s| !s.is_empty()).map(str::to_string).collect())
        .unwrap_or_else(|| DEFAULT_WORKERS.iter().map(|s| s.to_string()).collect());

    let ttl_ms = command.ttl.unwrap_or(12 * 60 * 60 * 1000); // 12h default
    // Don't claim a running PID — native start doesn't spawn a worker loop.
    // Record config + intent only. The PID field is omitted so stop/status
    // don't signal a short-lived CLI process that may have been reused.
    let state = json!({
        "startedAt": now_ms(),
        "running": false,
        "foreground": command.foreground,
        "config": {
            "workers": workers.iter().map(|w| json!({"type": w, "enabled": true})).collect::<Vec<_>>(),
            "maxConcurrent": LIMIT_CONCURRENT,
            "ttlMs": ttl_ms,
            "aiWorkersEnabled": command.headless,
            "resourceThresholds": {
                "maxCpuLoad": 4.0,
                "minFreeMemoryPercent": 15,
            },
        },
        "nativeManaged": true,
    });
    if !write_daemon_state(root, &state) {
        eprintln!("[ERROR] Failed to write daemon state.");
        return 1;
    }

    if command.quiet {
        return 0;
    }
    println!("\nRuFlo Daemon");
    println!("  Status:  \u{25cf} state recorded");
    println!("  PID:     {}", std::process::id());
    println!("  TTL:     {}h", if ttl_ms > 0 { ttl_ms / HOUR_MS } else { 0 });
    println!("  Workers: {}", workers.join(", "));
    println!("  AI:      {}", if command.headless { "enabled (budget-capped)" } else { "off (local-only)" });
    println!();
    eprintln!("[WARN] The worker event-loop is Node-based (ADR-0005). Native start");
    eprintln!("       records daemon state (status/stop/enable work) but does not spawn");
    eprintln!("       the worker loop. Run `ruflo daemon start` for live workers.");
    0
}

// ---- stop -------------------------------------------------------------------

fn stop(root: &Path, command: &DaemonCommand) -> u8 {
    if command.all {
        return stop_all();
    }
    let state = read_daemon_state(root);
    let pid = state["pid"].as_u64().map(|p| p as u32);
    let mut stopped = false;
    if let Some(pid) = pid {
        if is_process_running(pid) {
            // SIGTERM for graceful shutdown.
            libc_kill(pid, 15);
            stopped = true;
        }
    }
    // Mark stopped in state regardless.
    let mut next = state.clone();
    next["running"] = json!(false);
    next["stoppedAt"] = json!(now_ms());
    let _ = write_daemon_state(root, &next);
    if command.quiet {
        return 0;
    }
    if stopped {
        println!("Daemon (pid {:?}) stopped.", pid);
    } else {
        println!("No running daemon found in this workspace (state marked stopped).");
    }
    0
}

fn stop_all() -> u8 {
    // Enumerate running ruflo/claude-flow daemon processes and SIGTERM them.
    // /proc is Linux-specific; on other platforms there is no portable
    // enumeration, so report zero and let the per-workspace state drive stops.
    //
    // We must never kill our own ancestry: the ruflo process running this code
    // is itself launched from a shell/test-runner whose cmdline often contains
    // "ruflo" (via the repo path) and "daemon" (via the subcommand arg or the
    // parent binary name). Collect the full ancestor PID set and skip them.
    let ancestors = ancestor_pids();
    let mut killed = 0u32;
    #[cfg(unix)]
    {
        if let Ok(entries) = fs::read_dir("/proc") {
            for e in entries.flatten() {
                let name = e.file_name();
                let name = name.to_string_lossy();
                let Ok(pid) = name.parse::<u32>() else { continue };
                if pid == std::process::id() || ancestors.contains(&pid) {
                    continue;
                }
                if let Ok(cmdline) = fs::read_to_string(e.path().join("cmdline")) {
                    let cmd = cmdline.replace('\0', " ");
                    if (cmd.contains("ruflo") || cmd.contains("claude-flow")) && cmd.contains("daemon") {
                        libc_kill(pid, 15);
                        killed += 1;
                    }
                }
            }
        }
    }
    println!("Stopped {killed} daemon process(es) across all workspaces.");
    0
}

/// Walk `/proc/<pid>/stat` from the current process up through every parent,
/// returning the set of ancestor PIDs (excluding self). Used by `stop_all` to
/// avoid killing the invoking shell, test runner, or any other parent that
/// legitimately has "ruflo"+"daemon" in its cmdline.
#[cfg(unix)]
fn ancestor_pids() -> std::collections::HashSet<u32> {
    let mut set = std::collections::HashSet::new();
    let mut cur = std::process::id();
    // Bounded walk: a few hundred hops max is plenty for any real process tree.
    for _ in 0..512 {
        let Ok(stat) = fs::read_to_string(format!("/proc/{cur}/stat")) else { break };
        // /proc/<pid>/stat is "pid (comm) state ppid ...". comm may contain
        // spaces/parens, so find the last ')' and parse after it.
        let Some(after_comm) = stat.rfind(')') else { break };
        let mut fields = stat[after_comm + 1..].split_whitespace();
        fields.next(); // state
        let Some(ppid_str) = fields.next() else { break };
        let Ok(ppid) = ppid_str.parse::<u32>() else { break };
        if ppid == 0 || ppid == 1 || ppid == cur || set.contains(&ppid) {
            break;
        }
        set.insert(ppid);
        cur = ppid;
    }
    set
}

#[cfg(not(unix))]
fn ancestor_pids() -> std::collections::HashSet<u32> {
    std::collections::HashSet::new()
}

// ---- status -----------------------------------------------------------------

fn status(root: &Path, command: &DaemonCommand) -> u8 {
    if command.all {
        return status_all();
    }
    let state = read_daemon_state(root);
    let pid = state["pid"].as_u64().map(|p| p as u32);
    let running_flag = state["running"].as_bool().unwrap_or(false);
    let alive = pid.map(is_process_running).unwrap_or(false);
    let is_running = running_flag && alive;
    let started_at = state["startedAt"].as_u64().unwrap_or(0);
    let ttl_ms = state["config"]["ttlMs"].as_u64().unwrap_or(0);
    let ai_enabled = state["config"]["aiWorkersEnabled"].as_bool().unwrap_or(false);
    let workers = state["config"]["workers"].as_array().cloned().unwrap_or_default();
    let enabled_count = workers.iter().filter(|w| w["enabled"].as_bool().unwrap_or(false)).count();
    let max_concurrent = state["config"]["maxConcurrent"].as_u64().unwrap_or(LIMIT_CONCURRENT);
    let max_cpu = state["config"]["resourceThresholds"]["maxCpuLoad"].as_f64().unwrap_or(4.0);
    let min_mem = state["config"]["resourceThresholds"]["minFreeMemoryPercent"].as_u64().unwrap_or(15);

    let icon = if is_running { "\u{25cf}" } else { "\u{25cb}" };
    let st = if is_running { "RUNNING" } else { "STOPPED" };
    println!("\n\u{256d} RuFlo Daemon \u{256e}");
    println!("  Status: {icon} {st}");
    println!("  PID:    {:?}", pid);
    if started_at > 0 {
        println!("  Started: {}", fmt_iso(started_at));
    }
    if ttl_ms > 0 {
        println!("  TTL: {}h (self-shutdown)", ttl_ms / HOUR_MS);
    } else {
        println!("  TTL: off (runs until stopped)");
    }
    println!(
        "  AI Workers: {}",
        if ai_enabled { "enabled (budget-capped)" } else { "off (local-only, default)" }
    );
    println!("  Workers Enabled: {enabled_count}");
    println!("  Max Concurrent: {max_concurrent}");
    println!("  Max CPU Load: {max_cpu}");
    println!("  Min Free Memory: {min_mem}%");

    println!("\nWorker Status");
    println!("  {:<14} {:<8} Status", "Type", "Enabled");
    println!("  {} {} {}", "\u{2500}".repeat(14), "\u{2500}".repeat(8), "\u{2500}".repeat(12));
    for w in &workers {
        let t = w["type"].as_str().unwrap_or("?");
        let en = w["enabled"].as_bool().unwrap_or(false);
        let st = if en { "idle" } else { "disabled" };
        println!("  {:<14} {:<8} {}", t, if en { "\u{2713}" } else { "\u{25cb}" }, st);
    }

    if command.verbose || command.show_modes {
        println!("\nExecution Modes");
        println!("  Native build: workers are local-only (no Node event-loop).");
    }
    0
}

fn status_all() -> u8 {
    println!("\n\u{256d} Daemons Across All Workspaces \u{256e}");
    let mut count = 0u32;
    #[cfg(unix)]
    {
        if let Ok(entries) = fs::read_dir("/proc") {
            for e in entries.flatten() {
                let name = e.file_name();
                let name = name.to_string_lossy();
                let Ok(pid) = name.parse::<u32>() else { continue };
                if let Ok(cmdline) = fs::read_to_string(e.path().join("cmdline")) {
                    let cmd = cmdline.replace('\0', " ");
                    if (cmd.contains("ruflo") || cmd.contains("claude-flow")) && cmd.contains("daemon") {
                        let cwd = fs::read_link(e.path().join("cwd")).ok();
                        count += 1;
                        println!("  pid {pid:<7} \u{25cf} {}", cwd.map(|c| c.display().to_string()).unwrap_or_else(|| "?".into()));
                    }
                }
            }
        }
    }
    #[cfg(not(unix))]
    {
        let _ = &mut count;
        println!("  (cross-workspace daemon enumeration is Linux/proc-only)");
    }
    if count == 0 {
        println!("  No ruflo daemons running.");
    }
    0
}

// ---- trigger ----------------------------------------------------------------

fn trigger(root: &Path, command: &DaemonCommand) -> u8 {
    let worker = command.worker.as_deref().unwrap_or("all");
    let marker = root.join(".claude-flow/daemon-triggers.jsonl");
    if fs::create_dir_all(marker.parent().unwrap_or(Path::new("."))).is_err() {
        eprintln!("[ERROR] Failed to create trigger dir.");
        return 1;
    }
    let rec = json!({"worker": worker, "at": now_ms(), "nativeRecorded": true});
    match open_append_private(&marker).and_then(|mut f| writeln!(f, "{}", rec).map(|_| f)) {
        Ok(_) => {}
        Err(e) => {
            eprintln!("[ERROR] Failed to record trigger: {e}");
            return 1;
        }
    }
    println!("Trigger recorded for worker '{worker}'.");
    // Execute the worker tick natively (worker_daemon_v2 behavioral).
    let tick = crate::services::worker_daemon_v2::tick(worker);
    println!("Worker tick: {}", tick["status"].as_str().unwrap_or("done"));
    0
}

// ---- enable -----------------------------------------------------------------

fn enable(root: &Path, command: &DaemonCommand) -> u8 {
    let Some(worker) = &command.worker else {
        eprintln!("[ERROR] --worker <type> is required");
        return 1;
    };
    let turn_on = !command.disable;
    let mut state = read_daemon_state(root);
    let workers = state["config"]["workers"].as_array_mut();
    let mut found = false;
    if let Some(arr) = workers {
        for w in arr.iter_mut() {
            if w["type"].as_str() == Some(worker.as_str()) {
                w["enabled"] = json!(turn_on);
                found = true;
                break;
            }
        }
        if !found {
            arr.push(json!({"type": worker, "enabled": turn_on}));
        }
    } else {
        state["config"]["workers"] = json!([{"type": worker, "enabled": turn_on}]);
    }
    if !write_daemon_state(root, &state) {
        eprintln!("[ERROR] Failed to update daemon state.");
        return 1;
    }
    println!("Worker '{worker}' {}.", if turn_on { "enabled" } else { "disabled" });
    0
}

// ---- budget -----------------------------------------------------------------

fn budget(command: &DaemonCommand) -> u8 {
    match command.sub.as_deref() {
        None | Some("show") => budget_show(),
        Some("pause") => budget_pause(command.reason.as_deref()),
        Some("resume") => budget_resume(),
        Some(other) => {
            eprintln!("[ERROR] Unknown budget op: {other} (show|pause|resume)");
            1
        }
    }
}

fn budget_show() -> u8 {
    let _g = match LockGuard::acquire() {
        Some(g) => g,
        None => {
            eprintln!("[ERROR] Could not acquire budget lock (another daemon mutating?).");
            return 1;
        }
    };
    let ledger = match read_ledger() {
        Ok(v) => prune_ledger(v, now_ms()),
        Err(e) => {
            eprintln!("[ERROR] {e}");
            return 1;
        }
    };
    let now = now_ms();
    let launches = ledger["launches"].as_array();
    let last_hour = launches
        .map(|a| a.iter().filter(|l| now.saturating_sub(l["at"].as_u64().unwrap_or(0)) < HOUR_MS).count())
        .unwrap_or(0) as u64;
    let last_day = launches.map(|a| a.len()).unwrap_or(0) as u64;
    let active = ledger["active"].as_array().map(|a| a.len()).unwrap_or(0) as u64;
    let paused_until = ledger["pausedUntil"].as_u64().filter(|&t| t > now);
    let pause_reason = ledger["pauseReason"].as_str();

    // by-workspace (24h)
    let mut by_ws: std::collections::BTreeMap<String, u64> = std::collections::BTreeMap::new();
    if let Some(arr) = launches {
        for l in arr {
            if let Some(ws) = l["workspace"].as_str() {
                *by_ws.entry(ws.to_string()).or_insert(0) += 1;
            }
        }
    }
    let mut by_ws_vec: Vec<(String, u64)> = by_ws.into_iter().collect();
    by_ws_vec.sort_by(|a, b| b.1.cmp(&a.1));

    println!("\n\u{256d} Global AI Budget \u{256e}");
    println!("  Launches (last hour): {last_hour}/{}", LIMIT_PER_HOUR);
    println!("  Launches (last 24h):  {last_day}/{}", LIMIT_PER_DAY);
    println!("  Active Claude children: {active}/{}", LIMIT_CONCURRENT);
    if let Some(until) = paused_until {
        println!("  PAUSED until {} ({})", fmt_iso(until), pause_reason.unwrap_or("quota error"));
    } else {
        println!("  Circuit breaker: closed (normal)");
    }
    if !by_ws_vec.is_empty() {
        println!("\n  Launches by workspace (24h):");
        for (ws, n) in by_ws_vec.iter().take(10) {
            println!("    {n}x {ws}");
        }
    }
    0
}

fn budget_pause(reason: Option<&str>) -> u8 {
    let _g = match LockGuard::acquire() {
        Some(g) => g,
        None => {
            eprintln!("[ERROR] Could not acquire budget lock.");
            return 1;
        }
    };
    let now = now_ms();
    let mut ledger = match read_ledger() {
        Ok(v) => prune_ledger(v, now),
        Err(e) => {
            eprintln!("[ERROR] {e}");
            return 1;
        }
    };
    let r = reason.unwrap_or("manual pause (ruflo daemon budget pause)");
    ledger["pausedUntil"] = json!(MANUAL_PAUSE_SENTINEL);
    ledger["pauseReason"] = json!(r);
    if !write_ledger(&ledger) {
        eprintln!("[ERROR] Failed to write budget ledger.");
        return 1;
    }
    append_receipt(json!({"event": "manual-pause", "at": now, "reason": r}));
    println!("Autonomous AI worker launches paused across all daemons.");
    println!("Resume with: ruflo daemon budget resume");
    0
}

fn budget_resume() -> u8 {
    let _g = match LockGuard::acquire() {
        Some(g) => g,
        None => {
            eprintln!("[ERROR] Could not acquire budget lock.");
            return 1;
        }
    };
    let now = now_ms();
    let mut ledger = match read_ledger() {
        Ok(v) => prune_ledger(v, now),
        Err(e) => {
            eprintln!("[ERROR] {e}");
            return 1;
        }
    };
    let was_paused = ledger["pausedUntil"]
        .as_u64()
        .map(|t| t > now)
        .unwrap_or(false);
    ledger["pausedUntil"] = Value::Null;
    ledger["pauseReason"] = Value::Null;
    if !write_ledger(&ledger) {
        eprintln!("[ERROR] Failed to write budget ledger.");
        return 1;
    }
    if was_paused {
        append_receipt(json!({"event": "manual-resume", "at": now}));
    }
    println!("Autonomous AI worker launches resumed.");
    0
}

// ---- supervisor -------------------------------------------------------------

fn install_supervisor(root: &Path, _command: &DaemonCommand) -> u8 {
    // Write a crontab-style @reboot line the user can install. Native can't
    // install into launchd/systemd without platform work; degrade honestly.
    let dir = root.join(".claude-flow");
    let _ = fs::create_dir_all(&dir);
    let path = dir.join("daemon-supervisor.cron");
    let _ = fs::write(&path, "# install with: crontab daemon-supervisor.cron\n@reboot ruflo daemon start --background\n");
    println!("Supervisor config written to {}", path.display());
    println!("Install it with: crontab {}", path.display());
    eprintln!("[WARN] Native build writes the config only; install into launchd/systemd/cron is manual.");
    0
}

fn uninstall_supervisor(root: &Path, _command: &DaemonCommand) -> u8 {
    let path = root.join(".claude-flow/daemon-supervisor.cron");
    if path.exists() {
        let _ = fs::remove_file(&path);
        println!("Removed supervisor config ({}).", path.display());
        println!("Remove the matching @reboot line from your crontab.");
    } else {
        println!("No supervisor config found.");
    }
    0
}

// ---- helpers ----------------------------------------------------------------

fn fmt_iso(ms: u64) -> String {
    let secs = ms / 1000;
    let days = secs / 86400;
    let rem = secs % 86400;
    let h = rem / 3600;
    let m = (rem % 3600) / 60;
    let s = rem % 60;
    let (y, mo, d) = civil_from_days(days as i64);
    format!("{y:04}-{mo:02}-{d:02}T{h:02}:{m:02}:{s:02}Z")
}

fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    // RUFLO_AI_BUDGET_DIR is process-global; serialize the tests that set it.
    static BUDGET_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn budget_pause_resume_roundtrip() {
        let _lock = BUDGET_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        // Use a temp budget dir.
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("RUFLO_AI_BUDGET_DIR", tmp.path());
        // Fresh ledger.
        assert!(read_ledger().unwrap()["launches"].as_array().unwrap().is_empty());
        // Pause.
        let _g = LockGuard::acquire().expect("lock");
        let mut l = read_ledger().unwrap();
        l["pausedUntil"] = json!(MANUAL_PAUSE_SENTINEL);
        l["pauseReason"] = json!("test");
        assert!(write_ledger(&l));
        drop(_g);
        let l2 = read_ledger().unwrap();
        assert_eq!(l2["pausedUntil"], MANUAL_PAUSE_SENTINEL);
        // Resume clears it.
        let _g2 = LockGuard::acquire().expect("lock2");
        let mut l3 = read_ledger().unwrap();
        l3["pausedUntil"] = Value::Null;
        assert!(write_ledger(&l3));
        drop(_g2);
        let l4 = read_ledger().unwrap();
        assert!(l4["pausedUntil"].is_null());
        std::env::remove_var("RUFLO_AI_BUDGET_DIR");
    }

    #[test]
    fn malformed_ledger_is_rejected_not_reset() {
        let _lock = BUDGET_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("RUFLO_AI_BUDGET_DIR", tmp.path());
        // Write a corrupt ledger; read_ledger must error rather than reset.
        std::fs::write(ledger_file(), "{ not valid json").unwrap();
        assert!(read_ledger().is_err());
        std::env::remove_var("RUFLO_AI_BUDGET_DIR");
    }

    #[test]
    fn prune_drops_old_launches() {
        let now: u64 = 100_000_000;
        let ledger = json!({
            "version": 1,
            "launches": [{"at": now - DAY_MS - 1, "pid": 1, "workerType": "x", "model": "m", "workspace": "w"}],
            "active": [{"permitId": "p", "at": now - ACTIVE_STALE_MS - 1, "pid": 2, "workerType": "x"}]
        });
        let pruned = prune_ledger(ledger, now);
        assert!(pruned["launches"].as_array().unwrap().is_empty());
        assert!(pruned["active"].as_array().unwrap().is_empty());
    }

    #[test]
    fn civil_from_days_epoch() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
    }
}
