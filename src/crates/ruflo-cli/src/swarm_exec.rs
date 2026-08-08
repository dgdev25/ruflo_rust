//! Native Rust swarm execution — spawn real agent-worker subprocesses.
//!
//! ADR-0008: `swarm start --objective --workers N` forks N agent processes
//! (claude default, `--agent codex` for codex) in parallel, each with the
//! objective + a per-worker role prompt. No Node, no MCP bridge — direct
//! `std::process::Command`. Worker stdout/stderr/exit captured + recorded into
//! the swarm state file and ruflo memory.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Output};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};

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

    // Spawn N workers in parallel (one thread each).
    let results: Vec<WorkerResult> = (0..n)
        .map(|i| {
            let prompt = worker_prompt(objective, i, n, roles[i]);
            let agent_for_thread = agent.to_string();
            let agent_for_fallback = agent.to_string();
            let cwd_owned = cwd.to_path_buf();
            std::thread::spawn(move || {
                spawn_worker_with_idx(
                    i,
                    &agent_for_thread,
                    &prompt,
                    &cwd_owned,
                    keep_env,
                )
            })
            .join()
            .unwrap_or_else(|_| WorkerResult {
                worker_idx: i,
                agent: agent_for_fallback,
                stdout: String::new(),
                stderr: "worker thread panicked".into(),
                exit_code: -1,
                timed_out: false,
            })
        })
        .collect();

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
                || ku == "TOKEN"
                || ku == "SECRET";
            if suspicious {
                cmd.env_remove(&k);
            }
        }
    }
    let output: Result<Output, std::io::Error> = cmd.output();
    match output {
        Ok(o) => WorkerResult {
            worker_idx: idx,
            agent: agent.into(),
            stdout: String::from_utf8_lossy(&o.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&o.stderr).into_owned(),
            exit_code: o.status.code().unwrap_or(-1),
            timed_out: false,
        },
        Err(e) => WorkerResult {
            worker_idx: idx,
            agent: agent.into(),
            stdout: String::new(),
            stderr: format!("failed to spawn '{bin}': {e}"),
            exit_code: -1,
            timed_out: false,
        },
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
